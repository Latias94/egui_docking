mod support;

use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectResult, EffectTransition, PlatformEffect,
};
use dockspace::engine::{DockEngine, EngineError, EngineInput};
use dockspace::frame::{PanelFocus, ViewportCoordinatorError};
use dockspace::geometry::{LogicalPoint, LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{InteractionCancelReason, InteractionOutcome, InteractionStatus};
use dockspace::platform::{
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCoordinateObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::{
    DesktopDockRoute, DesktopRouteFact, PointerCaptureOwner, PointerEdge, PointerEdgeJournal,
    PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence, PointerEventDeliveryOwner,
    PointerProviderScope, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::transition::InputOutcome;
use dockspace::viewport::{
    CoordinateObservationGeneration, PresentationObservationGeneration, ViewportBinding,
    ViewportRole, WindowToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
    ViewportActivationRequest,
};
use dockspace::{PlatformObservationAuthorityError, PlatformObservationLease};
use support::{
    TestPresentationHost, complete_host_frame_with_retained_or_unavailable, publish_surface,
    submit_input,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const WINDOW: WindowToken = WindowToken::new(1);
const POINTER: PointerId = PointerId::new(1);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0x5052_4f56_4944_4552);

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("provider test workspace is valid")
}

fn logical_bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test logical bounds are valid")
}

fn physical_bounds() -> PhysicalRect {
    PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test physical bounds are valid")
}

fn platform_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    capabilities
}

fn platform_snapshot(generation: u64, binding: ViewportBinding) -> PlatformSnapshot {
    let window = ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_bounds()),
            Authority::Known(physical_bounds()),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let windows = vec![window];
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, platform_capabilities()),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        support::known_inventory_observation(generation, &windows),
        windows,
        Vec::new(),
        support::unknown_work_area_observation(generation),
    )
    .expect("provider test platform snapshot is canonical")
}

struct Fixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: PlatformObservationLease,
    binding: ViewportBinding,
}

fn fixture() -> Fixture {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let provider = host.platform_provider();
    let expected = engine.version();
    let registered = submit_input(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE,
            token: WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("viewport registration reduces");
    let InputOutcome::ViewportRegistered { binding } = registered.reduced_inputs()[0].outcome()
    else {
        panic!(
            "unexpected viewport registration outcome: {:?}",
            registered.reduced_inputs()[0].outcome()
        );
    };
    let binding = *binding;
    let expected_epoch = engine.version().epoch();
    let published = submit_input(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: platform_snapshot(1, binding),
        },
    )
    .expect("initial provider snapshot reduces");
    assert!(matches!(
        published.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    Fixture {
        engine,
        host,
        provider,
        binding,
    }
}

fn request_focus_effect(fixture: &mut Fixture) -> dockspace::effect::PlatformEffectEmission {
    let expected = fixture.engine.version();
    let transition = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(fixture.binding, PanelFocus::Item(ITEM)),
        },
    )
    .expect("explicit activation reduces");
    transition
        .platform_effects()
        .iter()
        .find(|emission| {
            matches!(
                emission.effect(),
                PlatformEffect::RequestFocus { binding, .. } if *binding == fixture.binding
            )
        })
        .cloned()
        .expect("explicit activation emits one focus request")
}

fn arm_desktop_global_tab_drag(fixture: &mut Fixture) {
    publish_surface(
        &mut fixture.engine,
        &mut fixture.host,
        SURFACE,
        logical_bounds(),
    );
    let provider = fixture
        .engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global pointer provider is admitted");
    let projection = fixture
        .engine
        .interaction_projection(SURFACE)
        .expect("published surface has interaction authority");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabBody(_)))
        .expect("published surface has a tab body receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("tab midpoint is finite");
    let desktop = PhysicalPoint::new(point.x(), point.y()).expect("desktop point is finite");
    let route = DesktopRouteFact::dock(DesktopDockRoute::new(
        fixture.binding,
        projection.authority().coordinate_generation(),
        desktop,
        point,
    ));
    let sequence = PointerEdgeSequence::new(1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        sequence,
        vec![PointerEdge::new_with_delivery(
            sequence,
            POINTER,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::Desktop { route },
            Authority::Known(PointerEventDeliveryOwner::Native(fixture.binding)),
            Authority::Known(PointerCaptureOwner::Native(fixture.binding)),
        )],
    )
    .expect("desktop press journal is contiguous");

    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("desktop press freezes its receiver candidate");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("desktop press has a receiver roster")
        .candidates()[0]
        .clone();
    let frame_projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("frozen frame retains interaction authority");
    let delivery = PointerReceiverDelivery::new(
        frame_projection,
        PointerReceiverDeliveryDisposition::Dock(region.id()),
    )
    .expect("tab delivery is bound to the frozen presentation");
    let presented =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(delivery)])
            .expect("press answers its exact delivery probe");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(presented))
            ])
            .expect("one receipt answers the candidate exactly once"),
        )
        .expect("desktop press receipt stages");
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        fixture.engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
}

