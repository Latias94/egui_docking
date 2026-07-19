use std::collections::BTreeMap;

use dockspace::command::{DockTarget, MovePayload, RootPresentationTarget, WorkspaceCommand};
use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectResult, PlatformEffect,
};
use dockspace::engine::DockEngine;
use dockspace::error::CommandError;
use dockspace::frame::{
    PanelFocus, ViewportCloseDecision, ViewportCloseDecisionRejection, ViewportClosePlan,
    ViewportCloseRequestId, ViewportMergeBackPlan,
};
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason, ContainedTearOffProposal};
use dockspace::platform::{
    InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, WindowInputObservation, WindowInputState,
    WindowPresentationState,
};
use dockspace::policy::DockPolicy;
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{
    InputObservationGeneration, ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{
    ActivationStartOutcome, FocusObservationEnvelope, FocusObservationGeneration,
    GlobalFocusedWindow, PaneFocusObservation, PaneFocusObservationGeneration,
    ViewportActivationCause, unknown_focus_observation,
};

const ROOT_HOST: RootId = RootId::new(1);
const ROOT_CHILD_MAIN: RootId = RootId::new(2);
const ROOT_CHILD_FLOATING_A: RootId = RootId::new(3);
const ROOT_CHILD_FLOATING_B: RootId = RootId::new(4);
const ROOT_HOST_FLOATING: RootId = RootId::new(5);
const ROOT_COLLISION: RootId = RootId::new(6);
const ROOT_UNRELATED: RootId = RootId::new(7);

const SURFACE_HOST: SurfaceId = SurfaceId::new(1);
const SURFACE_CHILD: SurfaceId = SurfaceId::new(2);
const SURFACE_UNRELATED: SurfaceId = SurfaceId::new(3);

const FLOATING_HOST: FloatingPresentationId = FloatingPresentationId::new(10);
const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(11);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(12);
const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const FRESH_RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(21);

const HOST_TOKEN: WindowToken = WindowToken::new(101);
const CHILD_TOKEN: WindowToken = WindowToken::new(102);
const FRESH_CHILD_TOKEN: WindowToken = WindowToken::new(103);
const FRESH_HOST_TOKEN: WindowToken = WindowToken::new(104);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(201);

const MAIN_ITEMS: [ItemId; 3] = [ItemId::new(10), ItemId::new(11), ItemId::new(12)];

#[derive(Debug, Clone, PartialEq)]
struct RootSnapshot {
    root: RootId,
    record: RootRecord,
    nodes: Vec<(NodeId, Node)>,
    items: BTreeMap<ItemId, usize>,
    selections: Vec<(NodeId, Option<ItemId>)>,
}

#[derive(Debug, Clone, PartialEq)]
struct ContainedSnapshot {
    floating: FloatingPresentationId,
    root: RootSnapshot,
    rect: LogicalRect,
    z_order: u64,
}

#[derive(Debug, Clone, PartialEq)]
struct SurfaceRosterSnapshot {
    main: RootSnapshot,
    contained: BTreeMap<FloatingPresentationId, ContainedSnapshot>,
    items: BTreeMap<ItemId, usize>,
}

#[derive(Debug, Clone, Copy)]
struct WindowGeometry {
    content: PhysicalRect,
    outer: PhysicalRect,
    scale: ScaleFactor,
}

struct Fixture {
    engine: DockEngine,
    host_binding: ViewportBinding,
    child_binding: ViewportBinding,
    next_input_generation: InputObservationGeneration,
    host_geometry: WindowGeometry,
    child_geometry: WindowGeometry,
    child_main_left: NodeId,
    recovery: ContainedTearOffProposal,
    child_roster: SurfaceRosterSnapshot,
    initial_items: BTreeMap<ItemId, usize>,
}

#[derive(Debug, Clone, Copy)]
enum TestWindow {
    Host,
    HostClosing,
    HostUnavailable,
    Child { close_requested: bool },
    ChildUnavailable { close_requested: bool },
    Replacement(ViewportBinding),
}

impl Fixture {
    fn publish_windows(
        &mut self,
        windows: impl IntoIterator<Item = TestWindow>,
    ) -> EngineTransition {
        let observations = self.collect_windows(windows);
        publish_windows(&mut self.engine, observations)
    }

    fn publish_windows_with_global_focus(
        &mut self,
        windows: impl IntoIterator<Item = TestWindow>,
        focused: GlobalFocusedWindow,
    ) -> EngineTransition {
        let observations = self.collect_windows(windows);
        publish_windows_with_global_focus(&mut self.engine, observations, focused)
    }

    fn collect_windows(
        &mut self,
        windows: impl IntoIterator<Item = TestWindow>,
    ) -> Vec<ObservedWindow> {
        let mut observations = Vec::new();
        for window in windows {
            let observation = match window {
                TestWindow::Host => {
                    let generation = self.take_input_generation();
                    ready_window(self.host_binding, generation, self.host_geometry, false)
                }
                TestWindow::HostClosing => {
                    let generation = self.take_input_generation();
                    ready_window(self.host_binding, generation, self.host_geometry, true)
                }
                TestWindow::HostUnavailable => ObservedWindow::new(self.host_binding.token())
                    .with_close_requested(Authority::Known(false)),
                TestWindow::Child { close_requested } => {
                    let generation = self.take_input_generation();
                    ready_window(
                        self.child_binding,
                        generation,
                        self.child_geometry,
                        close_requested,
                    )
                }
                TestWindow::ChildUnavailable { close_requested } => {
                    ObservedWindow::new(self.child_binding.token())
                        .with_close_requested(Authority::Known(close_requested))
                }
                TestWindow::Replacement(binding) => {
                    let generation = self.take_input_generation();
                    ready_window(binding, generation, self.child_geometry, false)
                }
            };
            observations.push(observation);
        }
        observations
    }

    fn take_input_generation(&mut self) -> InputObservationGeneration {
        let generation = self.next_input_generation;
        self.next_input_generation = generation
            .checked_next()
            .expect("test input observation generation must not exhaust");
        generation
    }
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn merge_items(target: &mut BTreeMap<ItemId, usize>, source: &BTreeMap<ItemId, usize>) {
    for (item, count) in source {
        *target.entry(*item).or_default() += count;
    }
}

fn root_snapshot(workspace: &Workspace, root: RootId) -> RootSnapshot {
    let record = *workspace
        .root(root)
        .expect("snapshot root must exist in the workspace");
    let mut nodes = Vec::new();
    let mut items = BTreeMap::new();
    let mut selections = Vec::new();
    let mut stack = vec![record.node];

    while let Some(node_id) = stack.pop() {
        let node = workspace
            .node(node_id)
            .expect("every snapshot node must remain reachable")
            .clone();
        match &node {
            Node::Tabs {
                items: tab_items,
                selected,
            } => {
                for item in tab_items {
                    *items.entry(*item).or_default() += 1;
                }
                selections.push((node_id, *selected));
            }
            Node::Split { children, .. } => {
                stack.extend(children.iter().rev().copied());
            }
        }
        nodes.push((node_id, node));
    }

    RootSnapshot {
        root,
        record,
        nodes,
        items,
        selections,
    }
}

fn surface_roster_snapshot(workspace: &Workspace, surface: SurfaceId) -> SurfaceRosterSnapshot {
    let presentation = workspace
        .surface(surface)
        .expect("snapshot surface must exist");
    let main = root_snapshot(workspace, presentation.main_root);
    let mut items = main.items.clone();
    let contained = presentation
        .contained
        .iter()
        .copied()
        .map(|floating| {
            let record = workspace
                .contained_floating(floating)
                .expect("surface roster must reference an existing contained presentation");
            let root = root_snapshot(workspace, record.root);
            merge_items(&mut items, &root.items);
            (
                floating,
                ContainedSnapshot {
                    floating,
                    root,
                    rect: record.rect,
                    z_order: record.z_order,
                },
            )
        })
        .collect();

    SurfaceRosterSnapshot {
        main,
        contained,
        items,
    }
}

#[allow(clippy::too_many_lines)]
fn workspace(with_recovery_collision: bool, host_z_order: u64) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();

    let host_tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(1), ItemId::new(2)],
        Some(ItemId::new(2)),
    ));

    let child_main_left =
        builder.insert_node(Node::tabs_with_selection(MAIN_ITEMS, Some(ItemId::new(11))));

    let floating_a_tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(20), ItemId::new(21)],
        Some(ItemId::new(21)),
    ));

    let floating_b_top = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(30), ItemId::new(31)],
        Some(ItemId::new(31)),
    ));
    let floating_b_bottom = builder.insert_node(Node::tabs([ItemId::new(32)]));
    let floating_b_split = builder.insert_node(
        Node::split(
            Axis::Vertical,
            [floating_b_top, floating_b_bottom],
            [0.4, 0.6],
        )
        .expect("contained split must be valid"),
    );
    let host_floating_tabs = builder.insert_node(Node::tabs([ItemId::new(90)]));
    let unrelated_tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(100), ItemId::new(101)],
        Some(ItemId::new(101)),
    ));

    builder.set_root(
        ROOT_HOST,
        RootRecord::new(host_tabs).with_central(host_tabs),
    );
    builder.set_root(
        ROOT_CHILD_MAIN,
        RootRecord::new(child_main_left).with_central(child_main_left),
    );
    builder.set_root(
        ROOT_CHILD_FLOATING_A,
        RootRecord::new(floating_a_tabs).with_central(floating_a_tabs),
    );
    builder.set_root(
        ROOT_CHILD_FLOATING_B,
        RootRecord::new(floating_b_split).with_central(floating_b_bottom),
    );
    builder.set_root(
        ROOT_HOST_FLOATING,
        RootRecord::new(host_floating_tabs).with_central(host_floating_tabs),
    );
    builder.set_root(
        ROOT_UNRELATED,
        RootRecord::new(unrelated_tabs).with_central(unrelated_tabs),
    );
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    builder.set_surface(SURFACE_CHILD, SurfacePresentation::new(ROOT_CHILD_MAIN));
    builder.set_surface(SURFACE_UNRELATED, SurfacePresentation::new(ROOT_UNRELATED));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_HOST,
        ROOT_HOST_FLOATING,
        SURFACE_HOST,
        logical_rect(120.0, 140.0, 240.0, 160.0),
        host_z_order,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_A,
        ROOT_CHILD_FLOATING_A,
        SURFACE_CHILD,
        logical_rect(24.0, 32.0, 320.0, 240.0),
        4,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_B,
        ROOT_CHILD_FLOATING_B,
        SURFACE_CHILD,
        logical_rect(380.0, 96.0, 420.0, 310.0),
        9,
    ));
    builder
        .attach_contained(SURFACE_HOST, FLOATING_HOST)
        .expect("host surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_B)
        .expect("child surface must exist");
    builder
        .attach_contained(SURFACE_CHILD, FLOATING_A)
        .expect("child surface must exist");

    if with_recovery_collision {
        let collision_tabs = builder.insert_node(Node::tabs([ItemId::new(91)]));
        builder.set_root(ROOT_COLLISION, RootRecord::new(collision_tabs));
        builder.set_contained_floating(ContainedFloating::new(
            RECOVERY_FLOATING,
            ROOT_COLLISION,
            SURFACE_HOST,
            logical_rect(80.0, 100.0, 260.0, 180.0),
            15,
        ));
        builder
            .attach_contained(SURFACE_HOST, RECOVERY_FLOATING)
            .expect("host surface must exist");
    }

    (
        builder.build().expect("test workspace must be valid"),
        child_main_left,
    )
}

