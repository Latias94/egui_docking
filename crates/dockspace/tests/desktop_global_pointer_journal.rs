mod support;

use dockspace::drop_target::DropTargetId;
use dockspace::effect::PlatformEffect;
use dockspace::engine::{CoreHostFrame, DockEngine, EngineInput};
use dockspace::geometry::{LogicalPoint, LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::{
    InteractionCancelReason, InteractionDelivery, InteractionOutcome, InteractionRejection,
    InteractionStatus, PreviewResolutionStatus, PreviewVisual, ScrollReductionOutcome,
};
use dockspace::platform::{
    InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::{
    DesktopRouteFact, DesktopWorkAreaRoute, FiniteScrollVector, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerEventDeliveryOwner, PointerInputLease, PointerProviderScope, ScrollDeliveryEndpoint,
    ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation, PointerReceiverProbeReceipt,
    PointerReceiverProbeRequest, PointerReceiverReceipt, PointerReceiverReceiptBatch,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::transition::InputOutcome;
use dockspace::viewport::{
    CoordinateObservationGeneration, InputObservationGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaGeneration, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use support::{
    TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_retained_or_unavailable, publish_surfaces, submit_input,
    submit_inputs,
};

const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const SOURCE_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const SOURCE_WINDOW: WindowToken = WindowToken::new(10);
const TARGET_WINDOW: WindowToken = WindowToken::new(20);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xD0C5);
const POINTER: PointerId = PointerId::new(7);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(1);

const SOURCE_ORIGIN_X: f64 = 0.0;
const TARGET_ORIGIN_X: f64 = 2_000.0;
const SOURCE_SCALE: f64 = 1.0;
const TARGET_SCALE: f64 = 2.0;

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("test logical rectangle is valid")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle is valid")
}

fn workspace() -> (Workspace, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(
        SOURCE_ROOT,
        RootRecord::new(source_tabs).with_central(source_tabs),
    );
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder.build().expect("test workspace is valid"),
        source_tabs,
        target_tabs,
    )
}

fn scroll_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_items = [
        ItemId::new(1),
        ItemId::new(2),
        ItemId::new(3),
        ItemId::new(4),
        ItemId::new(5),
    ];
    let source_tabs = builder.insert_node(Node::tabs_with_selection(
        source_items,
        Some(source_items[0]),
    ));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(6)]));
    builder.set_root(
        SOURCE_ROOT,
        RootRecord::new(source_tabs).with_central(source_tabs),
    );
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    builder.build().expect("scroll workspace is valid")
}

fn workspace_without_source_central_root() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder.build().expect("test workspace is valid"),
        target_tabs,
    )
}

fn capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities
}

fn work_area_observation(generation: u64) -> dockspace::platform::WorkAreaRosterObservation {
    work_area_observation_with_scale(generation, 1.0)
}

fn work_area_observation_with_scale(
    generation: u64,
    scale: f64,
) -> dockspace::platform::WorkAreaRosterObservation {
    support::known_work_area_observation(
        generation,
        vec![ObservedWorkArea::new(
            WORK_AREA,
            physical_rect(-1_000.0, -1_000.0, 5_000.0, 3_000.0),
            ScaleFactor::new(scale).expect("test work-area scale is valid"),
        )],
    )
}

fn observed_window(
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    origin_x: f64,
    scale: f64,
) -> ObservedWindow {
    observed_window_at(binding, generation, origin_x, 0.0, scale)
}

fn observed_window_at(
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    origin_x: f64,
    origin_y: f64,
    scale: f64,
) -> ObservedWindow {
    let width = 640.0 * scale;
    let height = 480.0 * scale;
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            generation,
            Authority::Known(physical_rect(origin_x, origin_y, width, height)),
            Authority::Known(physical_rect(origin_x, origin_y, width, height)),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
        ))
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(generation.get()),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
}

fn publish_native_snapshot(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: ViewportBinding,
    target: ViewportBinding,
) {
    publish_native_snapshot_with_work_area_scale(engine, host, source, target, 1.0);
}

fn publish_native_snapshot_with_work_area_scale(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: ViewportBinding,
    target: ViewportBinding,
    work_area_scale: f64,
) {
    publish_native_snapshot_with_geometry_and_work_area_scale(
        engine,
        host,
        source,
        target,
        (SOURCE_ORIGIN_X, 0.0),
        (TARGET_ORIGIN_X, 0.0),
        work_area_scale,
    );
}

fn publish_native_snapshot_with_geometry(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: ViewportBinding,
    target: ViewportBinding,
    source_origin: (f64, f64),
    target_origin: (f64, f64),
) {
    publish_native_snapshot_with_geometry_and_work_area_scale(
        engine,
        host,
        source,
        target,
        source_origin,
        target_origin,
        1.0,
    );
}

fn publish_native_snapshot_with_geometry_and_work_area_scale(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    source: ViewportBinding,
    target: ViewportBinding,
    source_origin: (f64, f64),
    target_origin: (f64, f64),
    work_area_scale: f64,
) {
    let generation = host.next_platform_observation_generation();
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let source_window = observed_window_at(
        source,
        coordinate_generation,
        source_origin.0,
        source_origin.1,
        SOURCE_SCALE,
    )
    .with_presentation_observation(WindowPresentationObservation::new(
        source,
        presentation_generation,
        Authority::Known(WindowPresentationState::Visible),
        PresentationEffectAcknowledgement::known(None),
    ));
    let target_window = observed_window_at(
        target,
        coordinate_generation,
        target_origin.0,
        target_origin.1,
        TARGET_SCALE,
    )
    .with_presentation_observation(WindowPresentationObservation::new(
        target,
        presentation_generation,
        Authority::Known(WindowPresentationState::Visible),
        PresentationEffectAcknowledgement::known(None),
    ));
    let windows = vec![source_window, target_window];
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    let snapshot = PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        work_area_observation_with_scale(generation, work_area_scale),
    )
    .expect("test native platform snapshot is canonical");
    let expected_epoch = engine.version().epoch();
    let platform_provider = host.platform_provider();
    let transition = submit_input(
        engine,
        host,
        INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider: platform_provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("native snapshot publishes");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
}

fn publish_resized_source_snapshot(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source: ViewportBinding,
    target: ViewportBinding,
) {
    let generation = host.next_platform_observation_generation();
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let source_window = observed_window(source, coordinate_generation, SOURCE_ORIGIN_X, 1.25)
        .with_presentation_observation(WindowPresentationObservation::new(
            source,
            presentation_generation,
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let target_window =
        observed_window(target, coordinate_generation, TARGET_ORIGIN_X, TARGET_SCALE)
            .with_presentation_observation(WindowPresentationObservation::new(
                target,
                presentation_generation,
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
    let windows = vec![source_window, target_window];
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    let snapshot = PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        work_area_observation(generation),
    )
    .expect("resized-source platform snapshot is canonical");
    let source_sequence = engine
        .semantic_input_watermark()
        .expect("native setup published platform input")
        .checked_next()
        .expect("test source sequence does not exhaust");
    let expected_epoch = engine.version().epoch();
    let platform_provider = host.platform_provider();
    let mut frame = host.begin(engine);
    frame
        .append_input(
            INPUT_SOURCE,
            source_sequence,
            EngineInput::PublishPlatformSnapshot {
                provider: platform_provider,
                expected_epoch,
                snapshot,
            },
        )
        .expect("resized-source snapshot stages");
    frame
        .submit_pointer_journal(
            provider,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(2),
                PointerEdgeSequence::new(2),
                Vec::new(),
            )
            .expect("platform-only frame preserves the provider watermark"),
        )
        .expect("platform-only frame stages an empty journal");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has no receiver receipts"),
        )
        .expect("empty receipt set stages");
    complete(engine, &mut frame);
    let transition = host.finish(frame, engine);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
}

fn source_only_native_snapshot(generation: u64, source: ViewportBinding) -> PlatformSnapshot {
    let source_window = observed_window(
        source,
        CoordinateObservationGeneration::new(generation),
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
    )
    .with_presentation_observation(WindowPresentationObservation::new(
        source,
        PresentationObservationGeneration::new(generation),
        Authority::Known(WindowPresentationState::Visible),
        PresentationEffectAcknowledgement::known(None),
    ));
    let windows = vec![source_window];
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        work_area_observation(generation),
    )
    .expect("source-only native snapshot is canonical")
}

