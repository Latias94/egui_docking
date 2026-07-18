use dockspace::command::MovePayload;
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::intent::{
    Authority, ContainedTearOffProposal, NativeTearOffCapability, NativeTearOffProposal,
    NativeTearOffUnavailableReason, PointerButton, PointerButtonState, PointerId, RendererIntent,
    TearOffRequest,
};
use dockspace::interaction::{
    DragSessionId, InteractionCancelReason, InteractionDelivery, InteractionOutcome,
    InteractionRejection, InteractionStatus, PreviewResolutionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use dockspace::policy::{ContainedFallback, DockPolicy};
use dockspace::scene::{BuildingScene, ReadySurfaceScene};
use dockspace::transition::InputOutcome;

const ROOT_A: RootId = RootId::new(1);
const ROOT_B: RootId = RootId::new(2);
const ROOT_NEW: RootId = RootId::new(10);
const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const SURFACE_NEW: SurfaceId = SurfaceId::new(10);
const FLOATING_NEW: FloatingPresentationId = FloatingPresentationId::new(10);
const POINTER: PointerId = PointerId::new(1);

struct Fixture {
    engine: DockEngine,
    tabs_a: NodeId,
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle must be valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn fixture(policy: DockPolicy, source_items: &[u64]) -> Fixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs(source_items.iter().copied().map(ItemId::new)));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT_A, RootRecord::new(tabs_a));
    builder.set_root(ROOT_B, RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(ROOT_B));
    let workspace = builder.build().expect("test workspace must be valid");
    let engine = DockEngine::new(workspace, policy).expect("test engine must be valid");
    Fixture { engine, tabs_a }
}

fn publish_scene(fixture: &mut Fixture) {
    let mut scene = BuildingScene::new([SURFACE_A, SURFACE_B]).expect("test roster must be unique");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_A,
            logical_rect(0.0, 0.0, 400.0, 300.0),
        ))
        .expect("source facts must be complete");
    scene
        .insert_ready(ReadySurfaceScene::new(
            SURFACE_B,
            logical_rect(0.0, 0.0, 400.0, 300.0),
        ))
        .expect("target facts must be complete");
    fixture
        .engine
        .enqueue_scene(scene)
        .expect("scene sequence must be available");
    fixture.engine.reduce_pending().expect("scene must publish");
}

fn arm_and_begin(fixture: &mut Fixture) -> DragSessionId {
    let payload = MovePayload::Item(
        fixture
            .engine
            .workspace()
            .capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(1))
            .expect("source item must be current"),
    );
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ArmDrag {
            pointer: POINTER,
            button: PointerButton::Primary,
            payload,
        })
        .expect("arm sequence must be available");
    let transition = fixture.engine.reduce_pending().expect("arm must reduce");
    let session = match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::DragArmed { session, .. },
            ..
        } => *session,
        outcome => panic!("unexpected arm outcome: {outcome:?}"),
    };
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::BeginDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
        })
        .expect("begin sequence must be available");
    fixture.engine.reduce_pending().expect("begin must reduce");
    session
}

fn preview_tear_off(
    fixture: &mut Fixture,
    session: DragSessionId,
    request: TearOffRequest,
) -> InteractionOutcome {
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::UpdateDrag {
            session,
            target: Authority::Known(None),
            tear_off: Some(request),
        })
        .expect("update sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("tear-off preview must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected preview outcome: {outcome:?}"),
    }
}

fn release_tear_off(
    fixture: &mut Fixture,
    session: DragSessionId,
    request: TearOffRequest,
) -> InteractionOutcome {
    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("preview must exist before release")
        .acknowledgement();
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::ReleaseDrag {
            session,
            pointer: POINTER,
            button: PointerButton::Primary,
            button_state: Authority::Known(PointerButtonState::Released),
            target: Authority::Known(None),
            tear_off: Some(request),
        })
        .expect("release sequence must be available");
    fixture
        .engine
        .enqueue_renderer_intent(RendererIntent::AcknowledgePreview(acknowledgement))
        .expect("ack sequence must be available");
    let transition = fixture
        .engine
        .reduce_pending()
        .expect("tear-off release must reduce");
    match transition.reduced_inputs()[1].outcome() {
        InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
        outcome => panic!("unexpected release outcome: {outcome:?}"),
    }
}