fn workspace_with_changed_child_contract() -> Workspace {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(200)]));
    let child_tabs = builder.insert_node(Node::tabs([ItemId::new(201), ItemId::new(202)]));
    let unrelated_tabs = builder.insert_node(Node::tabs([ItemId::new(203)]));
    builder.set_root(
        ROOT_HOST,
        RootRecord::new(host_tabs).with_central(host_tabs),
    );
    builder.set_root(
        ROOT_COLLISION,
        RootRecord::new(child_tabs).with_central(child_tabs),
    );
    builder.set_root(
        ROOT_UNRELATED,
        RootRecord::new(unrelated_tabs).with_central(unrelated_tabs),
    );
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    builder.set_surface(SURFACE_CHILD, SurfacePresentation::new(ROOT_COLLISION));
    builder.set_surface(SURFACE_UNRELATED, SurfacePresentation::new(ROOT_UNRELATED));
    builder
        .build()
        .expect("changed child recovery contract must remain a valid workspace")
}

fn register_fresh_recovery_contract(fixture: &mut Fixture) -> ViewportBinding {
    fixture.host_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("retained host must remain registered")
        .binding();
    publish_scene(&mut fixture.engine);
    let placement = fixture
        .engine
        .contained_placement(
            SURFACE_UNRELATED,
            logical_rect(80.0, 90.0, 500.0, 360.0),
            LogicalSize::new(0.0, 0.0).expect("fresh recovery minimum must be valid"),
        )
        .expect("new recovery host scene must authorize a fresh placement");
    let fresh_recovery =
        ContainedTearOffProposal::new(ROOT_COLLISION, FRESH_RECOVERY_FLOATING, placement, 23);
    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_UNRELATED,
            FRESH_HOST_TOKEN,
            ViewportRole::Root,
            None,
        )
        .expect("fresh recovery host registration must enqueue");
    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_CHILD,
            FRESH_CHILD_TOKEN,
            ViewportRole::Child,
            Some(fresh_recovery),
        )
        .expect("fresh child recovery contract must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("fresh recovery host and child contract must register");
    let fresh_host_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_UNRELATED)
        .expect("fresh recovery host must be registered")
        .binding();
    fixture.child_binding = fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("fresh child binding must be registered")
        .binding();
    assert_eq!(fixture.child_binding.token(), FRESH_CHILD_TOKEN);
    fresh_host_binding
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
    generation: InputObservationGeneration,
    geometry: WindowGeometry,
    close_requested: bool,
) -> ObservedWindow {
    ObservedWindow::new(binding.token())
        .with_content_bounds(Authority::Known(geometry.content))
        .with_outer_bounds(Authority::Known(geometry.outer))
        .with_scale_factor(Authority::Known(geometry.scale))
        .with_input_observation(WindowInputObservation::new(
            binding,
            generation,
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
        .with_presentation(Authority::Known(WindowPresentationState::Visible))
        .with_close_requested(Authority::Known(close_requested))
}

fn platform_snapshot(engine: &DockEngine, windows: Vec<ObservedWindow>) -> PlatformSnapshot {
    let focus_generation = engine
        .viewport()
        .registry()
        .inventory_generation()
        .checked_next()
        .expect("test focus observation generation must not exhaust");
    PlatformSnapshot::new(
        platform_capabilities(),
        unknown_focus_observation(
            FocusObservationGeneration::new(focus_generation.get()),
            AuthorityUnavailableReason::NotReported,
        ),
        windows,
        Vec::new(),
        vec![ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1920.0, -200.0, 3840.0, 1600.0),
            ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
        )],
    )
    .expect("test platform snapshot must be canonical")
}

fn platform_snapshot_with_global_focus(
    engine: &DockEngine,
    windows: Vec<ObservedWindow>,
    focused: GlobalFocusedWindow,
) -> PlatformSnapshot {
    let focus_generation = engine
        .viewport()
        .registry()
        .inventory_generation()
        .checked_next()
        .expect("test focus observation generation must not exhaust");
    PlatformSnapshot::new(
        platform_capabilities(),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(focus_generation.get()),
            Authority::Known(focused),
            Authority::Known(None),
        ),
        windows,
        Vec::new(),
        vec![ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1920.0, -200.0, 3840.0, 1600.0),
            ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
        )],
    )
    .expect("test focused platform snapshot must be canonical")
}

fn publish_windows(engine: &mut DockEngine, windows: Vec<ObservedWindow>) -> EngineTransition {
    let snapshot = platform_snapshot(engine, windows);
    engine
        .enqueue_platform_snapshot(snapshot)
        .expect("platform snapshot must enqueue");
    engine
        .reduce_pending()
        .expect("platform snapshot must reduce")
}

fn publish_windows_with_global_focus(
    engine: &mut DockEngine,
    windows: Vec<ObservedWindow>,
    focused: GlobalFocusedWindow,
) -> EngineTransition {
    let snapshot = platform_snapshot_with_global_focus(engine, windows, focused);
    engine
        .enqueue_platform_snapshot(snapshot)
        .expect("focused platform snapshot must enqueue");
    engine
        .reduce_pending()
        .expect("focused platform snapshot must reduce")
}

fn publish_scene(engine: &mut DockEngine) {
    publish_scene_with_host_bounds(engine, logical_rect(0.0, 0.0, 1000.0, 800.0));
}