fn resized_target_native_snapshot(
    generation: u64,
    source: ViewportBinding,
    target: ViewportBinding,
) -> PlatformSnapshot {
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let source_window =
        observed_window(source, coordinate_generation, SOURCE_ORIGIN_X, SOURCE_SCALE)
            .with_presentation_observation(WindowPresentationObservation::new(
                source,
                presentation_generation,
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
    let target_window = observed_window(target, coordinate_generation, TARGET_ORIGIN_X, 1.5)
        .with_presentation_observation(WindowPresentationObservation::new(
            target,
            presentation_generation,
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let windows = vec![source_window, target_window];
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        work_area_observation(generation),
    )
    .expect("resized-target native snapshot is canonical")
}

fn interaction(
    engine: &DockEngine,
    surface: SurfaceId,
) -> dockspace::scene::SurfaceInteractionProjection<'_> {
    engine
        .interaction_projection(surface)
        .expect("native test surface has an exact interactive output")
}

fn tab_point(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    item: ItemId,
) -> (
    dockspace::presentation_hit::PresentationHitRegionId,
    LogicalPoint,
) {
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == item
            )
        })
        .expect("source item has a tab receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("tab midpoint is finite");
    (region.id(), point)
}

fn tab_close_point(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    item: ItemId,
) -> (
    dockspace::presentation_hit::PresentationHitRegionId,
    LogicalPoint,
) {
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabClose(tab) if tab.item == item
            )
        })
        .expect("source item has a tab-close receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("tab-close midpoint is finite");
    (region.id(), point)
}

fn center_drop_point(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    tabs: NodeId,
) -> (
    dockspace::presentation_hit::PresentationHitRegionId,
    LogicalPoint,
) {
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::DropTarget(DropTargetId::Center { tabs: actual, .. })
                    if actual == tabs
            )
        })
        .expect("target surface has a center drop receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("drop target midpoint is finite");
    (region.id(), point)
}

fn desktop_route(
    binding: ViewportBinding,
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    origin_x: f64,
    scale: f64,
    local: LogicalPoint,
) -> DesktopRouteFact {
    assert_eq!(projection.authority().binding(), Some(binding));
    let physical = PhysicalPoint::new(origin_x + local.x() * scale, local.y() * scale)
        .expect("desktop test point is finite");
    DesktopRouteFact::dock_from_desktop_position(binding, Authority::Known(physical))
}

fn outside_all_route(engine: &DockEngine, position: PhysicalPoint) -> DesktopRouteFact {
    outside_all_route_with_work_area(
        position,
        Authority::Known(DesktopWorkAreaRoute::new(
            engine
                .platform_provider()
                .expect("native test platform provider remains active"),
            engine.viewport().work_area_generation(),
            WORK_AREA,
        )),
    )
}

fn outside_all_route_with_work_area(
    position: PhysicalPoint,
    work_area: Authority<DesktopWorkAreaRoute>,
) -> DesktopRouteFact {
    DesktopRouteFact::no_window(Authority::Known(position), work_area)
}

fn pointer_edge_journal(
    previous: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdgeJournal {
    let delivery = match capture {
        Authority::Known(PointerCaptureOwner::Native(binding)) => {
            Authority::Known(PointerEventDeliveryOwner::Native(binding))
        }
        Authority::Known(
            PointerCaptureOwner::ProviderEndpoint
            | PointerCaptureOwner::Foreign
            | PointerCaptureOwner::None,
        )
        | Authority::Unknown(_) => Authority::Unknown(AuthorityUnavailableReason::NotReported),
    };
    pointer_edge_journal_with_delivery(previous, kind, location, delivery, capture)
}

fn pointer_edge_journal_with_delivery(
    previous: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    delivery: Authority<PointerEventDeliveryOwner>,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new_with_delivery(
            sequence, POINTER, kind, location, delivery, capture,
        )],
    )
    .expect("single desktop pointer edge is contiguous")
}

fn complete(engine: &DockEngine, frame: &mut CoreHostFrame) {
    complete_host_frame_with_retained_or_unavailable(engine, frame);
}

struct PressedDesktopCloseFixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
}

fn pressed_desktop_close() -> PressedDesktopCloseFixture {
    let (workspace, _, _) = workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let platform_provider = host.platform_provider();
    let expected = engine.version();
    let registration = submit_inputs(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        vec![
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: SOURCE_SURFACE,
                token: SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: TARGET_SURFACE,
                token: TARGET_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("native viewports register");
    let bindings = registration
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect::<Vec<_>>();
    let [source_binding, target_binding] = bindings.as_slice() else {
        panic!("two registrations must mint two bindings: {bindings:?}");
    };
    publish_native_snapshot(&mut engine, &mut host, *source_binding, *target_binding);
    publish_surfaces(
        &mut engine,
        &mut host,
        vec![
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global provider is admitted");
    let projection = interaction(&engine, SOURCE_SURFACE);
    let (close, point) = tab_close_point(projection, ItemId::new(1));
    let route = desktop_route(
        *source_binding,
        projection,
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
        point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(1),
            POINTER,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::Desktop { route },
            Authority::Known(PointerEventDeliveryOwner::Native(*source_binding)),
            Authority::Known(PointerCaptureOwner::Native(*source_binding)),
        )],
    )
    .expect("desktop close press is contiguous");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("desktop close press freezes one receiver candidate");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("desktop close press has a candidate roster")
        .candidates()[0]
        .clone();
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(close))
            .expect("desktop close delivery is output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("desktop close press answers delivery"),
                ),
            )])
            .expect("desktop close press receipt set is exact"),
        )
        .expect("desktop close press receipt stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Pressed { .. }
    ));
    assert_eq!(engine.active_close_plans().count(), 0);
    PressedDesktopCloseFixture {
        engine,
        host,
        provider,
        source_binding: *source_binding,
    }
}

#[test]
fn desktop_close_release_without_a_dock_receiver_terminates_with_typed_causes() {
    let desktop_position = Authority::Known(
        PhysicalPoint::new(10_000.0, 10_000.0).expect("desktop release point is finite"),
    );
    for (route, expected_reason) in [
        (
            DesktopRouteFact::no_window(
                desktop_position,
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
            InteractionCancelReason::ClickReceiverMismatch,
        ),
        (
            DesktopRouteFact::foreign(desktop_position),
            InteractionCancelReason::OpaquePointerBlocker,
        ),
        (
            DesktopRouteFact::unknown(desktop_position, AuthorityUnavailableReason::NotReported),
            InteractionCancelReason::UnknownTargetAuthority,
        ),
    ] {
        let PressedDesktopCloseFixture {
            mut engine,
            mut host,
            provider,
            source_binding,
        } = pressed_desktop_close();
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(1),
            PointerEdgeSequence::new(2),
            vec![PointerEdge::new_with_delivery(
                PointerEdgeSequence::new(2),
                POINTER,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                PointerEdgeLocation::Desktop { route },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            )],
        )
        .expect("desktop close release follows the press watermark");
        let mut frame = host.begin(&engine);
        frame
            .submit_pointer_journal(provider, journal)
            .expect("desktop close release freezes one receiver candidate");
        let candidate = frame
            .pointer_receiver_candidates()
            .expect("desktop close release has a candidate roster")
            .candidates()[0]
            .clone();
        frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::NotApplicable)
                ])
                .expect("non-dock desktop release receipt set is exact"),
            )
            .expect("non-dock desktop release receipt stages");
        complete(&engine, &mut frame);
        let transition = host.finish(frame, &mut engine);

        assert!(matches!(
            transition.reduced_pointer_edges()[0].interaction_outcomes(),
            [InteractionOutcome::Cancelled {
                status: InteractionStatus::Pressed { .. },
                reason,
            }] if *reason == expected_reason
        ));
        assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
        assert_eq!(engine.active_close_plans().count(), 0);
    }
}

struct ActiveNativeDragFixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
    target_point: LogicalPoint,
}

struct DesktopNativeFixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
}

