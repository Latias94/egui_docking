mod support;

use dockspace::engine::{DockEngine, EngineInput};
use dockspace::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::platform::{
    ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCoordinateObservation, WindowPresentationObservation,
    WindowPresentationState,
};
use dockspace::policy::DockPolicy;
use dockspace::viewport::{
    CoordinateObservationGeneration, PresentationObservationGeneration, ViewportBinding,
    ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use support::{TestPresentationHost, submit_input, submit_inputs};

const SURFACE_ONE: SurfaceId = SurfaceId::new(1);
const SURFACE_ONE_AND_HALF: SurfaceId = SurfaceId::new(2);
const SURFACE_TWO: SurfaceId = SurfaceId::new(3);

const WINDOW_ONE: WindowToken = WindowToken::new(11);
const WINDOW_ONE_AND_HALF: WindowToken = WindowToken::new(12);
const WINDOW_TWO: WindowToken = WindowToken::new(13);

const WORK_AREA_LEFT: WorkAreaToken = WorkAreaToken::new(21);
const WORK_AREA_CENTER: WorkAreaToken = WorkAreaToken::new(22);
const WORK_AREA_RIGHT: WorkAreaToken = WorkAreaToken::new(23);
const MIXED_DPI_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xD911);

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn workspace(surfaces: &[SurfaceId]) -> Workspace {
    let mut builder = Workspace::builder();
    for (index, surface) in surfaces.iter().copied().enumerate() {
        let id = u64::try_from(index + 1).expect("test surface count must fit u64");
        let node = builder.insert_node(Node::tabs([ItemId::new(id)]));
        builder.set_root(RootId::new(id), RootRecord::new(node));
        builder.set_surface(surface, SurfacePresentation::with_main(RootId::new(id)));
    }
    builder.build().expect("test workspace must be valid")
}

fn supported_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities
}

fn observed_window(
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    content_bounds: PhysicalRect,
    scale: f64,
) -> ObservedWindow {
    ObservedWindow::new(binding).with_coordinate_observation(WindowCoordinateObservation::new(
        binding,
        generation,
        Authority::Known(content_bounds),
        Authority::Known(content_bounds),
        Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
        Authority::Known(ScaleFactor::new(scale).expect("test scale factor must be valid")),
    ))
}

fn observed_work_area(token: WorkAreaToken, bounds: PhysicalRect, scale: f64) -> ObservedWorkArea {
    ObservedWorkArea::new(
        token,
        bounds,
        ScaleFactor::new(scale).expect("test work-area scale factor must be valid"),
    )
}

fn monitor_roster() -> Vec<ObservedWorkArea> {
    vec![
        observed_work_area(
            WORK_AREA_LEFT,
            physical_rect(-1920.0, 0.0, 1920.0, 1080.0),
            1.0,
        ),
        observed_work_area(
            WORK_AREA_CENTER,
            physical_rect(0.0, 200.0, 1920.0, 1080.0),
            1.5,
        ),
        observed_work_area(
            WORK_AREA_RIGHT,
            physical_rect(2560.0, -400.0, 2560.0, 1440.0),
            2.0,
        ),
    ]
}

fn register_viewports(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    registrations: &[(SurfaceId, WindowToken)],
) {
    let expected = engine.version();
    let provider = presentation_host.platform_provider();
    submit_inputs(
        engine,
        presentation_host,
        MIXED_DPI_INPUT_SOURCE,
        registrations
            .iter()
            .map(|&(surface, token)| EngineInput::RegisterViewport {
                provider,
                expected,
                surface,
                token,
                role: ViewportRole::Root,
                recovery_target: None,
            }),
    )
    .expect("viewport registrations must reduce atomically");
}

fn viewport_binding(engine: &DockEngine, surface: SurfaceId) -> ViewportBinding {
    engine
        .viewport()
        .viewport(surface)
        .expect("registered test viewport must remain present")
        .binding()
}

fn publish_snapshot(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    mut windows: Vec<ObservedWindow>,
    work_areas: Vec<ObservedWorkArea>,
) {
    let observation_generation = presentation_host.next_platform_observation_generation();
    for window in &mut windows {
        let binding = window.binding();
        *window = window
            .clone()
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(observation_generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
    }
    let inventory_observation =
        support::known_inventory_observation(observation_generation, &windows);
    let snapshot = PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(observation_generation),
        support::known_capability_observation(observation_generation, supported_capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(observation_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        support::known_work_area_observation(observation_generation, work_areas),
    )
    .expect("test platform snapshot must be valid");
    let expected_epoch = engine.version().epoch();
    let provider = presentation_host.platform_provider();
    submit_input(
        engine,
        presentation_host,
        MIXED_DPI_INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("platform snapshot must publish");
}

#[test]
fn placement_requires_an_explicit_noncontiguous_target_work_area() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_TWO]), DockPolicy::default())
        .expect("test engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_viewports(
        &mut engine,
        &mut presentation_host,
        &[(SURFACE_TWO, WINDOW_TWO)],
    );
    let binding = viewport_binding(&engine, SURFACE_TWO);

    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![observed_window(
            binding,
            CoordinateObservationGeneration::new(1),
            physical_rect(-1600.0, -300.0, 2000.0, 1600.0),
            2.0,
        )],
        monitor_roster(),
    );

    let right = engine
        .viewport_placement(
            SURFACE_TWO,
            logical_rect(1800.0, 900.0, 600.0, 400.0),
            WORK_AREA_RIGHT,
        )
        .expect("current coordinate facts must produce a placement proof");
    let left = engine
        .viewport_placement(
            SURFACE_TWO,
            logical_rect(1800.0, 900.0, 600.0, 400.0),
            WORK_AREA_LEFT,
        )
        .expect("the same request must support an explicit alternate target");

    assert_eq!(
        right.physical_rect(),
        physical_rect(2560.0, 240.0, 1200.0, 800.0)
    );
    assert_eq!(
        left.physical_rect(),
        physical_rect(-600.0, 680.0, 600.0, 400.0)
    );
    assert_eq!(
        right.logical_rect(),
        logical_rect(1800.0, 900.0, 600.0, 400.0)
    );
    assert_eq!(right.work_area(), WORK_AREA_RIGHT);
    assert_eq!(left.work_area(), WORK_AREA_LEFT);
}