fn publish_scene_with_host_bounds(engine: &mut DockEngine, host_bounds: LogicalRect) {
    let surfaces: Vec<_> = engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    let mut scene =
        BuildingScene::new(surfaces.iter().copied()).expect("surface roster must be unique");
    for surface in surfaces {
        let bounds = if surface == SURFACE_HOST {
            host_bounds
        } else {
            logical_rect(0.0, 0.0, 1000.0, 800.0)
        };
        scene
            .insert_ready(ReadySurfaceScene::new(surface, bounds))
            .expect("surface scene must be unique");
    }
    engine.enqueue_scene(scene).expect("scene must enqueue");
    engine.reduce_pending().expect("scene must publish");
}

fn publish_bootstrap_scene(engine: &mut DockEngine) {
    let surfaces: Vec<_> = engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    let scene = BuildingScene::new(surfaces).expect("surface roster must be unique");
    engine.enqueue_scene(scene).expect("scene must enqueue");
    engine
        .reduce_pending()
        .expect("bootstrap scene must publish");
}

fn fixture(with_recovery_collision: bool) -> Fixture {
    fixture_with_geometry(
        with_recovery_collision,
        100,
        WindowGeometry {
            content: physical_rect(0.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(-8.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
        },
        WindowGeometry {
            content: physical_rect(1000.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(992.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("child scale factor must be valid"),
        },
    )
}

fn fixture_with_geometry(
    with_recovery_collision: bool,
    host_z_order: u64,
    host_geometry: WindowGeometry,
    child_geometry: WindowGeometry,
) -> Fixture {
    let (workspace, child_main_left) = workspace(with_recovery_collision, host_z_order);
    let child_roster = surface_roster_snapshot(&workspace, SURFACE_CHILD);
    let initial_items = workspace.item_multiset();
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");
    publish_scene(&mut engine);
    let placement = engine
        .contained_placement(
            SURFACE_HOST,
            logical_rect(60.0, 70.0, 620.0, 460.0),
            LogicalSize::new(0.0, 0.0).expect("minimum size must be valid"),
        )
        .expect("host scene must authorize recovery placement");
    let recovery = ContainedTearOffProposal::new(ROOT_CHILD_MAIN, RECOVERY_FLOATING, placement, 17);

    engine
        .enqueue_viewport_registration(SURFACE_HOST, HOST_TOKEN, ViewportRole::Root, None)
        .expect("host viewport registration must enqueue");
    engine
        .enqueue_viewport_registration(
            SURFACE_CHILD,
            CHILD_TOKEN,
            ViewportRole::Child,
            Some(recovery),
        )
        .expect("child viewport registration must enqueue");
    engine
        .reduce_pending()
        .expect("viewport registrations must reduce");
    let host_binding = engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("host viewport must be registered")
        .binding();
    let child_binding = engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("child viewport must be registered")
        .binding();
    let mut fixture = Fixture {
        engine,
        host_binding,
        child_binding,
        next_input_generation: InputObservationGeneration::new(1),
        host_geometry,
        child_geometry,
        child_main_left,
        recovery,
        child_roster,
        initial_items,
    };
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: false,
        },
    ]);
    publish_scene(&mut fixture.engine);
    fixture
}

fn close_request_from(transition: &EngineTransition) -> ViewportCloseRequestId {
    transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { transition, .. } => {
                transition.close_requests().first().copied()
            }
            _ => None,
        })
        .expect("snapshot must publish one child close request")
}

fn merge_back_plan(fixture: &Fixture) -> ViewportClosePlan {
    let host = fixture
        .engine
        .workspace()
        .surface(SURFACE_HOST)
        .expect("merge host surface must exist");
    let tabs = fixture
        .engine
        .workspace()
        .root(host.main_root)
        .expect("merge host main root must exist")
        .node;
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(host.main_root, tabs)
        .expect("merge host must expose one current tabs target");
    ViewportClosePlan::merge_back(ViewportMergeBackPlan::new(SURFACE_HOST, target))
}

fn select_child_item(fixture: &mut Fixture, item: ItemId) {
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_CHILD_MAIN, fixture.child_main_left, item)
        .expect("child item source must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source })
        .expect("child selection must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("child selection must reduce");
    publish_scene(&mut fixture.engine);
}

fn selected_item_in_tabs_containing(workspace: &Workspace, item: ItemId) -> Option<ItemId> {
    workspace.nodes().find_map(|(_, node)| match node {
        Node::Tabs { items, selected } if items.contains(&item) => *selected,
        Node::Tabs { .. } | Node::Split { .. } => None,
    })
}

fn accept_close(fixture: &mut Fixture) -> (ViewportCloseRequestId, EffectId) {
    let close = fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let request = close_request_from(&close);
    let release = accept_close_request(fixture, request);
    (request, release)
}

fn accept_close_request(fixture: &mut Fixture, request: ViewportCloseRequestId) -> EffectId {
    let plan = merge_back_plan(fixture);
    fixture
        .engine
        .enqueue_viewport_close_decision(request, ViewportCloseDecision::Accept(plan))
        .expect("close decision must enqueue");
    let decided = fixture
        .engine
        .reduce_pending()
        .expect("close decision must reduce");
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .viewport_close_request(request)
            .and_then(|request| request.plan()),
        Some(ViewportClosePlan::MergeBack(_))
    ));
    let releases: Vec<_> = decided
        .platform_effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == fixture.child_binding
            )
        })
        .collect();
    assert_eq!(
        releases.len(),
        1,
        "accepted child close must request exactly one release"
    );
    releases[0].id()
}

fn request_replacement(transition: &EngineTransition) -> (EffectId, ViewportBinding) {
    let replacements: Vec<_> = transition
        .platform_effects()
        .iter()
        .filter_map(|request| match request.effect() {
            PlatformEffect::RequestReplacement { binding, .. } => Some((request.id(), *binding)),
            _ => None,
        })
        .collect();
    assert_eq!(replacements.len(), 1, "expected one replacement request");
    replacements[0]
}

fn assert_root_unchanged(workspace: &Workspace, expected: &RootSnapshot) {
    assert_eq!(root_snapshot(workspace, expected.root), *expected);
}

fn projected_sibling_rect(fixture: &Fixture, source: LogicalRect) -> LogicalRect {
    let desktop = source
        .to_desktop_physical(
            fixture.child_geometry.content.min(),
            fixture.child_geometry.scale,
        )
        .expect("source sibling projection must be representable");
    let requested = desktop
        .to_target_logical(
            fixture.host_geometry.content.min(),
            fixture.host_geometry.scale,
        )
        .expect("target sibling projection must be representable");
    let bounds = logical_rect(0.0, 0.0, 1000.0, 800.0);
    let width = requested.width().min(bounds.width());
    let height = requested.height().min(bounds.height());
    logical_rect(
        requested.x().clamp(bounds.x(), bounds.max().x() - width),
        requested.y().clamp(bounds.y(), bounds.max().y() - height),
        width,
        height,
    )
}

fn projected_main_rect(fixture: &Fixture) -> LogicalRect {
    let requested = fixture
        .child_geometry
        .outer
        .to_target_logical(
            fixture.host_geometry.content.min(),
            fixture.host_geometry.scale,
        )
        .expect("main recovery projection must be representable");
    let bounds = logical_rect(0.0, 0.0, 1000.0, 800.0);
    let width = requested.width().min(bounds.width());
    let height = requested.height().min(bounds.height());
    logical_rect(
        requested.x().clamp(bounds.x(), bounds.max().x() - width),
        requested.y().clamp(bounds.y(), bounds.max().y() - height),
        width,
        height,
    )
}

fn assert_contained_siblings_rehomed(
    fixture: &Fixture,
    expected: &SurfaceRosterSnapshot,
    leading_z_order: u64,
    trailing_z_order: u64,
) {
    let workspace = fixture.engine.workspace();
    for (floating, snapshot) in &expected.contained {
        let actual = workspace
            .contained_floating(*floating)
            .expect("every contained sibling must survive recovery");
        assert_eq!(actual.id, snapshot.floating);
        assert_eq!(actual.root, snapshot.root.root);
        assert_eq!(actual.surface, SURFACE_HOST);
        assert_eq!(actual.rect, projected_sibling_rect(fixture, snapshot.rect));
        assert_eq!(
            actual.z_order,
            match *floating {
                FLOATING_A => leading_z_order,
                FLOATING_B => trailing_z_order,
                _ => panic!("unexpected recovered child floating {floating:?}"),
            }
        );
        assert_root_unchanged(workspace, &snapshot.root);
    }
}