fn desktop_native_fixture() -> DesktopNativeFixture {
    desktop_native_fixture_with_work_area_scale(1.0)
}

fn desktop_native_fixture_with_work_area_scale(work_area_scale: f64) -> DesktopNativeFixture {
    let (workspace, _source_tabs, target_tabs) = workspace();
    desktop_native_fixture_from_workspace(workspace, target_tabs, work_area_scale)
}

fn desktop_native_fixture_from_workspace(
    workspace: Workspace,
    target_tabs: NodeId,
    work_area_scale: f64,
) -> DesktopNativeFixture {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(workspace, policy).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let platform_provider = host.platform_provider();

    let expected = engine.version();
    let registration = submit_inputs(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        [
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: SOURCE_SURFACE,
                token: SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: TARGET_SURFACE,
                token: TARGET_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("two root native bindings register atomically");
    let bindings = registration
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect::<Vec<_>>();
    let [source_binding, target_binding] = bindings.as_slice() else {
        panic!("two registrations must mint two bindings: {bindings:?}");
    };
    let source_binding = *source_binding;
    let target_binding = *target_binding;

    publish_native_snapshot_with_work_area_scale(
        &mut engine,
        &mut host,
        source_binding,
        target_binding,
        work_area_scale,
    );
    publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global provider is admitted");

    DesktopNativeFixture {
        engine,
        host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
    }
}

#[test]
fn desktop_drag_threshold_uses_physical_distance_across_surface_local_spaces() {
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
    } = desktop_native_fixture();

    let source_projection = interaction(&engine, SOURCE_SURFACE);
    let target_projection = interaction(&engine, TARGET_SURFACE);
    let (_, source_point) = tab_point(source_projection, ItemId::new(1));
    let (_, target_point) = center_drop_point(target_projection, target_tabs);
    let source_physical_x = SOURCE_ORIGIN_X + source_point.x() * SOURCE_SCALE;
    let source_physical_y = source_point.y() * SOURCE_SCALE;
    let target_origin_x = source_physical_x + 1.0 - target_point.x() * TARGET_SCALE;
    let target_origin_y = source_physical_y - target_point.y() * TARGET_SCALE;

    engine
        .retire_pointer_provider(provider)
        .expect("coordinate setup retires the initial idle provider");
    publish_native_snapshot_with_geometry(
        &mut engine,
        &mut host,
        source_binding,
        target_binding,
        (SOURCE_ORIGIN_X, 0.0),
        (target_origin_x, target_origin_y),
    );
    publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global provider is reinstalled after coordinate setup");

    let source_projection = interaction(&engine, SOURCE_SURFACE);
    let target_projection = interaction(&engine, TARGET_SURFACE);
    let (source_tab, source_point) = tab_point(source_projection, ItemId::new(1));
    let source_route = desktop_route(
        source_binding,
        source_projection,
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
        source_point,
    );
    let target_physical = PhysicalPoint::new(
        target_origin_x + target_point.x() * TARGET_SCALE,
        target_origin_y + target_point.y() * TARGET_SCALE,
    )
    .expect("target desktop point is finite");
    let target_route = DesktopRouteFact::dock_from_desktop_position(
        target_binding,
        Authority::Known(target_physical),
    );
    let source_physical = source_route
        .position()
        .known()
        .copied()
        .expect("source desktop point is known");
    let target_physical = target_route
        .position()
        .known()
        .copied()
        .expect("target desktop point is known");
    assert_eq!(target_physical.x() - source_physical.x(), 1.0);
    assert_eq!(target_physical.y(), source_physical.y());
    assert!(
        (target_point.x() - source_point.x()).abs()
            > engine.presentation_config().pointer_drag_start_distance()
    );

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                0,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                PointerEdgeLocation::Desktop {
                    route: source_route,
                },
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            ),
        )
        .expect("cross-surface press stages");
    let source_candidate = frame
        .pointer_receiver_candidates()
        .expect("cross-surface press freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let source_delivery = PointerReceiverDelivery::new(
        source_projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source tab receives the press");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([source_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(source_delivery),
                    ])
                    .expect("press answers delivery"),
                ),
            )])
            .expect("cross-surface press receipt set is exact"),
        )
        .expect("cross-surface press receipt reduces");

    frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                1,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: target_route,
                },
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            ),
        )
        .expect("cross-surface move stages");
    let target_candidate = frame
        .pointer_receiver_candidates()
        .expect("cross-surface move freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let (target_region, expected_target_point) = center_drop_point(target_projection, target_tabs);
    assert_eq!(target_point, expected_target_point);
    let target_hover = PointerReceiverHoverHit::new(
        target_projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    )
    .expect("target receiver fact is point-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([target_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(target_hover),
                    ])
                    .expect("move answers hover"),
                ),
            )])
            .expect("cross-surface move receipt set is exact"),
        )
        .expect("cross-surface move receipt reduces");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(
        transition.reduced_pointer_edges()[1]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
}

fn active_native_drag() -> ActiveNativeDragFixture {
    active_native_drag_with_work_area_scale(1.0)
}

fn active_native_drag_with_work_area_scale(work_area_scale: f64) -> ActiveNativeDragFixture {
    active_native_drag_from_capture_and_work_area_scale(work_area_scale, |source| {
        Authority::Known(PointerCaptureOwner::Native(source))
    })
}

fn active_native_drag_from_capture_and_work_area_scale(
    work_area_scale: f64,
    capture: impl FnOnce(ViewportBinding) -> Authority<PointerCaptureOwner>,
) -> ActiveNativeDragFixture {
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
    } = desktop_native_fixture_with_work_area_scale(work_area_scale);

    let initial_capture = capture(source_binding);
    let target_point = arm_native_drag_with_capture(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        0,
        initial_capture,
    );

    ActiveNativeDragFixture {
        engine,
        host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
    }
}

fn arm_native_drag(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
    previous: u64,
) -> LogicalPoint {
    arm_native_drag_with_capture(
        engine,
        host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        previous,
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    )
}

fn arm_native_tab_press(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    capture: Authority<PointerCaptureOwner>,
) {
    let transition = submit_native_tab_press(
        engine,
        host,
        provider,
        source_binding,
        ItemId::new(1),
        capture,
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
}

fn submit_native_tab_press(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    item: ItemId,
    capture: Authority<PointerCaptureOwner>,
) -> dockspace::transition::EngineTransition {
    submit_native_tab_press_with_delivery(
        engine,
        host,
        provider,
        source_binding,
        item,
        Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
        capture,
    )
}

#[allow(clippy::too_many_arguments)]
fn submit_native_tab_press_with_delivery(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    item: ItemId,
    delivery_owner: Authority<PointerEventDeliveryOwner>,
    capture: Authority<PointerCaptureOwner>,
) -> dockspace::transition::EngineTransition {
    let projection = interaction(engine, SOURCE_SURFACE);
    let (source_tab, source_point) = tab_point(projection, item);
    let source_route = desktop_route(
        source_binding,
        projection,
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
        source_point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(1),
            POINTER,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: source_route,
            },
            delivery_owner,
            capture,
        )],
    )
    .expect("native tab press is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("native tab press stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("native tab press freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("native tab receives the press");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("native tab press answers delivery"),
                ),
            )])
            .expect("native tab press receipt set is exact"),
        )
        .expect("native tab press receipt stages");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

