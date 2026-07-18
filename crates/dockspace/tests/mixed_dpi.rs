use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, PointerId};
use dockspace::platform::{
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot, PointerObservation,
    PointerWindow, WindowInputState,
};
use dockspace::policy::DockPolicy;
use dockspace::viewport::{ViewportRole, WindowToken};

const SURFACE_ONE: SurfaceId = SurfaceId::new(1);
const SURFACE_ONE_AND_HALF: SurfaceId = SurfaceId::new(2);
const SURFACE_TWO: SurfaceId = SurfaceId::new(3);

const WINDOW_ONE: WindowToken = WindowToken::new(11);
const WINDOW_ONE_AND_HALF: WindowToken = WindowToken::new(12);
const WINDOW_TWO: WindowToken = WindowToken::new(13);

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_point(x: f64, y: f64) -> PhysicalPoint {
    PhysicalPoint::new(x, y).expect("test physical point must be valid")
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
        builder.set_surface(surface, SurfacePresentation::new(RootId::new(id)));
    }
    builder.build().expect("test workspace must be valid")
}

fn supported_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_pointer_passthrough(PlatformCapability::Supported);
    capabilities
}

fn observed_window(
    token: WindowToken,
    content_bounds: PhysicalRect,
    scale: f64,
    work_area: PhysicalRect,
) -> ObservedWindow {
    ObservedWindow::new(token)
        .with_content_bounds(Authority::Known(content_bounds))
        .with_outer_bounds(Authority::Known(content_bounds))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(scale).expect("test scale factor must be valid"),
        ))
        .with_work_area(Authority::Known(Some(work_area)))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_close_requested(Authority::Known(false))
}

fn register_viewports(engine: &mut DockEngine, registrations: &[(SurfaceId, WindowToken)]) {
    for &(surface, token) in registrations {
        engine
            .enqueue_viewport_registration(surface, token, ViewportRole::Root, None)
            .expect("viewport registration sequence must be available");
    }
    engine
        .reduce_pending()
        .expect("viewport registrations must reduce atomically");
}

fn publish_snapshot(
    engine: &mut DockEngine,
    windows: Vec<ObservedWindow>,
    pointers: Vec<PointerObservation>,
) {
    let snapshot = PlatformSnapshot::new(supported_capabilities(), windows, pointers)
        .expect("test platform snapshot must be valid");
    engine
        .enqueue_platform_snapshot(snapshot)
        .expect("platform snapshot sequence must be available");
    engine
        .reduce_pending()
        .expect("platform snapshot must publish");
}

fn assert_near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-9,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn routes_desktop_physical_points_with_each_target_scale_exactly_once() {
    let surfaces = [SURFACE_ONE, SURFACE_ONE_AND_HALF, SURFACE_TWO];
    let mut engine = DockEngine::new(workspace(&surfaces), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(
        &mut engine,
        &[
            (SURFACE_ONE, WINDOW_ONE),
            (SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF),
            (SURFACE_TWO, WINDOW_TWO),
        ],
    );

    let work_area = physical_rect(-3000.0, -1000.0, 7000.0, 3000.0);
    let windows = vec![
        observed_window(
            WINDOW_ONE,
            physical_rect(-1920.0, -100.0, 1000.0, 800.0),
            1.0,
            work_area,
        ),
        observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(0.0, 200.0, 1500.0, 1200.0),
            1.5,
            work_area,
        ),
        observed_window(
            WINDOW_TWO,
            physical_rect(2560.0, -400.0, 2000.0, 1600.0),
            2.0,
            work_area,
        ),
    ];
    let route_cases = [
        (
            PointerId::new(1),
            WINDOW_ONE,
            SURFACE_ONE,
            physical_point(-1800.0, -20.0),
        ),
        (
            PointerId::new(2),
            WINDOW_ONE_AND_HALF,
            SURFACE_ONE_AND_HALF,
            physical_point(180.0, 320.0),
        ),
        (
            PointerId::new(3),
            WINDOW_TWO,
            SURFACE_TWO,
            physical_point(2800.0, -240.0),
        ),
    ];
    let pointers = route_cases
        .iter()
        .map(|&(pointer, window, _, desktop_position)| {
            PointerObservation::new(
                pointer,
                Authority::Known(PointerWindow::Dock(window)),
                Authority::Known(desktop_position),
                Authority::Known(Vec::new()),
            )
            .expect("test pointer observation must be valid")
        })
        .collect();
    publish_snapshot(&mut engine, windows, pointers);

    for (pointer, _, expected_surface, _) in route_cases {
        let proof = engine
            .viewport()
            .route(pointer)
            .expect("each pointer must have a route proof");
        let target = match proof.target() {
            Authority::Known(Some(target)) => *target,
            target => panic!("expected an authoritative dock target, got {target:?}"),
        };
        assert_eq!(target.surface(), expected_surface);
        assert_near(target.position().x(), 120.0);
        assert_near(target.position().y(), 80.0);
    }
}

