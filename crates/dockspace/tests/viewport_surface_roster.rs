mod support;

use std::collections::BTreeMap;

use dockspace::RootPresentationOwner;
use dockspace::command::{ContainedPosition, RootPresentationTarget, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::CommandError;
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::platform::{
    CloseEffectAcknowledgement, InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputObservation,
    WindowInputState, WindowPresentationObservation, WindowPresentationState,
};
use dockspace::policy::DockPolicy;
use dockspace::surface_recovery::{ConvertedMainRecovery, SurfaceRecoveryTarget};
use dockspace::transition::{InputOutcome, SurfaceContributionOutcome};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use support::{TestPresentationHost, submit_input};

const ROOT_HOST: RootId = RootId::new(1);
const ROOT_CHILD_MAIN: RootId = RootId::new(2);
const ROOT_CHILD_FLOATING_A: RootId = RootId::new(3);
const ROOT_CHILD_FLOATING_B: RootId = RootId::new(4);
const ROOT_HOST_FLOATING: RootId = RootId::new(5);
const ROOT_RESTORED_HOST: RootId = RootId::new(101);
const ROOT_RESTORED_CHILD_MAIN: RootId = RootId::new(102);

const SURFACE_HOST: SurfaceId = SurfaceId::new(1);
const SURFACE_CHILD: SurfaceId = SurfaceId::new(2);
const SURFACE_RESTORED_HOST: SurfaceId = SurfaceId::new(101);
const SURFACE_RESTORED_CHILD: SurfaceId = SurfaceId::new(102);

const FLOATING_HOST: FloatingPresentationId = FloatingPresentationId::new(10);
const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(11);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(12);
const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const RECOVERY_RESTORED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(101);

const HOST_TOKEN: WindowToken = WindowToken::new(101);
const CHILD_TOKEN: WindowToken = WindowToken::new(102);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(201);
const ROSTER_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xA057);

#[derive(Debug, Clone, Copy)]
struct WindowGeometry {
    content: PhysicalRect,
    outer: PhysicalRect,
    scale: ScaleFactor,
}

#[derive(Debug, Clone, Copy)]
enum TestWindow {
    Host,
    Child,
    ChildUnavailable,
}

struct Fixture {
    engine: DockEngine,
    presentation_host: TestPresentationHost,
    host_binding: ViewportBinding,
    child_binding: ViewportBinding,
    host_geometry: WindowGeometry,
    child_geometry: WindowGeometry,
    next_input_generation: u64,
    child_roots: Vec<(RootId, RootRecord)>,
    initial_items: BTreeMap<ItemId, usize>,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn rooted_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let child_main = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    let child_a = builder.insert_node(Node::tabs([ItemId::new(20), ItemId::new(21)]));
    let child_b = builder.insert_node(Node::tabs([ItemId::new(30), ItemId::new(31)]));
    let host_floating = builder.insert_node(Node::tabs([ItemId::new(90)]));