#[allow(clippy::too_many_arguments)]
fn arm_native_drag_with_capture(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    source_binding: ViewportBinding,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
    previous: u64,
    capture: Authority<PointerCaptureOwner>,
) -> LogicalPoint {
    let source_projection = interaction(engine, SOURCE_SURFACE);
    let target_projection = interaction(engine, TARGET_SURFACE);
    assert_eq!(
        source_projection.authority().binding(),
        Some(source_binding)
    );
    assert_eq!(
        target_projection.authority().binding(),
        Some(target_binding)
    );

    let (source_tab, source_point) = tab_point(source_projection, ItemId::new(1));
    let (target_drop, target_point) = center_drop_point(target_projection, target_tabs);
    let source_route = desktop_route(
        source_binding,
        source_projection,
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
        source_point,
    );
    let target_route = desktop_route(
        target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    assert_ne!(
        source_projection.authority().coordinate_generation(),
        dockspace::viewport::CoordinateGeneration::default(),
        "native source output must carry a real coordinate authority"
    );
    assert_ne!(
        target_projection.authority().coordinate_generation(),
        dockspace::viewport::CoordinateGeneration::default(),
        "native target output must carry a real coordinate authority"
    );

    let mut gesture_frame = host.begin(engine);
    gesture_frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal_with_delivery(
                previous,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                PointerEdgeLocation::Desktop {
                    route: source_route,
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                capture,
            ),
        )
        .expect("desktop press freezes one exact receiver candidate");
    let source_projection = gesture_frame
        .view()
        .interaction_projection(SOURCE_SURFACE)
        .expect("sealed gesture frame retains source authority");
    let source_candidate = gesture_frame
        .pointer_receiver_candidates()
        .expect("desktop press creates a candidate roster")
        .candidates()[0]
        .clone();

    let source_delivery = PointerReceiverDelivery::new(
        source_projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source receipt is bound to source native output");
    gesture_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([source_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(source_delivery),
                    ])
                    .expect("press answers source delivery"),
                ),
            )])
            .expect("cross-window press receipt set is exact"),
        )
        .expect("cross-window press receipt reduces");

    gesture_frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal_with_delivery(
                previous + 1,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: target_route,
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                capture,
            ),
        )
        .expect("desktop move freezes one exact receiver candidate");
    let target_candidate = gesture_frame
        .pointer_receiver_candidates()
        .expect("desktop move creates a candidate roster")
        .candidates()[0]
        .clone();
    assert_eq!(target_candidate.hover_point(), Some(target_point));
    let target_projection = gesture_frame
        .view()
        .interaction_projection(TARGET_SURFACE)
        .expect("sealed gesture frame retains target authority");
    let target_hover = PointerReceiverHoverHit::new(
        target_projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_drop),
    )
    .expect("target receipt is bound to target native output");
    gesture_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([target_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(target_hover),
                    ])
                    .expect("move answers target hover"),
                ),
            )])
            .expect("cross-window move receipt set is exact"),
        )
        .expect("cross-window move receipt reduces");
    complete(engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, engine);
    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    target_point
}

#[test]
fn canonical_native_drag_starts_source_pointer_passthrough_routing() {
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
    } = desktop_native_fixture();

    let _ = arm_native_drag(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        0,
    );

    assert!(engine.viewport().effects().records().any(|(_, record)| {
        matches!(
            record.request().effect(),
            PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                ..
            } if *binding == source_binding
        )
    }));
}

struct ReincarnatedNativeDragFixture {
    drag: ActiveNativeDragFixture,
    stale_source_binding: ViewportBinding,
}

fn reincarnated_native_drag() -> ReincarnatedNativeDragFixture {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider: stale_provider,
        source_binding: stale_source_binding,
        target_binding: stale_target_binding,
        target_tabs,
        ..
    } = active_native_drag();
    engine
        .retire_pointer_provider(stale_provider)
        .expect("retiring the A1 provider cancels its original gesture");

    let (replacement, _, _) = workspace();
    let replacement_transition = submit_input(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        EngineInput::ReplaceWorkspace(replacement),
    )
    .expect("workspace replacement reincarnates retained native bindings");
    assert!(matches!(
        replacement_transition.reduced_inputs()[0].outcome(),
        InputOutcome::WorkspaceReplaced { .. }
    ));

    let source_binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source native viewport survives replacement")
        .binding();
    let target_binding = engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .expect("target native viewport survives replacement")
        .binding();
    assert_eq!(source_binding.token(), stale_source_binding.token());
    assert_eq!(target_binding.token(), stale_target_binding.token());
    assert_ne!(source_binding, stale_source_binding);
    assert_ne!(target_binding, stale_target_binding);

    publish_native_snapshot(&mut engine, &mut host, source_binding, target_binding);
    publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("A2 desktop-global provider is admitted");
    let target_point = arm_native_drag(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        0,
    );

    ReincarnatedNativeDragFixture {
        drag: ActiveNativeDragFixture {
            engine,
            host,
            provider,
            source_binding,
            target_binding,
            target_tabs,
            target_point,
        },
        stale_source_binding,
    }
}

fn unknown_receipts(frame: &CoreHostFrame) -> PointerReceiverReceiptBatch {
    PointerReceiverReceiptBatch::new(
        frame
            .pointer_receiver_candidates()
            .expect("journal freezes an exact receiver roster")
            .candidates()
            .iter()
            .map(|candidate| {
                let observation = if candidate.receiver_is_applicable() {
                    PointerReceiverObservation::Unknown(
                        PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                    )
                } else {
                    PointerReceiverObservation::NotApplicable
                };
                candidate.receipt(observation)
            }),
    )
    .expect("unknown receipts answer the exact candidate roster")
}

fn submit_active_drag_edge(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    delivery_binding: ViewportBinding,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
    target_point: LogicalPoint,
    kind: PointerEdgeKind,
    capture: Authority<PointerCaptureOwner>,
) -> dockspace::transition::EngineTransition {
    submit_active_drag_edge_with_delivery(
        engine,
        host,
        provider,
        Authority::Known(PointerEventDeliveryOwner::Native(delivery_binding)),
        target_binding,
        target_tabs,
        target_point,
        kind,
        capture,
    )
}

#[allow(clippy::too_many_arguments)]
fn submit_active_drag_edge_with_delivery(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    delivery: Authority<PointerEventDeliveryOwner>,
    target_binding: ViewportBinding,
    target_tabs: NodeId,
    target_point: LogicalPoint,
    kind: PointerEdgeKind,
    capture: Authority<PointerCaptureOwner>,
) -> dockspace::transition::EngineTransition {
    let projection = interaction(engine, TARGET_SURFACE);
    let (target_region, expected_point) = center_drop_point(projection, target_tabs);
    assert_eq!(target_point, expected_point);
    let route = desktop_route(
        target_binding,
        projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            kind,
            PointerEdgeLocation::Desktop { route },
            delivery,
            capture,
        )],
    )
    .expect("active drag edge is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("active drag edge stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("active drag edge freezes one hover candidate")
        .candidates()[0]
        .clone();
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    )
    .expect("active drag hover is output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(target_hover),
                    ])
                    .expect("active drag edge answers hover"),
                ),
            )])
            .expect("active drag receipt set is exact"),
        )
        .expect("active drag receipt stages");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn native_preview_placement_for_work_area_scale(work_area_scale: f64) -> PhysicalRect {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        ..
    } = active_native_drag_with_work_area_scale(work_area_scale);
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");
    let journal = pointer_edge_journal(
        2,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop {
            route: outside_all_route(&engine, outside),
        },
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("outside-all move stages");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("outside-all move has no widget receiver");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            status: PreviewResolutionStatus::Resolved,
            ..
        }]
    ));

    match engine
        .interaction()
        .preview()
        .expect("outside-all move publishes one native preview")
        .visual()
    {
        PreviewVisual::Native { placement, .. } => *placement,
        visual => panic!("outside-all move published a non-native preview: {visual:?}"),
    }
}

#[test]
fn desktop_global_journal_native_placement_scales_grab_offset_and_size_once() {
    let one_x = native_preview_placement_for_work_area_scale(1.0);
    let two_x = native_preview_placement_for_work_area_scale(2.0);
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");
    let assert_near = |actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= 1.0e-9,
            "expected {expected}, got {actual}"
        );
    };

    assert_near(two_x.width(), one_x.width() * 2.0);
    assert_near(two_x.height(), one_x.height() * 2.0);
    assert_near(outside.x() - two_x.x(), (outside.x() - one_x.x()) * 2.0);
    assert_near(outside.y() - two_x.y(), (outside.y() - one_x.y()) * 2.0);
}

#[test]
fn native_preview_rejects_when_the_post_move_recovery_host_would_disappear() {
    let (workspace, target_tabs) = workspace_without_source_central_root();
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        ..
    } = desktop_native_fixture_from_workspace(workspace, target_tabs, 1.0);
    let _ = arm_native_drag(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        0,
    );
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");
    let journal = pointer_edge_journal(
        2,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop {
            route: outside_all_route(&engine, outside),
        },
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("outside-all move stages");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("outside-all move has no widget receiver");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::Rejected,
            ..
        }]
    ));
    assert!(engine.interaction().preview().is_none());
    assert!(engine.viewport().native_create_sagas().next().is_none());
}