#[test]
fn placement_supports_negative_origins_and_clamps_to_the_physical_work_area() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_TWO]), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(&mut engine, &[(SURFACE_TWO, WINDOW_TWO)]);

    let work_area = physical_rect(-1920.0, -400.0, 3840.0, 1400.0);
    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_TWO,
            physical_rect(-1600.0, -300.0, 2000.0, 1600.0),
            2.0,
            work_area,
        )],
        Vec::new(),
    );

    let proof = engine
        .viewport_placement(SURFACE_TWO, logical_rect(1800.0, 900.0, 600.0, 400.0))
        .expect("current coordinate facts must produce a placement proof");

    assert_eq!(
        proof.physical_rect(),
        physical_rect(720.0, 200.0, 1200.0, 800.0)
    );
    assert_eq!(
        proof.logical_rect(),
        logical_rect(1800.0, 900.0, 600.0, 400.0)
    );
}

#[test]
fn identical_inventory_facts_keep_an_existing_placement_proof_current() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE_AND_HALF]), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(&mut engine, &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)]);

    let work_area = physical_rect(-2000.0, -1000.0, 5000.0, 3000.0);
    let first_window = observed_window(
        WINDOW_ONE_AND_HALF,
        physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
        1.5,
        work_area,
    );
    publish_snapshot(&mut engine, vec![first_window.clone()], Vec::new());
    let old_proof = engine
        .viewport_placement(SURFACE_ONE_AND_HALF, logical_rect(40.0, 20.0, 200.0, 100.0))
        .expect("first snapshot must produce a placement proof");
    let old_generation = old_proof.coordinate_generation();

    publish_snapshot(&mut engine, vec![first_window], Vec::new());
    let current_proof = engine
        .viewport_placement(SURFACE_ONE_AND_HALF, logical_rect(40.0, 20.0, 200.0, 100.0))
        .expect("latest snapshot must produce a placement proof");

    assert_eq!(old_generation, current_proof.coordinate_generation());
    assert_eq!(old_proof.binding(), current_proof.binding());
    assert_eq!(old_proof.physical_rect(), current_proof.physical_rect());
}

#[test]
fn changed_placement_facts_advance_the_coordinate_generation() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE_AND_HALF]), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(&mut engine, &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)]);

    let work_area = physical_rect(-2000.0, -1000.0, 5000.0, 3000.0);
    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
            1.5,
            work_area,
        )],
        Vec::new(),
    );
    let old_proof = engine
        .viewport_placement(SURFACE_ONE_AND_HALF, logical_rect(40.0, 20.0, 200.0, 100.0))
        .expect("first snapshot must produce a placement proof");

    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(-900.0, -200.0, 1500.0, 1200.0),
            1.5,
            work_area,
        )],
        Vec::new(),
    );
    let current_proof = engine
        .viewport_placement(SURFACE_ONE_AND_HALF, logical_rect(40.0, 20.0, 200.0, 100.0))
        .expect("changed snapshot must produce a placement proof");

    assert_ne!(
        old_proof.coordinate_generation(),
        current_proof.coordinate_generation()
    );
    assert_ne!(old_proof.physical_rect(), current_proof.physical_rect());
}