fn contained_proposal(root: RootId, x: f64) -> ContainedTearOffProposal {
    ContainedTearOffProposal::new(
        SURFACE_B,
        root,
        FLOATING_NEW,
        logical_rect(x, 20.0, 240.0, 180.0),
        7,
    )
}

#[test]
fn contained_preview_and_delivery_use_the_same_exact_command() {
    let mut fixture = fixture(DockPolicy::default(), &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let request = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0));

    let preview = preview_tear_off(&mut fixture, session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            status: PreviewResolutionStatus::Resolved,
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: false, .. })
    ));
    let delivery = release_tear_off(&mut fixture, session, request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::Contained,
                changed: true,
                ..
            },
            ..
        }
    ));
    assert!(fixture.engine.workspace().root(ROOT_NEW).is_some());
    let floating = fixture
        .engine
        .workspace()
        .contained_floating(FLOATING_NEW)
        .expect("contained presentation must exist");
    assert_eq!(floating.root, ROOT_NEW);
    assert_eq!(floating.surface, SURFACE_B);
}

#[test]
fn native_release_prepares_a_saga_without_moving_content() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = fixture(policy, &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let request = TearOffRequest::Native {
        proposal: NativeTearOffProposal::new(
            SURFACE_NEW,
            ROOT_NEW,
            physical_rect(100.0, 120.0, 640.0, 480.0),
        ),
        capability: NativeTearOffCapability::Supported,
        contained_fallback: None,
    };
    let before = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    let preview = preview_tear_off(&mut fixture, session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Native { .. })
    ));
    let delivery = release_tear_off(&mut fixture, session, request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::NativePrepared(ref prepared),
            ..
        } if prepared.source_version() == version
            && prepared.proposal().surface() == SURFACE_NEW
            && prepared.payload() == &MovePayload::Item(
                before.capture_item_source(ROOT_A, fixture.tabs_a, ItemId::new(1))
                    .expect("original source must be capturable")
            )
    ));
    assert_eq!(fixture.engine.workspace(), &before);
    assert_eq!(fixture.engine.version(), version);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.scene().is_some());
}

#[test]
fn native_tear_off_rejects_an_existing_surface_even_for_a_complete_root() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = fixture(policy, &[1]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let request = TearOffRequest::Native {
        proposal: NativeTearOffProposal::new(
            SURFACE_A,
            ROOT_A,
            physical_rect(100.0, 120.0, 640.0, 480.0),
        ),
        capability: NativeTearOffCapability::Supported,
        contained_fallback: None,
    };

    let outcome = preview_tear_off(&mut fixture, session, request);
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));
    assert!(fixture.engine.workspace().surface(SURFACE_A).is_some());
    assert!(fixture.engine.interaction().preview().is_none());
}