    builder.set_root(
        ROOT_HOST,
        RootRecord::new(host_tabs).with_central(host_tabs),
    );
    builder.set_root(
        ROOT_CHILD_MAIN,
        RootRecord::new(child_main).with_central(child_main),
    );
    builder.set_root(
        ROOT_CHILD_FLOATING_A,
        RootRecord::new(child_a).with_central(child_a),
    );
    builder.set_root(
        ROOT_CHILD_FLOATING_B,
        RootRecord::new(child_b).with_central(child_b),
    );
    builder.set_root(
        ROOT_HOST_FLOATING,
        RootRecord::new(host_floating).with_central(host_floating),
    );
    builder.set_surface(SURFACE_HOST, SurfacePresentation::with_main(ROOT_HOST));
    builder.set_surface(
        SURFACE_CHILD,
        SurfacePresentation::with_main(ROOT_CHILD_MAIN),
    );
    builder.set_contained_floating(
        FLOATING_HOST,
        ContainedFloating::new(ROOT_HOST_FLOATING, logical_rect(120.0, 140.0, 240.0, 160.0)),
    );
    builder.set_contained_floating(
        FLOATING_A,
        ContainedFloating::new(
            ROOT_CHILD_FLOATING_A,
            logical_rect(24.0, 32.0, 320.0, 240.0),
        ),
    );
    builder.set_contained_floating(
        FLOATING_B,
        ContainedFloating::new(
            ROOT_CHILD_FLOATING_B,
            logical_rect(380.0, 96.0, 420.0, 310.0),
        ),
    );
    builder
        .attach_contained(SURFACE_HOST, FLOATING_HOST)
        .expect("host surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_A)
        .expect("child surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_B)
        .expect("child surface must exist");
    builder.build().expect("rooted workspace must validate")
}

fn rootless_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let host_floating = builder.insert_node(Node::tabs([ItemId::new(90)]));
    let child_a = builder.insert_node(Node::tabs([ItemId::new(20), ItemId::new(21)]));
    let child_b = builder.insert_node(Node::tabs([ItemId::new(30)]));

    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_root(ROOT_HOST_FLOATING, RootRecord::new(host_floating));
    builder.set_root(ROOT_CHILD_FLOATING_A, RootRecord::new(child_a));
    builder.set_root(ROOT_CHILD_FLOATING_B, RootRecord::new(child_b));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::with_main(ROOT_HOST));
    builder.set_surface(SURFACE_CHILD, SurfacePresentation::rootless());
    builder.set_contained_floating(
        FLOATING_HOST,
        ContainedFloating::new(ROOT_HOST_FLOATING, logical_rect(120.0, 140.0, 240.0, 160.0)),
    );
    builder.set_contained_floating(
        FLOATING_A,
        ContainedFloating::new(
            ROOT_CHILD_FLOATING_A,
            logical_rect(24.0, 32.0, 320.0, 240.0),
        ),
    );
    builder.set_contained_floating(
        FLOATING_B,
        ContainedFloating::new(
            ROOT_CHILD_FLOATING_B,
            logical_rect(380.0, 96.0, 420.0, 310.0),
        ),
    );
    builder
        .attach_contained(SURFACE_HOST, FLOATING_HOST)
        .expect("host surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_A)
        .expect("rootless child surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_B)
        .expect("rootless child surface must exist");
    builder.build().expect("rootless workspace must validate")
}

fn parking_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let root = RootId::new(100);
    let surface = SurfaceId::new(100);
    let tabs = builder.insert_node(Node::tabs([ItemId::new(100)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    builder.build().expect("parking workspace must validate")
}

fn restored_recovery_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(101)]));
    let child_main = builder.insert_node(Node::tabs([ItemId::new(102)]));
    builder.set_root(ROOT_RESTORED_HOST, RootRecord::new(host_tabs));
    builder.set_root(ROOT_RESTORED_CHILD_MAIN, RootRecord::new(child_main));
    builder.set_surface(
        SURFACE_RESTORED_HOST,
        SurfacePresentation::with_main(ROOT_RESTORED_HOST),
    );
    builder.set_surface(
        SURFACE_RESTORED_CHILD,
        SurfacePresentation::with_main(ROOT_RESTORED_CHILD_MAIN),
    );
    builder
        .build()
        .expect("restored recovery workspace must validate")
}

fn root_records(
    workspace: &Workspace,
    roots: impl IntoIterator<Item = RootId>,
) -> Vec<(RootId, RootRecord)> {
    roots
        .into_iter()
        .map(|root| {
            (
                root,
                *workspace
                    .root(root)
                    .expect("fixture root must exist before destruction"),
            )
        })
        .collect()
}

fn platform_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    capabilities
}

fn ready_window(
    binding: ViewportBinding,
    generation: u64,
    geometry: WindowGeometry,
) -> ObservedWindow {
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let input_generation = InputObservationGeneration::new(generation);
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            coordinate_generation,
            Authority::Known(geometry.content),
            Authority::Known(geometry.outer),
            Authority::Known(geometry.scale),
            Authority::Known(geometry.scale),
        ))
        .with_input_observation(WindowInputObservation::new(
            binding,
            input_generation,
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ))
}