fn assert_complete_roster_merged_back(
    fixture: &Fixture,
    expected_roster: &SurfaceRosterSnapshot,
    expected_items: &BTreeMap<ItemId, usize>,
) {
    let workspace = fixture.engine.workspace();
    assert!(
        workspace.surface(SURFACE_CHILD).is_none(),
        "destroyed child surface must be removed after its full roster is rehomed"
    );
    assert!(
        workspace.root(expected_roster.main.root).is_none(),
        "source main tabs must be consumed by the host tabs target"
    );
    let host = workspace
        .surface(SURFACE_HOST)
        .expect("merge host surface must survive");
    let host_items = root_snapshot(workspace, host.main_root).items;
    for item in expected_roster.main.items.keys() {
        assert!(
            host_items.contains_key(item),
            "merged host must contain {item:?}"
        );
    }
    assert_contained_siblings_rehomed(fixture, expected_roster, 101, 102);
    assert_eq!(workspace.item_multiset(), *expected_items);
}

fn assert_complete_roster_rehomed(
    fixture: &Fixture,
    expected_roster: &SurfaceRosterSnapshot,
    expected_items: &BTreeMap<ItemId, usize>,
) {
    let workspace = fixture.engine.workspace();
    assert!(workspace.surface(SURFACE_CHILD).is_none());
    let recovered_main = workspace
        .contained_floating(RECOVERY_FLOATING)
        .expect("direct recovery must preserve the child main root");
    assert_eq!(recovered_main.root, expected_roster.main.root);
    assert_eq!(recovered_main.surface, SURFACE_HOST);
    assert_eq!(recovered_main.rect, projected_main_rect(fixture));
    assert_eq!(recovered_main.z_order, 101);
    assert_root_unchanged(workspace, &expected_roster.main);
    assert_contained_siblings_rehomed(fixture, expected_roster, 102, 103);
    assert_eq!(workspace.item_multiset(), *expected_items);
}

fn assert_frozen_roster_rejects(fixture: &mut Fixture, command: WorkspaceCommand) {
    let before = fixture.engine.workspace().clone();
    fixture
        .engine
        .enqueue_command(command)
        .expect("frozen-roster command must enqueue");
    let rejected = fixture
        .engine
        .reduce_pending()
        .expect("frozen-roster command must reduce as a typed rejection");
    assert!(matches!(
        rejected.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::CommandRejected {
                    error: CommandError::SurfaceLifecycleFrozen { surface },
                    ..
                } if *surface == SURFACE_CHILD
            )
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn accepted_close_freezes_every_presentation_fact_until_terminal_destruction() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);

    let floating_a = *fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_A)
        .expect("first contained presentation must exist");
    assert_frozen_roster_rejects(
        &mut fixture,
        WorkspaceCommand::UpdateContainedRect {
            surface: SURFACE_CHILD,
            root: floating_a.root,
            floating: FLOATING_A,
            expected_rect: floating_a.rect,
            rect: logical_rect(40.0, 48.0, 360.0, 260.0),
        },
    );

    let source = fixture
        .engine
        .workspace()
        .capture_node_source(
            ROOT_CHILD_FLOATING_A,
            fixture.child_roster.contained[&FLOATING_A].root.record.node,
        )
        .expect("contained root source must remain current");
    assert_frozen_roster_rejects(
        &mut fixture,
        WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: SURFACE_HOST,
                floating: FLOATING_A,
                rect: floating_a.rect,
                z_order: floating_a.z_order,
            },
        },
    );

    let source = fixture
        .engine
        .workspace()
        .capture_node_source(
            ROOT_CHILD_FLOATING_B,
            fixture.child_roster.contained[&FLOATING_B].root.record.node,
        )
        .expect("contained root close source must remain current");
    assert_frozen_roster_rejects(&mut fixture, WorkspaceCommand::CloseRoot { source });
}

#[test]
fn accepted_close_rejects_every_source_root_mutation_and_still_merges() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);

    let commands = {
        let workspace = fixture.engine.workspace();
        let main_target = workspace
            .capture_tab_target(ROOT_CHILD_MAIN, fixture.child_main_left)
            .expect("main tabs target must be current");
        let floating_a_tabs = workspace
            .root(ROOT_CHILD_FLOATING_A)
            .expect("first contained root must exist")
            .node;
        let floating_a_target = workspace
            .capture_tab_target(ROOT_CHILD_FLOATING_A, floating_a_tabs)
            .expect("contained tabs target must be current");
        let floating_b_split = workspace
            .root(ROOT_CHILD_FLOATING_B)
            .expect("split contained root must exist")
            .node;
        let floating_a = *workspace
            .contained_floating(FLOATING_A)
            .expect("contained placement must exist");

        vec![
            WorkspaceCommand::Open {
                item: ItemId::new(500),
                target: DockTarget::Center(main_target),
            },
            WorkspaceCommand::Close {
                source: workspace
                    .capture_item_source(ROOT_CHILD_MAIN, fixture.child_main_left, ItemId::new(12))
                    .expect("main item close source must be current"),
            },
            WorkspaceCommand::Select {
                source: workspace
                    .capture_item_source(ROOT_CHILD_MAIN, fixture.child_main_left, ItemId::new(10))
                    .expect("main item select source must be current"),
            },
            WorkspaceCommand::Reorder {
                source: workspace
                    .capture_item_source(ROOT_CHILD_MAIN, fixture.child_main_left, ItemId::new(10))
                    .expect("main item reorder source must be current"),
                insertion_index: 3,
            },
            WorkspaceCommand::Select {
                source: workspace
                    .capture_item_source(ROOT_CHILD_FLOATING_A, floating_a_tabs, ItemId::new(20))
                    .expect("contained item select source must be current"),
            },
            WorkspaceCommand::ResizeSplit {
                split: workspace
                    .capture_node_source(ROOT_CHILD_FLOATING_B, floating_b_split)
                    .expect("contained split source must be current"),
                weights: SplitWeight::normalize([0.6, 0.4])
                    .expect("test split weights must normalize"),
            },
            WorkspaceCommand::Move {
                payload: MovePayload::Item(
                    workspace
                        .capture_item_source(
                            ROOT_CHILD_MAIN,
                            fixture.child_main_left,
                            ItemId::new(10),
                        )
                        .expect("same-surface move source must be current"),
                ),
                target: DockTarget::Center(floating_a_target),
            },
            WorkspaceCommand::UpdateContainedRect {
                surface: SURFACE_CHILD,
                root: floating_a.root,
                floating: FLOATING_A,
                expected_rect: floating_a.rect,
                rect: logical_rect(40.0, 48.0, 360.0, 260.0),
            },
        ]
    };

    for command in commands {
        assert_frozen_roster_rejects(&mut fixture, command);
    }

    fixture.publish_windows([TestWindow::Host]);
    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn prevent_after_failed_accept_releases_the_main_root_fingerprint_barrier() {
    let mut fixture = fixture(false);
    let (request, release) = accept_close(&mut fixture);
    fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            release,
            fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        ))
        .expect("release failure must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("release failure must reduce");
    fixture
        .engine
        .enqueue_viewport_close_decision(request, ViewportCloseDecision::Prevent)
        .expect("failed accept must permit an explicit veto");
    fixture
        .engine
        .reduce_pending()
        .expect("veto must release the accepted disposition");

    let source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_CHILD_MAIN, fixture.child_main_left, ItemId::new(10))
        .expect("post-prevent main item source must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source })
        .expect("post-prevent root mutation must enqueue");
    let updated = fixture
        .engine
        .reduce_pending()
        .expect("post-prevent root mutation must reduce");
    assert!(matches!(
        updated.reduced_inputs(),
        [input] if matches!(input.outcome(), InputOutcome::CommandProcessed { changed: true, .. })
    ));
}