#[test]
fn native_unavailable_uses_contained_only_when_fallback_is_explicitly_enabled() {
    let mut disabled_policy = DockPolicy::default();
    disabled_policy.set_allow_native_surfaces(true);
    let mut disabled = fixture(disabled_policy, &[1, 2]);
    publish_scene(&mut disabled);
    let disabled_session = arm_and_begin(&mut disabled);
    let request = TearOffRequest::Native {
        proposal: NativeTearOffProposal::new(
            SURFACE_NEW,
            ROOT_NEW,
            physical_rect(10.0, 10.0, 500.0, 400.0),
        ),
        capability: NativeTearOffCapability::Unsupported(
            NativeTearOffUnavailableReason::BackendUnsupported,
        ),
        contained_fallback: Some(contained_proposal(ROOT_NEW, 10.0)),
    };
    let outcome = preview_tear_off(&mut disabled, disabled_session, request.clone());
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));

    let mut enabled_policy = DockPolicy::default();
    enabled_policy.set_allow_native_surfaces(true);
    enabled_policy.set_contained_fallback(ContainedFallback::Enabled);
    let mut enabled = fixture(enabled_policy, &[1, 2]);
    publish_scene(&mut enabled);
    let enabled_session = arm_and_begin(&mut enabled);
    let preview = preview_tear_off(&mut enabled, enabled_session, request.clone());
    assert!(matches!(
        preview,
        InteractionOutcome::PreviewUpdated {
            preview: Some(ref preview),
            ..
        } if matches!(preview.visual(), PreviewVisual::Contained { fallback: true, .. })
    ));
    let delivery = release_tear_off(&mut enabled, enabled_session, request);
    assert!(matches!(
        delivery,
        InteractionOutcome::DragDelivered {
            delivery: InteractionDelivery::Workspace {
                kind: WorkspaceDeliveryKind::ContainedFallback,
                changed: true,
                ..
            },
            ..
        }
    ));
}

#[test]
fn unknown_native_capability_cancels_instead_of_falling_back() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy.set_contained_fallback(ContainedFallback::Enabled);
    let mut fixture = fixture(policy, &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let request = TearOffRequest::Native {
        proposal: NativeTearOffProposal::new(
            SURFACE_NEW,
            ROOT_NEW,
            physical_rect(10.0, 10.0, 500.0, 400.0),
        ),
        capability: NativeTearOffCapability::Unknown(
            NativeTearOffUnavailableReason::RoutingUnavailable,
        ),
        contained_fallback: Some(contained_proposal(ROOT_NEW, 10.0)),
    };

    let outcome = preview_tear_off(&mut fixture, session, request);
    assert!(matches!(
        outcome,
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::NativeCapabilityUnknown,
            ..
        }
    ));
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn release_rejects_a_changed_tear_off_placement_even_after_ack() {
    let mut fixture = fixture(DockPolicy::default(), &[1, 2]);
    publish_scene(&mut fixture);
    let session = arm_and_begin(&mut fixture);
    let painted = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0));
    let changed = TearOffRequest::Contained(contained_proposal(ROOT_NEW, 30.0));
    preview_tear_off(&mut fixture, session, painted);
    let before = fixture.engine.workspace().clone();

    let outcome = release_tear_off(&mut fixture, session, changed);
    assert!(matches!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TargetChanged)
    ));
    assert_eq!(fixture.engine.workspace(), &before);
}

#[test]
fn complete_root_tear_off_preserves_root_identity_and_rejects_replacement_identity() {
    let mut mismatch = fixture(DockPolicy::default(), &[1]);
    publish_scene(&mut mismatch);
    let mismatch_session = arm_and_begin(&mut mismatch);
    let outcome = preview_tear_off(
        &mut mismatch,
        mismatch_session,
        TearOffRequest::Contained(contained_proposal(ROOT_NEW, 10.0)),
    );
    assert!(matches!(
        outcome,
        InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }
    ));

    let mut preserved = fixture(DockPolicy::default(), &[1]);
    publish_scene(&mut preserved);
    let session = arm_and_begin(&mut preserved);
    let request = TearOffRequest::Contained(contained_proposal(ROOT_A, 10.0));
    preview_tear_off(&mut preserved, session, request.clone());
    let delivery = release_tear_off(&mut preserved, session, request);
    assert!(matches!(delivery, InteractionOutcome::DragDelivered { .. }));
    assert!(preserved.engine.workspace().root(ROOT_A).is_some());
    assert!(preserved.engine.workspace().root(ROOT_NEW).is_none());
    assert!(preserved.engine.workspace().surface(SURFACE_A).is_none());
    assert_eq!(
        preserved
            .engine
            .workspace()
            .contained_floating(FLOATING_NEW)
            .expect("root must be rehomed")
            .root,
        ROOT_A
    );
}
