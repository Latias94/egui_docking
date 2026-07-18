use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectPhase, EffectRequest,
    EffectResult, PlatformEffect,
};
use dockspace::engine::DockEngine;
use dockspace::frame::{
    RecoveryPendingStatus, ViewportCloseDecision, ViewportClosePlan, ViewportCloseRequestId,
};
use dockspace::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{Authority, ContainedTearOffProposal};
use dockspace::platform::{
    ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    WindowInputState,
};
use dockspace::policy::DockPolicy;
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken, WorkAreaToken};

const ROOT_HOST: RootId = RootId::new(1);
const ROOT_CHILD: RootId = RootId::new(2);
const SURFACE_HOST: SurfaceId = SurfaceId::new(1);
const SURFACE_CHILD: SurfaceId = SurfaceId::new(2);
const RECOVERY_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const HOST_TOKEN: WindowToken = WindowToken::new(10);
const CHILD_TOKEN: WindowToken = WindowToken::new(20);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(30);

struct Fixture {
    engine: DockEngine,
    child_binding: ViewportBinding,
    child_root_node: NodeId,
    initial_items: std::collections::BTreeMap<ItemId, usize>,
}

struct PendingFixture {
    fixture: Fixture,
    replacement_binding: ViewportBinding,
    replacement_effect: EffectId,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn child_outer_bounds() -> PhysicalRect {
    physical_rect(1192.0, -30.0, 916.0, 738.0)
}

fn workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let child_left = builder.insert_node(Node::tabs([ItemId::new(2), ItemId::new(3)]));
    let child_right = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let child_root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [child_left, child_right])
            .expect("test split must be valid"),
    );
    builder.set_root(ROOT_HOST, RootRecord::new(host_tabs));
    builder.set_root(ROOT_CHILD, RootRecord::new(child_root));
    builder.set_surface(SURFACE_HOST, SurfacePresentation::new(ROOT_HOST));
    builder.set_surface(SURFACE_CHILD, SurfacePresentation::new(ROOT_CHILD));
    (
        builder.build().expect("test workspace must be valid"),
        child_root,
    )
}

fn platform_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities
}