#[test]
fn desktop_global_journal_requests_native_create_after_painted_outside_all_preview() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        ..
    } = active_native_drag();
    let before = engine.workspace().clone();
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");

    let move_journal = pointer_edge_journal(
        2,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop {
            route: outside_all_route(&engine, outside),
        },
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut move_frame = host.begin(&engine);
    move_frame
        .submit_pointer_journal(provider, move_journal)
        .expect("outside-all move stages");
    move_frame
        .submit_pointer_receiver_receipts(unknown_receipts(&move_frame))
        .expect("outside-all move has no widget receiver");
    complete(&engine, &mut move_frame);
    let moved = host.finish(move_frame, &mut engine);

    assert!(matches!(
        moved.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: Some(preview),
            status: PreviewResolutionStatus::Resolved,
            ..
        }] if matches!(preview.visual(), PreviewVisual::Native { .. })
    ));
    assert_eq!(engine.workspace(), &before);
    let acknowledgement = engine
        .interaction()
        .preview()
        .expect("outside-all move publishes a native preview")
        .acknowledgement();
    let release_journal = pointer_edge_journal_with_delivery(
        3,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        PointerEdgeLocation::Desktop {
            route: outside_all_route(&engine, outside),
        },
        Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
        Authority::Known(PointerCaptureOwner::None),
    );
    let mut release_frame = host.begin(&engine);
    let input_sequence = engine
        .semantic_input_watermark()
        .expect("native fixture already published semantic input")
        .checked_next()
        .expect("test input sequence does not exhaust");
    release_frame
        .append_input(
            INPUT_SOURCE,
            input_sequence,
            EngineInput::AcknowledgePreview {
                expected: engine.version(),
                acknowledgement,
            },
        )
        .expect("native preview acknowledgement stages before release");
    release_frame
        .submit_pointer_journal(provider, release_journal)
        .expect("outside-all release stages");
    release_frame
        .submit_pointer_receiver_receipts(unknown_receipts(&release_frame))
        .expect("outside-all release has no widget receiver");
    complete(&engine, &mut release_frame);
    let released = host.finish(release_frame, &mut engine);

    assert!(matches!(
        released.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::PreviewAcknowledged { changed: true, .. },
            ..
        }
    ));

    let requested = released.reduced_pointer_edges()[0]
        .interaction_outcomes()
        .iter()
        .find_map(|outcome| match outcome {
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::NativeRequested(request),
                ..
            } => Some(*request),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "outside-all release must request one native create saga: {:#?}",
                released.reduced_pointer_edges()[0].interaction_outcomes()
            )
        });
    assert_eq!(engine.workspace(), &before);
    assert_eq!(requested.binding().surface(), SurfaceId::new(3));
    assert!(
        engine
            .viewport()
            .native_create_saga(requested.saga())
            .is_some()
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn desktop_global_journal_unknown_work_area_clears_native_preview_without_fallback() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        ..
    } = active_native_drag();
    let before = engine.workspace().clone();
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");
    let journal = pointer_edge_journal(
        2,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop {
            route: outside_all_route_with_work_area(
                outside,
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
        },
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("outside-all move stages");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("outside-all move has no widget receiver");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::NativePlacementUnavailable,
            ..
        }]
    ));
    assert_eq!(engine.workspace(), &before);
    assert!(engine.viewport().native_create_sagas().next().is_none());
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
}

#[test]
fn desktop_global_journal_stale_work_area_generation_cannot_authorize_native_preview() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        ..
    } = active_native_drag();
    let before = engine.workspace().clone();
    let current_generation = engine.viewport().work_area_generation();
    let stale_generation = WorkAreaGeneration::default();
    assert_ne!(stale_generation, current_generation);
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("outside-all point is finite");
    let route = outside_all_route_with_work_area(
        outside,
        Authority::Known(DesktopWorkAreaRoute::new(
            engine
                .platform_provider()
                .expect("native test platform provider remains active"),
            stale_generation,
            WORK_AREA,
        )),
    );
    let journal = pointer_edge_journal(
        2,
        PointerEdgeKind::Moved,
        PointerEdgeLocation::Desktop { route },
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("stale work-area move stages as a provider fact");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("outside-all move has no widget receiver");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: None,
            status: PreviewResolutionStatus::NativePlacementUnavailable,
            ..
        }]
    ));
    assert_eq!(engine.workspace(), &before);
    assert!(engine.viewport().native_create_sagas().next().is_none());
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
}

#[test]
fn unknown_capture_with_exact_source_delivery_advances_drag_move() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        ..
    } = active_native_drag();
    let transition = submit_active_drag_edge(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        PointerEdgeKind::Moved,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );

    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .iter()
            .all(|outcome| !matches!(outcome, InteractionOutcome::Cancelled { .. }))
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
}

#[test]
fn unknown_capture_with_exact_source_delivery_reaches_release_pending() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
    } = active_native_drag();
    let before = engine.workspace().clone();
    let transition = submit_active_drag_edge(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );

    let [edge] = transition.reduced_pointer_edges() else {
        panic!("terminal release must reduce exactly one pointer edge");
    };
    assert!(matches!(
        edge.interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            },
            InteractionOutcome::ReleasePending { .. }
        ]
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn unknown_capture_with_exact_source_delivery_arms_and_drags_from_press() {
    let fixture = active_native_drag_from_capture_and_work_area_scale(1.0, |_| {
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    });

    assert!(matches!(
        fixture.engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(fixture.engine.interaction().preview().is_some());
}

#[test]
fn native_press_requires_exact_current_delivery_binding() {
    for unknown in [false, true] {
        let DesktopNativeFixture {
            mut engine,
            mut host,
            provider,
            source_binding,
            target_binding,
            ..
        } = desktop_native_fixture();
        let delivery = if unknown {
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        } else {
            Authority::Known(PointerEventDeliveryOwner::Native(target_binding))
        };

        let transition = submit_native_tab_press_with_delivery(
            &mut engine,
            &mut host,
            provider,
            source_binding,
            ItemId::new(1),
            delivery,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );

        if unknown {
            assert!(matches!(
                transition.reduced_pointer_edges()[0].interaction_outcomes(),
                [InteractionOutcome::Rejected(
                    InteractionRejection::DeliveryAuthorityUnavailable
                )]
            ));
        } else {
            assert!(matches!(
                transition.reduced_pointer_edges()[0].interaction_outcomes(),
                [InteractionOutcome::Rejected(
                    InteractionRejection::DeliveryOwnerMismatch { expected, actual }
                )] if *expected == PointerEventDeliveryOwner::Native(source_binding)
                    && *actual == PointerEventDeliveryOwner::Native(target_binding)
            ));
        }
        assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    }
}

#[test]
fn non_source_delivery_cancels_an_active_native_drag_fail_closed() {
    for case in 0..4 {
        let ActiveNativeDragFixture {
            mut engine,
            mut host,
            provider,
            source_binding,
            target_binding,
            target_tabs,
            target_point,
        } = active_native_drag();
        let before = engine.workspace().clone();
        let (delivery, expected_reason) = match case {
            0 => (
                Authority::Known(PointerEventDeliveryOwner::Native(target_binding)),
                InteractionCancelReason::DeliveryOwnerLost,
            ),
            1 => (
                Authority::Known(PointerEventDeliveryOwner::Foreign),
                InteractionCancelReason::DeliveryOwnerLost,
            ),
            2 => (
                Authority::Known(PointerEventDeliveryOwner::None),
                InteractionCancelReason::DeliveryOwnerLost,
            ),
            _ => (
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                InteractionCancelReason::DeliveryAuthorityUnavailable,
            ),
        };

        let transition = submit_active_drag_edge_with_delivery(
            &mut engine,
            &mut host,
            provider,
            delivery,
            target_binding,
            target_tabs,
            target_point,
            PointerEdgeKind::Moved,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );

        assert!(matches!(
            transition.reduced_pointer_edges()[0].interaction_outcomes(),
            [InteractionOutcome::Cancelled {
                status: InteractionStatus::Dragging { .. },
                reason,
            }] if *reason == expected_reason
        ));
        assert_eq!(engine.workspace(), &before);
        assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
        assert_ne!(source_binding, target_binding);
    }
}

#[test]
fn wrong_known_capture_owner_rejects_primary_press_before_selection_or_gesture() {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(3)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder.build().expect("capture fixture workspace is valid");
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        ..
    } = desktop_native_fixture_from_workspace(workspace, target_tabs, 1.0);
    let before = engine.workspace().clone();

    let transition = submit_native_tab_press(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        ItemId::new(3),
        Authority::Known(PointerCaptureOwner::Native(target_binding)),
    );

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Rejected(
            InteractionRejection::CaptureOwnerMismatch { expected, actual }
        )] if *expected == PointerCaptureOwner::Native(source_binding)
            && *actual == PointerCaptureOwner::Native(target_binding)
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn wrong_native_capture_on_move_cancels_instead_of_updating_the_drag() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        ..
    } = active_native_drag();
    let before = engine.workspace().clone();

    let transition = submit_active_drag_edge(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        PointerEdgeKind::Moved,
        Authority::Known(PointerCaptureOwner::Native(target_binding)),
    );

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::CaptureLost,
        }]
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().preview().is_none());
}