fn platform_snapshot(
    generation: u64,
    windows: Vec<ObservedWindow>,
    destroyed: &[ViewportBinding],
) -> PlatformSnapshot {
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, platform_capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        destroyed
            .iter()
            .map(|binding| {
                WindowCloseObservation::new(
                    *binding,
                    CloseObservationGeneration::new(generation),
                    Authority::Known(WindowCloseState::Destroyed),
                    CloseEffectAcknowledgement::known(None),
                )
            })
            .collect(),
        support::known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                WORK_AREA,
                physical_rect(-1920.0, -200.0, 3840.0, 1600.0),
                ScaleFactor::new(1.0).expect("work area scale factor must be valid"),
            )],
        ),
    )
    .expect("platform snapshot must be canonical")
}

fn publish_windows(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
) {
    let expected_epoch = engine.version().epoch();
    let generation = presentation_host.next_platform_observation_generation();
    let snapshot = platform_snapshot(generation, windows, &[]);
    let provider = presentation_host.platform_provider();
    submit_input(
        engine,
        presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("platform snapshot must reduce");
}

fn publish_windows_with_destroyed(
    engine: &mut DockEngine,
    presentation_host: &mut TestPresentationHost,
    windows: Vec<ObservedWindow>,
    destroyed: &[ViewportBinding],
) {
    let expected_epoch = engine.version().epoch();
    let generation = presentation_host.next_platform_observation_generation();
    let snapshot = platform_snapshot(generation, windows, destroyed);
    let provider = presentation_host.platform_provider();
    submit_input(
        engine,
        presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("platform snapshot must reduce");
}

fn publish_scene(engine: &mut DockEngine, presentation_host: &mut TestPresentationHost) {
    let surfaces: Vec<_> = engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    for surface in surfaces {
        if engine
            .viewport()
            .viewport(surface)
            .is_some_and(|record| !record.is_ready())
        {
            continue;
        }
        support::publish_surface(
            engine,
            presentation_host,
            surface,
            logical_rect(0.0, 0.0, 1000.0, 800.0),
        );
    }
}

fn fixture(workspace: Workspace, child_roots: Vec<RootId>) -> Fixture {
    let initial_items = workspace.item_multiset();
    let child_roots = root_records(&workspace, child_roots);
    let child_main = workspace
        .surface(SURFACE_CHILD)
        .expect("child surface must exist")
        .main_root;
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must validate");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    support::publish_surfaces(
        &mut engine,
        &mut presentation_host,
        [
            (SURFACE_HOST, logical_rect(0.0, 0.0, 1000.0, 800.0)),
            (SURFACE_CHILD, logical_rect(0.0, 0.0, 1000.0, 800.0)),
        ],
    );
    let expected = engine.version();
    let root_registration = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("host viewport registration must reduce");
    assert!(matches!(
        root_registration.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding } if binding.surface() == SURFACE_HOST
    ));
    let recovery_anchor = engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("registered host root must issue a recovery anchor");
    let recovery_target = match child_main {
        Some(root) => SurfaceRecoveryTarget::with_converted_main(
            recovery_anchor,
            ConvertedMainRecovery::new(
                root,
                RECOVERY_FLOATING,
                LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
            ),
        ),
        None => SurfaceRecoveryTarget::forest_only(recovery_anchor),
    };
    let expected = engine.version();
    let child_registration = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    )
    .expect("child viewport registration must reduce");
    assert!(matches!(
        child_registration.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding } if binding.surface() == SURFACE_CHILD
    ));
    assert!(engine.root_recovery_anchor(SURFACE_CHILD).is_none());

    let host_binding = engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("host binding must exist")
        .binding();
    let child_binding = engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("child binding must exist")
        .binding();
    let mut fixture = Fixture {
        engine,
        presentation_host,
        host_binding,
        child_binding,
        host_geometry: WindowGeometry {
            content: physical_rect(0.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(-8.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("host scale must be valid"),
        },
        child_geometry: WindowGeometry {
            content: physical_rect(1000.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(992.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("child scale must be valid"),
        },
        next_input_generation: 1,
        child_roots,
        initial_items,
    };
    fixture.publish([TestWindow::Host, TestWindow::Child]);
    publish_scene(&mut fixture.engine, &mut fixture.presentation_host);
    fixture
}

impl Fixture {
    fn publish(&mut self, windows: impl IntoIterator<Item = TestWindow>) {
        let generation = self.next_generation();
        let windows = windows
            .into_iter()
            .map(|window| match window {
                TestWindow::Host => ready_window(self.host_binding, generation, self.host_geometry),
                TestWindow::Child => {
                    ready_window(self.child_binding, generation, self.child_geometry)
                }
                TestWindow::ChildUnavailable => ObservedWindow::new(self.child_binding),
            })
            .collect();
        publish_windows(&mut self.engine, &mut self.presentation_host, windows);
    }

    fn publish_destroyed(
        &mut self,
        destroyed: ViewportBinding,
        windows: impl IntoIterator<Item = TestWindow>,
    ) {
        let generation = self.next_generation();
        let windows = windows
            .into_iter()
            .map(|window| match window {
                TestWindow::Host => ready_window(self.host_binding, generation, self.host_geometry),
                TestWindow::Child => {
                    ready_window(self.child_binding, generation, self.child_geometry)
                }
                TestWindow::ChildUnavailable => ObservedWindow::new(self.child_binding),
            })
            .collect();
        publish_windows_with_destroyed(
            &mut self.engine,
            &mut self.presentation_host,
            windows,
            &[destroyed],
        );
    }

    fn next_generation(&mut self) -> u64 {
        let generation = self.next_input_generation;
        self.next_input_generation = self
            .next_input_generation
            .checked_add(1)
            .expect("input generation must not exhaust");
        generation
    }
}

fn assert_child_roots_preserved(fixture: &Fixture) {
    for (root, expected) in &fixture.child_roots {
        assert_eq!(
            fixture.engine.workspace().root(*root),
            Some(expected),
            "complete recovery must preserve root {root:?}"
        );
    }
}

#[test]
fn foreign_recovery_anchor_cannot_authorize_child_registration() {
    let workspace = rooted_workspace();
    let mut authority_engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("authority engine must validate");
    let mut authority_host = TestPresentationHost::new(&mut authority_engine);
    let authority_provider = authority_host.platform_provider();
    let expected = authority_engine.version();
    submit_input(
        &mut authority_engine,
        &mut authority_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider: authority_provider,
            expected,
            surface: SURFACE_HOST,
            token: WindowToken::new(301),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("foreign root registration must reduce");
    let foreign_anchor = authority_engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("foreign engine must issue its own anchor");

    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("target engine must validate");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("target root registration must reduce");
    let own_anchor = engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("target engine must issue its own anchor");
    assert_ne!(foreign_anchor, own_anchor);

    // Establish the unrelated child surface as painted before testing the
    // rejected semantic input. The registered host remains non-interactive
    // until it receives coordinate authority from the platform.
    publish_scene(&mut engine, &mut presentation_host);

    let workspace_before = engine.workspace().clone();
    let viewport_before = engine.viewport().clone();
    let scene_before = engine.scene().clone();
    let version_before = engine.version();
    let rejected = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected: version_before,
            surface: SURFACE_CHILD,
            token: CHILD_TOKEN,
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                foreign_anchor,
                ConvertedMainRecovery::new(
                    ROOT_CHILD_MAIN,
                    RECOVERY_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("foreign-anchor registration must reduce to a rejection");

    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: SURFACE_CHILD
        }
    ));
    assert_eq!(engine.workspace(), &workspace_before);
    assert_eq!(engine.viewport(), &viewport_before);
    assert_eq!(
        engine.scene().surface(SURFACE_CHILD),
        scene_before.surface(SURFACE_CHILD),
        "the rejected registration must retain unrelated painted authority"
    );
    assert!(
        rejected
            .surface_contributions()
            .iter()
            .any(|outcome| matches!(
                outcome,
                SurfaceContributionOutcome::Unavailable {
                    surface: SURFACE_HOST,
                    ..
                }
            ))
    );
    assert_eq!(engine.version(), version_before);
    assert_eq!(engine.root_recovery_anchor(SURFACE_HOST), Some(own_anchor));
    assert!(engine.root_recovery_anchor(SURFACE_CHILD).is_none());
    assert!(engine.pending_surface_recovery(SURFACE_CHILD).is_none());
}

#[test]
fn workspace_epoch_replacement_invalidates_old_recovery_anchor() {
    let mut engine = DockEngine::new(rooted_workspace(), DockPolicy::default())
        .expect("initial engine must validate");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("initial root registration must reduce");
    let stale_anchor = engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("initial root registration must issue an anchor");

    submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(parking_workspace()),
    )
    .expect("parking workspace replacement must reduce");
    assert!(engine.root_recovery_anchor(SURFACE_HOST).is_none());
    submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(restored_recovery_workspace()),
    )
    .expect("rooted workspace restoration must reduce");
    assert!(engine.root_recovery_anchor(SURFACE_RESTORED_HOST).is_none());
    assert!(engine.workspace().surface(SURFACE_HOST).is_none());
    assert!(engine.workspace().surface(SURFACE_RESTORED_HOST).is_some());

    let expected = engine.version();
    submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_RESTORED_HOST,
            token: WindowToken::new(401),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("restored root registration must reduce");
    let current_anchor = engine
        .root_recovery_anchor(SURFACE_RESTORED_HOST)
        .expect("restored root registration must issue a new anchor");
    assert_ne!(stale_anchor, current_anchor);

    let expected = engine.version();
    let rejected = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_RESTORED_CHILD,
            token: WindowToken::new(402),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                stale_anchor,
                ConvertedMainRecovery::new(
                    ROOT_RESTORED_CHILD_MAIN,
                    RECOVERY_RESTORED_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("stale-anchor registration must reduce to a rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: SURFACE_RESTORED_CHILD
        }
    ));
    assert!(engine.viewport().viewport(SURFACE_RESTORED_CHILD).is_none());

    let expected = engine.version();
    let registered = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_RESTORED_CHILD,
            token: WindowToken::new(403),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                current_anchor,
                ConvertedMainRecovery::new(
                    ROOT_RESTORED_CHILD_MAIN,
                    RECOVERY_RESTORED_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("current-anchor registration must reduce");
    assert!(matches!(
        registered.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == SURFACE_RESTORED_CHILD
    ));
    assert!(
        engine
            .root_recovery_anchor(SURFACE_RESTORED_CHILD)
            .is_none()
    );
}