fn ready_window(token: WindowToken, x: f64, close_requested: bool) -> ObservedWindow {
    ObservedWindow::new(token)
        .with_content_bounds(Authority::Known(physical_rect(x, 0.0, 900.0, 700.0)))
        .with_outer_bounds(Authority::Known(physical_rect(
            x - 8.0,
            -30.0,
            916.0,
            738.0,
        )))
        .with_scale_factor(Authority::Known(
            ScaleFactor::new(1.0).expect("test scale factor must be valid"),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_close_requested(Authority::Known(close_requested))
}

fn host_window() -> ObservedWindow {
    ready_window(HOST_TOKEN, 0.0, false)
}

fn child_window(close_requested: bool) -> ObservedWindow {
    ready_window(CHILD_TOKEN, 1200.0, close_requested)
}

fn unavailable_host_window() -> ObservedWindow {
    ObservedWindow::new(HOST_TOKEN).with_close_requested(Authority::Known(false))
}

fn replacement_window(binding: ViewportBinding) -> ObservedWindow {
    ready_window(binding.token(), 1200.0, false)
}

fn partially_observed_replacement_window(binding: ViewportBinding) -> ObservedWindow {
    ObservedWindow::new(binding.token()).with_close_requested(Authority::Known(false))
}

fn publish_windows(engine: &mut DockEngine, windows: Vec<ObservedWindow>) -> EngineTransition {
    let snapshot = PlatformSnapshot::new(
        platform_capabilities(),
        windows,
        Vec::new(),
        vec![ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
            ScaleFactor::new(1.0).expect("test work-area scale factor must be valid"),
        )],
    )
    .expect("test platform snapshot must be canonical");
    engine
        .enqueue_platform_snapshot(snapshot)
        .expect("platform snapshot sequence must be available");
    engine
        .reduce_pending()
        .expect("platform snapshot must publish")
}

fn fixture() -> Fixture {
    let (workspace, child_root_node) = workspace();
    let initial_items = workspace.item_multiset();
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("test engine must be valid");
    engine
        .enqueue_viewport_registration(SURFACE_HOST, HOST_TOKEN, ViewportRole::Root, None)
        .expect("host registration must enqueue");
    engine
        .enqueue_viewport_registration(
            SURFACE_CHILD,
            CHILD_TOKEN,
            ViewportRole::Child,
            Some(recovery()),
        )
        .expect("child registration must enqueue");
    engine
        .reduce_pending()
        .expect("viewport registrations must reduce");
    let child_binding = engine
        .viewport()
        .viewport(SURFACE_CHILD)
        .expect("child viewport must be registered")
        .binding();
    publish_windows(&mut engine, vec![host_window(), child_window(false)]);
    Fixture {
        engine,
        child_binding,
        child_root_node,
        initial_items,
    }
}

fn recovery() -> ContainedTearOffProposal {
    ContainedTearOffProposal::new(
        SURFACE_HOST,
        ROOT_CHILD,
        RECOVERY_FLOATING,
        logical_rect(40.0, 50.0, 480.0, 360.0),
        7,
    )
}

fn close_request_from(transition: &EngineTransition) -> ViewportCloseRequestId {
    transition
        .reduced_inputs()
        .iter()
        .find_map(|reduced| match reduced.outcome() {
            InputOutcome::PlatformSnapshotPublished { transition } => {
                transition.close_requests().first().copied()
            }
            _ => None,
        })
        .expect("snapshot must publish one child close request")
}

fn accept_child_close(fixture: &mut Fixture) {
    let close = publish_windows(&mut fixture.engine, vec![host_window(), child_window(true)]);
    let request = close_request_from(&close);
    fixture
        .engine
        .enqueue_viewport_close_decision(
            request,
            ViewportCloseDecision::Accept(ViewportClosePlan::new(None, recovery())),
        )
        .expect("close decision must enqueue");
    let decided = fixture
        .engine
        .reduce_pending()
        .expect("close decision must reduce");
    let releases = decided
        .platform_effects()
        .iter()
        .filter(|request| {
            matches!(
                request.effect(),
                PlatformEffect::ReleaseChild { binding }
                    if *binding == fixture.child_binding
            )
        })
        .count();
    assert_eq!(releases, 1);
}

fn request_replacement(transition: &EngineTransition) -> (EffectId, ViewportBinding) {
    let matching: Vec<_> = transition
        .platform_effects()
        .iter()
        .filter_map(|request| match request.effect() {
            PlatformEffect::RequestReplacement {
                binding,
                placement,
                role,
            } => Some((request.id(), *binding, *placement, *role)),
            _ => None,
        })
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one replacement request"
    );
    let (effect, binding, placement, role) = matching[0];
    assert_eq!(binding.surface(), SURFACE_CHILD);
    assert_eq!(placement, child_outer_bounds());
    assert_eq!(role, ViewportRole::Child);
    (effect, binding)
}

fn pending_fixture() -> PendingFixture {
    let mut fixture = fixture();
    accept_child_close(&mut fixture);
    let destroyed = publish_windows(&mut fixture.engine, vec![unavailable_host_window()]);
    let (replacement_effect, replacement_binding) = request_replacement(&destroyed);
    let pending = fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("destroyed child must retain a queryable recovery");
    assert_eq!(pending.destroyed_binding(), fixture.child_binding);
    assert_eq!(pending.recovery(), recovery());
    assert_eq!(pending.replacement_binding(), Some(replacement_binding));
    assert_eq!(pending.replacement_effect(), Some(replacement_effect));
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::ReplacementRequested {
            effect: replacement_effect
        }
    );
    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );
    PendingFixture {
        fixture,
        replacement_binding,
        replacement_effect,
    }
}

fn assert_whole_root_recovered(fixture: &Fixture) {
    assert!(fixture.engine.workspace().surface(SURFACE_CHILD).is_none());
    let recovered = fixture
        .engine
        .workspace()
        .contained_floating(RECOVERY_FLOATING)
        .expect("the complete child root must be recovered as one contained presentation");
    assert_eq!(recovered.root, ROOT_CHILD);
    assert_eq!(recovered.surface, SURFACE_HOST);
    assert_eq!(recovered.rect, recovery().rect());
    assert_eq!(recovered.z_order, recovery().z_order());
    assert_eq!(
        fixture
            .engine
            .workspace()
            .root(ROOT_CHILD)
            .expect("recovered root must remain durable")
            .node,
        fixture.child_root_node
    );
    assert_eq!(
        fixture.engine.workspace().item_multiset(),
        fixture.initial_items
    );
}