fn submit_capture_change(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: PointerInputLease,
    previous: u64,
    route_binding: ViewportBinding,
    route_origin_x: f64,
    route_scale: f64,
    route_point: LogicalPoint,
    capture: Authority<PointerCaptureOwner>,
) -> dockspace::transition::EngineTransition {
    let route = desktop_route(
        route_binding,
        interaction(engine, route_binding.surface()),
        route_origin_x,
        route_scale,
        route_point,
    );
    let sequence = PointerEdgeSequence::new(previous + 1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::Desktop { route },
            capture,
        )],
    )
    .expect("capture transition journal is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("capture transition freezes an exact receiver roster");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("capture transition receives an exact receipt batch");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

#[test]
fn capture_change_to_another_current_native_window_cancels_desktop_drag() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_point,
        ..
    } = active_native_drag();
    let before = engine.workspace().clone();
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_ne!(source_binding, target_binding);

    let transition = submit_capture_change(
        &mut engine,
        &mut host,
        provider,
        2,
        target_binding,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
        Authority::Known(PointerCaptureOwner::Native(target_binding)),
    );

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::CaptureLost,
        }]
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().preview().is_none());
    assert_eq!(engine.pointer_provider(), Some(provider));
}

#[test]
fn capture_acquisition_from_unowned_state_is_retained_until_explicit_loss() {
    let DesktopNativeFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
    } = desktop_native_fixture();
    arm_native_tab_press(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    let (_, target_point) = center_drop_point(interaction(&engine, TARGET_SURFACE), target_tabs);

    let acquired = submit_capture_change(
        &mut engine,
        &mut host,
        provider,
        1,
        target_binding,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    assert!(
        acquired.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "the exact press-bound owner may acquire capture"
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let moved = submit_active_drag_edge(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
        PointerEdgeKind::Moved,
        Authority::Known(PointerCaptureOwner::Native(source_binding)),
    );
    assert!(matches!(
        moved.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));

    let lost = submit_capture_change(
        &mut engine,
        &mut host,
        provider,
        3,
        target_binding,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
        Authority::Known(PointerCaptureOwner::Foreign),
    );
    assert!(matches!(
        lost.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::CaptureLost,
        }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().preview().is_none());
    assert_eq!(engine.pointer_provider(), Some(provider));
}

#[test]
fn stale_native_capture_after_binding_reincarnation_cancels_without_reducing_later_edges() {
    let ReincarnatedNativeDragFixture {
        drag:
            ActiveNativeDragFixture {
                mut engine,
                mut host,
                provider,
                source_binding,
                target_binding,
                target_tabs,
                target_point,
            },
        stale_source_binding,
    } = reincarnated_native_drag();
    let before = engine.workspace().clone();
    let target_projection = interaction(&engine, TARGET_SURFACE);
    let route = desktop_route(
        target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let mut frame = host.begin(&engine);
    for (previous, kind) in [
        (2, PointerEdgeKind::CaptureChanged),
        (3, PointerEdgeKind::Moved),
        (4, PointerEdgeKind::ButtonReleased(PointerButton::Primary)),
    ] {
        frame
            .submit_pointer_journal(
                provider,
                pointer_edge_journal(
                    previous,
                    kind,
                    PointerEdgeLocation::Desktop { route },
                    Authority::Known(PointerCaptureOwner::Native(stale_source_binding)),
                ),
            )
            .expect("the provider may report the exact retired binding as a fact");
        frame
            .submit_pointer_receiver_receipts(unknown_receipts(&frame))
            .expect("each stale capture edge retains an exact receipt roster");
    }
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::CaptureLost,
        }]
    ));
    assert!(
        transition.reduced_pointer_edges()[1]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(
        transition.reduced_pointer_edges()[2]
            .interaction_outcomes()
            .is_empty()
    );
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().preview().is_none());
    assert_eq!(engine.pointer_provider(), Some(provider));

    let _ = arm_native_drag(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        5,
    );
}

fn assert_stale_native_capture_semantic_edge_cancels(kind: PointerEdgeKind) {
    let ReincarnatedNativeDragFixture {
        drag:
            ActiveNativeDragFixture {
                mut engine,
                mut host,
                provider,
                target_binding,
                target_point,
                ..
            },
        stale_source_binding,
    } = reincarnated_native_drag();
    let before = engine.workspace().clone();
    let route = desktop_route(
        target_binding,
        interaction(&engine, TARGET_SURFACE),
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            kind,
            PointerEdgeLocation::Desktop { route },
            Authority::Known(PointerEventDeliveryOwner::Native(stale_source_binding)),
            Authority::Known(PointerCaptureOwner::Native(stale_source_binding)),
        )],
    )
    .expect("one delayed A1 semantic edge is contiguous");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("the provider may report a delayed exact A1 capture");
    let candidates = frame
        .pointer_receiver_candidates()
        .expect("the active drag edge freezes a receiver roster")
        .candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].probes(),
        PointerReceiverProbeRequest::HoverHit
    );
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("the delayed semantic edge has an exact receipt");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        kind,
        PointerEdgeKind::Moved | PointerEdgeKind::ButtonReleased(PointerButton::Primary)
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::DeliveryOwnerLost,
        }]
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().preview().is_none());
    assert_eq!(engine.pointer_provider(), Some(provider));
}

#[test]
fn stale_native_capture_on_move_cancels_the_fresh_drag() {
    assert_stale_native_capture_semantic_edge_cancels(PointerEdgeKind::Moved);
}

#[test]
fn stale_native_capture_on_release_cannot_authorize_delivery() {
    assert_stale_native_capture_semantic_edge_cancels(PointerEdgeKind::ButtonReleased(
        PointerButton::Primary,
    ));
}

#[test]
fn stale_native_capture_without_a_gesture_is_a_consumed_no_op() {
    let ReincarnatedNativeDragFixture {
        drag:
            ActiveNativeDragFixture {
                mut engine,
                mut host,
                provider,
                target_binding,
                target_point,
                ..
            },
        stale_source_binding,
    } = reincarnated_native_drag();
    engine
        .retire_pointer_provider(provider)
        .expect("retirement clears the setup gesture");
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("idle desktop provider is admitted");
    let route = desktop_route(
        target_binding,
        interaction(&engine, TARGET_SURFACE),
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![PointerEdge::new(
            PointerEdgeSequence::new(1),
            POINTER,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::Desktop { route },
            Authority::Known(PointerCaptureOwner::Native(stale_source_binding)),
        )],
    )
    .expect("idle stale capture is contiguous");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("idle stale capture stages");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("idle stale capture has an exact receipt");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.pointer_provider(), Some(provider));
}