#[test]
fn workspace_restore_releases_the_contained_root_fingerprint_barrier() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);
    let restored = fixture.engine.workspace().clone();
    fixture
        .engine
        .enqueue_workspace_replacement(restored)
        .expect("workspace restore must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("workspace restore must clear old lifecycle state");

    let floating_tabs = fixture
        .engine
        .workspace()
        .root(ROOT_CHILD_FLOATING_A)
        .expect("restored contained root must exist")
        .node;
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_CHILD_FLOATING_A, floating_tabs, ItemId::new(20))
        .expect("post-restore contained item source must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source })
        .expect("post-restore contained mutation must enqueue");
    let updated = fixture
        .engine
        .reduce_pending()
        .expect("post-restore contained mutation must reduce");
    assert!(matches!(
        updated.reduced_inputs(),
        [input] if matches!(input.outcome(), InputOutcome::CommandProcessed { changed: true, .. })
    ));
}

#[test]
fn unrelated_surface_mutation_can_republish_scene_and_complete_merge_back() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);
    let unrelated_tabs = fixture
        .engine
        .workspace()
        .root(ROOT_UNRELATED)
        .expect("unrelated root must exist")
        .node;
    let selected = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_UNRELATED, unrelated_tabs, ItemId::new(100))
        .expect("unrelated item must be selectable");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source: selected })
        .expect("unrelated root command must enqueue");
    let selected = fixture
        .engine
        .reduce_pending()
        .expect("unrelated root command must remain allowed");
    assert!(matches!(
        selected.reduced_inputs(),
        [input] if matches!(input.outcome(), InputOutcome::CommandProcessed { changed: true, .. })
    ));
    publish_scene(&mut fixture.engine);
    fixture.publish_windows([TestWindow::Host]);

    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn target_resize_in_close_request_snapshot_cannot_freeze_old_scene_bounds() {
    let mut fixture = fixture(false);
    fixture.host_geometry = WindowGeometry {
        content: physical_rect(0.0, 0.0, 500.0, 400.0),
        outer: physical_rect(-8.0, -30.0, 516.0, 438.0),
        scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
    };
    let close = fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let request = close_request_from(&close);
    let plan = merge_back_plan(&fixture);
    fixture
        .engine
        .enqueue_viewport_close_decision(request, ViewportCloseDecision::Accept(plan))
        .expect("mixed-authority close decision must enqueue");

    let rejected = fixture
        .engine
        .reduce_pending()
        .expect("mixed coordinates and scene must reject deterministically");

    assert!(matches!(
        rejected.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::ViewportCloseDecisionRejected {
                    reason: ViewportCloseDecisionRejection::MergeBackTargetUnavailable {
                        surface
                    },
                    ..
                } if *surface == SURFACE_HOST
            )
    ));
    assert!(matches!(
        fixture
            .engine
            .scene()
            .and_then(|scene| scene.surface(SURFACE_HOST)),
        Some(dockspace::scene::SurfaceScene::Ready(_))
    ));
}

#[test]
fn scene_enqueued_after_new_coordinates_cannot_retroactively_authorize_them() {
    let mut fixture = fixture(false);
    fixture.host_geometry = WindowGeometry {
        content: physical_rect(0.0, 0.0, 500.0, 400.0),
        outer: physical_rect(-8.0, -30.0, 516.0, 438.0),
        scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
    };

    let generation = fixture.take_input_generation();
    let host = ready_window(
        fixture.host_binding,
        generation,
        fixture.host_geometry,
        false,
    );
    let snapshot = platform_snapshot(&fixture.engine, vec![host]);
    fixture
        .engine
        .enqueue_platform_snapshot(snapshot)
        .expect("new target coordinates must enqueue");

    let surfaces: Vec<_> = fixture
        .engine
        .workspace()
        .surfaces()
        .map(|(surface, _)| surface)
        .collect();
    let mut stale_scene =
        BuildingScene::new(surfaces.iter().copied()).expect("scene roster must be unique");
    for surface in surfaces {
        stale_scene
            .insert_ready(ReadySurfaceScene::new(
                surface,
                logical_rect(0.0, 0.0, 1000.0, 800.0),
            ))
            .expect("stale ready scene must be unique");
    }
    fixture
        .engine
        .enqueue_scene(stale_scene)
        .expect("scene measured from the published coordinates must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("mixed coordinate and scene inputs must reduce deterministically");

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some(),
        "a scene enqueued with g1 coordinates must not authorize recovery after g2 reduces first"
    );

    publish_scene_with_host_bounds(&mut fixture.engine, logical_rect(0.0, 0.0, 500.0, 400.0));
    fixture.publish_windows([TestWindow::Host]);

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_none());
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none(),
        "a scene enqueued after g2 becomes current must authorize the pending recovery"
    );
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        fixture.initial_items
    );
}

#[test]
fn matching_target_repaint_after_resize_restores_merge_authority() {
    let mut fixture = fixture(false);
    fixture.host_geometry = WindowGeometry {
        content: physical_rect(0.0, 0.0, 500.0, 400.0),
        outer: physical_rect(-8.0, -30.0, 516.0, 438.0),
        scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
    };
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: false,
        },
    ]);
    publish_scene_with_host_bounds(&mut fixture.engine, logical_rect(0.0, 0.0, 500.0, 400.0));
    let close = fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let request = close_request_from(&close);
    accept_close_request(&mut fixture, request);

    fixture.publish_windows([TestWindow::Host]);

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_none());
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none()
    );
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        fixture.initial_items
    );
}

#[test]
fn source_coordinate_change_does_not_stale_unchanged_target_scene_authority() {
    let mut fixture = fixture(false);
    fixture.child_geometry = WindowGeometry {
        content: physical_rect(1200.0, 80.0, 900.0, 700.0),
        outer: physical_rect(1192.0, 50.0, 916.0, 738.0),
        scale: ScaleFactor::new(1.0).expect("child scale factor must be valid"),
    };
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: false,
        },
    ]);
    let close = fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let request = close_request_from(&close);
    accept_close_request(&mut fixture, request);

    fixture.publish_windows([TestWindow::Host]);

    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn changed_target_tabs_keep_the_complete_roster_pending() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);
    let host_tabs = fixture
        .engine
        .workspace()
        .root(ROOT_HOST)
        .expect("host root must exist")
        .node;
    let selected = fixture
        .engine
        .workspace()
        .capture_item_source(ROOT_HOST, host_tabs, ItemId::new(1))
        .expect("host selection source must be current");
    fixture
        .engine
        .enqueue_command(WorkspaceCommand::Select { source: selected })
        .expect("target-root mutation must enqueue");
    let selected = fixture
        .engine
        .reduce_pending()
        .expect("target-root mutation is outside the source barrier");
    assert!(matches!(
        selected.reduced_inputs(),
        [input] if matches!(input.outcome(), InputOutcome::CommandProcessed { changed: true, .. })
    ));
    publish_scene(&mut fixture.engine);

    fixture.publish_windows([TestWindow::Host]);

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some(),
        "a changed target fingerprint must fail closed"
    );
}

#[test]
fn changed_target_scene_bounds_keep_the_complete_roster_pending() {
    let mut fixture = fixture(false);
    accept_close(&mut fixture);
    publish_scene_with_host_bounds(&mut fixture.engine, logical_rect(0.0, 0.0, 900.0, 700.0));

    fixture.publish_windows([TestWindow::Host]);

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some(),
        "changed target-local scene bounds must fail closed"
    );
}

#[test]
fn accepted_merge_back_atomically_moves_main_tabs_and_the_complete_floating_forest() {
    let mut fixture = fixture(false);
    let (request, _) = accept_close(&mut fixture);

    let merged = fixture.publish_windows([TestWindow::Host]);

    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
    let activation = merged
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { activations, .. } => activations.first(),
            _ => None,
        })
        .expect("successful merge-back must publish one recovery activation");
    let ActivationStartOutcome::ObserveOnlyRecorded { record } = activation.outcome() else {
        panic!("unknown global focus must retain recovery without raising the host");
    };
    assert_eq!(record.request().focus(), PanelFocus::None);
    assert_eq!(
        record.request().cause(),
        ViewportActivationCause::CloseRecovery { request }
    );
    assert!(
        !merged
            .platform_effects()
            .iter()
            .any(|effect| matches!(effect.effect(), PlatformEffect::RequestFocus { .. }))
    );
}