fn effect_count(engine: &DockEngine, predicate: impl Fn(&PlatformEffect) -> bool) -> usize {
    engine
        .viewport()
        .effects()
        .records()
        .filter(|(_, record)| predicate(record.request().effect()))
        .count()
}

fn assert_no_new_effects(transition: &EngineTransition) {
    assert!(transition.platform_effects().is_empty());
}

#[test]
fn destroyed_child_recovers_the_whole_root_when_the_host_is_ready() {
    let mut fixture = fixture();
    accept_child_close(&mut fixture);

    let destroyed = publish_windows(&mut fixture.engine, vec![host_window()]);

    assert_no_new_effects(&destroyed);
    assert_whole_root_recovered(&fixture);
    assert!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert_eq!(
        effect_count(&fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { .. }
        )),
        0
    );
}

#[test]
fn unavailable_host_keeps_recovery_pending_and_requests_one_last_outer_placement() {
    let mut pending = pending_fixture();

    let repeated = publish_windows(&mut pending.fixture.engine, vec![unavailable_host_window()]);

    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { binding, .. }
                if *binding == pending.replacement_binding
        )),
        1
    );
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("recovery must remain pending")
            .status(),
        RecoveryPendingStatus::ReplacementRequested {
            effect: pending.replacement_effect
        }
    );
}

#[test]
fn replacement_ready_resolves_pending_without_moving_topology() {
    let mut pending = pending_fixture();
    let before = pending.fixture.engine.workspace().clone();

    let ready = publish_windows(
        &mut pending.fixture.engine,
        vec![
            unavailable_host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );

    assert_no_new_effects(&ready);
    assert_eq!(pending.fixture.engine.workspace(), &before);
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .surface(SURFACE_CHILD)
            .is_some()
    );
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("replacement effect must remain auditable")
            .phase(),
        EffectPhase::ObservedApplied { .. }
    ));

    let repeated = publish_windows(
        &mut pending.fixture.engine,
        vec![
            unavailable_host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&repeated);
}

#[test]
fn adopted_replacement_retains_recovery_for_a_second_destruction() {
    let mut pending = pending_fixture();
    publish_windows(
        &mut pending.fixture.engine,
        vec![
            unavailable_host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );

    publish_windows(&mut pending.fixture.engine, vec![host_window()]);

    assert_whole_root_recovered(&pending.fixture);
}

#[test]
fn vetoed_child_close_still_recovers_after_unexpected_destruction() {
    let mut fixture = fixture();
    let close = publish_windows(&mut fixture.engine, vec![host_window(), child_window(true)]);
    let request = close_request_from(&close);
    fixture
        .engine
        .enqueue_viewport_close_decision(request, ViewportCloseDecision::Veto)
        .expect("close veto must enqueue");
    fixture
        .engine
        .reduce_pending()
        .expect("close veto must reduce");

    publish_windows(&mut fixture.engine, vec![host_window()]);

    assert_whole_root_recovered(&fixture);
}

#[test]
fn host_ready_before_replacement_rehomes_and_compensates_exactly_once() {
    let mut pending = pending_fixture();

    let recovered = publish_windows(&mut pending.fixture.engine, vec![host_window()]);
    let compensations: Vec<&EffectRequest> = recovered
        .platform_effects()
        .iter()
        .filter(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
        })
        .collect();

    assert_eq!(compensations.len(), 1);
    let compensation = compensations[0].id();
    assert_whole_root_recovered(&pending.fixture);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement compensation must remain queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement {
            effect: compensation
        }
    );

    let repeated = publish_windows(&mut pending.fixture.engine, vec![host_window()]);
    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } if *binding == pending.replacement_binding
                && *compensates == pending.replacement_effect
        )),
        1
    );
}