#[test]
fn unknown_capture_authority_does_not_cancel_a_current_native_drag() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        target_binding,
        target_point,
        ..
    } = active_native_drag();
    let route = desktop_route(
        target_binding,
        interaction(&engine, TARGET_SURFACE),
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new(
            PointerEdgeSequence::new(3),
            POINTER,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::Desktop { route },
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )],
    )
    .expect("unknown capture edge is contiguous");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("unknown capture edge stages");
    frame
        .submit_pointer_receiver_receipts(unknown_receipts(&frame))
        .expect("unknown capture edge has an exact receipt");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(engine.interaction().preview().is_some());
}

#[test]
fn source_coordinate_change_retains_journal_drag_but_requires_fresh_source_presentation() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
    } = active_native_drag();

    publish_resized_source_snapshot(
        &mut engine,
        &mut host,
        provider,
        source_binding,
        target_binding,
    );

    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(
        engine
            .interaction()
            .active_drag_view()
            .is_some_and(|drag| drag.target().is_none())
    );
    assert!(engine.interaction().preview().is_none());

    let target_projection = interaction(&engine, TARGET_SURFACE);
    let (target_drop, current_target_point) = center_drop_point(target_projection, target_tabs);
    assert_eq!(current_target_point, target_point);
    let target_route = desktop_route(
        target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let blocked_move = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::Desktop {
                route: target_route,
            },
            Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
            Authority::Known(PointerCaptureOwner::Native(source_binding)),
        )],
    )
    .expect("blocked move remains provider-contiguous");
    let mut blocked_frame = host.begin(&engine);
    blocked_frame
        .submit_pointer_journal(provider, blocked_move)
        .expect("blocked move freezes one receiver candidate");
    let blocked_candidate = blocked_frame
        .pointer_receiver_candidates()
        .expect("blocked move has a receiver roster")
        .candidates()[0]
        .clone();
    let blocked_hover = PointerReceiverHoverHit::new(
        target_projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_drop),
    )
    .expect("blocked move hover remains bound to target output");
    blocked_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([blocked_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(blocked_hover),
                    ])
                    .expect("blocked move answers hover"),
                ),
            )])
            .expect("blocked move receipt set is exact"),
        )
        .expect("blocked move receipts stage");
    complete(&engine, &mut blocked_frame);
    let blocked = host.finish(blocked_frame, &mut engine);

    assert!(matches!(
        blocked.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Rejected(
            dockspace::interaction::InteractionRejection::StaleScene
        )]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(engine.interaction().preview().is_none());
}

#[test]
fn target_coordinate_change_precedes_journal_release_and_blocks_delivery() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
    } = active_native_drag();

    let target_projection = interaction(&engine, TARGET_SURFACE);
    let stale_release_route = desktop_route(
        target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let release = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: stale_release_route,
            },
            Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("stale release remains provider-contiguous input");
    let generation = host.next_platform_observation_generation();
    let snapshot = resized_target_native_snapshot(generation, source_binding, target_binding);
    let source_sequence = engine
        .semantic_input_watermark()
        .expect("native setup published platform input")
        .checked_next()
        .expect("test source sequence does not exhaust");
    let before = engine.workspace().clone();
    let platform_provider = host.platform_provider();

    let mut frame = host.begin(&engine);
    frame
        .append_input(
            INPUT_SOURCE,
            source_sequence,
            EngineInput::PublishPlatformSnapshot {
                provider: platform_provider,
                expected_epoch: engine.version().epoch(),
                snapshot,
            },
        )
        .expect("target coordinate fact stages before release");
    frame
        .submit_pointer_journal(provider, release)
        .expect("stale release stages after the platform fact");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("release freezes one receiver candidate")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Unknown(
                    PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                ),
            )])
            .expect("unknown receiver observation answers the exact candidate"),
        )
        .expect("release receiver authority stages");
    complete(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    assert_eq!(transition.reduced_inputs()[0].causal_ordinal().get(), 0);
    assert_eq!(
        transition.reduced_pointer_edges()[0].causal_ordinal().get(),
        1
    );
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    assert!(
        !transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .iter()
            .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. })),
        "a release using the superseded target coordinate generation must never deliver"
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.workspace(), &before);
    assert!(matches!(
        engine.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. }) if !items.as_slice().contains(&ItemId::new(1))
    ));
}

/// Scroll delivery follows the native receiver which received the event, not
/// the independently observed window under the pointer.
#[test]
fn desktop_scroll_delivery_route_is_independent_from_hover_route() {
    const TARGET_OVERLAP_ORIGIN_X: f64 = -40.0;

    let mut engine =
        DockEngine::new(scroll_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let platform_provider = host.platform_provider();
    let expected = engine.version();
    let registration = submit_inputs(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        [
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: SOURCE_SURFACE,
                token: SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: TARGET_SURFACE,
                token: TARGET_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("overlapping native bindings register");
    let bindings = registration
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect::<Vec<_>>();
    let [source_binding, target_binding] = bindings.as_slice() else {
        panic!("two registrations must mint two bindings: {bindings:?}");
    };

    publish_native_snapshot_with_geometry(
        &mut engine,
        &mut host,
        *source_binding,
        *target_binding,
        (SOURCE_ORIGIN_X, 0.0),
        (TARGET_OVERLAP_ORIGIN_X, 0.0),
    );
    publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 180.0, 180.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 180.0, 180.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global provider is admitted");

    let source_projection = interaction(&engine, SOURCE_SURFACE);
    let source_region = source_projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripScroll(_)
            )
        })
        .expect("overflowing source tab strip publishes a scroll receiver");
    let source_rect = source_region.hit().rect();
    let source_region_id = source_region.id();
    let source_point = LogicalPoint::new(
        source_rect.x() + source_rect.width() * 0.5,
        source_rect.y() + source_rect.height() * 0.5,
    )
    .expect("source scroll receiver midpoint is finite");
    let desktop_position = PhysicalPoint::new(
        SOURCE_ORIGIN_X + source_point.x() * SOURCE_SCALE,
        source_point.y() * SOURCE_SCALE,
    )
    .expect("desktop scroll position is finite");
    let target_point = LogicalPoint::new(
        (desktop_position.x() - TARGET_OVERLAP_ORIGIN_X) / TARGET_SCALE,
        desktop_position.y() / TARGET_SCALE,
    )
    .expect("target-local hover point is finite");
    assert_ne!(source_point, target_point);

    let endpoint = ScrollDeliveryEndpoint::new(
        host.lease(),
        SOURCE_SURFACE,
        Some(*source_binding),
        source_projection.authority().coordinate_generation(),
    )
    .expect("scroll endpoint binds the source presentation");
    let scroll = ScrollEdge::new(
        ScrollDeviceId::new(1),
        None,
        ScrollPhase::Discrete,
        Some(ScrollDelta::Lines(
            FiniteScrollVector::new(-1.0, 0.0).expect("scroll delta is finite"),
        )),
        Authority::Known(ScrollMomentum::Direct),
        Authority::Known(ScrollModifiers::default()),
        Authority::Known(endpoint),
    )
    .expect("discrete scroll edge is valid");
    let edge = PointerEdge::new_with_delivery(
        PointerEdgeSequence::new(1),
        POINTER,
        PointerEdgeKind::Scrolled(scroll),
        PointerEdgeLocation::Desktop {
            route: DesktopRouteFact::dock_from_desktop_position(
                *target_binding,
                Authority::Known(desktop_position),
            ),
        },
        Authority::Known(PointerEventDeliveryOwner::Native(*source_binding)),
        Authority::Known(PointerCaptureOwner::None),
    );
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![edge],
    )
    .expect("scroll journal is contiguous");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("scroll journal stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("scroll requests delivery evidence")
        .candidates()[0]
        .clone();
    assert_eq!(candidate.route_point(), Some(source_point));
    assert_ne!(candidate.route_point(), Some(target_point));
    let delivery = PointerReceiverDelivery::new(
        source_projection,
        PointerReceiverDeliveryDisposition::Dock(source_region_id),
    )
    .expect("delivery receipt binds the source scroll receiver");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("scroll receipt answers delivery only"),
                ),
            )])
            .expect("scroll receipt roster is exact"),
        )
        .expect("source delivery receipt stages");
    complete(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application))]
            if application.receiver() == source_region_id
                && application.applied_delta() > 0.0
    ));
}