#[test]
fn same_roster_workspace_replacement_preserves_root_binding_anchor_atomicity() {
    let mut engine = DockEngine::new(rooted_workspace(), DockPolicy::default())
        .expect("initial engine must validate");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let registered = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: HOST_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("initial root registration must reduce");
    let old_binding = match registered.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected root registration outcome: {outcome:?}"),
    };
    let old_anchor = engine
        .root_recovery_anchor(SURFACE_HOST)
        .expect("initial root registration must issue an anchor");

    let replaced = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(rooted_workspace()),
    )
    .expect("same-roster workspace replacement must reduce");
    let reconciliation = match replaced.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation,
        outcome => panic!("unexpected workspace replacement outcome: {outcome:?}"),
    };

    let current_anchor = if let Some(record) = engine.viewport().viewport(SURFACE_HOST) {
        let rebound_binding = record.binding();
        assert_ne!(rebound_binding, old_binding);
        assert!(
            reconciliation
                .rebound()
                .contains(&(old_binding, rebound_binding)),
            "retained root binding must be reported as rebound"
        );
        engine
            .root_recovery_anchor(SURFACE_HOST)
            .expect("rebound root binding must atomically receive a new recovery anchor")
    } else {
        assert!(
            reconciliation.retired().contains(&old_binding),
            "a non-rebound root binding must be explicitly retired"
        );
        let expected = engine.version();
        submit_input(
            &mut engine,
            &mut presentation_host,
            ROSTER_INPUT_SOURCE,
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SURFACE_HOST,
                token: WindowToken::new(601),
                role: ViewportRole::Root,
                recovery_target: None,
            },
        )
        .expect("retired root surface must permit a fresh registration");
        engine
            .root_recovery_anchor(SURFACE_HOST)
            .expect("fresh root registration must issue a new recovery anchor")
    };
    assert_ne!(current_anchor, old_anchor);

    let expected = engine.version();
    let rejected = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: WindowToken::new(602),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                old_anchor,
                ConvertedMainRecovery::new(
                    ROOT_CHILD_MAIN,
                    RECOVERY_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("old-anchor registration must reduce to a rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: SURFACE_CHILD
        }
    ));
    assert!(engine.viewport().viewport(SURFACE_CHILD).is_none());

    let expected = engine.version();
    let child = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: WindowToken::new(603),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                current_anchor,
                ConvertedMainRecovery::new(
                    ROOT_CHILD_MAIN,
                    RECOVERY_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("current-anchor registration must reduce");
    assert!(matches!(
        child.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistered { binding } if binding.surface() == SURFACE_CHILD
    ));
    assert!(engine.root_recovery_anchor(SURFACE_CHILD).is_none());
}