#[test]
fn failed_replacement_compensation_retries_only_after_explicit_input() {
    let mut pending = pending_fixture();
    let recovered = publish_windows(&mut pending.fixture.engine, vec![host_window()]);
    let failed_cleanup = recovered
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("recovery must request one replacement cleanup");
    pending
        .fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            failed_cleanup,
            pending.fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
        ))
        .expect("replacement cleanup failure must enqueue");
    let failed = pending
        .fixture
        .engine
        .reduce_pending()
        .expect("replacement cleanup failure must reduce");
    assert_no_new_effects(&failed);

    pending
        .fixture
        .engine
        .enqueue_viewport_cleanup_retry(failed_cleanup)
        .expect("replacement cleanup retry must enqueue");
    let retried = pending
        .fixture
        .engine
        .reduce_pending()
        .expect("replacement cleanup retry must reduce");
    let retry = retried
        .platform_effects()
        .iter()
        .find_map(|request| {
            matches!(
                request.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(request.id())
        })
        .expect("explicit retry must issue one new replacement cleanup");
    assert_ne!(retry, failed_cleanup);
    assert_eq!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .expect("replacement cleanup must remain queryable")
            .status(),
        RecoveryPendingStatus::CompensatingReplacement { effect: retry }
    );

    let late = publish_windows(
        &mut pending.fixture.engine,
        vec![
            host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&late);
}

#[test]
fn replacement_dispatch_failure_remains_recoverable_without_redispatch() {
    let mut pending = pending_fixture();
    pending
        .fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            pending.replacement_effect,
            pending.fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ))
        .expect("replacement failure must enqueue");
    let failed = pending
        .fixture
        .engine
        .reduce_pending()
        .expect("replacement failure must reduce");

    assert_no_new_effects(&failed);
    let recovery_pending = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("dispatch failure must retain the recovery");
    assert_eq!(recovery_pending.replacement_binding(), None);
    assert_eq!(
        recovery_pending.status(),
        RecoveryPendingStatus::ReplacementFailed {
            effect: pending.replacement_effect
        }
    );
    assert!(matches!(
        pending
            .fixture
            .engine
            .viewport()
            .effects()
            .record(pending.replacement_effect)
            .expect("failed replacement effect must remain auditable")
            .phase(),
        EffectPhase::DispatchFailed(DispatchFailureReason::AdapterRejected)
    ));

    let repeated = publish_windows(&mut pending.fixture.engine, vec![unavailable_host_window()]);
    assert_no_new_effects(&repeated);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::RequestReplacement { .. }
        )),
        1
    );

    let recovered = publish_windows(&mut pending.fixture.engine, vec![host_window()]);
    assert_no_new_effects(&recovered);
    assert_whole_root_recovered(&pending.fixture);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { .. }
        )),
        0
    );
}

#[test]
fn observed_replacement_survives_dispatch_failure_and_is_adopted_when_ready() {
    let mut pending = pending_fixture();
    publish_windows(
        &mut pending.fixture.engine,
        vec![
            unavailable_host_window(),
            partially_observed_replacement_window(pending.replacement_binding),
        ],
    );
    pending
        .fixture
        .engine
        .enqueue_platform_effect_result(EffectResult::new(
            pending.replacement_effect,
            pending.fixture.engine.version().epoch(),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ))
        .expect("replacement failure must enqueue");
    pending
        .fixture
        .engine
        .reduce_pending()
        .expect("observed replacement failure must reduce");

    let failed = pending
        .fixture
        .engine
        .viewport()
        .recovery_pending(SURFACE_CHILD)
        .expect("observed replacement must retain recovery association");
    assert_eq!(
        failed.replacement_binding(),
        Some(pending.replacement_binding)
    );
    assert_eq!(
        failed.status(),
        RecoveryPendingStatus::ReplacementFailed {
            effect: pending.replacement_effect
        }
    );

    let ready = publish_windows(
        &mut pending.fixture.engine,
        vec![
            unavailable_host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&ready);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none()
    );
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .viewport(SURFACE_CHILD)
            .is_some_and(dockspace::viewport_registry::ViewportRecord::is_ready)
    );

    let host_ready = publish_windows(
        &mut pending.fixture.engine,
        vec![
            host_window(),
            replacement_window(pending.replacement_binding),
        ],
    );
    assert_no_new_effects(&host_ready);
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .surface(SURFACE_CHILD)
            .is_some()
    );
    assert!(
        pending
            .fixture
            .engine
            .workspace()
            .contained_floating(RECOVERY_FLOATING)
            .is_none()
    );
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { .. }
        )),
        0
    );
}