#[test]
fn identical_inventory_facts_keep_an_existing_placement_proof_current() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE_AND_HALF]), DockPolicy::default())
        .expect("test engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_viewports(
        &mut engine,
        &mut presentation_host,
        &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)],
    );
    let binding = viewport_binding(&engine, SURFACE_ONE_AND_HALF);

    let first_window = observed_window(
        binding,
        CoordinateObservationGeneration::new(1),
        physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
        1.5,
    );
    let roster = monitor_roster();
    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![first_window.clone()],
        roster.clone(),
    );
    let old_proof = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(1000.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("first snapshot must produce a placement proof");
    let old_generation = old_proof.coordinate_generation();

    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![first_window],
        roster,
    );
    let current_proof = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(1000.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("latest snapshot must produce a placement proof");

    assert_eq!(old_generation, current_proof.coordinate_generation());
    assert_eq!(
        old_proof.work_area_generation(),
        current_proof.work_area_generation()
    );
    assert_eq!(old_proof.binding(), current_proof.binding());
    assert_eq!(old_proof.physical_rect(), current_proof.physical_rect());
}

#[test]
fn changed_placement_facts_advance_the_coordinate_generation() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE_AND_HALF]), DockPolicy::default())
        .expect("test engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_viewports(
        &mut engine,
        &mut presentation_host,
        &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)],
    );
    let binding = viewport_binding(&engine, SURFACE_ONE_AND_HALF);

    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![observed_window(
            binding,
            CoordinateObservationGeneration::new(1),
            physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
            1.5,
        )],
        monitor_roster(),
    );
    let old_proof = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(1000.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("first snapshot must produce a placement proof");

    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![observed_window(
            binding,
            CoordinateObservationGeneration::new(2),
            physical_rect(-900.0, -200.0, 1500.0, 1200.0),
            1.5,
        )],
        monitor_roster(),
    );
    let current_proof = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(1000.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("changed snapshot must produce a placement proof");

    assert_ne!(
        old_proof.coordinate_generation(),
        current_proof.coordinate_generation()
    );
    assert_ne!(old_proof.physical_rect(), current_proof.physical_rect());
}

#[test]
fn changed_work_area_roster_invalidates_only_the_work_area_proof_generation() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE_AND_HALF]), DockPolicy::default())
        .expect("test engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_viewports(
        &mut engine,
        &mut presentation_host,
        &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)],
    );
    let binding = viewport_binding(&engine, SURFACE_ONE_AND_HALF);
    let window = observed_window(
        binding,
        CoordinateObservationGeneration::new(1),
        physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
        1.5,
    );
    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![window.clone()],
        monitor_roster(),
    );
    let old = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(40.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("first roster must produce a placement proof");

    let mut changed_roster = monitor_roster();
    changed_roster[1] = observed_work_area(
        WORK_AREA_CENTER,
        physical_rect(100.0, 200.0, 1800.0, 1080.0),
        1.5,
    );
    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![window],
        changed_roster,
    );
    let current = engine
        .viewport_placement(
            SURFACE_ONE_AND_HALF,
            logical_rect(40.0, 20.0, 200.0, 100.0),
            WORK_AREA_CENTER,
        )
        .expect("changed roster must produce a new placement proof");

    assert_eq!(old.coordinate_generation(), current.coordinate_generation());
    assert_ne!(old.work_area_generation(), current.work_area_generation());
}

#[test]
fn unknown_work_area_token_fails_closed() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE]), DockPolicy::default())
        .expect("test engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_viewports(
        &mut engine,
        &mut presentation_host,
        &[(SURFACE_ONE, WINDOW_ONE)],
    );
    let binding = viewport_binding(&engine, SURFACE_ONE);
    publish_snapshot(
        &mut engine,
        &mut presentation_host,
        vec![observed_window(
            binding,
            CoordinateObservationGeneration::new(1),
            physical_rect(-1920.0, 0.0, 1000.0, 800.0),
            1.0,
        )],
        monitor_roster(),
    );

    assert!(matches!(
        engine.viewport_placement(
            SURFACE_ONE,
            logical_rect(0.0, 0.0, 300.0, 200.0),
            WorkAreaToken::new(999),
        ),
        Err(dockspace::coordinates::CoordinateUnavailable::UnknownWorkArea { .. })
    ));
}