#[test]
fn a_surface_cannot_use_its_own_root_anchor_as_a_child_recovery_target() {
    let mut engine =
        DockEngine::new(rooted_workspace(), DockPolicy::default()).expect("engine must validate");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let root_registration = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: CHILD_TOKEN,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("root registration must reduce");
    let root_binding = match root_registration.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("unexpected root registration outcome: {outcome:?}"),
    };
    let self_anchor = engine
        .root_recovery_anchor(SURFACE_CHILD)
        .expect("root registration must issue an anchor");

    let expected = engine.version();
    let rejected = submit_input(
        &mut engine,
        &mut presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_CHILD,
            token: WindowToken::new(501),
            role: ViewportRole::Child,
            recovery_target: Some(SurfaceRecoveryTarget::with_converted_main(
                self_anchor,
                ConvertedMainRecovery::new(
                    ROOT_CHILD_MAIN,
                    RECOVERY_FLOATING,
                    LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid"),
                ),
            )),
        },
    )
    .expect("self-targeting registration must reduce to a rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::ViewportRegistrationRejected {
            surface: SURFACE_CHILD
        }
    ));
    let record = engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("rejection must preserve the existing root binding");
    assert_eq!(record.binding(), root_binding);
    assert_eq!(record.role(), ViewportRole::Root);
    assert_eq!(
        engine.root_recovery_anchor(SURFACE_CHILD),
        Some(self_anchor)
    );
}