fn arm_surface_local_native_tab_drag(fixture: &mut Fixture) -> SurfaceLocalPointerProvider {
    publish_surface(
        &mut fixture.engine,
        &mut fixture.host,
        SURFACE,
        logical_bounds(),
    );
    let provider = fixture
        .engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                fixture.host.lease(),
                SurfaceLocalPointerEndpoint::Native(fixture.binding),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("native surface-local pointer provider is admitted");
    let projection = fixture
        .engine
        .interaction_projection(SURFACE)
        .expect("published native surface has interaction authority");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabBody(_)))
        .expect("published native surface has a tab body receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("native tab midpoint is finite");
    let sequence = PointerEdgeSequence::new(1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    )
    .expect("native surface-local press journal is contiguous");

    let mut frame = fixture.host.begin(&fixture.engine);
    frame
        .submit_surface_pointer_journal(&provider, journal)
        .expect("native surface-local press freezes its receiver candidate");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("native surface-local press has a receiver roster")
        .candidates()[0]
        .clone();
    let frame_projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("frozen native frame retains interaction authority");
    let delivery = PointerReceiverDelivery::new(
        frame_projection,
        PointerReceiverDeliveryDisposition::Dock(region.id()),
    )
    .expect("native tab delivery is bound to the frozen presentation");
    let presented =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(delivery)])
            .expect("native press answers its exact delivery probe");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(presented))
            ])
            .expect("one native receipt answers the candidate exactly once"),
        )
        .expect("native surface-local press receipt stages");
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    assert_eq!(
        provider.committed_through(),
        sequence,
        "native press advances the committed pointer watermark"
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        fixture.engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    provider
}