#[test]
fn merge_back_freezes_recorded_pane_focus_and_applies_it_without_raising_the_host() {
    let mut fixture = fixture(false);
    fixture
        .engine
        .enqueue_pane_focus_observation(PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            fixture.child_binding,
            PanelFocus::Item(ItemId::new(11)),
        ))
        .expect("pane focus observation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("pane focus observation must reduce");
    select_child_item(&mut fixture, ItemId::new(10));
    assert_eq!(
        selected_item_in_tabs_containing(fixture.engine.workspace(), ItemId::new(11)),
        Some(ItemId::new(10)),
        "the recorded pane focus must be hidden before merge-back"
    );

    let close = fixture.publish_windows_with_global_focus(
        [
            TestWindow::Host,
            TestWindow::Child {
                close_requested: true,
            },
        ],
        GlobalFocusedWindow::Dock(fixture.host_binding),
    );
    let request = close_request_from(&close);
    let _ = accept_close_request(&mut fixture, request);

    let merged = fixture.publish_windows_with_global_focus(
        [TestWindow::Host],
        GlobalFocusedWindow::Dock(fixture.host_binding),
    );
    assert_eq!(
        merged.after().revision().get(),
        merged.before().revision().get() + 1,
        "merge and pane selection must publish through one revision boundary"
    );
    assert!(
        merged
            .events()
            .iter()
            .all(|event| event.version() == merged.after()),
        "every merge and selection outcome must share the published version"
    );
    let activation = merged
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { activations, .. } => activations.first(),
            _ => None,
        })
        .expect("successful merge-back must publish one recovery activation");
    let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome() else {
        panic!(
            "already-focused host must receive the frozen pane focus immediately: {:?}",
            activation.outcome()
        );
    };
    assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(11)));
    assert_eq!(
        intent.cause(),
        Some(ViewportActivationCause::CloseRecovery { request })
    );
    assert!(
        !merged
            .platform_effects()
            .iter()
            .any(|effect| matches!(effect.effect(), PlatformEffect::RequestFocus { .. }))
    );
    assert_eq!(
        selected_item_in_tabs_containing(fixture.engine.workspace(), ItemId::new(11)),
        Some(ItemId::new(11)),
        "the frozen hidden item must be selected inside the merge candidate"
    );
    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn unfocused_merge_back_records_focus_without_preselecting_the_item() {
    let mut fixture = fixture(false);
    fixture
        .engine
        .enqueue_pane_focus_observation(PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            fixture.child_binding,
            PanelFocus::Item(ItemId::new(11)),
        ))
        .expect("pane focus observation must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("pane focus observation must reduce");
    select_child_item(&mut fixture, ItemId::new(10));

    let close = fixture.publish_windows_with_global_focus(
        [
            TestWindow::Host,
            TestWindow::Child {
                close_requested: true,
            },
        ],
        GlobalFocusedWindow::Foreign,
    );
    let request = close_request_from(&close);
    let _ = accept_close_request(&mut fixture, request);

    let merged =
        fixture.publish_windows_with_global_focus([TestWindow::Host], GlobalFocusedWindow::Foreign);
    let activation = merged
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { activations, .. } => activations.first(),
            _ => None,
        })
        .expect("successful merge-back must publish one recovery activation");
    let ActivationStartOutcome::ObserveOnlyRecorded { record } = activation.outcome() else {
        panic!(
            "unfocused host must retain pane focus without revealing it: {:?}",
            activation.outcome()
        );
    };
    assert_eq!(record.request().focus(), PanelFocus::Item(ItemId::new(11)));
    assert_eq!(
        selected_item_in_tabs_containing(fixture.engine.workspace(), ItemId::new(11)),
        Some(ItemId::new(10)),
        "observe-only recovery must preserve the selection produced by the merge"
    );
    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn accepted_merge_back_rejects_the_closing_surface_as_its_target() {
    let mut fixture = fixture(false);
    let close = fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let request = close_request_from(&close);
    let child = fixture
        .engine
        .workspace()
        .surface(SURFACE_CHILD)
        .expect("closing child surface must still exist");
    let child_tabs = fixture
        .engine
        .workspace()
        .root(child.main_root)
        .expect("closing child main root must still exist")
        .node;
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(child.main_root, child_tabs)
        .expect("closing child main root must be tabs");
    fixture
        .engine
        .enqueue_viewport_close_decision(
            request,
            ViewportCloseDecision::Accept(ViewportClosePlan::merge_back(
                ViewportMergeBackPlan::new(SURFACE_CHILD, target),
            )),
        )
        .expect("cyclic close decision must enqueue");

    let rejected = fixture
        .engine
        .reduce_pending()
        .expect("cyclic close decision must become a typed rejection");

    assert!(matches!(
        rejected.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::ViewportCloseDecisionRejected {
                    request: actual,
                    reason: ViewportCloseDecisionRejection::MergeBackTargetsClosingSurface {
                        surface
                    },
                    ..
                } if *actual == request && *surface == SURFACE_CHILD
            )
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
}

#[test]
fn accepted_merge_back_rejects_a_different_target_surface_that_is_closing() {
    let mut fixture = fixture(false);
    let close = fixture.publish_windows([
        TestWindow::HostClosing,
        TestWindow::Child {
            close_requested: true,
        },
    ]);
    let requests: BTreeMap<_, _> = close
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { transition, .. } => Some(
                transition
                    .close_requests()
                    .iter()
                    .map(|request| {
                        let surface = fixture
                            .engine
                            .viewport()
                            .viewport_close_request(*request)
                            .expect("close request must remain queryable")
                            .binding()
                            .surface();
                        (surface, *request)
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("both close requests must publish");
    let request = requests[&SURFACE_CHILD];
    let plan = merge_back_plan(&fixture);
    fixture
        .engine
        .enqueue_viewport_close_decision(request, ViewportCloseDecision::Accept(plan))
        .expect("merge decision must enqueue");

    let rejected = fixture
        .engine
        .reduce_pending()
        .expect("closing target must reject deterministically");

    assert!(matches!(
        rejected.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::ViewportCloseDecisionRejected {
                    reason: ViewportCloseDecisionRejection::MergeBackTargetsClosingSurface {
                        surface
                    },
                    ..
                } if *surface == SURFACE_HOST
            )
    ));
}

#[test]
fn unplanned_destruction_recovers_the_complete_surface_roster() {
    let mut fixture = fixture(false);

    fixture.publish_windows([TestWindow::Host]);

    assert_complete_roster_rehomed(&fixture, &fixture.child_roster, &fixture.initial_items);
}

#[test]
fn changed_child_root_and_host_require_a_fresh_recovery_registration() {
    let mut fixture = fixture(false);
    let child_before = fixture.child_binding;
    let restored = workspace_with_changed_child_contract();

    fixture
        .engine
        .enqueue_workspace_replacement(restored)
        .expect("workspace restore must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("workspace restore must fail closed for the old child contract");
    let reconciliation = transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::WorkspaceReplaced { reconciliation, .. } => Some(reconciliation),
            _ => None,
        })
        .expect("restore must publish its binding reconciliation");
    assert_eq!(reconciliation.retired(), &[child_before]);
    assert_eq!(
        reconciliation.unbound_surfaces(),
        &[SURFACE_CHILD, SURFACE_UNRELATED]
    );
    assert!(reconciliation.replacements().is_empty());
    assert!(
        reconciliation
            .rebound()
            .iter()
            .all(|(before, _)| *before != child_before)
    );
    assert!(
        transition.platform_effects().iter().all(|request| {
            !matches!(request.effect(), PlatformEffect::RequestReplacement { .. })
        })
    );
    assert!(fixture.engine.viewport().viewport(SURFACE_CHILD).is_none());

    let fresh_host_binding = register_fresh_recovery_contract(&mut fixture);

    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Replacement(fresh_host_binding),
        TestWindow::Child {
            close_requested: false,
        },
    ]);
    publish_scene(&mut fixture.engine);
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Replacement(fresh_host_binding),
    ]);

    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_none());
    let recovered = fixture
        .engine
        .workspace()
        .contained_floating(FRESH_RECOVERY_FLOATING)
        .expect("fresh recovery plan must rehome the replacement root");
    assert_eq!(recovered.root, ROOT_COLLISION);
    assert_eq!(recovered.surface, SURFACE_UNRELATED);
}

#[test]
fn changed_target_coordinate_generation_keeps_merge_back_pending() {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    accept_close(&mut fixture);

    let pending = fixture.publish_windows([TestWindow::HostUnavailable]);
    request_replacement(&pending);
    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_some(),
        "the complete roster recovery must remain pending"
    );
    let pending = fixture
        .engine
        .pending_surface_recovery(SURFACE_CHILD)
        .expect("engine must retain one semantic roster disposition");
    assert_eq!(pending.main_root(), ROOT_CHILD_MAIN);
    assert_eq!(
        pending
            .contained()
            .iter()
            .map(dockspace::surface_recovery::ContainedRootDisposition::floating)
            .collect::<Vec<_>>(),
        vec![FLOATING_B, FLOATING_A]
    );

    fixture.publish_windows([TestWindow::Host]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some(),
        "new target coordinates must not mix with the accepted scene dependency"
    );
}