/// Locks the native cross-window contract at the journal boundary.
///
/// Surface A owns the press and source tab. Both move and release are desktop
/// routes into native surface B, whose independent output authority is the
/// only one permitted to supply drop evidence. The test intentionally uses
/// different scale factors so a token match or an unconverted physical point
/// cannot accidentally satisfy the target receipt.
#[test]
fn desktop_global_journal_uses_target_native_route_for_cross_surface_drop() {
    let (workspace, _source_tabs, target_tabs) = workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let platform_provider = host.platform_provider();

    let expected = engine.version();
    let registration = submit_inputs(
        &mut engine,
        &mut host,
        INPUT_SOURCE,
        [
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: SOURCE_SURFACE,
                token: SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider: platform_provider,
                expected,
                surface: TARGET_SURFACE,
                token: TARGET_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("two root native bindings register atomically");
    let bindings = registration
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect::<Vec<_>>();
    let [source_binding, target_binding] = bindings.as_slice() else {
        panic!("two registrations must mint two bindings: {bindings:?}");
    };

    publish_native_snapshot(&mut engine, &mut host, *source_binding, *target_binding);
    publish_surfaces(
        &mut engine,
        &mut host,
        [
            (SOURCE_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
            (TARGET_SURFACE, logical_rect(0.0, 0.0, 640.0, 480.0)),
        ],
    );
    let provider = engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global provider is admitted");

    let source_projection = interaction(&engine, SOURCE_SURFACE);
    let target_projection = interaction(&engine, TARGET_SURFACE);
    assert_eq!(
        source_projection.authority().binding(),
        Some(*source_binding)
    );
    assert_eq!(
        target_projection.authority().binding(),
        Some(*target_binding)
    );

    let (source_tab, source_point) = tab_point(source_projection, ItemId::new(1));
    let (target_drop, target_point) = center_drop_point(target_projection, target_tabs);
    let source_route = desktop_route(
        *source_binding,
        source_projection,
        SOURCE_ORIGIN_X,
        SOURCE_SCALE,
        source_point,
    );
    let target_route = desktop_route(
        *target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    assert_ne!(
        source_projection.authority().coordinate_generation(),
        dockspace::viewport::CoordinateGeneration::default(),
        "native source output must carry a real coordinate authority"
    );
    assert_ne!(
        target_projection.authority().coordinate_generation(),
        dockspace::viewport::CoordinateGeneration::default(),
        "native target output must carry a real coordinate authority"
    );

    let mut gesture_frame = host.begin(&engine);
    gesture_frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                0,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                PointerEdgeLocation::Desktop {
                    route: source_route,
                },
                Authority::Known(PointerCaptureOwner::Native(*source_binding)),
            ),
        )
        .expect("source press freezes one exact receiver candidate");
    let source_candidate = gesture_frame
        .pointer_receiver_candidates()
        .expect("source press creates a candidate roster")
        .candidates()[0]
        .clone();

    let source_delivery = PointerReceiverDelivery::new(
        source_projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source receipt is bound to source native output");
    gesture_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([source_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(source_delivery),
                    ])
                    .expect("press answers source delivery"),
                ),
            )])
            .expect("source press receipt set is exact"),
        )
        .expect("source press receipt reduces");

    gesture_frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                1,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: target_route,
                },
                Authority::Known(PointerCaptureOwner::Native(*source_binding)),
            ),
        )
        .expect("target move freezes one exact receiver candidate");
    let target_candidate = gesture_frame
        .pointer_receiver_candidates()
        .expect("target move creates a candidate roster")
        .candidates()[0]
        .clone();
    assert_eq!(target_candidate.hover_point(), Some(target_point));
    let target_hover = PointerReceiverHoverHit::new(
        target_projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_drop),
    )
    .expect("target receipt is bound to target native output");
    gesture_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([target_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(target_hover),
                    ])
                    .expect("move answers target hover"),
                ),
            )])
            .expect("target move receipt set is exact"),
        )
        .expect("target move receipt reduces");
    complete(&engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, &mut engine);

    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    let empty = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(2),
        Vec::new(),
    )
    .expect("empty journal preserves the provider watermark");
    let mut paint_frame = host.begin(&engine);
    paint_frame
        .submit_pointer_journal(provider, empty)
        .expect("preview paint frame preserves the journal watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has no receiver receipts"),
        )
        .expect("empty receipt set stages");
    let paint_frame = complete_host_frame_with_current_outputs(&engine, paint_frame);
    host.finish_presentation(paint_frame, &mut engine);

    let release_projection = interaction(&engine, TARGET_SURFACE);
    assert_eq!(
        release_projection.authority().binding(),
        Some(*target_binding)
    );
    let release_route = desktop_route(
        *target_binding,
        release_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let release_journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: release_route,
            },
            Authority::Known(PointerEventDeliveryOwner::Native(*source_binding)),
            Authority::Known(PointerCaptureOwner::Native(*source_binding)),
        )],
    )
    .expect("target-bound release journal is contiguous");
    let mut release_frame = host.begin(&engine);
    release_frame
        .submit_pointer_journal(provider, release_journal)
        .expect("target-bound release freezes exact receiver candidate");
    let release_projection = release_frame
        .view()
        .interaction_projection(TARGET_SURFACE)
        .expect("sealed release frame has current target authority");
    let release_candidate = release_frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()
        .first()
        .expect("release has one candidate")
        .clone();
    assert_eq!(release_candidate.hover_point(), Some(target_point));
    assert_eq!(
        release_candidate.probes(),
        PointerReceiverProbeRequest::HoverHit
    );
    let release_hover = PointerReceiverHoverHit::new(
        release_projection,
        target_point,
        PointerReceiverHoverHitDisposition::Dock(target_drop),
    )
    .expect("release hover remains bound to target native output");
    release_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(release_hover),
                    ])
                    .expect("active drag release answers only target hover"),
                ),
            )])
            .expect("release receipt set is exact"),
        )
        .expect("release receipts stage");
    complete(&engine, &mut release_frame);
    let release_transition = host.finish(release_frame, &mut engine);

    assert!(matches!(
        release_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            },
            InteractionOutcome::DragDelivered { .. }
        ]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. })
            if items.as_slice().contains(&ItemId::new(1))
                && items.as_slice().contains(&ItemId::new(2))
    ));
}

#[test]
fn platform_destruction_before_release_prevents_stale_cross_window_drop() {
    let ActiveNativeDragFixture {
        mut engine,
        mut host,
        provider,
        source_binding,
        target_binding,
        target_tabs,
        target_point,
    } = active_native_drag();

    let target_projection = interaction(&engine, TARGET_SURFACE);
    let stale_release_route = desktop_route(
        target_binding,
        target_projection,
        TARGET_ORIGIN_X,
        TARGET_SCALE,
        target_point,
    );
    let release = PointerEdgeJournal::new(
        PointerEdgeSequence::new(2),
        PointerEdgeSequence::new(3),
        vec![PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(3),
            POINTER,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: stale_release_route,
            },
            Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("stale release remains provider-contiguous input");
    let generation = host.next_platform_observation_generation();
    let snapshot = source_only_native_snapshot(generation, source_binding);
    let source_sequence = engine
        .semantic_input_watermark()
        .expect("native setup published platform input")
        .checked_next()
        .expect("test source sequence does not exhaust");
    let platform_provider = host.platform_provider();

    let mut frame = host.begin(&engine);
    frame
        .append_input(
            INPUT_SOURCE,
            source_sequence,
            EngineInput::PublishPlatformSnapshot {
                provider: platform_provider,
                expected_epoch: engine.version().epoch(),
                snapshot,
            },
        )
        .expect("target destruction fact stages before release");
    frame
        .submit_pointer_journal(provider, release)
        .expect("stale release stages after the platform fact");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("release freezes one receiver candidate")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("unknown receiver observation answers the exact candidate"),
        )
        .expect("release receiver authority stages");
    complete(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    assert_eq!(transition.reduced_inputs()[0].causal_ordinal().get(), 0);
    assert_eq!(
        transition.reduced_pointer_edges()[0].causal_ordinal().get(),
        1
    );
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    assert!(
        !transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .iter()
            .any(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. })),
        "a release routed through a destroyed binding must never deliver the drag"
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. }) if !items.as_slice().contains(&ItemId::new(1))
    ));
}