#[test]
fn unplanned_child_destruction_rehomes_the_complete_rooted_roster() {
    let mut fixture = fixture(
        rooted_workspace(),
        vec![
            ROOT_CHILD_MAIN,
            ROOT_CHILD_FLOATING_A,
            ROOT_CHILD_FLOATING_B,
        ],
    );

    let destroyed = fixture.child_binding;
    fixture.publish_destroyed(destroyed, [TestWindow::Host]);

    let workspace = fixture.engine.workspace();
    assert!(workspace.surface(SURFACE_CHILD).is_none());
    assert_eq!(
        workspace.presentation_for_root(ROOT_CHILD_MAIN),
        Some(RootPresentationOwner::Contained {
            surface: SURFACE_HOST,
            floating: RECOVERY_FLOATING,
        })
    );
    for (root, floating) in [
        (ROOT_CHILD_FLOATING_A, FLOATING_A),
        (ROOT_CHILD_FLOATING_B, FLOATING_B),
    ] {
        assert_eq!(
            workspace.presentation_for_root(root),
            Some(RootPresentationOwner::Contained {
                surface: SURFACE_HOST,
                floating,
            })
        );
    }
    assert_eq!(
        workspace
            .surface(SURFACE_HOST)
            .expect("recovery host must survive")
            .contained,
        [FLOATING_HOST, RECOVERY_FLOATING, FLOATING_A, FLOATING_B]
    );
    assert_child_roots_preserved(&fixture);
    assert_eq!(workspace.item_multiset(), fixture.initial_items);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none()
    );
}

