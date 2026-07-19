use dockspace::coordinates::TearOffPlacementRequest;
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, NativePlacementProof, PointerId};
use dockspace::platform::{
    ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PointerObservation, PointerWindow, WindowInputState, WindowPresentationState,
};
use dockspace::policy::DockPolicy;
use dockspace::viewport::{ViewportRole, WindowToken, WorkAreaToken};

const SURFACE_ONE: SurfaceId = SurfaceId::new(1);
const SURFACE_ONE_AND_HALF: SurfaceId = SurfaceId::new(2);
const SURFACE_TWO: SurfaceId = SurfaceId::new(3);

const WINDOW_ONE: WindowToken = WindowToken::new(11);
const WINDOW_ONE_AND_HALF: WindowToken = WindowToken::new(12);
const WINDOW_TWO: WindowToken = WindowToken::new(13);

const WORK_AREA_LEFT: WorkAreaToken = WorkAreaToken::new(21);
const WORK_AREA_CENTER: WorkAreaToken = WorkAreaToken::new(22);
const WORK_AREA_RIGHT: WorkAreaToken = WorkAreaToken::new(23);

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
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    capabilities
}

fn observed_window(token: WindowToken, content_bounds: PhysicalRect, scale: f64) -> ObservedWindow {
    ObservedWindow::new(token)
        .with_content_bounds(Authority::Known(content_bounds))
        .with_outer_bounds(Authority::Known(content_bounds))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(scale).expect("test scale factor must be valid"),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation(Authority::Known(WindowPresentationState::Visible))
        .with_close_requested(Authority::Known(false))
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
    work_areas: Vec<ObservedWorkArea>,
) {
    let snapshot = PlatformSnapshot::new(supported_capabilities(), windows, pointers, work_areas)
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

    let windows = vec![
        observed_window(
            WINDOW_ONE,
            physical_rect(-1920.0, -100.0, 1000.0, 800.0),
            1.0,
        ),
        observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(0.0, 200.0, 1500.0, 1200.0),
            1.5,
        ),
        observed_window(
            WINDOW_TWO,
            physical_rect(2560.0, -400.0, 2000.0, 1600.0),
            2.0,
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
    publish_snapshot(&mut engine, windows, pointers, monitor_roster());

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
fn placement_requires_an_explicit_noncontiguous_target_work_area() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_TWO]), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(&mut engine, &[(SURFACE_TWO, WINDOW_TWO)]);

    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_TWO,
            physical_rect(-1600.0, -300.0, 2000.0, 1600.0),
            2.0,
        )],
        Vec::new(),
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
    register_viewports(&mut engine, &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)]);

    let first_window = observed_window(
        WINDOW_ONE_AND_HALF,
        physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
        1.5,
    );
    let roster = monitor_roster();
    publish_snapshot(
        &mut engine,
        vec![first_window.clone()],
        Vec::new(),
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

    publish_snapshot(&mut engine, vec![first_window], Vec::new(), roster);
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
    register_viewports(&mut engine, &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)]);

    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
            1.5,
        )],
        Vec::new(),
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
        vec![observed_window(
            WINDOW_ONE_AND_HALF,
            physical_rect(-900.0, -200.0, 1500.0, 1200.0),
            1.5,
        )],
        Vec::new(),
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
    register_viewports(&mut engine, &[(SURFACE_ONE_AND_HALF, WINDOW_ONE_AND_HALF)]);
    let window = observed_window(
        WINDOW_ONE_AND_HALF,
        physical_rect(-1200.0, -200.0, 1500.0, 1200.0),
        1.5,
    );
    publish_snapshot(
        &mut engine,
        vec![window.clone()],
        Vec::new(),
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
    publish_snapshot(&mut engine, vec![window], Vec::new(), changed_roster);
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
    register_viewports(&mut engine, &[(SURFACE_ONE, WINDOW_ONE)]);
    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE,
            physical_rect(-1920.0, 0.0, 1000.0, 800.0),
            1.0,
        )],
        Vec::new(),
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

#[test]
fn tear_off_placement_uses_the_explicit_release_anchor_and_target_scale() {
    let mut engine = DockEngine::new(workspace(&[SURFACE_ONE]), DockPolicy::default())
        .expect("test engine must be valid");
    register_viewports(&mut engine, &[(SURFACE_ONE, WINDOW_ONE)]);
    let pointer = PointerId::new(9);
    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE,
            physical_rect(0.0, 0.0, 1000.0, 800.0),
            1.0,
        )],
        vec![
            PointerObservation::new(
                pointer,
                Authority::Known(PointerWindow::None),
                Authority::Known(physical_point(100.0, 200.0)),
                Authority::Known(Vec::new()),
            )
            .expect("test pointer observation must be valid"),
        ],
        vec![observed_work_area(
            WORK_AREA_RIGHT,
            physical_rect(0.0, 0.0, 2000.0, 1600.0),
            2.0,
        )],
    );

    let proof = engine
        .tear_off_placement(
            pointer,
            TearOffPlacementRequest::new(
                LogicalSize::new(10.0, 20.0).expect("offset must be valid"),
                LogicalSize::new(300.0, 200.0).expect("preferred size must be valid"),
                LogicalSize::new(400.0, 250.0).expect("minimum size must be valid"),
                WORK_AREA_RIGHT,
            ),
        )
        .expect("authoritative route and work area must solve placement");
    assert_eq!(proof.pointer(), pointer);
    assert_eq!(
        proof.physical_rect(),
        physical_rect(80.0, 160.0, 800.0, 500.0)
    );
    assert_eq!(
        proof.requested_rect(),
        logical_rect(40.0, 80.0, 400.0, 250.0)
    );
    let native_proof = NativePlacementProof::from(proof);
    assert!(engine.native_placement_is_current(&native_proof));

    publish_snapshot(
        &mut engine,
        vec![observed_window(
            WINDOW_ONE,
            physical_rect(0.0, 0.0, 1000.0, 800.0),
            1.0,
        )],
        vec![
            PointerObservation::new(
                pointer,
                Authority::Known(PointerWindow::None),
                Authority::Known(physical_point(101.0, 200.0)),
                Authority::Known(Vec::new()),
            )
            .expect("test pointer observation must be valid"),
        ],
        vec![observed_work_area(
            WORK_AREA_RIGHT,
            physical_rect(0.0, 0.0, 2000.0, 1600.0),
            2.0,
        )],
    );
    assert!(!engine.native_placement_is_current(&native_proof));
}