fn assert_target_geometry_change_keeps_merge_pending(new_geometry: WindowGeometry) {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    accept_close(&mut fixture);
    fixture.host_geometry = new_geometry;

    fixture.publish_windows([TestWindow::Host]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some()
    );
}

#[test]
fn target_origin_change_invalidates_the_accepted_merge_dependency() {
    assert_target_geometry_change_keeps_merge_pending(WindowGeometry {
        content: physical_rect(120.0, 80.0, 1000.0, 800.0),
        outer: physical_rect(112.0, 50.0, 1016.0, 838.0),
        scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
    });
}

#[test]
fn target_resize_invalidates_the_accepted_merge_dependency() {
    assert_target_geometry_change_keeps_merge_pending(WindowGeometry {
        content: physical_rect(0.0, 0.0, 1200.0, 900.0),
        outer: physical_rect(-8.0, -30.0, 1216.0, 938.0),
        scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
    });
}

#[test]
fn target_scale_change_invalidates_the_accepted_merge_dependency() {
    assert_target_geometry_change_keeps_merge_pending(WindowGeometry {
        content: physical_rect(0.0, 0.0, 2000.0, 1600.0),
        outer: physical_rect(-16.0, -60.0, 2032.0, 1676.0),
        scale: ScaleFactor::new(2.0).expect("host scale factor must be valid"),
    });
}

#[test]
fn ready_replacement_preserves_the_original_complete_child_roster() {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    accept_close(&mut fixture);
    let pending = fixture.publish_windows([TestWindow::HostUnavailable]);
    let (_, replacement) = request_replacement(&pending);

    fixture.publish_windows([
        TestWindow::HostUnavailable,
        TestWindow::Replacement(replacement),
    ]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        surface_roster_snapshot(fixture.engine.workspace(), SURFACE_CHILD),
        fixture.child_roster
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .expect("replacement must retain the logical child surface")
            .binding(),
        replacement
    );
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none(),
        "ready replacement must atomically release semantic recovery ownership"
    );
}

#[test]
fn mixed_dpi_projection_clamps_the_complete_roster_and_preserves_stack_order() {
    let mut fixture = fixture_with_geometry(
        false,
        100,
        WindowGeometry {
            content: physical_rect(-400.0, 50.0, 2000.0, 1600.0),
            outer: physical_rect(-416.0, -10.0, 2032.0, 1676.0),
            scale: ScaleFactor::new(2.0).expect("host scale factor must be valid"),
        },
        WindowGeometry {
            content: physical_rect(1800.0, -200.0, 1250.0, 1000.0),
            outer: physical_rect(1788.0, -238.0, 1274.0, 1048.0),
            scale: ScaleFactor::new(1.25).expect("child scale factor must be valid"),
        },
    );
    accept_close(&mut fixture);

    fixture.publish_windows([TestWindow::Host]);

    assert_complete_roster_merged_back(&fixture, &fixture.child_roster, &fixture.initial_items);
    let floating_a = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_A)
        .expect("first sibling must survive mixed-DPI recovery");
    let floating_b = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_B)
        .expect("second sibling must survive mixed-DPI recovery");
    assert_eq!(floating_a.z_order, 101);
    assert_eq!(floating_b.z_order, 102);
    assert!(floating_a.rect.x() >= 0.0 && floating_a.rect.max().x() <= 1000.0);
    assert!(floating_b.rect.y() >= 0.0 && floating_b.rect.max().y() <= 800.0);
}

#[test]
fn direct_destruction_without_source_geometry_commits_a_complete_pending_roster() {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::ChildUnavailable {
            close_requested: false,
        },
    ]);

    fixture.publish_windows([TestWindow::Host]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_some(),
        "destroyed binding must become queryable pending state"
    );
    let pending = fixture
        .engine
        .pending_surface_recovery(SURFACE_CHILD)
        .expect("semantic pending state must retain the complete roster");
    assert!(!pending.source_geometry_available());
    assert_eq!(pending.main_root(), ROOT_CHILD_MAIN);
    assert_eq!(pending.contained().len(), 2);
}

#[test]
fn explicit_replacement_registration_completes_geometryless_pending_recovery() {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::ChildUnavailable {
            close_requested: false,
        },
    ]);
    fixture.publish_windows([TestWindow::Host]);
    publish_bootstrap_scene(&mut fixture.engine);

    let replacement_token = WindowToken::new(401);
    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_CHILD,
            replacement_token,
            ViewportRole::Child,
            Some(fixture.recovery),
        )
        .expect("explicit replacement registration must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("explicit replacement registration must reduce");
    let replacement = fixture
        .engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("explicit replacement must own the logical surface")
        .binding();
    assert_eq!(replacement.token(), replacement_token);
    assert_ne!(replacement, fixture.child_binding);
    fixture.child_binding = replacement;

    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: false,
        },
    ]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(
        surface_roster_snapshot(fixture.engine.workspace(), SURFACE_CHILD),
        fixture.child_roster
    );
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_none()
    );
}

#[test]
fn replacement_registration_rejects_a_changed_pending_contract_with_ready_target_scene() {
    let mut fixture = fixture(false);
    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::ChildUnavailable {
            close_requested: false,
        },
    ]);
    fixture.publish_windows([TestWindow::Host]);
    publish_scene(&mut fixture.engine);

    let mismatched = ContainedTearOffProposal::new(
        fixture.recovery.root(),
        fixture.recovery.floating(),
        fixture.recovery.placement(),
        fixture
            .recovery
            .z_order()
            .checked_add(1)
            .expect("test z-order must not exhaust"),
    );
    fixture
        .engine
        .enqueue_viewport_registration(
            SURFACE_CHILD,
            WindowToken::new(403),
            ViewportRole::Child,
            Some(mismatched),
        )
        .expect("mismatched replacement registration must enqueue");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("mismatched replacement registration must reduce");

    assert!(matches!(
        transition.reduced_inputs(),
        [input]
            if matches!(
                input.outcome(),
                InputOutcome::ViewportRegistrationRejected {
                    surface: SURFACE_CHILD
                }
            )
    ));
    assert!(fixture.engine.viewport().viewport(SURFACE_CHILD).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_some()
    );
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SURFACE_CHILD)
            .is_some()
    );
    assert!(
        fixture.engine.pending_inputs().is_empty(),
        "deterministic registration rejection must consume the queued input"
    );
}