#[test]
fn unplanned_rootless_destruction_rehomes_all_contained_roots_without_a_main_root() {
    let mut fixture = fixture(
        rootless_workspace(),
        vec![ROOT_CHILD_FLOATING_A, ROOT_CHILD_FLOATING_B],
    );

    let destroyed = fixture.child_binding;
    fixture.publish_destroyed(destroyed, [TestWindow::Host]);

    let workspace = fixture.engine.workspace();
    assert!(workspace.surface(SURFACE_CHILD).is_none());
    assert!(workspace.contained_floating(RECOVERY_FLOATING).is_none());
    for (root, floating) in [
        (ROOT_CHILD_FLOATING_A, FLOATING_A),
        (ROOT_CHILD_FLOATING_B, FLOATING_B),
    ] {
        assert_eq!(
            workspace.presentation_for_root(root),
            Some(RootPresentationOwner::Contained {
                surface: SURFACE_HOST,
                floating,
            })
        );
    }
    assert_eq!(
        workspace
            .surface(SURFACE_HOST)
            .expect("rootless recovery host must survive")
            .contained,
        [FLOATING_HOST, FLOATING_A, FLOATING_B]
    );
    assert_child_roots_preserved(&fixture);
    assert_eq!(workspace.item_multiset(), fixture.initial_items);
}

#[test]
fn rehoming_one_rootless_contained_root_keeps_the_source_surface_live() {
    let mut fixture = fixture(
        rootless_workspace(),
        vec![ROOT_CHILD_FLOATING_A, ROOT_CHILD_FLOATING_B],
    );
    let source = fixture
        .engine
        .workspace()
        .capture_node_source(
            ROOT_CHILD_FLOATING_A,
            fixture
                .engine
                .workspace()
                .root(ROOT_CHILD_FLOATING_A)
                .expect("source root must exist")
                .node,
        )
        .expect("contained source must be current");
    let rect = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_A)
        .expect("contained presentation must exist")
        .rect;
    let expected = fixture.engine.version();
    submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::RehomeRoot {
                source,
                target: RootPresentationTarget::Contained {
                    surface: SURFACE_HOST,
                    floating: FLOATING_A,
                    rect,
                    position: ContainedPosition::Front,
                },
            },
        },
    )
    .expect("partial rehome must reduce");

    assert_eq!(
        fixture
            .engine
            .workspace()
            .surface(SURFACE_CHILD)
            .expect("remaining contained root keeps its rootless surface live")
            .contained,
        [FLOATING_B]
    );
    assert_eq!(
        fixture
            .engine
            .workspace()
            .surface(SURFACE_HOST)
            .expect("target host must survive")
            .contained,
        [FLOATING_HOST, FLOATING_A]
    );
    assert!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .is_some_and(|record| record.binding() == fixture.child_binding)
    );
}