#[test]
fn replacement_revokes_a1_ingress_effect_results_and_owned_desktop_gesture() {
    let mut fixture = fixture();
    let a1_emission = request_focus_effect(&mut fixture);
    assert_eq!(a1_emission.provider(), fixture.provider);
    arm_desktop_global_tab_drag(&mut fixture);
    let retired_pointer_provider = fixture
        .engine
        .pointer_provider()
        .expect("desktop-global provider remains live before replacement");

    let start = fixture
        .engine
        .begin_platform_provider_replacement(fixture.provider)
        .expect("A1 replacement begins atomically");
    let ticket = start.ticket();
    assert_eq!(ticket.predecessor(), fixture.provider);
    assert_eq!(fixture.engine.platform_provider(), None);
    assert_eq!(fixture.engine.pointer_provider(), None);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(start.transition().platform_effects().is_empty());
    assert!(matches!(
        start.transition().interaction_events(),
        [event]
            if matches!(
                event.kind(),
                dockspace::interaction::InteractionEventKind::Cancelled {
                    status: InteractionStatus::Armed { .. },
                    reason: InteractionCancelReason::PointerProviderRetired,
                }
            )
    ));

    let expected_epoch = fixture.engine.version().epoch();
    let late_snapshot = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider: fixture.provider,
            expected_epoch,
            snapshot: platform_snapshot(2, fixture.binding),
        },
    )
    .expect("late A1 snapshot is consumed as a typed inert input");
    assert!(matches!(
        late_snapshot.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformProviderRejected {
            provider,
            error: PlatformObservationAuthorityError::SupersededLease { lease, .. },
        } if *provider == fixture.provider && *lease == fixture.provider
    ));

    let late_result = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::ReportPlatformEffect {
            provider: fixture.provider,
            expected_epoch,
            result: EffectResult::new(
                a1_emission.id(),
                a1_emission.epoch(),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            ),
        },
    )
    .expect("late A1 result is consumed as a typed inert input");
    assert!(matches!(
        late_result.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformProviderRejected {
            provider,
            error: PlatformObservationAuthorityError::SupersededLease { lease, .. },
        } if *provider == fixture.provider && *lease == fixture.provider
    ));

    let a2 = fixture
        .engine
        .finish_platform_provider_replacement(ticket)
        .expect("exact handoff ticket activates A2");
    assert_ne!(a2, fixture.provider);
    assert_eq!(fixture.engine.platform_provider(), Some(a2));

    let a2_snapshot = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider: a2,
            expected_epoch,
            snapshot: platform_snapshot(1, fixture.binding),
        },
    )
    .expect("A2 starts a fresh provider-local observation namespace");
    assert!(matches!(
        a2_snapshot.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    let a2_emission = request_focus_effect(&mut fixture);
    assert_eq!(a2_emission.provider(), a2);
    assert!(matches!(
        a2_emission.effect(),
        PlatformEffect::RequestFocus { after: None, .. }
    ));

    let a2_result_for_a1 = submit_input(
        &mut fixture.engine,
        &mut fixture.host,
        INPUT_SOURCE,
        EngineInput::ReportPlatformEffect {
            provider: a2,
            expected_epoch,
            result: EffectResult::new(
                a1_emission.id(),
                a1_emission.epoch(),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            ),
        },
    )
    .expect("A2 mismatch is a typed effect outcome");
    assert!(matches!(
        a2_result_for_a1.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformEffectReported {
            effect,
            transition: EffectTransition::ProviderMismatch,
            ..
        } if *effect == a1_emission.id()
    ));

    assert!(matches!(
        fixture
            .engine
            .retire_pointer_provider(retired_pointer_provider),
        Err(EngineError::PointerJournal { .. })
    ));
}

#[test]
fn foreign_and_duplicate_replacement_tickets_are_rejected_atomically() {
    let mut first = fixture();
    let first_start = first
        .engine
        .begin_platform_provider_replacement(first.provider)
        .expect("first handoff begins");

    let mut foreign = fixture();
    let foreign_start = foreign
        .engine
        .begin_platform_provider_replacement(foreign.provider)
        .expect("foreign handoff begins");
    let tick_before_foreign_ticket = first.engine.last_reducer_tick();
    assert!(matches!(
        first
            .engine
            .finish_platform_provider_replacement(foreign_start.ticket()),
        Err(EngineError::Viewport {
            source: ViewportCoordinatorError::PlatformProvider(
                PlatformObservationAuthorityError::ForeignReplacementTicket { .. }
            ),
            ..
        })
    ));
    assert_eq!(first.engine.platform_provider(), None);
    assert_eq!(first.engine.last_reducer_tick(), tick_before_foreign_ticket);

    let ticket = first_start.ticket();
    let a2 = first
        .engine
        .finish_platform_provider_replacement(ticket)
        .expect("the still-pending exact ticket activates A2");
    assert_eq!(first.engine.platform_provider(), Some(a2));
    let tick_before_duplicate = first.engine.last_reducer_tick();
    assert!(matches!(
        first.engine.finish_platform_provider_replacement(ticket),
        Err(EngineError::Viewport {
            source: ViewportCoordinatorError::PlatformProvider(
                PlatformObservationAuthorityError::UnknownReplacementTicket
            ),
            ..
        })
    ));
    assert_eq!(first.engine.platform_provider(), Some(a2));
    assert_eq!(first.engine.last_reducer_tick(), tick_before_duplicate);
}

#[test]
fn replacement_retires_native_surface_local_provider_and_its_owned_gesture() {
    let mut fixture = fixture();
    let retired = arm_surface_local_native_tab_drag(&mut fixture);
    assert_eq!(fixture.engine.pointer_provider(), Some(retired.lease()));

    let start = fixture
        .engine
        .begin_platform_provider_replacement(fixture.provider)
        .expect("native provider replacement begins atomically");

    assert_eq!(fixture.engine.platform_provider(), None);
    assert_eq!(fixture.engine.pointer_provider(), None);
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(matches!(
        start.transition().interaction_events(),
        [event]
            if matches!(
                event.kind(),
                dockspace::interaction::InteractionEventKind::Cancelled {
                    status: InteractionStatus::Armed { .. },
                    reason: InteractionCancelReason::PointerProviderRetired,
                }
            )
    ));
    let mut receipt = retired
        .drain()
        .expect("retired native provider has no in-flight host frame");
    assert!(matches!(
        fixture
            .engine
            .retire_quiesced_surface_local_pointer_provider(&mut receipt),
        Ok(outcome) if !outcome.interaction_changed()
    ));
}

#[test]
fn replacement_preserves_independent_logical_surface_local_provider() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let platform_provider = host.platform_provider();
    publish_surface(&mut engine, &mut host, SURFACE, logical_bounds());
    let logical_provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("logical surface-local pointer provider is admitted");

    let start = engine
        .begin_platform_provider_replacement(platform_provider)
        .expect("platform provider replacement preserves logical input authority");
    assert_eq!(engine.platform_provider(), None);
    assert_eq!(engine.pointer_provider(), Some(logical_provider.lease()));
    assert!(start.transition().interaction_events().is_empty());

    let mut frame = host.begin(&engine);
    frame
        .submit_surface_pointer_journal(
            &logical_provider,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("logical provider preserves its committed watermark"),
        )
        .expect("logical provider remains authorized during platform handoff");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::new())
                .expect("empty journal has an exact empty receipt batch"),
        )
        .expect("logical provider empty receipt batch stages");
    complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert_eq!(
        logical_provider.committed_through(),
        PointerEdgeSequence::new(0),
        "empty checkpoint preserves the committed pointer watermark"
    );
    assert!(transition.reduced_pointer_edges().is_empty());
    assert_eq!(engine.pointer_provider(), Some(logical_provider.lease()));

    let replacement = engine
        .finish_platform_provider_replacement(start.ticket())
        .expect("exact ticket activates the successor platform provider");
    assert_eq!(engine.platform_provider(), Some(replacement));
    assert_eq!(engine.pointer_provider(), Some(logical_provider.lease()));

    let mut receipt = logical_provider
        .drain()
        .expect("logical provider has no in-flight host frame");
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        .expect("logical provider retires at its committed watermark");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    assert_eq!(engine.pointer_provider(), None);
}