#[test]
fn unplanned_root_destruction_only_unbinds_runtime_and_allows_reregistration() {
    let mut fixture = fixture(false);
    let before = fixture.engine.workspace().clone();
    let destroyed = fixture.host_binding;

    fixture.publish_windows([TestWindow::Child {
        close_requested: false,
    }]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(fixture.engine.viewport().viewport(SURFACE_HOST).is_none());

    let replacement_token = WindowToken::new(402);
    fixture
        .engine
        .enqueue_viewport_registration(SURFACE_HOST, replacement_token, ViewportRole::Root, None)
        .expect("root re-registration must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("root re-registration must reduce");
    let replacement = fixture
        .engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("same logical root surface must accept a new binding")
        .binding();
    assert_eq!(replacement.token(), replacement_token);
    assert_ne!(replacement, destroyed);
    fixture.host_binding = replacement;

    fixture.publish_windows([
        TestWindow::Host,
        TestWindow::Child {
            close_requested: false,
        },
    ]);

    assert_eq!(fixture.engine.workspace(), &before);
    assert!(
        fixture
            .engine
            .viewport()
            .viewport(SURFACE_HOST)
            .is_some_and(|record| record.is_ready() && record.binding() == replacement)
    );
}

#[test]
fn target_z_overflow_leaves_the_entire_roster_pending_without_partial_commit() {
    let mut fixture = fixture_with_geometry(
        false,
        u64::MAX,
        WindowGeometry {
            content: physical_rect(0.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(-8.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("host scale factor must be valid"),
        },
        WindowGeometry {
            content: physical_rect(1000.0, 0.0, 1000.0, 800.0),
            outer: physical_rect(992.0, -30.0, 1016.0, 838.0),
            scale: ScaleFactor::new(1.0).expect("child scale factor must be valid"),
        },
    );
    let before = fixture.engine.workspace().clone();
    accept_close(&mut fixture);

    fixture.publish_windows([TestWindow::Host]);

    assert_eq!(fixture.engine.workspace(), &before);
    let pending = fixture
        .engine
        .pending_surface_recovery(SURFACE_CHILD)
        .expect("z overflow must retain an atomic pending roster");
    assert_eq!(pending.main_root(), ROOT_CHILD_MAIN);
    assert_eq!(pending.contained().len(), 2);
}

#[test]
#[allow(clippy::too_many_lines)]
fn same_snapshot_uses_batch_frozen_target_facts_for_merge_and_direct_recovery() {
    const ROOT_A_MAIN: RootId = RootId::new(101);
    const ROOT_A_FLOATING: RootId = RootId::new(102);
    const ROOT_B_MAIN: RootId = RootId::new(103);
    const ROOT_B_FLOATING: RootId = RootId::new(104);
    const SURFACE_A: SurfaceId = SurfaceId::new(101);
    const SURFACE_B: SurfaceId = SurfaceId::new(102);
    const FLOATING_A_CHILD: FloatingPresentationId = FloatingPresentationId::new(101);
    const FLOATING_B_CHILD: FloatingPresentationId = FloatingPresentationId::new(102);
    const RECOVERY_A: FloatingPresentationId = FloatingPresentationId::new(103);
    const RECOVERY_B: FloatingPresentationId = FloatingPresentationId::new(104);
    const TOKEN_A: WindowToken = WindowToken::new(301);
    const TOKEN_B: WindowToken = WindowToken::new(302);

    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(100)]));
    let a_main_tabs = builder.insert_node(Node::tabs([ItemId::new(101)]));
    let a_floating_tabs = builder.insert_node(Node::tabs([ItemId::new(102)]));
    let b_main_tabs = builder.insert_node(Node::tabs([ItemId::new(103)]));
    let b_floating_tabs = builder.insert_node(Node::tabs([ItemId::new(104)]));
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_root(ROOT_A_MAIN, RootRecord::new(a_main_tabs));
    builder.set_root(ROOT_A_FLOATING, RootRecord::new(a_floating_tabs));
    builder.set_root(ROOT_B_MAIN, RootRecord::new(b_main_tabs));
    builder.set_root(ROOT_B_FLOATING, RootRecord::new(b_floating_tabs));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A_MAIN));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B_MAIN));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_A_CHILD,
        ROOT_A_FLOATING,
        SURFACE_A,
        logical_rect(20.0, 30.0, 180.0, 140.0),
        2,
    ));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING_B_CHILD,
        ROOT_B_FLOATING,
        SURFACE_B,
        logical_rect(40.0, 50.0, 200.0, 150.0),
        3,
    ));
    builder
        .attach_contained(SURFACE_A, FLOATING_A_CHILD)
        .expect("surface A must exist");
    builder
        .attach_contained(SURFACE_B, FLOATING_B_CHILD)
        .expect("surface B must exist");
    let workspace = builder.build().expect("multi-surface workspace must build");
    let initial_items = workspace.item_multiset();
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("multi engine must build");
    publish_scene(&mut engine);
    let zero = LogicalSize::new(0.0, 0.0).expect("zero minimum must be valid");
    let proposal_a = ContainedTearOffProposal::new(
        ROOT_A_MAIN,
        RECOVERY_A,
        engine
            .contained_placement(SURFACE_HOST, logical_rect(80.0, 90.0, 420.0, 320.0), zero)
            .expect("surface A recovery must be placeable"),
        10,
    );
    let proposal_b = ContainedTearOffProposal::new(
        ROOT_B_MAIN,
        RECOVERY_B,
        engine
            .contained_placement(SURFACE_HOST, logical_rect(180.0, 190.0, 420.0, 320.0), zero)
            .expect("surface B recovery must be placeable"),
        20,
    );
    engine
        .enqueue_viewport_registration(SURFACE_HOST, HOST_TOKEN, ViewportRole::Root, None)
        .expect("host registration must enqueue");
    engine
        .enqueue_viewport_registration(SURFACE_A, TOKEN_A, ViewportRole::Child, Some(proposal_a))
        .expect("surface A registration must enqueue");
    engine
        .enqueue_viewport_registration(SURFACE_B, TOKEN_B, ViewportRole::Child, Some(proposal_b))
        .expect("surface B registration must enqueue");
    engine
        .reduce_pending()
        .expect("multi registrations must reduce");
    let host_binding = engine
        .viewport()
        .viewport(SURFACE_HOST)
        .expect("host binding must exist")
        .binding();
    let binding_a = engine
        .viewport()
        .viewport(SURFACE_A)
        .expect("surface A binding must exist")
        .binding();
    let binding_b = engine
        .viewport()
        .viewport(SURFACE_B)
        .expect("surface B binding must exist")
        .binding();
    let geometry_host = WindowGeometry {
        content: physical_rect(0.0, 0.0, 1000.0, 800.0),
        outer: physical_rect(-8.0, -30.0, 1016.0, 838.0),
        scale: ScaleFactor::new(1.0).expect("host scale must be valid"),
    };
    let geometry_a = WindowGeometry {
        content: physical_rect(1000.0, 0.0, 800.0, 600.0),
        outer: physical_rect(992.0, -30.0, 816.0, 638.0),
        scale: ScaleFactor::new(1.0).expect("surface A scale must be valid"),
    };
    let geometry_b = WindowGeometry {
        content: physical_rect(2000.0, 0.0, 800.0, 600.0),
        outer: physical_rect(1992.0, -30.0, 816.0, 638.0),
        scale: ScaleFactor::new(1.0).expect("surface B scale must be valid"),
    };
    publish_windows(
        &mut engine,
        vec![
            ready_window(
                host_binding,
                InputObservationGeneration::new(1),
                geometry_host,
                false,
            ),
            ready_window(
                binding_a,
                InputObservationGeneration::new(2),
                geometry_a,
                false,
            ),
            ready_window(
                binding_b,
                InputObservationGeneration::new(3),
                geometry_b,
                false,
            ),
        ],
    );
    publish_scene(&mut engine);
    let close = publish_windows(
        &mut engine,
        vec![
            ready_window(
                host_binding,
                InputObservationGeneration::new(4),
                geometry_host,
                false,
            ),
            ready_window(
                binding_a,
                InputObservationGeneration::new(5),
                geometry_a,
                true,
            ),
            ready_window(
                binding_b,
                InputObservationGeneration::new(6),
                geometry_b,
                false,
            ),
        ],
    );
    let requests: BTreeMap<_, _> = close
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::PlatformSnapshotPublished { transition, .. } => Some(
                transition
                    .close_requests()
                    .iter()
                    .map(|request| {
                        let surface = engine
                            .viewport()
                            .viewport_close_request(*request)
                            .expect("close request must remain queryable")
                            .binding()
                            .surface();
                        (surface, *request)
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("close snapshot must publish surface A request");
    assert_eq!(requests.len(), 1);
    let target = engine
        .workspace()
        .capture_tab_target(ROOT_HOST, host_tabs)
        .expect("host merge target must be current");
    engine
        .enqueue_viewport_close_decision(
            requests[&SURFACE_A],
            ViewportCloseDecision::Accept(ViewportClosePlan::merge_back(
                ViewportMergeBackPlan::new(SURFACE_HOST, target),
            )),
        )
        .expect("surface A close decision must enqueue");
    engine
        .reduce_pending()
        .expect("surface A accepted close must reduce");

    publish_windows(
        &mut engine,
        vec![ready_window(
            host_binding,
            InputObservationGeneration::new(7),
            geometry_host,
            false,
        )],
    );

    let workspace = engine.workspace();
    assert!(workspace.surface(SURFACE_A).is_none());
    assert!(workspace.surface(SURFACE_B).is_none());
    assert!(workspace.contained_floating(RECOVERY_A).is_none());
    for floating in [FLOATING_A_CHILD, FLOATING_B_CHILD, RECOVERY_B] {
        assert_eq!(
            workspace
                .contained_floating(floating)
                .expect("every recovered presentation must survive")
                .surface,
            SURFACE_HOST
        );
    }
    assert_eq!(workspace.item_multiset(), initial_items);
    assert!(engine.pending_surface_recovery(SURFACE_A).is_none());
    assert!(engine.pending_surface_recovery(SURFACE_B).is_none());
}