#[test]
fn coordinate_gap_child_destruction_retains_trusted_geometry_in_the_pending_roster() {
    let mut fixture = fixture(
        rooted_workspace(),
        vec![
            ROOT_CHILD_MAIN,
            ROOT_CHILD_FLOATING_A,
            ROOT_CHILD_FLOATING_B,
        ],
    );
    let before = fixture.engine.workspace().clone();

    fixture.publish([TestWindow::Host, TestWindow::ChildUnavailable]);
    let destroyed = fixture.child_binding;
    fixture.publish_destroyed(destroyed, [TestWindow::Host]);

    assert_eq!(fixture.engine.workspace(), &before);
    let pending = fixture
        .engine
        .pending_surface_recovery(SURFACE_CHILD)
        .expect("unmeasured source must retain a semantic pending roster");
    assert_eq!(pending.main_root(), Some(ROOT_CHILD_MAIN));
    assert_eq!(pending.contained().len(), 2);
    assert!(
        pending.source_geometry_available(),
        "current coordinate authority is unavailable, but destruction recovery must retain the last exact-binding placement anchor"
    );
}

#[test]
fn unplanned_root_destruction_only_unbinds_runtime_and_allows_reregistration() {
    let mut fixture = fixture(
        rooted_workspace(),
        vec![
            ROOT_CHILD_MAIN,
            ROOT_CHILD_FLOATING_A,
            ROOT_CHILD_FLOATING_B,
        ],
    );
    let before = fixture.engine.workspace().clone();
    let old_binding = fixture.host_binding;

    let destroyed = fixture.host_binding;
    fixture.publish_destroyed(destroyed, [TestWindow::Child]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());

    let replacement_token = WindowToken::new(402);
    let expected = fixture.engine.version();
    let provider = fixture.presentation_host.platform_provider();
    submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE_HOST,
            token: replacement_token,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("root re-registration must reduce");
    fixture.host_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("root surface must accept a new binding")
        .binding();
    assert_eq!(fixture.host_binding.token(), replacement_token);
    assert_ne!(fixture.host_binding, old_binding);

    fixture.publish([TestWindow::Host, TestWindow::Child]);
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn stale_contained_roster_is_rejected_without_a_partial_workspace_publication() {
    let mut fixture = fixture(
        rooted_workspace(),
        vec![
            ROOT_CHILD_MAIN,
            ROOT_CHILD_FLOATING_A,
            ROOT_CHILD_FLOATING_B,
        ],
    );
    let stale_roster = fixture
        .engine
        .workspace()
        .capture_contained_roster(SURFACE_CHILD)
        .expect("initial roster must be capturable");
    let stale_source = fixture
        .engine
        .workspace()
        .capture_node_source(
            ROOT_CHILD_FLOATING_B,
            fixture
                .engine
                .workspace()
                .root(ROOT_CHILD_FLOATING_B)
                .expect("stale source root must exist")
                .node,
        )
        .expect("stale source must be capturable");
    let fresh_roster = fixture
        .engine
        .workspace()
        .capture_contained_roster(SURFACE_CHILD)
        .expect("fresh roster must be capturable");
    let fresh_source = fixture
        .engine
        .workspace()
        .capture_node_source(
            ROOT_CHILD_FLOATING_A,
            fixture
                .engine
                .workspace()
                .root(ROOT_CHILD_FLOATING_A)
                .expect("fresh source root must exist")
                .node,
        )
        .expect("fresh source must be capturable");
    let expected = fixture.engine.version();
    submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::RaiseContained {
                source: fresh_source,
                floating: FLOATING_A,
                expected_roster: fresh_roster,
            },
        },
    )
    .expect("fresh raise must reduce");

    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let rejected = submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        ROSTER_INPUT_SOURCE,
        EngineInput::WorkspaceCommand {
            expected: version,
            command: WorkspaceCommand::RaiseContained {
                source: stale_source,
                floating: FLOATING_B,
                expected_roster: stale_roster,
            },
        },
    )
    .expect("stale raise must reduce as a typed rejection");

    assert!(matches!(
        rejected.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::CommandRejected {
                    error: CommandError::StaleContainedRoster {
                        surface: SURFACE_CHILD,
                        ..
                    },
                    ..
                }
            )
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
    assert!(rejected.events().is_empty());
}
