mod support;

use std::collections::BTreeSet;

use dockspace::drop_target::DropTargetId;
use dockspace::engine::{
    CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
    PointerReceiverGeometryError,
};
use dockspace::event::ReductionCause;
use dockspace::geometry::{LogicalPoint, LogicalRect, PhysicalRect, ScaleFactor};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, CloseSceneTarget, PointerButton, PointerId,
};
use dockspace::interaction::{
    ClickSessionId, DragPhase, EscapeDelivery, InteractionCancelReason, InteractionEventKind,
    InteractionOutcome, InteractionRejection, InteractionStatus, PreviewResolutionStatus,
    PreviewVisual, ScrollReductionOutcome, ScrollTerminationReason,
};
use dockspace::platform::{
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCoordinateObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::{
    FiniteScrollVector, PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind,
    PointerEdgeLocation, PointerEdgeSequence, PointerJournalLedgerError, PointerProviderScope,
    ScrollDeliveryEndpoint, ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers,
    ScrollMomentum, ScrollPhase, ScrollSequenceToken, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbe, PointerReceiverProbeReceipt, PointerReceiverProbeRequest,
    PointerReceiverReceipt, PointerReceiverReceiptBatch, PointerReceiverReceiptValidationError,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::policy::{
    CloseCapability, DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use dockspace::presentation_config::DockPresentationConfig;
use dockspace::presentation_hit::{PresentationHitRegionKind, PresentationPointerLane};
use dockspace::presentation_observation::{
    HostInteractionPresentation, HostPresentationObservation, PresentationHostRetirementReason,
};
use dockspace::scene::{ContainedResizeDirection, TabSceneId};
use dockspace::scene_manifest::{
    MeasurementUnavailableReason, TabListMenuMetrics, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement,
};
use dockspace::semantic_input::{
    SemanticDelivery, SemanticKey, SemanticReceiverAction, SemanticReceiverEvent,
};
use dockspace::tab_strip::TabStripControlId;
use dockspace::transition::{InputOutcome, PresentationHostRetirementOutcome};
use dockspace::viewport::{
    CoordinateObservationGeneration, PresentationObservationGeneration, ViewportBinding,
    ViewportRole, WindowToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use dockspace::{CloseDecision, ClosePlanLookup, ClosePlanPhase};
use support::{
    MeasurementProfile, TestPresentationHost, append_host_input,
    complete_host_frame_with_current_outputs, complete_host_frame_with_retained_or_unavailable,
    measurements, publish_surface, publish_surface_with,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(10);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const TARGET_ROOT: RootId = RootId::new(20);
const CONTAINED_ROOT: RootId = RootId::new(11);
const CONTAINED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const NATIVE_WINDOW: WindowToken = WindowToken::new(0xA11);
const NATIVE_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xA11);
const WORKSPACE_REBIND_SOURCE: StableInputSourceId = StableInputSourceId::new(0xA12);

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("test workspace is valid")
}

fn overflowing_tab_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let items = [
        ItemId::new(1),
        ItemId::new(2),
        ItemId::new(3),
        ItemId::new(4),
    ];
    let tabs = builder.insert_node(Node::tabs_with_selection(items, Some(items[0])));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("overflow workspace is valid")
}

fn split_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let root_node = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_tabs, target_tabs])
            .expect("test split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(root_node).with_central(target_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        builder.build().expect("split workspace is valid"),
        target_tabs,
    )
}

fn splitter_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("test split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(split).with_central(left));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("splitter workspace is valid"), split)
}

fn two_surface_splitter_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("test split is valid"),
    );
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT, RootRecord::new(split).with_central(left));
    builder.set_root(
        TARGET_ROOT,
        RootRecord::new(target_tabs).with_central(target_tabs),
    );
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        builder
            .build()
            .expect("two-surface splitter workspace is valid"),
        split,
    )
}

fn contained_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(ROOT, RootRecord::new(main_tabs).with_central(main_tabs));
    builder.set_root(CONTAINED_ROOT, RootRecord::new(contained_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        CONTAINED_FLOATING,
        ContainedFloating::new(
            CONTAINED_ROOT,
            LogicalRect::new(160.0, 110.0, 180.0, 130.0)
                .expect("contained test rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, CONTAINED_FLOATING)
        .expect("contained floating attaches to the surface");
    builder.build().expect("contained workspace is valid")
}

fn rootless_contained_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let contained_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(
        CONTAINED_ROOT,
        RootRecord::new(contained_tabs).with_central(contained_tabs),
    );
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        CONTAINED_FLOATING,
        ContainedFloating::new(
            CONTAINED_ROOT,
            LogicalRect::new(160.0, 110.0, 180.0, 130.0)
                .expect("contained test rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, CONTAINED_FLOATING)
        .expect("contained floating attaches to the rootless surface");
    builder
        .build()
        .expect("rootless contained workspace is valid")
}

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds are valid")
}

fn physical_bounds() -> PhysicalRect {
    PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test physical bounds are valid")
}

fn native_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities
}

fn native_snapshot(generation: u64, binding: ViewportBinding) -> PlatformSnapshot {
    let observed = ObservedWindow::new(binding)
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
    let windows = vec![observed];
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, native_capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        Vec::new(),
        support::unknown_work_area_observation(generation),
    )
    .expect("test native snapshot is canonical")
}

fn register_native_viewport(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
) -> ViewportBinding {
    let expected = engine.version();
    let platform_provider = host.platform_provider();
    let transition = support::submit_input(
        engine,
        host,
        NATIVE_INPUT_SOURCE,
        EngineInput::RegisterViewport {
            provider: platform_provider,
            expected,
            surface: SURFACE,
            token: NATIVE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("native viewport registration must reduce");
    match transition.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("native viewport registration was rejected: {outcome:?}"),
    }
}

fn publish_native_snapshot(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    binding: ViewportBinding,
) {
    let expected_epoch = engine.version().epoch();
    let generation = host.next_platform_observation_generation();
    let platform_provider = host.platform_provider();
    support::submit_input(
        engine,
        host,
        NATIVE_INPUT_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider: platform_provider,
            expected_epoch,
            snapshot: native_snapshot(generation, binding),
        },
    )
    .expect("native platform snapshot must reduce");
}

fn overflowing_tab_bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 260.0, 180.0).expect("overflow bounds are valid")
}

fn overflowing_tab_profile() -> MeasurementProfile {
    MeasurementProfile {
        tab_content_width: 88.0,
        tab_strip_controls: Some(
            TabStripControlMetrics::new(4.0)
                .expect("control metrics are valid")
                .with_scroll_backward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                        .expect("backward control metric is valid"),
                )
                .with_scroll_forward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                        .expect("forward control metric is valid"),
                )
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .expect("menu control metric is valid"),
                ),
        ),
        tab_list_menu: Some(
            TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("menu metrics are valid"),
        ),
        ..MeasurementProfile::default()
    }
}

fn press_journal(previous: u64) -> PointerEdgeJournal {
    press_at_journal(
        previous,
        LogicalPoint::new(96.0, 24.0).expect("test point is valid"),
    )
}

fn press_at_journal(previous: u64, position: LogicalPoint) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("test pointer journal is contiguous")
}

fn release_journal(previous: u64, position: Authority<LogicalPoint>) -> PointerEdgeJournal {
    button_release_journal(previous, PointerButton::Primary, position)
}

fn button_release_journal(
    previous: u64,
    button: PointerButton,
    position: Authority<LogicalPoint>,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            PointerEdgeKind::ButtonReleased(button),
            PointerEdgeLocation::SurfaceLocal { position },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("test pointer journal is contiguous")
}

fn moved_journal(previous: u64, position: Authority<LogicalPoint>) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal { position },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("test pointer journal is contiguous")
}

fn pointer_edge_journal(
    previous: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            kind,
            location,
            capture,
        )],
    )
    .expect("single pointer edge journal is contiguous")
}

fn local_scroll_edge(
    sequence: u64,
    point: LogicalPoint,
    endpoint: ScrollDeliveryEndpoint,
    phase: ScrollPhase,
    token: Option<ScrollSequenceToken>,
    delta: Option<ScrollDelta>,
) -> PointerEdge {
    let scroll = ScrollEdge::new(
        ScrollDeviceId::new(1),
        token,
        phase,
        delta,
        Authority::Known(ScrollMomentum::Direct),
        Authority::Known(ScrollModifiers::default()),
        Authority::Known(endpoint),
    )
    .expect("test scroll edge has a legal phase shape");
    PointerEdge::new(
        PointerEdgeSequence::new(sequence),
        PointerId::new(7),
        PointerEdgeKind::Scrolled(scroll),
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Known(point),
        },
        Authority::Known(PointerCaptureOwner::None),
    )
}

fn line_delta(x: f64, y: f64) -> ScrollDelta {
    ScrollDelta::Lines(FiniteScrollVector::new(x, y).expect("test scroll delta is finite"))
}

fn submit_exact_scroll_edge(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    edge: PointerEdge,
    region: dockspace::presentation_hit::PresentationHitRegionId,
) -> dockspace::transition::EngineTransition {
    let through = edge.sequence();
    let journal = PointerEdgeJournal::new(PointerEdgeSequence::new(previous), through, vec![edge])
        .expect("single scroll-edge journal is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("scroll edge journal stages");
    let projection = frame
        .view()
        .interaction_projection(region.surface())
        .expect("scroll output remains interactive");
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(region))
            .expect("scroll delivery is bound to the exact output");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("scroll edge requests delivery evidence")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                Some(delivery),
                None,
            ))])
            .expect("single scroll receipt roster is exact"),
        )
        .expect("scroll receipt reduces");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn empty_journal(watermark: u64) -> PointerEdgeJournal {
    let watermark = PointerEdgeSequence::new(watermark);
    PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("empty journal preserves its watermark")
}

fn submit_pointer_edge_with_observation(
    frame: &mut CoreHostFrame,
    provider: dockspace::pointer_journal::PointerInputLease,
    journal: PointerEdgeJournal,
    observation: PointerReceiverObservation,
) {
    frame
        .submit_pointer_journal(provider, journal)
        .expect("single pointer edge prepares");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("single pointer edge candidate exists")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("single pointer receipt batch is exact"),
        )
        .expect("single pointer edge receipt reduces");
}

fn create_local_provider(engine: &mut DockEngine, host: &TestPresentationHost) {
    engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("test local pointer provider is admitted");
}

fn create_native_provider(
    engine: &mut DockEngine,
    host: &TestPresentationHost,
    binding: ViewportBinding,
) -> dockspace::pointer_journal::PointerInputLease {
    engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Native(binding),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("native surface-local pointer provider is admitted")
}

fn submit_input_with_empty_journal(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    source: StableInputSourceId,
    input: EngineInput,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("active provider publishes its complete empty journal");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    support::TestInputStream::resume(engine, source)
        .append(&mut frame, input)
        .expect("input stages after the active pointer provider snapshot");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn primary_press_receipts(
    _engine: &DockEngine,
    frame: &CoreHostFrame,
) -> PointerReceiverReceiptBatch {
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("journal freezes one receiver roster")
        .candidates()
        .first()
        .expect("primary press creates one candidate")
        .clone();
    let interaction = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("published test surface is interactive");
    let tab = interaction
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabBody(_)))
        .expect("test surface has a tab receiver")
        .id();
    let delivery =
        PointerReceiverDelivery::new(interaction, PointerReceiverDeliveryDisposition::Dock(tab))
            .expect("tab supports manifest-derived delivery capability");
    let presented =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(delivery)])
            .expect("primary press answers exactly one delivery probe");
    PointerReceiverReceiptBatch::new([
        candidate.receipt(PointerReceiverObservation::Presented(presented))
    ])
    .expect("one receipt answers the candidate exactly once")
}

fn interaction(engine: &DockEngine) -> dockspace::scene::SurfaceInteractionProjection<'_> {
    engine
        .interaction_projection(SURFACE)
        .expect("published test surface is interactive")
}

fn arm_journal_close(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
) -> ClickSessionId {
    arm_journal_close_after(engine, host, provider, region, point, 0)
}

fn arm_journal_close_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
    previous: u64,
) -> ClickSessionId {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(previous, point))
        .expect("close press journal stages");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed close frame has current interaction authority");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("close press candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(region))
            .expect("close press delivery is current-output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("close press answers delivery"),
                ),
            )])
            .expect("close press receipts are exact"),
        )
        .expect("close press receipts stage");
    complete(engine, &mut frame);
    let transition = host.finish(frame, engine);
    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    let InteractionStatus::Pressed { session } = engine.interaction().status() else {
        panic!("close press must arm one click session");
    };
    session
}

fn submit_dock_press_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
    previous: u64,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(previous, point))
        .expect("dock press journal stages");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed press frame has current interaction authority");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("dock press candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(region))
            .expect("dock press delivery is current-output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("dock press answers delivery"),
                ),
            )])
            .expect("dock press receipts are exact"),
        )
        .expect("dock press receipts stage");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn submit_unknown_capture_release_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    point: LogicalPoint,
    previous: u64,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                previous,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
        )
        .expect("unknown-capture release journal stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("unknown-capture release candidate exists")
        .candidates()[0]
        .clone();
    let observation = if candidate.receiver_is_applicable() {
        PointerReceiverObservation::Unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        )
    } else {
        PointerReceiverObservation::NotApplicable
    };
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("unknown-capture release receipts are exact"),
        )
        .expect("unknown-capture release receipt stages");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn assert_unknown_capture_release_cancelled_once(
    transition: &dockspace::transition::EngineTransition,
    expected_status: InteractionStatus,
) {
    let [edge] = transition.reduced_pointer_edges() else {
        panic!("terminal release must reduce exactly one pointer edge");
    };
    assert_eq!(
        edge.interaction_outcomes(),
        &[InteractionOutcome::Cancelled {
            status: expected_status,
            reason: InteractionCancelReason::CaptureAuthorityUnavailable,
        }]
    );
    let cancellations = transition
        .interaction_events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                InteractionEventKind::Cancelled { status, reason }
                    if *status == expected_status
                        && *reason == InteractionCancelReason::CaptureAuthorityUnavailable
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert_eq!(cancellations[0].cause(), edge.cause());
}

fn release_journal_click(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    point: LogicalPoint,
    disposition: PointerReceiverDeliveryDisposition,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, release_journal(previous, Authority::Known(point)))
        .expect("click release follows the provider watermark");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed click frame has current interaction authority");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("click release candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(projection, disposition)
        .expect("click release delivery is valid");
    let hover = PointerReceiverHoverHit::new(
        projection,
        point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("click release hover is current-output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                Some(delivery),
                Some(hover),
            ))])
            .expect("click release receipts are exact"),
        )
        .expect("click release receipts stage");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn known_hover(
    interaction: dockspace::scene::SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
) -> PointerReceiverHoverHit {
    PointerReceiverHoverHit::new(
        interaction,
        point,
        PointerReceiverHoverHitDisposition::Blocked,
    )
    .expect("known hover blocker is output-bound")
}

fn exact_candidate_observation(
    candidate: &PointerReceiverCandidate,
    delivery: Option<PointerReceiverDelivery>,
    hover: Option<PointerReceiverHoverHit>,
) -> PointerReceiverObservation {
    let probes = match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => {
            return PointerReceiverObservation::NotApplicable;
        }
        PointerReceiverProbeRequest::Delivery => vec![PointerReceiverProbeReceipt::Delivery(
            delivery.expect("candidate requires one delivery fact"),
        )],
        PointerReceiverProbeRequest::HoverHit => vec![PointerReceiverProbeReceipt::HoverHit(
            hover.expect("candidate requires one hover fact"),
        )],
        PointerReceiverProbeRequest::DeliveryAndHoverHit => vec![
            PointerReceiverProbeReceipt::Delivery(
                delivery.expect("candidate requires one delivery fact"),
            ),
            PointerReceiverProbeReceipt::HoverHit(
                hover.expect("candidate requires one hover fact"),
            ),
        ],
    };
    PointerReceiverObservation::Presented(
        PresentedPointerReceiverObservation::new(probes)
            .expect("candidate observation answers its exact probe roster"),
    )
}

fn exact_hover_receipt(frame: &CoreHostFrame, point: LogicalPoint) -> PointerReceiverHoverHit {
    frame
        .view()
        .resolve_hover_drop_receiver(SURFACE, point)
        .expect("sealed core authority resolves one exact HoverDrop receipt")
}

fn point_inside_click_region(
    engine: &DockEngine,
    matches_kind: impl Fn(PresentationHitRegionKind) -> bool,
) -> (
    dockspace::presentation_hit::PresentationHitRegionId,
    LogicalPoint,
) {
    let region = interaction(engine)
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches_kind(region.id().kind()))
        .expect("published projection contains the requested click receiver");
    let bounds = region.hit().rect();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("receiver midpoint is finite");
    (region.id(), point)
}

fn submit_exact_click_edge(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    pressed: bool,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
) -> dockspace::transition::EngineTransition {
    let frame = stage_exact_click_edge(engine, host, provider, previous, pressed, region, point);
    host.finish(frame, engine)
}

fn stage_exact_click_edge(
    engine: &DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    pressed: bool,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
) -> CoreHostFrame {
    stage_exact_click_edge_with_completion(
        engine, host, provider, previous, pressed, region, point, complete,
    )
}

#[allow(clippy::too_many_arguments)]
fn stage_exact_click_edge_with_completion(
    engine: &DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    pressed: bool,
    region: dockspace::presentation_hit::PresentationHitRegionId,
    point: LogicalPoint,
    complete_frame: fn(&DockEngine, &mut CoreHostFrame),
) -> CoreHostFrame {
    let sequence = PointerEdgeSequence::new(previous + 1);
    let edge_kind = if pressed {
        PointerEdgeKind::ButtonPressed(PointerButton::Primary)
    } else {
        PointerEdgeKind::ButtonReleased(PointerButton::Primary)
    };
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            edge_kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            Authority::Known(if pressed {
                PointerCaptureOwner::ProviderEndpoint
            } else {
                PointerCaptureOwner::None
            }),
        )],
    )
    .expect("single click edge journal is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("click edge journal follows the provider watermark");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed click frame has current interaction authority");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("click edge candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(region))
            .expect("click delivery is bound to the exact published output");
    let hover = (!pressed).then(|| exact_hover_receipt(&frame, point));
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                Some(delivery),
                hover,
            ))])
            .expect("one exact receipt answers the click edge"),
        )
        .expect("click receipt stages");
    complete_frame(engine, &mut frame);
    frame
}

fn open_overflow_menu_through_journal(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
) -> dockspace::pointer_journal::PointerInputLease {
    create_local_provider(engine, host);
    let provider = engine.pointer_provider().expect("pointer provider is live");
    let (control, point) = point_inside_click_region(engine, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::TabStripControl(TabStripControlId::TabListMenu(_))
        )
    });
    let pressed = submit_exact_click_edge(engine, host, provider, 0, true, control, point);
    assert!(
        pressed.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Pressed { .. }
    ));
    let released = submit_exact_click_edge(engine, host, provider, 1, false, control, point);
    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabStripControlActivated {
            changed: true,
            menu: Some(_),
            ..
        }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    provider
}

fn republish_open_overflow_menu(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
) -> dockspace::pointer_journal::PointerInputLease {
    engine
        .retire_pointer_provider(provider)
        .expect("idle provider retirement is valid");
    publish_surface_with(
        engine,
        host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    create_local_provider(engine, host);
    engine
        .pointer_provider()
        .expect("successor pointer provider is live")
}

fn commit_empty_pointer_journal(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    watermark: u64,
) {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, empty_journal(watermark))
        .expect("empty journal must continue from the committed release watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    complete(engine, &mut frame);
    host.finish(frame, engine);
}

fn complete(engine: &DockEngine, frame: &mut CoreHostFrame) {
    complete_host_frame_with_retained_or_unavailable(engine, frame);
}

#[test]
fn ordered_smooth_scroll_locks_one_tab_strip_and_updates_core_offset() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("pointer provider is live");
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("overflowing strip is interactive");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripScroll(_)
            )
        })
        .expect("overflowing strip publishes one scroll receiver");
    let region_id = region.id();
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("scroll receiver midpoint is finite");
    let endpoint = ScrollDeliveryEndpoint::new(
        host.lease(),
        SURFACE,
        None,
        projection.authority().coordinate_generation(),
    )
    .expect("headless delivery endpoint matches the presentation host");
    let token = ScrollSequenceToken::new(41);
    let begin = submit_exact_scroll_edge(
        &mut engine,
        &mut host,
        provider,
        0,
        local_scroll_edge(1, point, endpoint, ScrollPhase::Begin, Some(token), None),
        region_id,
    );
    let first = begin.reduced_pointer_edges()[0].interaction_outcomes();
    assert!(matches!(
        first,
        [InteractionOutcome::Scroll(ScrollReductionOutcome::AwaitingFirstDelta {
            sequence,
            ..
        })] if *sequence == token
    ));

    let update = submit_exact_scroll_edge(
        &mut engine,
        &mut host,
        provider,
        1,
        local_scroll_edge(
            2,
            point,
            endpoint,
            ScrollPhase::Update,
            Some(token),
            Some(line_delta(-1.0, 0.0)),
        ),
        region_id,
    );
    assert!(matches!(
        update.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::Scroll(ScrollReductionOutcome::Began { receiver, .. }),
            InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application)),
        ]
            if *receiver == region_id
                && application.receiver() == region_id
                && application.requested_delta() == 40.0
                && application.applied_delta() == 40.0
                && application.offset() == 40.0
    ));

    let end_edge = local_scroll_edge(3, point, endpoint, ScrollPhase::End, Some(token), None);
    let mut end_frame = host.begin(&engine);
    end_frame
        .submit_pointer_journal(
            provider,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(2),
                PointerEdgeSequence::new(3),
                vec![end_edge],
            )
            .expect("terminal journal is contiguous"),
        )
        .expect("terminal scroll edge stages without a current projection");
    let candidate = end_frame
        .pointer_receiver_candidates()
        .expect("terminal edge still requests receiver evidence")
        .candidates()[0]
        .clone();
    end_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Unknown(PointerReceiverUnknownReason::NotReported),
            )])
            .expect("unknown terminal receipt roster is exact"),
        )
        .expect("unknown terminal receipt still closes the locked session");
    complete(&engine, &mut end_frame);
    let end = host.finish(end_frame, &mut engine);
    assert!(matches!(
        end.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Scroll(ScrollReductionOutcome::Terminated {
            receiver: Some(receiver),
            reason: ScrollTerminationReason::Completed,
            ..
        })] if *receiver == region_id
    ));

    engine
        .retire_pointer_provider(provider)
        .expect("completed scroll provider can retire");
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    let offset = engine
        .interaction_projection(SURFACE)
        .expect("republished strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .scroll_offset();
    assert_eq!(offset, 40.0, "the adapter owns no mirrored scroll state");
}

#[test]
fn smooth_scroll_rejects_token_replacement_until_the_active_sequence_terminates() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("pointer provider is live");
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("overflowing strip is interactive");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripScroll(_)
            )
        })
        .expect("overflowing strip publishes one scroll receiver");
    let region_id = region.id();
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("scroll receiver midpoint is finite");
    let endpoint = ScrollDeliveryEndpoint::new(
        host.lease(),
        SURFACE,
        None,
        projection.authority().coordinate_generation(),
    )
    .expect("headless delivery endpoint matches the presentation host");
    let active_token = ScrollSequenceToken::new(41);
    let replacement_token = ScrollSequenceToken::new(42);

    submit_exact_scroll_edge(
        &mut engine,
        &mut host,
        provider,
        0,
        local_scroll_edge(
            1,
            point,
            endpoint,
            ScrollPhase::Begin,
            Some(active_token),
            None,
        ),
        region_id,
    );

    let replacement_edge = local_scroll_edge(
        2,
        point,
        endpoint,
        ScrollPhase::Begin,
        Some(replacement_token),
        None,
    );
    let replacement_journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(1),
        PointerEdgeSequence::new(2),
        vec![replacement_edge],
    )
    .expect("replacement begin journal is contiguous");
    let version_before_rejection = engine.version();
    let mut replacement = host.begin(&engine);
    replacement
        .submit_pointer_journal(provider, replacement_journal)
        .expect("replacement begin stages before ordered reduction");
    let projection = replacement
        .view()
        .interaction_projection(SURFACE)
        .expect("scroll output remains interactive");
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(region_id),
    )
    .expect("replacement receipt binds the exact output");
    let candidate = replacement
        .pointer_receiver_candidates()
        .expect("replacement begin requests delivery evidence")
        .candidates()[0]
        .clone();
    let error = replacement
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                Some(delivery),
                None,
            ))])
            .expect("replacement receipt roster is exact"),
        )
        .expect_err("a live smooth sequence cannot be replaced by a newer token");
    assert_eq!(error, CoreHostFrameError::InputPrefixReductionFailed);
    drop(replacement);
    assert_eq!(engine.version(), version_before_rejection);

    let resumed = submit_exact_scroll_edge(
        &mut engine,
        &mut host,
        provider,
        1,
        local_scroll_edge(
            2,
            point,
            endpoint,
            ScrollPhase::Update,
            Some(active_token),
            Some(line_delta(-1.0, 0.0)),
        ),
        region_id,
    );
    assert!(matches!(
        resumed.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::Scroll(ScrollReductionOutcome::Began { receiver, .. }),
            InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application)),
        ] if *receiver == region_id && application.offset() == 40.0
    ));
}

fn point_inside_inactive_tab(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
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
                PresentationHitRegionKind::TabBody(tab) if tab.item == ItemId::new(2)
            )
        })
        .expect("inactive tab has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("tab midpoint is finite");
    (region.id(), point)
}

fn point_inside_tab(
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
        .expect("item has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("tab midpoint is finite");
    (region.id(), point)
}

fn point_inside_tab_group(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    item: ItemId,
) -> (
    dockspace::presentation_hit::PresentationHitRegionId,
    LogicalPoint,
) {
    let tab = projection
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().item == item)
        .expect("item has an exact tab record");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabGroupGrip(group)
                    if group.root == tab.id().root && group.tabs == tab.id().tabs
            )
        })
        .expect("item's tab stack has an exact group-grip region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("group-grip midpoint is finite");
    (region.id(), point)
}

fn point_without_hover_receiver(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    away_from: LogicalPoint,
) -> LogicalPoint {
    let bounds = projection.plan().bounds();
    for y_step in 1..10 {
        for x_step in 1..10 {
            let point = LogicalPoint::new(
                bounds.x() + bounds.width() * f64::from(x_step) / 10.0,
                bounds.y() + bounds.height() * f64::from(y_step) / 10.0,
            )
            .expect("sample point is finite");
            let delta_x = point.x() - away_from.x();
            let delta_y = point.y() - away_from.y();
            let has_hover_receiver = projection.hit_manifest().regions().iter().any(|region| {
                !region.is_passive()
                    && region.lanes().contains(PresentationPointerLane::HoverDrop)
                    && region.hit().contains(point)
            });
            if delta_x * delta_x + delta_y * delta_y > 64.0 && !has_hover_receiver {
                return point;
            }
        }
    }
    panic!("test presentation has no unambiguous no-receiver point");
}

fn point_inside_tab_close(
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
        .expect("closeable tab has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("close midpoint is finite");
    (region.id(), point)
}

fn point_inside_center_target(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    target_tabs: NodeId,
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
                PresentationHitRegionKind::DropTarget(DropTargetId::Center { tabs, .. })
                    if tabs == target_tabs
            )
        })
        .expect("target tabs have a center drop receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("drop target midpoint is finite");
    (region.id(), point)
}

fn point_inside_surface_background(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
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
                PresentationHitRegionKind::DropTarget(DropTargetId::SurfaceBackground {
                    surface: SURFACE
                })
            )
        })
        .expect("rootless surface has an exact background receiver");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("background midpoint is finite");
    (region.id(), point)
}

fn point_inside_splitter(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    split: NodeId,
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
                PresentationHitRegionKind::SplitterHandle(splitter) if splitter.split == split
            )
        })
        .expect("splitter has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("splitter midpoint is finite");
    (region.id(), point)
}

#[test]
fn sealed_view_resolves_output_bound_hover_drop_receipts_without_adapter_ranking() {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());

    let frame = host.begin(&engine);
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed frame has exact interaction authority");
    let (expected, target_point) = point_inside_center_target(projection, target_tabs);
    let target = frame
        .view()
        .resolve_hover_drop_receiver(SURFACE, target_point)
        .expect("core resolves the unique center target");
    assert_eq!(
        target.disposition(),
        PointerReceiverHoverHitDisposition::Dock(expected)
    );
    assert_eq!(target.output(), Some(projection.output_ticket()));
    assert_eq!(target.authority(), Some(projection.authority()));
    assert_eq!(target.point(), Some(target_point));

    let empty_point = point_without_hover_receiver(projection, target_point);
    let empty = frame
        .view()
        .resolve_hover_drop_receiver(SURFACE, empty_point)
        .expect("core proves the exact no-receiver result");
    assert_eq!(
        empty.disposition(),
        PointerReceiverHoverHitDisposition::NoReceiver
    );
    assert_eq!(empty.output(), Some(projection.output_ticket()));
    assert_eq!(empty.authority(), Some(projection.authority()));
    assert_eq!(empty.point(), Some(empty_point));
}

fn point_inside_contained_resize(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    floating: FloatingPresentationId,
    direction: ContainedResizeDirection,
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
                PresentationHitRegionKind::ContainedResize {
                    floating: actual,
                    direction: actual_direction,
                } if actual == floating && actual_direction == direction
            )
        })
        .expect("contained resize chrome has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("contained resize midpoint is finite");
    (region.id(), point)
}

fn point_inside_contained_title(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    floating: FloatingPresentationId,
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
                PresentationHitRegionKind::ContainedTitle(actual) if actual == floating
            )
        })
        .expect("contained title has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("contained title midpoint is finite");
    (region.id(), point)
}

fn point_inside_contained_close(
    projection: dockspace::scene::SurfaceInteractionProjection<'_>,
    floating: FloatingPresentationId,
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
                PresentationHitRegionKind::ContainedClose(actual) if actual == floating
            )
        })
        .expect("contained close has an exact delivery region");
    let rect = region.hit().rect();
    let point = LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("contained close midpoint is finite");
    (region.id(), point)
}

fn begin_journal_drag(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    source_region: dockspace::presentation_hit::PresentationHitRegionId,
    press: LogicalPoint,
    target: LogicalPoint,
    target_disposition: PointerReceiverHoverHitDisposition,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(0, press))
        .expect("press edge freezes one receiver challenge");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed drag frame has current interaction authority");
    let press_candidate = frame
        .pointer_receiver_candidates()
        .expect("press candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_region),
    )
    .expect("source region receives the press");
    let hover = PointerReceiverHoverHit::new(projection, target, target_disposition)
        .expect("hover disposition is bound to the frozen output");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                exact_candidate_observation(&press_candidate, Some(delivery), None),
            )])
            .expect("press receipt set is exact"),
        )
        .expect("press receipt reduces before the move challenge");

    frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(target)))
        .expect("move edge follows the reduced press prefix");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("move candidate roster exists")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([move_candidate.receipt(
                exact_candidate_observation(&move_candidate, None, Some(hover)),
            )])
            .expect("gesture receipt set is exact"),
        )
        .expect("gesture receipts stage");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn paint_active_journal_drag(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
) {
    paint_active_journal_drag_after(engine, host, provider, 2);
}

fn paint_active_journal_drag_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    watermark: u64,
) {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, empty_journal(watermark))
        .expect("paint frame preserves the provider watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty receipt set stages");
    let frame = complete_host_frame_with_current_outputs(engine, frame);
    host.finish_presentation(frame, engine);
}

fn update_journal_drag_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    target: LogicalPoint,
    target_disposition: PointerReceiverHoverHitDisposition,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(provider, moved_journal(previous, Authority::Known(target)))
        .expect("move follows the committed watermark");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed move frame has current interaction authority");
    let hover = PointerReceiverHoverHit::new(projection, target, target_disposition)
        .expect("move hover is bound to the current output");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("move candidate roster exists")
        .candidates()
        .first()
        .expect("move creates one candidate")
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                None,
                Some(hover),
            ))])
            .expect("move receipt set is exact"),
        )
        .expect("move receipt stages");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn release_journal_drag(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    target: LogicalPoint,
    target_disposition: PointerReceiverHoverHitDisposition,
) -> dockspace::transition::EngineTransition {
    release_journal_drag_after(engine, host, provider, 2, target, target_disposition)
}

fn release_journal_drag_after(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    previous: u64,
    target: LogicalPoint,
    target_disposition: PointerReceiverHoverHitDisposition,
) -> dockspace::transition::EngineTransition {
    let mut frame = host.begin(engine);
    frame
        .submit_pointer_journal(
            provider,
            release_journal(previous, Authority::Known(target)),
        )
        .expect("release follows the committed watermark");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed release frame has current interaction authority");
    let hover = PointerReceiverHoverHit::new(projection, target, target_disposition)
        .expect("release hover is bound to the frozen output");
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::NoReceiver)
            .expect("release delivery absence is output-bound");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()
        .first()
        .expect("release creates one candidate")
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                Some(delivery),
                Some(hover),
            ))])
            .expect("release receipt set is exact"),
        )
        .expect("release receipt stages");
    complete(engine, &mut frame);
    host.finish(frame, engine)
}

fn republish_surface_with_active_provider(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: dockspace::pointer_journal::PointerInputLease,
    watermark: u64,
) {
    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("staging surface contribution begins");
    let contribution = engine
        .prepare_surface_contribution(
            token,
            measurements(engine, SURFACE, bounds(), MeasurementProfile::default()),
        )
        .expect("fresh staging measurements prepare");
    let mut measurement_frame = host.begin(engine);
    measurement_frame
        .submit_pointer_journal(provider, empty_journal(watermark))
        .expect("measurement frame preserves the provider watermark");
    measurement_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty measurement receipts stage");
    measurement_frame
        .push_surface_contribution(contribution)
        .expect("fresh staging contribution stages");
    complete(engine, &mut measurement_frame);
    host.finish(measurement_frame, engine);

    let mut paint_frame = host.begin(engine);
    paint_frame
        .submit_pointer_journal(provider, empty_journal(watermark))
        .expect("paint frame preserves the provider watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty paint receipts stage");
    let paint_frame = complete_host_frame_with_current_outputs(engine, paint_frame);
    host.finish_presentation(paint_frame, engine);

    let mut observation_frame = host.begin(engine);
    observation_frame
        .submit_pointer_journal(provider, empty_journal(watermark))
        .expect("observation frame preserves the provider watermark");
    observation_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty observation receipts stage");
    complete(engine, &mut observation_frame);
    host.finish(observation_frame, engine);
    assert!(
        engine.interaction_projection(SURFACE).is_some(),
        "fresh output observation reopens the global popup gate"
    );
}

#[test]
fn resolved_journal_preview_does_not_commit_before_presentation_proof() {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_inactive_tab(projection);
    let (drop_region, target) = point_inside_center_target(projection, target_tabs);

    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source tab receives press");
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(drop_region),
    )
    .expect("target hover is bound to the frozen output");
    let mut frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("release answers hover"),
        ),
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    let move_outcomes = transition.reduced_pointer_edges()[1].interaction_outcomes();
    assert!(
        matches!(
            move_outcomes,
            [
                InteractionOutcome::DragBegan { .. },
                InteractionOutcome::PreviewUpdated {
                    preview: None,
                    status: PreviewResolutionStatus::UnknownAuthority,
                    ..
                }
            ]
        ),
        "unexpected move outcomes: {move_outcomes:?}"
    );
    let release_outcomes = transition.reduced_pointer_edges()[2].interaction_outcomes();
    assert!(
        matches!(
            release_outcomes,
            [
                InteractionOutcome::PreviewUpdated {
                    preview: None,
                    status: PreviewResolutionStatus::UnknownAuthority,
                    ..
                },
                InteractionOutcome::Cancelled {
                    status: InteractionStatus::Dragging { .. },
                    reason: InteractionCancelReason::UnknownTargetAuthority,
                }
            ]
        ),
        "unexpected release outcomes: {release_outcomes:?}"
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. }) if items.as_slice() == [ItemId::new(3)]
    ));
    assert!(!transition.interaction_events().iter().any(|event| {
        matches!(
            event.kind(),
            dockspace::interaction::InteractionEventKind::PreviewPublished { .. }
        )
    }));
}

#[test]
fn inactive_tab_continuation_waits_for_fresh_target_authority_then_commits() {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_inactive_tab(projection);
    let (stale_target_region, stale_target) = point_inside_center_target(projection, target_tabs);

    let activation = begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source_tab,
        press,
        stale_target,
        PointerReceiverHoverHitDisposition::Dock(stale_target_region),
    );
    assert!(matches!(
        activation.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::UnknownAuthority,
                ..
            }
        ]
    ));
    assert!(engine.interaction_projection(SURFACE).is_none());
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    republish_surface_with_active_provider(&mut engine, &mut host, provider, 2);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let projection = interaction(&engine);
    let (target_region, target) = point_inside_center_target(projection, target_tabs);
    let before_update_version = engine.version();
    let before_update_requirements = engine.presentation_requirements().revision();
    let update = update_journal_drag_after(
        &mut engine,
        &mut host,
        provider,
        2,
        target,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    );
    assert!(matches!(
        update.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            preview: Some(_),
            status: PreviewResolutionStatus::Resolved,
            ..
        }]
    ));
    assert_eq!(engine.version(), before_update_version);
    assert_eq!(
        engine.presentation_requirements().revision(),
        before_update_requirements
    );
    assert!(
        engine.interaction_projection(SURFACE).is_some(),
        "preview publication must not reset the authoritative popup gate"
    );

    paint_active_journal_drag_after(&mut engine, &mut host, provider, 3);
    let release_projection = interaction(&engine);
    let (release_region, release_target) =
        point_inside_center_target(release_projection, target_tabs);
    let release = release_journal_drag_after(
        &mut engine,
        &mut host,
        provider,
        3,
        release_target,
        PointerReceiverHoverHitDisposition::Dock(release_region),
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
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
            if items.as_slice() == [ItemId::new(3), ItemId::new(2)]
    ));
}

#[test]
fn surface_local_splitter_press_move_release_commits_stream_owned_resize() {
    let (workspace, split) = splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (splitter, press) = point_inside_splitter(projection, split);
    let moved = LogicalPoint::new(press.x() + 24.0, press.y()).expect("moved point is finite");
    let released = LogicalPoint::new(press.x() + 48.0, press.y()).expect("release point is finite");
    let before = match engine.workspace().node(split) {
        Some(Node::Split { weights, .. }) => weights.clone(),
        node => panic!("expected split node, got {node:?}"),
    };

    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(splitter),
    )
    .expect("splitter delivery is output-bound");
    let mut frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(released),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeBegan { .. }]
    ));
    let move_weights = match transition.reduced_pointer_edges()[1].interaction_outcomes() {
        [InteractionOutcome::ResizeUpdated { splits, .. }] if splits.len() == 1 => {
            splits[0].weights().to_vec()
        }
        outcomes => panic!("move must publish one resize proposal, got {outcomes:?}"),
    };
    assert!(matches!(
        transition.reduced_pointer_edges()[2].interaction_outcomes(),
        [InteractionOutcome::ResizeDelivered { changed: true, .. }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights, .. }) if weights != &before && weights != &move_weights
    ));
    engine
        .workspace()
        .validate()
        .expect("resize leaves a valid workspace");
}

#[test]
fn unavailable_sibling_preserves_pressed_click_until_an_unknown_source_release() {
    let (workspace, _) = two_surface_splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surfaces(
        &mut engine,
        &mut host,
        [(SURFACE, bounds()), (TARGET_SURFACE, bounds())],
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);
    let before = engine.version();

    let target_token = engine
        .begin_surface_contribution(TARGET_SURFACE)
        .expect("sibling contribution begins");
    let unavailable = engine
        .prepare_surface_unavailable_contribution(
            target_token,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("sibling unavailability prepares");
    let mut revoke_frame = host.begin(&engine);
    revoke_frame
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("revocation frame preserves the press watermark");
    revoke_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty revocation receipts stage");
    revoke_frame
        .push_surface_contribution(unavailable)
        .expect("sibling unavailable contribution stages");
    complete(&engine, &mut revoke_frame);
    let revoked = host.finish(revoke_frame, &mut engine);

    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );
    assert_eq!(engine.version(), before);
    assert_eq!(engine.active_close_plans().count(), 0);
    assert!(engine.interaction_projection(SURFACE).is_some());
    assert!(engine.interaction_projection(TARGET_SURFACE).is_none());
    assert!(!revoked.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            status: InteractionStatus::Pressed { session: cancelled },
            reason: InteractionCancelReason::SceneUnavailable,
        } if *cancelled == session
    )));

    let mut release_frame = host.begin(&engine);
    release_frame
        .submit_pointer_journal(provider, release_journal(1, Authority::Known(point)))
        .expect("matching release journal stages");
    let release_candidate = release_frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()[0]
        .clone();
    release_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                PointerReceiverObservation::Unknown(
                    PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                ),
            )])
            .expect("staging release receipt batch is exact"),
        )
        .expect("staging release receipts stage");
    complete(&engine, &mut release_frame);
    let released = host.finish(release_frame, &mut engine);

    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.version(), before);
    assert_eq!(engine.active_close_plans().count(), 0);
    assert_eq!(released.reduced_pointer_edges().len(), 1);
    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Pressed { session: cancelled },
            reason: InteractionCancelReason::UnknownTargetAuthority,
        }] if *cancelled == session
    ));

    commit_empty_pointer_journal(&mut engine, &mut host, provider, 2);
}

#[test]
fn unknown_sibling_preserves_resize_until_an_unknown_source_release() {
    let (workspace, split) = two_surface_splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surfaces(
        &mut engine,
        &mut host,
        [(SURFACE, bounds()), (TARGET_SURFACE, bounds())],
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (splitter, press) = point_inside_splitter(projection, split);
    let released = LogicalPoint::new(press.x() + 48.0, press.y()).expect("release point is finite");

    let mut press_frame = host.begin(&engine);
    press_frame
        .submit_pointer_journal(provider, press_at_journal(0, press))
        .expect("splitter press journal stages");
    let press_candidate = press_frame
        .pointer_receiver_candidates()
        .expect("press candidate roster exists")
        .candidates()[0]
        .clone();
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(splitter),
    )
    .expect("splitter delivery is output-bound");
    press_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(press_delivery),
                    ])
                    .expect("press answers delivery"),
                ),
            )])
            .expect("splitter press receipt batch is exact"),
        )
        .expect("splitter press receipts stage");
    complete(&engine, &mut press_frame);
    let pressed = host.finish(press_frame, &mut engine);
    let session = match pressed.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::ResizeBegan { session, .. }] => *session,
        outcomes => panic!("splitter press must begin resize, got {outcomes:?}"),
    };
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Resizing { session }
    );

    let before_weights = match engine.workspace().node(split) {
        Some(Node::Split { weights, .. }) => weights.clone(),
        node => panic!("expected split node, got {node:?}"),
    };
    let before_version = engine.version();

    let mut emit_frame = host.begin(&engine);
    emit_frame
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("paint pass preserves the press watermark");
    emit_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty paint-pass receipts stage");
    let emit_frame = complete_host_frame_with_current_outputs(&engine, emit_frame);
    host.finish_presentation(emit_frame, &mut engine);
    assert_eq!(host.pending_output_count(), 2);

    let mut revoke_prelude = engine
        .begin_host_frame(host.lease())
        .expect("revocation frame begins");
    host.submit_partitioned_observation(
        &mut revoke_prelude,
        &BTreeSet::from([SURFACE]),
        &BTreeSet::from([TARGET_SURFACE]),
    );
    let mut revoke_frame = revoke_prelude
        .seal(&engine)
        .expect("revocation frame seals after observations");
    revoke_frame
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("revocation frame preserves the press watermark");
    revoke_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty revocation receipts stage");
    complete(&engine, &mut revoke_frame);
    let revoked = host.finish(revoke_frame, &mut engine);

    assert!(engine.interaction_projection(SURFACE).is_some());
    assert!(engine.interaction_projection(TARGET_SURFACE).is_none());
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Resizing { session }
    );
    assert_eq!(engine.version(), before_version);
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights, .. }) if weights == &before_weights
    ));
    assert!(!revoked.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            status: InteractionStatus::Resizing { session: cancelled },
            reason: InteractionCancelReason::SceneUnavailable,
        } if *cancelled == session
    )));

    let mut release_prelude = engine
        .begin_host_frame(host.lease())
        .expect("release frame begins");
    let no_surfaces = BTreeSet::new();
    host.submit_partitioned_observation(&mut release_prelude, &no_surfaces, &no_surfaces);
    let mut release_frame = release_prelude
        .seal(&engine)
        .expect("release frame seals after observations");
    release_frame
        .submit_pointer_journal(provider, release_journal(1, Authority::Known(released)))
        .expect("matching release journal stages");
    let release_candidate = release_frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()[0]
        .clone();
    release_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                release_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("staging release receipt batch is exact"),
        )
        .expect("staging release receipts stage");
    complete(&engine, &mut release_frame);
    let released_transition = host.finish(release_frame, &mut engine);

    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(
        engine.version().revision().get(),
        before_version.revision().get() + 1
    );
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights, .. }) if weights != &before_weights
    ));
    assert_eq!(released_transition.reduced_pointer_edges().len(), 1);
    assert!(matches!(
        released_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeDelivered {
            session: delivered,
            changed: true,
            ..
        }] if *delivered == session
    ));

    let mut watermark_probe = host.begin(&engine);
    watermark_probe
        .submit_pointer_journal(provider, empty_journal(2))
        .expect("accepted release advances the provider watermark");
    watermark_probe
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty watermark probe has an exact empty receipt set"),
        )
        .expect("empty watermark probe receipts stage");
    complete(&engine, &mut watermark_probe);
    host.finish(watermark_probe, &mut engine);
}

#[test]
fn surface_local_splitter_press_release_uses_the_release_position_without_a_move() {
    let (workspace, split) = splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (splitter, press) = point_inside_splitter(projection, split);
    let released = LogicalPoint::new(press.x() + 48.0, press.y()).expect("release point is finite");
    let before = match engine.workspace().node(split) {
        Some(Node::Split { weights, .. }) => weights.clone(),
        node => panic!("expected split node, got {node:?}"),
    };

    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(splitter),
    )
    .expect("splitter delivery is output-bound");
    let mut frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(released),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeBegan { .. }]
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [InteractionOutcome::ResizeDelivered { changed: true, .. }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights, .. }) if weights != &before
    ));
}

#[test]
fn surface_local_tab_close_requires_a_matching_release() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(0, point))
        .expect("close press freezes one receiver candidate");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("candidate roster exists")
        .candidates()
        .first()
        .expect("press creates one candidate")
        .clone();
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(close))
            .expect("tab close delivery is output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("close press answers delivery"),
                ),
            )])
            .expect("close receipt batch is exact"),
        )
        .expect("close receipt stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    let InteractionStatus::Pressed { session } = engine.interaction().status() else {
        panic!("close press must arm one click session");
    };
    assert_eq!(engine.active_close_plans().count(), 0);

    let release_projection = interaction(&engine);
    let mut release_frame = host.begin(&engine);
    release_frame
        .submit_pointer_journal(provider, release_journal(1, Authority::Known(point)))
        .expect("matching close release follows the press watermark");
    let release_candidate = release_frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()
        .first()
        .expect("release creates one candidate")
        .clone();
    let release_delivery = PointerReceiverDelivery::new(
        release_projection,
        PointerReceiverDeliveryDisposition::Dock(close),
    )
    .expect("matching close release is output-bound");
    let release_hover = PointerReceiverHoverHit::new(
        release_projection,
        point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("matching close hover is output-bound");
    release_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                exact_candidate_observation(
                    &release_candidate,
                    Some(release_delivery),
                    Some(release_hover),
                ),
            )])
            .expect("release receipt set is exact"),
        )
        .expect("release receipts stage");
    complete(&engine, &mut release_frame);
    let release = host.finish(release_frame, &mut engine);
    let [
        InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        },
    ] = release.reduced_pointer_edges()[0].interaction_outcomes()
    else {
        panic!("matching release must open exactly one close plan");
    };
    assert_eq!(session.epoch(), engine.version().epoch());
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), ItemId::new(1));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(release.reduced_inputs().is_empty());
    assert!(
        engine
            .workspace()
            .item_multiset()
            .contains_key(&ItemId::new(1))
    );
}

#[test]
fn journal_close_release_outside_consumes_without_opening_a_plan() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, press) = point_inside_tab_close(projection, ItemId::new(1));
    let outside = LogicalPoint::new(560.0, 420.0).expect("outside point is finite");
    arm_journal_close(&mut engine, &mut host, provider, close, press);

    let release = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        outside,
        PointerReceiverDeliveryDisposition::NoReceiver,
    );

    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Pressed { .. },
            reason: InteractionCancelReason::ClickReceiverMismatch,
        }]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.active_close_plans().count(), 0);
}

#[test]
fn journal_close_release_preserves_blocked_and_unknown_terminal_reasons() {
    use dockspace::pointer_receiver::PointerReceiverUnknownReason;

    for (disposition, expected_reason) in [
        (
            PointerReceiverDeliveryDisposition::Blocked,
            InteractionCancelReason::OpaquePointerBlocker,
        ),
        (
            PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::FrameworkDeliveryUnavailable,
            ),
            InteractionCancelReason::UnknownTargetAuthority,
        ),
    ] {
        let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface(&mut engine, &mut host, SURFACE, bounds());
        create_local_provider(&mut engine, &host);
        let provider = engine.pointer_provider().expect("provider is live");
        let projection = interaction(&engine);
        let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
        arm_journal_close(&mut engine, &mut host, provider, close, point);

        let release =
            release_journal_click(&mut engine, &mut host, provider, 1, point, disposition);
        assert!(matches!(
            release.reduced_pointer_edges()[0].interaction_outcomes(),
            [InteractionOutcome::Cancelled {
                status: InteractionStatus::Pressed { .. },
                reason,
            }] if *reason == expected_reason
        ));
        assert_eq!(engine.active_close_plans().count(), 0);
    }
}

#[test]
fn journal_close_move_out_and_back_release_inside_still_activates() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, press) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, press);
    let session = match engine.interaction().status() {
        InteractionStatus::Pressed { session } => session,
        status => panic!("expected pressed click, got {status:?}"),
    };

    let outside = LogicalPoint::new(560.0, 420.0).expect("outside point is finite");
    let move_projection = interaction(&engine);
    let mut move_frame = host.begin(&engine);
    move_frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(outside)))
        .expect("move follows the press watermark");
    let candidate = move_frame
        .pointer_receiver_candidates()
        .expect("move candidate roster exists")
        .candidates()[0]
        .clone();
    let hover = PointerReceiverHoverHit::new(
        move_projection,
        outside,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("move hover is current-output-bound");
    move_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                None,
                Some(hover),
            ))])
            .expect("move receipt is exact"),
        )
        .expect("move receipt stages");
    complete(&engine, &mut move_frame);
    host.finish(move_frame, &mut engine);
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );

    let release = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        2,
        press,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested { reused: false, .. }]
    ));
    assert_eq!(engine.active_close_plans().count(), 1);
}

#[test]
fn journal_close_duplicate_release_cannot_open_or_reuse_again() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);
    release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );

    let duplicate = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        2,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(
        duplicate.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert_eq!(engine.active_close_plans().count(), 1);
}

#[test]
fn journal_close_matching_second_click_reuses_the_existing_active_plan() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);
    let first = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    let first_plan = match first.reduced_pointer_edges()[0].interaction_outcomes() {
        [
            InteractionOutcome::CloseRequested {
                plan,
                reused: false,
            },
        ] => plan.clone(),
        outcomes => panic!("first click must open one close plan, got {outcomes:?}"),
    };

    arm_journal_close_after(&mut engine, &mut host, provider, close, point, 2);
    let second = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        3,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );

    assert!(matches!(
        second.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested {
            plan,
            reused: true,
        }] if plan.request() == first_plan.request()
    ));
    assert_eq!(engine.active_close_plans().count(), 1);
    assert_eq!(engine.close_plans().count(), 1);
}

#[test]
fn journal_close_non_primary_release_does_not_consume_the_pressed_click() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);

    let mut secondary = host.begin(&engine);
    secondary
        .submit_pointer_journal(
            provider,
            button_release_journal(1, PointerButton::Secondary, Authority::Known(point)),
        )
        .expect("secondary release follows the provider watermark");
    let candidate = secondary
        .pointer_receiver_candidates()
        .expect("secondary release candidate roster exists")
        .candidates()[0]
        .clone();
    assert_eq!(
        candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    secondary
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("secondary release has one exact terminal receipt"),
        )
        .expect("secondary release receipt stages");
    complete(&engine, &mut secondary);
    let secondary = host.finish(secondary, &mut engine);

    assert!(
        secondary.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );
    assert_eq!(engine.active_close_plans().count(), 0);

    let primary = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        2,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(matches!(
        primary.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested { reused: false, .. }]
    ));
}

#[test]
fn journal_close_release_inside_does_not_activate_after_press_on_another_receiver() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, close_point) = point_inside_tab_close(projection, ItemId::new(1));
    let (other_receiver, press_point) = point_inside_tab(projection, ItemId::new(1));
    let mut press = host.begin(&engine);
    press
        .submit_pointer_journal(provider, press_at_journal(0, press_point))
        .expect("press on another receiver freezes one candidate");
    let candidate = press
        .pointer_receiver_candidates()
        .expect("press candidate roster exists")
        .candidates()[0]
        .clone();
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(other_receiver),
    )
    .expect("other receiver delivery is current-output-bound");
    press
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("other receiver press answers delivery"),
                ),
            )])
            .expect("other receiver receipt set is exact"),
        )
        .expect("other receiver receipt stages");
    complete(&engine, &mut press);
    let press = host.finish(press, &mut engine);
    assert!(matches!(
        press.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));

    let release = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        close_point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::ReleasedBeforeDrag,
            ..
        }]
    ));
    assert_eq!(engine.active_close_plans().count(), 0);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn invalid_close_release_receipt_rolls_back_session_and_watermark_before_exact_retry() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);
    let before_version = engine.version();
    let before_workspace = engine.workspace().clone();

    let release_projection = interaction(&engine);
    let journal = release_journal(1, Authority::Known(point));
    let mut failed = host.begin(&engine);
    failed
        .submit_pointer_journal(provider, journal.clone())
        .expect("release journal stages provisionally");
    let candidate = failed
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()[0]
        .clone();
    let unexpected_hover = PointerReceiverHoverHit::new(
        release_projection,
        point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("unexpected hover is structurally valid");
    let incomplete =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
            unexpected_hover,
        )])
        .expect("hover-only observation omits the required delivery probe");
    assert_eq!(
        failed.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(incomplete))
            ])
            .expect("failed release receipt set is exact by candidate"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "an invalid receipt poisons the private input prefix immediately",
    );
    assert!(matches!(
        failed.finish(&mut engine),
        Err(EngineError::PointerReceiverReceipt {
            source: PointerReceiverReceiptValidationError::MissingProbe {
                probe: PointerReceiverProbe::Delivery,
                ..
            }
        })
    ));
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );
    assert_eq!(engine.active_close_plans().count(), 0);

    let retry_projection = interaction(&engine);
    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(provider, journal)
        .expect("failed frame left the provider watermark at the press sequence");
    let retry_candidate = retry
        .pointer_receiver_candidates()
        .expect("retry candidate roster exists")
        .candidates()[0]
        .clone();
    let retry_delivery = PointerReceiverDelivery::new(
        retry_projection,
        PointerReceiverDeliveryDisposition::Dock(close),
    )
    .expect("retry close delivery is current-output-bound");
    let retry_hover = PointerReceiverHoverHit::new(
        retry_projection,
        point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("retry hover is current-output-bound");
    retry
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([retry_candidate.receipt(
                exact_candidate_observation(
                    &retry_candidate,
                    Some(retry_delivery),
                    Some(retry_hover),
                ),
            )])
            .expect("retry receipt set is exact"),
        )
        .expect("retry receipt stages");
    complete(&engine, &mut retry);
    let retry = host.finish(retry, &mut engine);
    assert!(matches!(
        retry.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested { reused: false, .. }]
    ));
}

#[test]
fn journal_close_does_not_reuse_a_vetoed_terminal_plan() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);
    let first = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    let first_plan = match first.reduced_pointer_edges()[0].interaction_outcomes() {
        [
            InteractionOutcome::CloseRequested {
                plan,
                reused: false,
            },
        ] => plan.clone(),
        outcomes => panic!("first click must open one close plan, got {outcomes:?}"),
    };

    let mut veto = host.begin(&engine);
    veto.submit_pointer_journal(provider, empty_journal(2))
        .expect("veto frame preserves the pointer watermark");
    veto.submit_pointer_receiver_receipts(
        PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
            .expect("empty journal has an exact empty receipt set"),
    )
    .expect("empty receipt set stages");
    append_host_input(
        &mut veto,
        StableInputSourceId::new(92),
        SourceSequence::new(1),
        EngineInput::ResolveClose {
            request: first_plan.request(),
            token: first_plan.items()[0].token(),
            decision: CloseDecision::Veto,
        },
    )
    .expect("veto decision stages after the complete pointer journal");
    complete(&engine, &mut veto);
    host.finish(veto, &mut engine);
    assert_eq!(
        engine
            .close_plan(first_plan.request())
            .expect("vetoed plan remains inspectable")
            .phase(),
        ClosePlanPhase::Vetoed
    );
    assert_eq!(engine.active_close_plans().count(), 0);

    arm_journal_close_after(&mut engine, &mut host, provider, close, point, 2);
    let second = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        3,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(matches!(
        second.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        }] if plan.request() != first_plan.request()
    ));
    assert!(matches!(
        engine.lookup_close_plan(first_plan.request()),
        ClosePlanLookup::RetiredTerminal
    ));
    assert_eq!(engine.close_plans().count(), 1);
    assert_eq!(engine.active_close_plans().count(), 1);
}

#[test]
fn journal_close_escape_is_a_formal_engine_input() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);

    let expected = engine.version();
    let transition = submit_input_with_empty_journal(
        &mut engine,
        &mut host,
        provider,
        StableInputSourceId::new(91),
        EngineInput::CancelActiveInteractionWithEscape {
            expected,
            delivery: EscapeDelivery::Surface(SURFACE),
        },
    );
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        dockspace::transition::InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                status: InteractionStatus::Pressed { .. },
                reason: InteractionCancelReason::Escape,
            },
            ..
        }
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.active_close_plans().count(), 0);
}

#[test]
fn surface_escape_cannot_cancel_a_gesture_owned_by_another_surface() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);
    let wrong_surface = SurfaceId::new(99);
    let expected = engine.version();

    let transition = submit_input_with_empty_journal(
        &mut engine,
        &mut host,
        provider,
        StableInputSourceId::new(92),
        EngineInput::CancelActiveInteractionWithEscape {
            expected,
            delivery: EscapeDelivery::Surface(wrong_surface),
        },
    );

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        dockspace::transition::InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::EscapeDeliveryUnavailable {
                    delivery: EscapeDelivery::Surface(surface),
                },
            ),
            ..
        } if *surface == wrong_surface
    ));
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );
}

#[test]
fn native_escape_requires_the_exact_current_viewport_incarnation() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let stale = register_native_viewport(&mut engine, &mut host);
    support::submit_input(
        &mut engine,
        &mut host,
        WORKSPACE_REBIND_SOURCE,
        EngineInput::ReplaceWorkspace(workspace()),
    )
    .expect("workspace replacement reincarnates the native binding");
    let current = engine
        .viewport()
        .viewport(SURFACE)
        .expect("the logical viewport remains registered")
        .binding();
    assert_ne!(current, stale);
    publish_native_snapshot(&mut engine, &mut host, current);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);
    let expected = engine.version();

    let rejected = submit_input_with_empty_journal(
        &mut engine,
        &mut host,
        provider,
        StableInputSourceId::new(93),
        EngineInput::CancelActiveInteractionWithEscape {
            expected,
            delivery: EscapeDelivery::NativeBinding(stale),
        },
    );
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::EscapeDeliveryUnavailable {
                    delivery: EscapeDelivery::NativeBinding(binding),
                },
            ),
            ..
        } if *binding == stale
    ));
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );

    let accepted = submit_input_with_empty_journal(
        &mut engine,
        &mut host,
        provider,
        StableInputSourceId::new(94),
        EngineInput::CancelActiveInteractionWithEscape {
            expected,
            delivery: EscapeDelivery::NativeBinding(current),
        },
    );
    assert!(matches!(
        accepted.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Cancelled {
                status: InteractionStatus::Pressed { session: cancelled },
                reason: InteractionCancelReason::Escape,
            },
            ..
        } if *cancelled == session
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

fn assert_close_terminal_edge_cancels(
    kind: PointerEdgeKind,
    capture: Authority<PointerCaptureOwner>,
    expected_reason: InteractionCancelReason,
) {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);

    let sequence = PointerEdgeSequence::new(2);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(1),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            capture,
        )],
    )
    .expect("terminal edge journal is contiguous");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("terminal edge follows the press watermark");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("terminal edge candidate roster exists")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("terminal receipt set is exact"),
        )
        .expect("terminal receipt stages");
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

#[test]
fn journal_close_capture_none_and_foreign_cancel_the_pressed_click() {
    for owner in [PointerCaptureOwner::None, PointerCaptureOwner::Foreign] {
        assert_close_terminal_edge_cancels(
            PointerEdgeKind::CaptureChanged,
            Authority::Known(owner),
            InteractionCancelReason::CaptureLost,
        );
    }
}

#[test]
fn journal_close_stream_cancel_prevents_a_successor_release_from_settling_it() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);
    let mut frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::StreamCancelled(
                dockspace::pointer_journal::PointerStreamCancelReason::ExplicitPlatformCancellation,
            ),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Pressed { .. },
            reason: InteractionCancelReason::PointerStreamCancelled,
        }]
    ));
    assert!(
        transition.reduced_pointer_edges()[1]
            .interaction_outcomes()
            .is_empty()
    );
    assert_eq!(engine.active_close_plans().count(), 0);
}

#[test]
fn retiring_click_provider_or_host_cancels_pressed_state() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    arm_journal_close(&mut engine, &mut host, provider, close, point);
    let retirement = engine
        .retire_pointer_provider(provider)
        .expect("provider retirement succeeds");
    assert!(matches!(
        retirement.interaction_events(),
        [event] if matches!(
            event.kind(),
            dockspace::interaction::InteractionEventKind::Cancelled {
                status: InteractionStatus::Pressed { .. },
                reason: InteractionCancelReason::PointerProviderRetired,
            }
        )
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    create_local_provider(&mut engine, &host);
    let successor = engine
        .pointer_provider()
        .expect("successor provider is live");
    arm_journal_close(&mut engine, &mut host, successor, close, point);
    host.close(&mut engine)
        .expect("presentation host retirement succeeds");
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.active_close_plans().count(), 0);
}

#[test]
fn click_survives_a_new_emission_with_the_same_requirement_and_coordinates() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
    let session = arm_journal_close(&mut engine, &mut host, provider, close, point);

    let mut emission = host.begin(&engine);
    emission
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("emission preserves the active provider watermark");
    emission
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    let emission = complete_host_frame_with_current_outputs(&engine, emission);
    host.finish_presentation(emission, &mut engine);
    assert_eq!(
        engine.interaction().status(),
        InteractionStatus::Pressed { session }
    );

    let release = release_journal_click(
        &mut engine,
        &mut host,
        provider,
        1,
        point,
        PointerReceiverDeliveryDisposition::Dock(close),
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::CloseRequested { .. }]
    ));
}

#[test]
fn policy_source_and_presentation_lineage_changes_cancel_pressed_clicks() {
    enum Change {
        Policy,
        Source,
        Presentation,
    }
    for change in [Change::Policy, Change::Source, Change::Presentation] {
        let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface(&mut engine, &mut host, SURFACE, bounds());
        create_local_provider(&mut engine, &host);
        let provider = engine.pointer_provider().expect("provider is live");
        let projection = interaction(&engine);
        let (close, point) = point_inside_tab_close(projection, ItemId::new(1));
        arm_journal_close(&mut engine, &mut host, provider, close, point);
        let expected = engine.version();
        let input = match change {
            Change::Policy => {
                let mut policy = engine.policy().clone();
                policy.set_close_capability(CloseCapability::Disabled);
                EngineInput::ReplacePolicy { expected, policy }
            }
            Change::Source => {
                let mut builder = Workspace::builder();
                let tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
                builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
                builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
                EngineInput::ReplaceWorkspace(
                    builder.build().expect("replacement workspace is valid"),
                )
            }
            Change::Presentation => EngineInput::ReplacePresentationConfig {
                expected,
                config: DockPresentationConfig::builder()
                    .tab_bar_height(32.0)
                    .build()
                    .expect("replacement config is valid"),
            },
        };
        submit_input_with_empty_journal(
            &mut engine,
            &mut host,
            provider,
            StableInputSourceId::new(92),
            input,
        );
        assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
        assert_eq!(engine.active_close_plans().count(), 0);
    }
}

#[test]
fn resolved_journal_preview_commits_only_after_its_exact_presentation_is_observed() {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_tab(projection, ItemId::new(1));
    let (drop_region, target) = point_inside_center_target(projection, target_tabs);

    let mut gesture_frame = host.begin(&engine);
    let projection = gesture_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed gesture frame has current interaction authority");
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source tab receives press");
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(drop_region),
    )
    .expect("target hover is bound to the frozen output");
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    complete(&engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, &mut engine);
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

    let mut paint_frame = host.begin(&engine);
    paint_frame
        .submit_pointer_journal(provider, empty_journal(2))
        .expect("paint frame preserves the provider watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty receipt set stages");
    let paint_frame = complete_host_frame_with_current_outputs(&engine, paint_frame);
    let paint_transition = host.finish_presentation(paint_frame, &mut engine);
    let painted_preview = paint_transition
        .presentation_emissions()
        .iter()
        .find(|emission| emission.output().surface() == SURFACE)
        .and_then(|emission| emission.output().payload().interaction())
        .and_then(|interaction| interaction.drag_preview());
    assert_eq!(
        painted_preview,
        engine
            .interaction()
            .preview()
            .map(|preview| preview.token()),
        "the emitted output freezes the exact active preview token"
    );

    let mut release_frame = host.begin(&engine);
    let release_projection = release_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed release frame has current interaction authority");
    let release_hover = PointerReceiverHoverHit::new(
        release_projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(drop_region),
    )
    .expect("release hover is bound to the frame-begin output");
    submit_pointer_edge_with_observation(
        &mut release_frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                release_hover,
            )])
            .expect("release answers hover"),
        ),
    );
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
            if items.len() == 2
                && items.contains(&ItemId::new(1))
                && items.contains(&ItemId::new(3))
    ));
}

#[test]
fn journal_partial_subtree_known_none_uses_core_reserved_contained_identity() {
    let (workspace, _) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_group, press) = point_inside_tab_group(projection, ItemId::new(1));
    let target = point_without_hover_receiver(projection, press);

    let mut gesture_frame = host.begin(&engine);
    let projection = gesture_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed subtree frame has current interaction authority");
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_group),
    )
    .expect("group grip receives the press");
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("known absence is bound to the frozen output");
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    complete(&engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, &mut engine);

    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(preview),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ] if matches!(
            preview.visual(),
            PreviewVisual::Contained {
                surface: SURFACE,
                fallback: false,
                ..
            }
        )
    ));

    let mut paint_frame = host.begin(&engine);
    paint_frame
        .submit_pointer_journal(provider, empty_journal(2))
        .expect("paint frame preserves the provider watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty receipt set stages");
    let paint_frame = complete_host_frame_with_current_outputs(&engine, paint_frame);
    host.finish_presentation(paint_frame, &mut engine);

    let mut release_frame = host.begin(&engine);
    let release_projection = release_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed release frame has current interaction authority");
    let release_hover = PointerReceiverHoverHit::new(
        release_projection,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("release absence is bound to the frame-begin output");
    submit_pointer_edge_with_observation(
        &mut release_frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                release_hover,
            )])
            .expect("release answers hover"),
        ),
    );
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
    let (detached_root, detached_record) = engine
        .workspace()
        .roots()
        .find(|(root, _)| {
            *root != ROOT
                && matches!(
                    engine.workspace().presentation_for_root(*root),
                    Some(dockspace::RootPresentationOwner::Contained {
                        surface: SURFACE,
                        ..
                    })
                )
        })
        .expect("core created one fresh contained root for the partial subtree");
    assert_ne!(detached_root, ROOT);
    assert!(matches!(
        engine.workspace().node(detached_record.node),
        Some(Node::Tabs { items, .. })
            if items == &[ItemId::new(1), ItemId::new(2)]
    ));
    assert_eq!(
        engine.workspace().item_multiset(),
        [
            (ItemId::new(1), 1),
            (ItemId::new(2), 1),
            (ItemId::new(3), 1)
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn journal_known_none_contained_fallback_is_rejected_by_core_policy() {
    let (workspace, _) = split_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut engine = DockEngine::new(workspace, policy).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_group, press) = point_inside_tab_group(projection, ItemId::new(1));
    let target = point_without_hover_receiver(projection, press);
    let before_workspace = engine.workspace().clone();
    let before_frontier = engine.presentation_identity_frontier();

    let transition = begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source_group,
        press,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );

    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: None,
                status: PreviewResolutionStatus::Rejected,
                ..
            }
        ]
    ));
    assert_eq!(engine.workspace(), &before_workspace);
    assert!(
        engine.presentation_identity_frontier().last_root() > before_frontier.last_root(),
        "activation burns its core reservation even when policy rejects the candidate"
    );
    assert!(
        engine.presentation_identity_frontier().last_floating() > before_frontier.last_floating(),
        "all activation-time presentation identities remain retired"
    );
}

#[test]
fn journal_complete_root_known_none_reuses_root_and_mints_only_floating_identity() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_group, press) = point_inside_tab_group(projection, ItemId::new(1));
    let target = point_without_hover_receiver(projection, press);
    let before_frontier = engine.presentation_identity_frontier();

    let transition = begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source_group,
        press,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(preview),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ] if matches!(preview.visual(), PreviewVisual::Contained { .. })
    ));
    assert_eq!(
        engine.presentation_identity_frontier().last_root(),
        before_frontier.last_root(),
        "a complete root keeps its semantic identity"
    );
    assert!(
        engine.presentation_identity_frontier().last_floating() > before_frontier.last_floating(),
        "the new contained carrier receives a core-minted identity"
    );

    paint_active_journal_drag(&mut engine, &mut host, provider);
    let release = release_journal_drag(
        &mut engine,
        &mut host,
        provider,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                ..
            },
            InteractionOutcome::DragDelivered { .. }
        ]
    ));
    assert!(matches!(
        engine.workspace().presentation_for_root(ROOT),
        Some(dockspace::RootPresentationOwner::Contained {
            surface: SURFACE,
            ..
        })
    ));
    assert_eq!(
        engine
            .workspace()
            .surface(SURFACE)
            .and_then(|surface| surface.main_root),
        None
    );
}

#[test]
fn journal_partial_payload_installs_core_reserved_root_on_rootless_background() {
    let mut engine = DockEngine::new(rootless_contained_workspace(), DockPolicy::default())
        .expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_tab(projection, ItemId::new(1));
    let (background_region, target) = point_inside_surface_background(projection);
    let before_frontier = engine.presentation_identity_frontier();

    let transition = begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source_tab,
        press,
        target,
        PointerReceiverHoverHitDisposition::Dock(background_region),
    );
    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                preview: Some(preview),
                status: PreviewResolutionStatus::Resolved,
                ..
            }
        ] if matches!(
            preview.visual(),
            PreviewVisual::Dock {
                target: DropTargetId::SurfaceBackground { surface: SURFACE },
                ..
            }
        )
    ));
    assert!(
        engine.presentation_identity_frontier().last_root() > before_frontier.last_root(),
        "partial content reserves a fresh root at activation"
    );

    paint_active_journal_drag(&mut engine, &mut host, provider);
    let release = release_journal_drag(
        &mut engine,
        &mut host,
        provider,
        target,
        PointerReceiverHoverHitDisposition::Dock(background_region),
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                ..
            },
            InteractionOutcome::DragDelivered { .. }
        ]
    ));
    let main_root = engine
        .workspace()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root)
        .expect("background delivery installs a main root");
    assert_ne!(main_root, CONTAINED_ROOT);
    assert!(matches!(
        engine.workspace().presentation_for_root(main_root),
        Some(dockspace::RootPresentationOwner::Main { surface: SURFACE })
    ));
    let main = engine
        .workspace()
        .root(main_root)
        .expect("new main root exists");
    assert!(matches!(
        engine.workspace().node(main.node),
        Some(Node::Tabs { items, .. }) if items == &[ItemId::new(1)]
    ));
    let contained = engine
        .workspace()
        .root(CONTAINED_ROOT)
        .expect("source contained root remains");
    assert!(matches!(
        engine.workspace().node(contained.node),
        Some(Node::Tabs { items, .. }) if items == &[ItemId::new(2)]
    ));
}

#[test]
fn journal_contained_release_waits_for_the_release_edge_preview() {
    let (workspace, _) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_group, press) = point_inside_tab_group(projection, ItemId::new(1));
    let first_target = point_without_hover_receiver(projection, press);

    begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source_group,
        press,
        first_target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    paint_active_journal_drag(&mut engine, &mut host, provider);
    let release_projection = interaction(&engine);
    let final_target = point_without_hover_receiver(release_projection, first_target);
    assert_ne!(final_target, first_target);
    let before = engine.workspace().clone();

    let release = release_journal_drag(
        &mut engine,
        &mut host,
        provider,
        final_target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    assert!(matches!(
        release.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            },
            InteractionOutcome::ReleasePending { session, preview }
        ] if engine.pending_release_preview() == Some((*session, *preview))
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn journal_contained_resize_commits_only_after_its_exact_preview_is_presented() {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid workspace");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (resize, press) = point_inside_contained_resize(
        projection,
        CONTAINED_FLOATING,
        ContainedResizeDirection::East,
    );
    let moved = LogicalPoint::new(press.x() + 48.0, press.y())
        .expect("contained resize move point is finite");
    let before = engine
        .workspace()
        .contained_floating(CONTAINED_FLOATING)
        .expect("contained record exists")
        .rect;

    let mut gesture_frame = host.begin(&engine);
    let projection = gesture_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed contained frame has current interaction authority");
    let press_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(resize))
            .expect("contained resize delivery is output-bound");
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, &mut engine);

    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ContainedTransformBegan { .. }]
    ));
    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [InteractionOutcome::ContainedTransformPreviewUpdated { preview, .. }]
            if preview.rect().width() > before.width()
    ));
    assert_eq!(
        engine
            .workspace()
            .contained_floating(CONTAINED_FLOATING)
            .expect("contained record remains durable")
            .rect,
        before,
        "the first frame may publish a preview but must not mutate the durable rectangle"
    );

    let mut paint_frame = host.begin(&engine);
    paint_frame
        .submit_pointer_journal(provider, empty_journal(2))
        .expect("paint frame preserves the provider watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an empty receipt set"),
        )
        .expect("empty receipt set stages");
    let paint_frame = complete_host_frame_with_current_outputs(&engine, paint_frame);
    let paint_transition = host.finish_presentation(paint_frame, &mut engine);
    let painted_preview = paint_transition
        .presentation_emissions()
        .iter()
        .find(|emission| emission.output().surface() == SURFACE)
        .and_then(|emission| emission.output().payload().interaction())
        .and_then(|interaction| interaction.contained_transform_preview());
    let active_preview = engine
        .interaction()
        .active_contained_transform_view()
        .and_then(|transform| transform.preview())
        .expect("resize remains active after its preview paint")
        .token();
    assert_eq!(
        painted_preview,
        Some(active_preview),
        "the second frame paints the exact frozen contained preview token"
    );

    let mut release_frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut release_frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut release_frame);
    let release_transition = host.finish(release_frame, &mut engine);

    assert!(matches!(
        release_transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::ContainedTransformPreviewUpdated { .. },
            InteractionOutcome::ContainedTransformDelivered { changed: true, .. }
        ]
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    let after = engine
        .workspace()
        .contained_floating(CONTAINED_FLOATING)
        .expect("contained record remains durable")
        .rect;
    assert!(after.width() > before.width());
    assert_eq!(after.x(), before.x());
    assert_eq!(after.y(), before.y());
}

#[test]
fn journal_contained_title_drag_is_owned_by_the_pointer_stream() {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid workspace");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (title, press) = point_inside_contained_title(projection, CONTAINED_FLOATING);
    let moved = LogicalPoint::new(press.x() + 32.0, press.y())
        .expect("contained title move point is finite");

    let mut frame = host.begin(&engine);
    let press_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(title))
            .expect("contained title delivery is output-bound");
    let frame_blocker = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::ContainedFrameBlocker(actual)
                    if actual == CONTAINED_FLOATING
            ) && region.hit().contains(moved)
        })
        .expect("the moved point remains over the contained frame blocker");
    let move_hover = PointerReceiverHoverHit::new(
        projection,
        moved,
        PointerReceiverHoverHitDisposition::Dock(frame_blocker.id()),
    )
    .expect("contained title move is output-bound");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                move_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated { .. }
        ]
    ));
    let drag = engine
        .interaction()
        .active_drag_view()
        .expect("contained title move starts a journal-owned drag");
    assert_eq!(
        drag.journal_stream(),
        Some(transition.reduced_pointer_edges()[0].stream())
    );
    assert!(matches!(
        drag.payload(),
        dockspace::command::MovePayload::Subtree(_)
    ));
}

#[test]
fn journal_contained_close_requires_a_matching_release() {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid workspace");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (close, point) = point_inside_contained_close(projection, CONTAINED_FLOATING);
    let frame_blocker = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::ContainedFrameBlocker(actual)
                    if actual == CONTAINED_FLOATING
            ) && region.hit().contains(point)
        })
        .expect("the contained frame is the drag-lane blocker beneath its close button")
        .id();

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(0, point))
        .expect("contained close press freezes one receiver candidate");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("candidate roster exists")
        .candidates()
        .first()
        .expect("press creates one candidate")
        .clone();
    let delivery = PointerReceiverDelivery::from_lanes(
        projection,
        PointerReceiverDeliveryDisposition::Dock(close),
        PointerReceiverDeliveryDisposition::Dock(frame_blocker),
    )
    .expect("distinct click and drag receivers share one exact output authority");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("close press answers delivery"),
                ),
            )])
            .expect("contained close receipt batch is exact"),
        )
        .expect("contained close receipt stages");
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

    let release_projection = interaction(&engine);
    let mut release_frame = host.begin(&engine);
    release_frame
        .submit_pointer_journal(provider, release_journal(1, Authority::Known(point)))
        .expect("contained close release follows the press watermark");
    let release_candidate = release_frame
        .pointer_receiver_candidates()
        .expect("release candidate roster exists")
        .candidates()
        .first()
        .expect("release creates one candidate")
        .clone();
    let release_delivery = PointerReceiverDelivery::new(
        release_projection,
        PointerReceiverDeliveryDisposition::Dock(close),
    )
    .expect("contained close release is output-bound");
    let release_hover = PointerReceiverHoverHit::new(
        release_projection,
        point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("contained close hover is output-bound");
    release_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                exact_candidate_observation(
                    &release_candidate,
                    Some(release_delivery),
                    Some(release_hover),
                ),
            )])
            .expect("contained release receipt set is exact"),
        )
        .expect("contained release receipts stage");
    complete(&engine, &mut release_frame);
    let release = host.finish(release_frame, &mut engine);
    let [
        InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        },
    ] = release.reduced_pointer_edges()[0].interaction_outcomes()
    else {
        panic!("matching contained release must open one close plan");
    };
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), ItemId::new(2));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(release.reduced_inputs().is_empty());
    assert!(
        engine
            .workspace()
            .contained_floating(CONTAINED_FLOATING)
            .is_some()
    );
}

#[test]
fn surface_local_tab_press_subthreshold_move_release_is_ticket_causal() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (tab, press) = point_inside_inactive_tab(projection);
    let moved = LogicalPoint::new(press.x() + 1.0, press.y()).expect("move remains finite");
    let before = engine.version();

    let mut frame = host.begin(&engine);
    let press_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(tab))
            .expect("inactive tab receives the press");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    let moved_hover = PointerReceiverHoverHit::new(
        projection,
        moved,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("move has an exact known-none hover result");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                moved_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    let reduced = transition.reduced_pointer_edges();
    assert_eq!(reduced.len(), 3);
    assert!(
        reduced
            .windows(2)
            .all(|pair| pair[0].stream() == pair[1].stream())
    );
    assert!(matches!(
        reduced[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(reduced[1].interaction_outcomes().is_empty());
    assert!(matches!(
        reduced[2].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Armed { .. },
            reason: InteractionCancelReason::ReleasedBeforeDrag,
        }]
    ));
    assert!(transition.reduced_inputs().is_empty());
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(
        engine.version().revision().get(),
        before.revision().get() + 1
    );
    let root = engine
        .workspace()
        .root(ROOT)
        .expect("root remains available");
    assert!(matches!(
        engine.workspace().node(root.node),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
    let selected = transition
        .events()
        .iter()
        .find(|event| {
            matches!(
                event.kind(),
                dockspace::event::WorkspaceEventKind::CommandCommitted(
                    dockspace::command::CommandOutcome::Selected {
                        item,
                        changed: true,
                        ..
                    }
                ) if *item == ItemId::new(2)
            )
        })
        .expect("selection emits one workspace event");
    assert_eq!(selected.cause(), reduced[0].cause());
    assert_eq!(transition.interaction_events().len(), 1);
    assert_eq!(
        transition.interaction_events()[0].cause(),
        reduced[2].cause()
    );
    engine
        .workspace()
        .validate()
        .expect("journal interaction preserves workspace invariants");
}

#[test]
fn authoritative_tab_list_control_then_row_click_selects_and_closes() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let (row, point) = point_inside_click_region(&engine, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::TabListMenuRow { tab, .. }
                if tab.item == ItemId::new(2)
        )
    });
    let before = engine.version();
    let pressed = submit_exact_click_edge(&mut engine, &mut host, provider, 0, true, row, point);
    assert!(
        pressed.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    let released = submit_exact_click_edge(&mut engine, &mut host, provider, 1, false, row, point);

    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuItemSelected {
            tab,
            changed: true,
            ..
        }] if tab.item == ItemId::new(2)
    ));
    assert_eq!(released.events().len(), 1);
    assert_eq!(
        engine.version(),
        dockspace::transition::WorkspaceVersion::new(
            before.epoch(),
            before
                .revision()
                .checked_next()
                .expect("one successful row selection must advance exactly once"),
        )
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    let root = engine.workspace().root(ROOT).expect("root remains live");
    assert!(matches!(
        engine.workspace().node(root.node),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
    commit_empty_pointer_journal(&mut engine, &mut host, provider, 2);
}

#[test]
fn tab_list_row_click_survives_same_output_presentation_refresh() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let (row, point) = point_inside_click_region(&engine, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::TabListMenuRow { tab, .. }
                if tab.item == ItemId::new(2)
        )
    });
    let before = interaction(&engine);
    let output = before.output_ticket();
    let authority = before.authority();

    let press = stage_exact_click_edge_with_completion(
        &engine,
        &mut host,
        provider,
        0,
        true,
        row,
        point,
        |_engine, _frame| {},
    );
    let press = complete_host_frame_with_current_outputs(&engine, press);
    let pressed = host.finish_presentation(press, &mut engine);
    assert!(
        pressed.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );

    let release = stage_exact_click_edge(&engine, &mut host, provider, 1, false, row, point);
    let refreshed = release
        .view()
        .interaction_projection(SURFACE)
        .expect("release observes the repainted popup output");
    assert_eq!(refreshed.output_ticket(), output);
    assert_ne!(refreshed.authority(), authority);
    let released = host.finish(release, &mut engine);

    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuItemSelected {
            tab,
            changed: true,
            ..
        }] if tab.item == ItemId::new(2)
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn authoritative_selected_menu_row_closes_without_workspace_advance() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let (row, point) = point_inside_click_region(&engine, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::TabListMenuRow { tab, .. }
                if tab.item == ItemId::new(1)
        )
    });
    let before = engine.version();
    submit_exact_click_edge(&mut engine, &mut host, provider, 0, true, row, point);
    let released = submit_exact_click_edge(&mut engine, &mut host, provider, 1, false, row, point);

    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuItemSelected {
            tab,
            changed: false,
            ..
        }] if tab.item == ItemId::new(1)
    ));
    assert_eq!(engine.version(), before);
    assert!(released.events().is_empty());
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    commit_empty_pointer_journal(&mut engine, &mut host, provider, 2);

    engine
        .retire_pointer_provider(provider)
        .expect("idle provider retirement is valid");
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    assert!(
        interaction(&engine)
            .plan()
            .tab_list_menu_records()
            .is_empty(),
        "the no-op selection still closes the exact menu session"
    );
}

#[test]
fn authoritative_menu_backdrop_dismisses_the_active_menu_atomically() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let projection = interaction(&engine);
    let session = projection
        .plan()
        .popup()
        .session()
        .expect("open menu must own the popup plane");
    let backdrop = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            region.id().kind() == PresentationHitRegionKind::TabListMenuBackdrop(session)
        })
        .copied()
        .expect("open menu must publish a backdrop receiver");
    let backdrop_bounds = projection
        .plan()
        .tab_list_menu_backdrop_records()
        .iter()
        .find(|record| record.session() == session)
        .expect("open menu must publish one backdrop record")
        .bounds();
    let menu_bounds = projection
        .plan()
        .tab_list_menu_records()
        .iter()
        .find(|record| record.session() == session)
        .expect("open menu must publish one menu record")
        .bounds();
    let point = [
        LogicalPoint::new(backdrop_bounds.x() + 1.0, backdrop_bounds.y() + 1.0)
            .expect("backdrop corner must be finite"),
        LogicalPoint::new(
            backdrop_bounds.max().x() - 1.0,
            backdrop_bounds.max().y() - 1.0,
        )
        .expect("backdrop opposite corner must be finite"),
    ]
    .into_iter()
    .find(|point| backdrop.hit().contains(*point) && !menu_bounds.contains(*point))
    .expect("one surface corner must remain outside the menu frame");
    let before = engine.version();

    submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        0,
        true,
        backdrop.id(),
        point,
    );
    let released = submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        1,
        false,
        backdrop.id(),
        point,
    );

    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuDismissed { session: actual }] if *actual == session
    ));
    assert_eq!(engine.version(), before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        engine.presentation_requirements().popup(),
        dockspace::tab_strip::PopupPlaneRequirement::Inactive { .. }
    ));
    commit_empty_pointer_journal(&mut engine, &mut host, provider, 2);
}

#[test]
fn authoritative_menu_frame_blocker_consumes_without_dismissing() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let projection = interaction(&engine);
    let session = projection
        .plan()
        .popup()
        .session()
        .expect("open menu must own the popup plane");
    let blocker = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| region.id().kind() == PresentationHitRegionKind::TabListMenuBlocker(session))
        .copied()
        .expect("open menu must publish a frame blocker");
    let menu = projection
        .plan()
        .tab_list_menu_records()
        .iter()
        .find(|record| record.session() == session)
        .expect("open menu must publish one menu record");
    let point = [
        LogicalPoint::new(menu.bounds().x() + 1.0, menu.bounds().y() + 1.0)
            .expect("menu corner must be finite"),
        LogicalPoint::new(menu.bounds().max().x() - 1.0, menu.bounds().max().y() - 1.0)
            .expect("menu opposite corner must be finite"),
    ]
    .into_iter()
    .find(|point| {
        blocker.hit().contains(*point)
            && !menu
                .rows()
                .iter()
                .any(|row| row.hit().is_some_and(|hit| hit.contains(*point)))
    })
    .expect("menu padding must contain a blocker-only point");

    submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        0,
        true,
        blocker.id(),
        point,
    );
    let released = submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        1,
        false,
        blocker.id(),
        point,
    );

    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuFrameConsumed { session: actual }] if *actual == session
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(
        engine.presentation_requirements().popup().session(),
        Some(session)
    );
    commit_empty_pointer_journal(&mut engine, &mut host, provider, 2);
}

#[test]
fn menu_row_wins_over_a_claimed_frame_blocker_without_advancing_watermark() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );

    let provider = open_overflow_menu_through_journal(&mut engine, &mut host);
    let provider = republish_open_overflow_menu(&mut engine, &mut host, provider);
    let projection = interaction(&engine);
    let session = projection
        .plan()
        .popup()
        .session()
        .expect("open menu must own the popup plane");
    let row = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabListMenuRow { menu, .. } if menu == session
            )
        })
        .copied()
        .expect("open menu must publish a row receiver");
    let row_point = LogicalPoint::new(
        row.hit().rect().x() + row.hit().rect().width() * 0.5,
        row.hit().rect().y() + row.hit().rect().height() * 0.5,
    )
    .expect("row midpoint must be finite");
    let blocker = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| region.id().kind() == PresentationHitRegionKind::TabListMenuBlocker(session))
        .copied()
        .expect("open menu must publish a frame blocker");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                0,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(row_point),
                },
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            ),
        )
        .expect("row press prepares");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("row press candidate exists")
        .candidates()[0]
        .clone();
    let blocker_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(blocker.id()),
    )
    .expect("blocker delivery belongs to the frozen output");
    let observation = PointerReceiverObservation::Presented(
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
            blocker_delivery,
        )])
        .expect("blocker answers the delivery probe"),
    );
    assert_eq!(
        frame.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("one exact row receipt"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
    );
    let error = frame
        .finish(&mut engine)
        .expect_err("the lower-priority blocker claim must be rejected atomically");
    assert!(matches!(
        error,
        EngineError::PointerReceiverGeometry {
            source: PointerReceiverGeometryError::ReceiverWinnerMismatch {
                lane: PresentationPointerLane::Click,
                claimed: Some(claimed),
                winner: Some(winner),
                ..
            }
        } if claimed == blocker.id() && winner == row.id()
    ));
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let pressed = submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        0,
        true,
        row.id(),
        row_point,
    );
    assert!(
        pressed.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    let released = submit_exact_click_edge(
        &mut engine,
        &mut host,
        provider,
        1,
        false,
        row.id(),
        row_point,
    );
    assert!(matches!(
        released.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::TabListMenuItemSelected { .. }]
    ));
}

#[test]
fn policy_change_after_authoritative_control_press_cancels_atomically() {
    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("pointer provider is live");
    let (control, point) = point_inside_click_region(&engine, |kind| {
        matches!(
            kind,
            PresentationHitRegionKind::TabStripControl(TabStripControlId::TabListMenu(_))
        )
    });
    submit_exact_click_edge(&mut engine, &mut host, provider, 0, true, control, point);
    let pressed = engine.interaction().status();
    assert!(matches!(pressed, InteractionStatus::Pressed { .. }));

    let mut disabled = DockPolicy::default();
    disabled.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("policy frame preserves the committed pointer watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    append_host_input(
        &mut frame,
        StableInputSourceId::new(0x71),
        SourceSequence::new(1),
        EngineInput::ReplacePolicy {
            expected: engine.version(),
            policy: disabled,
        },
    )
    .expect("policy replacement stages after the complete pointer journal");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            status,
            reason: InteractionCancelReason::PolicyChanged,
        } if *status == pressed
    )));

    let mut watermark_probe = host.begin(&engine);
    watermark_probe
        .submit_pointer_journal(provider, empty_journal(1))
        .expect("policy cancellation neither replays nor skips the provider watermark");
    watermark_probe
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty retry has an exact empty receipt set"),
        )
        .expect("empty retry receipts stage");
    complete(&engine, &mut watermark_probe);
    host.finish(watermark_probe, &mut engine);
}

#[test]
fn surface_local_tab_exact_threshold_move_begins_drag() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (tab, press) = point_inside_inactive_tab(projection);
    let moved = LogicalPoint::new(press.x() + 6.0, press.y()).expect("move remains finite");

    let mut frame = host.begin(&engine);
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(tab))
            .expect("inactive tab receives the press");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    let hover = exact_hover_receipt(&frame, moved);
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                hover,
            )])
            .expect("move answers hover"),
        ),
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated { .. }
        ]
    ));
    let drag = engine
        .interaction()
        .active_drag_view()
        .expect("exact threshold starts the journal-owned drag");
    assert_eq!(drag.phase(), DragPhase::Dragging);
    assert_eq!(
        drag.journal_stream(),
        Some(transition.reduced_pointer_edges()[1].stream())
    );
}

#[test]
fn surface_local_tab_wrong_receiver_region_rejects_atomically() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (correct_tab, press) = point_inside_inactive_tab(projection);
    let wrong_tab = projection
        .hit_manifest()
        .regions()
        .iter()
        .find_map(|region| match region.id().kind() {
            PresentationHitRegionKind::TabBody(tab) if tab.item == ItemId::new(1) => {
                Some(region.id())
            }
            _ => None,
        })
        .expect("other tab has a receiver region");
    let before_workspace = engine.workspace().clone();
    let before_version = engine.version();
    let before_interaction = engine.interaction().clone();

    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![PointerEdge::new(
            PointerEdgeSequence::new(1),
            PointerId::new(7),
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    )
    .expect("journal is contiguous");
    let mut failed = host.begin(&engine);
    failed
        .submit_pointer_journal(provider, journal.clone())
        .expect("journal prepares");
    let failed_candidate = failed
        .pointer_receiver_candidates()
        .expect("candidate roster exists")
        .candidates()[0]
        .clone();
    let wrong_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(wrong_tab),
    )
    .expect("wrong region is structurally part of the same manifest");
    assert_eq!(
        failed.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([failed_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(wrong_delivery),
                    ])
                    .expect("delivery observation is structurally valid"),
                ),
            )])
            .expect("receipt set is exact"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "invalid geometry poisons the input prefix immediately",
    );
    assert!(matches!(
        failed.finish(&mut engine),
        Err(EngineError::PointerReceiverGeometry {
            source: PointerReceiverGeometryError::RegionDoesNotCoverPoint { .. }
                | PointerReceiverGeometryError::ReceiverWinnerMismatch { .. },
        })
    ));
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.interaction(), &before_interaction);

    let retry_projection = interaction(&engine);
    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(provider, journal)
        .expect("failed receipt did not advance the journal watermark");
    let retry_candidate = retry
        .pointer_receiver_candidates()
        .expect("retry candidate roster exists")
        .candidates()[0]
        .clone();
    let correct_delivery = PointerReceiverDelivery::new(
        retry_projection,
        PointerReceiverDeliveryDisposition::Dock(correct_tab),
    )
    .expect("correct tab receives the retry");
    retry
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([retry_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(correct_delivery),
                    ])
                    .expect("retry answers delivery"),
                ),
            )])
            .expect("retry receipt set is exact"),
        )
        .expect("retry receipts stage");
    complete(&engine, &mut retry);
    let transition = host.finish(retry, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
}

#[test]
fn host_frame_commits_journal_only_after_exact_receipts() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_journal(0))
        .expect("first journal is accepted provisionally");
    assert_eq!(
        frame
            .pointer_receiver_candidates()
            .expect("journal freezes one receiver roster")
            .candidates()[0]
            .probes(),
        PointerReceiverProbeRequest::Delivery
    );
    frame
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
        .expect("exact primary press receipts stage");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    let accepted = transition.reduced_pointer_edges();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].edge().sequence(), PointerEdgeSequence::new(1));
    assert_eq!(accepted[0].stream().pointer(), PointerId::new(7));
    assert!(matches!(
        accepted[0].cause(),
        ReductionCause::PointerEdge { ticket, stream, .. }
            if ticket == accepted[0].ticket() && stream == accepted[0].stream()
    ));

    let mut next = host.begin(&engine);
    next.submit_pointer_journal(provider, empty_journal(1))
        .expect("committed journal advanced the provider watermark");
    next.submit_pointer_receiver_receipts(
        PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
            .expect("empty candidate roster has an exact empty receipt set"),
    )
    .expect("empty receipt set stages");
    complete(&engine, &mut next);
    host.finish(next, &mut engine);
}

#[test]
fn host_frame_preserves_semantic_and_pointer_journal_arrival_order() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(91);

    let reduce = |semantic_first: bool| {
        let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface(&mut engine, &mut host, SURFACE, bounds());
        create_local_provider(&mut engine, &host);
        let provider = engine.pointer_provider().expect("provider is live");

        let mut frame = host.begin(&engine);
        if semantic_first {
            frame
                .append_input(
                    SOURCE,
                    SourceSequence::new(1),
                    EngineInput::ValidateWorkspace,
                )
                .expect("semantic input stages before the journal");
        }
        frame
            .submit_pointer_journal(provider, press_journal(0))
            .expect("pointer journal stages at its arrival position");
        if !semantic_first {
            frame
                .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
                .expect("pointer receipt completes its causal position");
            frame
                .append_input(
                    SOURCE,
                    SourceSequence::new(1),
                    EngineInput::ValidateWorkspace,
                )
                .expect("semantic input stages after the journal");
        } else {
            frame
                .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
                .expect("exact primary press receipts stage");
        }
        complete(&engine, &mut frame);
        host.finish(frame, &mut engine)
    };

    let semantic_then_pointer = reduce(true);
    assert!(
        semantic_then_pointer.reduced_inputs()[0]
            .causal_ordinal()
            .get()
            < semantic_then_pointer.reduced_pointer_edges()[0]
                .causal_ordinal()
                .get(),
        "a semantic fact submitted first must reduce before the pointer journal"
    );

    let pointer_then_semantic = reduce(false);
    assert!(
        pointer_then_semantic.reduced_pointer_edges()[0]
            .causal_ordinal()
            .get()
            < pointer_then_semantic.reduced_inputs()[0]
                .causal_ordinal()
                .get(),
        "a pointer journal submitted first must reduce before the semantic fact"
    );
}

#[test]
fn empty_pointer_checkpoint_does_not_consume_a_raw_causal_ordinal() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(0x7a01);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, empty_journal(0))
        .expect("empty provider checkpoint stages");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty checkpoint has an exact empty receipt set"),
        )
        .expect("empty checkpoint receipt set stages");
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        )
        .expect("semantic input follows the transport checkpoint");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(transition.reduced_pointer_edges().is_empty());
    assert_eq!(transition.reduced_inputs()[0].causal_ordinal().get(), 0);
}

#[test]
fn semantic_input_is_rejected_while_pointer_receipt_is_pending() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(0x7a02);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_journal(0))
        .expect("pointer challenge is staged");

    assert_eq!(
        frame.append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::PointerReceiverReceiptsMissingBeforeInput)
    );
    assert_eq!(engine.semantic_input_watermark(), None);
    assert_eq!(
        engine.pointer_button_authority(),
        dockspace::pointer_journal::AnyButtonDownAuthority::Unknown(
            AuthorityUnavailableReason::ProviderUnavailable
        )
    );
}

#[test]
fn same_frame_menu_open_does_not_authorize_an_unpresented_popup_backdrop() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(0x7a01);

    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        overflowing_tab_bounds(),
        overflowing_tab_profile(),
    );
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("pointer provider is live");

    let mut frame = host.begin(&engine);
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed surface is interactive");
    assert!(projection.plan().tab_list_menu_records().is_empty());
    let menu_control = projection
        .plan()
        .tab_strip_control_records()
        .iter()
        .find(|record| matches!(record.id(), TabStripControlId::TabListMenu(_)))
        .expect("overflowing strip publishes a menu control");
    let point = LogicalPoint::new(
        menu_control.bounds().x() + menu_control.bounds().width() * 0.5,
        menu_control.bounds().y() + menu_control.bounds().height() * 0.5,
    )
    .expect("menu-control midpoint is finite");
    let prepared = frame
        .view()
        .prepare_tab_strip_control_activation(SURFACE, menu_control.id())
        .expect("menu control activation is prepared from sealed authority");
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ActivateTabStripControl { prepared },
        )
        .expect("menu activation is sequenced before the pointer edge");
    frame
        .submit_pointer_journal(provider, moved_journal(0, Authority::Known(point)))
        .expect("same-frame pointer segment remains bound to the sealed output");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("move freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let hover = frame
        .view()
        .resolve_hover_drop_receiver(SURFACE, point)
        .expect("sealed core projection resolves hover authority");
    assert_eq!(
        hover.disposition(),
        PointerReceiverHoverHitDisposition::NoReceiver,
        "the closed-menu output cannot contain the future popup backdrop"
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                &candidate,
                None,
                Some(hover),
            ))])
            .expect("move receipt set is exact"),
        )
        .expect("sealed receipt stages");
    complete(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripControlActivated {
                changed: true,
                menu: Some(_),
                ..
            },
            ..
        }
    ));
    assert!(
        transition.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty(),
        "the later edge must not observe or dismiss an unpresented popup"
    );
    assert!(
        engine
            .presentation_requirements()
            .popup()
            .session()
            .is_some()
    );
}

#[test]
fn scene_semantics_coexist_with_pointer_provider() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(93);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("test surface has painted authority");
    let tab = ready
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.close_bounds().is_some())
        .expect("test surface exposes one close control");
    let target = CloseSceneTarget::Tab(*tab.id());
    let scene = ready.stamp();

    let mut frame = host.begin(&engine);
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::RequestSceneClose {
                expected: engine.version(),
                scene,
                target,
            },
        )
        .expect("scene-bound semantic input is valid with a pointer provider");
    frame
        .submit_pointer_journal(provider, empty_journal(0))
        .expect("provider publishes its complete empty journal");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty receipt set stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        dockspace::transition::InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::CloseRequested { .. },
            ..
        }
    ));
}

#[test]
fn presented_semantic_receiver_navigates_tabs_through_the_checked_command_path() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(94);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("test surface has presented semantic authority");
    let current = projection
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.selected())
        .map(|tab| *tab.id())
        .expect("one tab is selected");
    let event = SemanticReceiverEvent::new(
        projection.output_ticket(),
        projection.authority().emission(),
        SemanticDelivery::Headless,
        PresentationHitRegionKind::TabBody(current),
        SemanticReceiverAction::Key(SemanticKey::ArrowRight),
    );
    let expected = engine.version();

    let transition = support::submit_input(
        &mut engine,
        &mut host,
        SOURCE,
        EngineInput::ActivateSemanticReceiver { expected, event },
    )
    .expect("presentation-bound tab navigation reduces");

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert!(matches!(
        engine.workspace().node(current.tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
}

#[test]
fn repeated_semantic_navigation_uses_one_frozen_output_and_advances_logical_focus() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(96);

    let mut engine =
        DockEngine::new(overflowing_tab_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("test surface has presented semantic authority");
    let current = projection
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.selected())
        .map(|tab| *tab.id())
        .expect("one tab is selected");
    let event = SemanticReceiverEvent::new(
        projection.output_ticket(),
        projection.authority().emission(),
        SemanticDelivery::Headless,
        PresentationHitRegionKind::TabBody(current),
        SemanticReceiverAction::Key(SemanticKey::ArrowRight),
    );
    let expected = engine.version();

    let mut frame = host.begin(&engine);
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ActivateSemanticReceiver { expected, event },
        )
        .expect("first repeated edge reduces against the frozen output");
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(2),
            EngineInput::ActivateSemanticReceiver { expected, event },
        )
        .expect("second repeated edge reduces against the same frozen output");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        engine.workspace().node(current.tabs),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(3)
    ));
    assert_eq!(
        transition
            .interaction_events()
            .iter()
            .filter(|event| matches!(
                event.kind(),
                InteractionEventKind::SemanticFocusRequested { .. }
            ))
            .count(),
        2
    );
}

#[test]
fn presented_semantic_receiver_rejects_a_target_absent_from_the_exact_output() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(95);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("test surface has presented semantic authority");
    let missing = TabSceneId {
        root: ROOT,
        tabs: engine.workspace().root(ROOT).expect("root exists").node,
        item: ItemId::new(999),
    };
    let event = SemanticReceiverEvent::new(
        projection.output_ticket(),
        projection.authority().emission(),
        SemanticDelivery::Headless,
        PresentationHitRegionKind::TabBody(missing),
        SemanticReceiverAction::Key(SemanticKey::Enter),
    );
    let expected = engine.version();

    let transition = support::submit_input(
        &mut engine,
        &mut host,
        SOURCE,
        EngineInput::ActivateSemanticReceiver { expected, event },
    )
    .expect("an unavailable receiver is a typed inert outcome");

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::SemanticReceiverUnavailable { target }
            ),
            ..
        } if *target == PresentationHitRegionKind::TabBody(missing)
    ));
}

#[test]
fn semantic_receiver_rejects_a_delivery_from_another_native_incarnation() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(97);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("test surface has headless semantic authority");
    let current = projection
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.selected())
        .map(|tab| *tab.id())
        .expect("one tab is selected");

    let mut foreign_engine =
        DockEngine::new(workspace(), DockPolicy::default()).expect("valid foreign engine");
    let mut foreign_host = TestPresentationHost::new(&mut foreign_engine);
    let foreign_binding = register_native_viewport(&mut foreign_engine, &mut foreign_host);
    let event = SemanticReceiverEvent::new(
        projection.output_ticket(),
        projection.authority().emission(),
        SemanticDelivery::Native(foreign_binding),
        PresentationHitRegionKind::TabBody(current),
        SemanticReceiverAction::Key(SemanticKey::Enter),
    );
    let expected = engine.version();

    let transition = support::submit_input(
        &mut engine,
        &mut host,
        SOURCE,
        EngineInput::ActivateSemanticReceiver { expected, event },
    )
    .expect("a foreign delivery is a typed inert outcome");

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::SemanticDeliveryMismatch {
                    expected: SemanticDelivery::Native(binding),
                    actual: dockspace::presentation_observation::HostPresentationEndpoint::Headless,
                }
            ),
            ..
        } if *binding == foreign_binding
    ));
}

#[test]
fn host_frame_interleaves_pointer_segments_with_semantic_facts() {
    const SOURCE: StableInputSourceId = StableInputSourceId::new(92);

    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_journal(0))
        .expect("press segment stages first");
    frame
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
        .expect("press segment receipts stage");
    frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        )
        .expect("semantic fact stages between pointer segments");

    let moved = LogicalPoint::new(320.0, 254.0).expect("finite move point");
    frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(moved)))
        .expect("move segment is contiguous with the press segment");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("move segment freezes a receiver roster")
        .candidates()[0]
        .clone();
    let hover = exact_hover_receipt(&frame, moved);
    let presented =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(hover)])
            .expect("move segment answers its hover probe");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(presented))
            ])
            .expect("move segment has one exact receipt"),
        )
        .expect("move segment receipts stage");
    complete(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    let reduced_edges = transition.reduced_pointer_edges();
    assert_eq!(reduced_edges.len(), 2);
    assert_eq!(reduced_edges[0].causal_ordinal().get(), 0);
    assert_eq!(transition.reduced_inputs()[0].causal_ordinal().get(), 1);
    assert_eq!(reduced_edges[1].causal_ordinal().get(), 2);
    assert_eq!(reduced_edges[0].stream(), reduced_edges[1].stream());
}

#[test]
fn incomplete_later_pointer_segment_keeps_every_segment_uncommitted() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let before_version = engine.version();
    let before_interaction = engine.interaction().clone();

    let mut incomplete = host.begin(&engine);
    incomplete
        .submit_pointer_journal(provider, press_journal(0))
        .expect("first segment stages");
    incomplete
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &incomplete))
        .expect("first segment receipts stage");
    incomplete
        .submit_pointer_journal(
            provider,
            moved_journal(
                1,
                Authority::Known(LogicalPoint::new(320.0, 254.0).expect("finite move point")),
            ),
        )
        .expect("second segment is provider-contiguous");
    complete(&engine, &mut incomplete);
    assert!(matches!(
        incomplete.finish(&mut engine),
        Err(EngineError::HostFramePointerReceiverReceiptsMissing { provider: actual })
            if actual == provider
    ));
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.interaction(), &before_interaction);

    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(provider, press_journal(0))
        .expect("failed later segment did not advance the first segment watermark");
    retry
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &retry))
        .expect("retry receives a fresh exact attempt");
    complete(&engine, &mut retry);
    let transition = host.finish(retry, &mut engine);
    assert_eq!(
        transition.reduced_pointer_edges()[0].edge().sequence(),
        PointerEdgeSequence::new(1)
    );
}

#[test]
fn same_segment_drag_release_requires_delivery_and_hover_probe_set() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source, press) = point_inside_inactive_tab(projection);
    let moved = point_without_hover_receiver(projection, press);
    let mut frame = host.begin(&engine);
    let press_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(source))
            .expect("source press delivery is output-bound");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    let move_hover = PointerReceiverHoverHit::new(
        projection,
        moved,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("move hover is output-bound");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(moved),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                move_hover,
            )])
            .expect("move answers hover"),
        ),
    );

    frame
        .submit_pointer_journal(
            provider,
            pointer_edge_journal(
                2,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(moved),
                },
                Authority::Known(PointerCaptureOwner::None),
            ),
        )
        .expect("release follows the exact reduced drag prefix");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("release candidate exists")
        .candidates()[0]
        .clone();
    assert_eq!(candidate.probes(), PointerReceiverProbeRequest::HoverHit);
    assert_eq!(candidate.hover_point(), Some(moved));
    let release_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::NoReceiver)
            .expect("release delivery absence is output-bound");
    let incomplete =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
            release_delivery,
        )])
        .expect("delivery-only release is structurally valid but incomplete");
    assert_eq!(
        frame.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(incomplete))
            ])
            .expect("release candidate receipt set is exact"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "an incomplete release receipt poisons the private input prefix immediately",
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::PointerReceiverReceipt {
            source: PointerReceiverReceiptValidationError::MissingProbe {
                probe: PointerReceiverProbe::HoverHit,
                ..
            }
        })
    ));
}

#[test]
fn known_hover_must_echo_the_exact_edge_point() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source, press) = point_inside_inactive_tab(projection);
    let edge_point = point_without_hover_receiver(projection, press);
    begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source,
        press,
        edge_point,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    republish_surface_with_active_provider(&mut engine, &mut host, provider, 2);
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, moved_journal(2, Authority::Known(edge_point)))
        .expect("active drag move is provisional");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("active drag move freezes one candidate")
        .candidates()[0]
        .clone();
    let mismatched = LogicalPoint::new(edge_point.x() + 1.0, edge_point.y())
        .expect("mismatched hover point is finite");
    let presented =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
            known_hover(interaction(&engine), mismatched),
        )])
        .expect("known hover probe is structurally valid");
    assert_eq!(
        frame.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(presented))
            ])
            .expect("one candidate receipt"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::PointerReceiverReceipt {
            source: PointerReceiverReceiptValidationError::HoverHitPointMismatch {
                expected,
                submitted,
                ..
            }
        }) if expected == edge_point && submitted == mismatched
    ));
}

#[test]
fn unknown_local_point_requires_a_per_probe_unknown_hover_answer() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source, press) = point_inside_inactive_tab(projection);
    let target = point_without_hover_receiver(projection, press);
    begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source,
        press,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    republish_surface_with_active_provider(&mut engine, &mut host, provider, 2);

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(
            provider,
            moved_journal(
                2,
                Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
            ),
        )
        .expect("moved journal is provisional");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("move freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(candidate.probes(), PointerReceiverProbeRequest::HoverHit);
    assert_eq!(candidate.hover_point(), None);
    let known = PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
        known_hover(
            interaction(&engine),
            LogicalPoint::new(96.0, 24.0).expect("finite point"),
        ),
    )])
    .expect("known hover is structurally valid before candidate point join");
    assert_eq!(
        frame.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::Presented(known))
            ])
            .expect("one candidate receipt"),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::PointerReceiverReceipt {
            source: PointerReceiverReceiptValidationError::HoverHitPointUnavailable { .. }
        })
    ));

    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(
            provider,
            moved_journal(
                2,
                Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
            ),
        )
        .expect("rejected known hover left the watermark unchanged");
    let retry_candidate = retry
        .pointer_receiver_candidates()
        .expect("retry freezes one candidate")
        .candidates()[0]
        .clone();
    let unknown =
        PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
            PointerReceiverHoverHit::unknown(
                dockspace::pointer_receiver::PointerReceiverUnknownReason::NotReported,
            ),
        )])
        .expect("per-probe unknown answers the exact hover request");
    retry
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                retry_candidate.receipt(PointerReceiverObservation::Presented(unknown))
            ])
            .expect("one candidate receipt"),
        )
        .expect("per-probe unknown receipt stages");
    complete(&engine, &mut retry);
    assert_eq!(
        host.finish(retry, &mut engine)
            .reduced_pointer_edges()
            .len(),
        1
    );
}

#[test]
fn transition_preserves_stream_incarnation_for_cancel_then_same_pointer_reuse() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let point = LogicalPoint::new(96.0, 24.0).expect("test point is valid");
    let reason =
        dockspace::pointer_journal::PointerStreamCancelReason::ExplicitPlatformCancellation;

    let mut frame = host.begin(&engine);
    for previous in [0, 1] {
        submit_pointer_edge_with_observation(
            &mut frame,
            provider,
            pointer_edge_journal(
                previous,
                PointerEdgeKind::StreamCancelled(reason),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
                Authority::Known(PointerCaptureOwner::None),
            ),
            PointerReceiverObservation::NotApplicable,
        );
    }
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    let accepted = transition.reduced_pointer_edges();
    assert_eq!(accepted.len(), 2);
    assert_eq!(
        accepted[0].stream().pointer(),
        accepted[1].stream().pointer()
    );
    assert_ne!(accepted[0].stream(), accepted[1].stream());
    assert!(accepted[0].stream().incarnation() < accepted[1].stream().incarnation());
    assert!(matches!(
        accepted[0].edge().kind(),
        PointerEdgeKind::StreamCancelled(_)
    ));
    assert!(matches!(
        accepted[1].edge().kind(),
        PointerEdgeKind::StreamCancelled(_)
    ));
}

#[test]
fn stream_cancellation_terminates_an_active_drag_exactly_once() {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_inactive_tab(projection);
    let (target_region, target) = point_inside_center_target(projection, target_tabs);
    let cancel_reason =
        dockspace::pointer_journal::PointerStreamCancelReason::ExplicitPlatformCancellation;

    let mut frame = host.begin(&engine);
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source tab receives the press");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    )
    .expect("move hover is output-bound");
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::StreamCancelled(cancel_reason),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut frame,
        provider,
        pointer_edge_journal(
            3,
            PointerEdgeKind::StreamCancelled(cancel_reason),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::None),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    let reduced = transition.reduced_pointer_edges();
    assert_eq!(reduced.len(), 4);
    assert!(matches!(
        reduced[2].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::PointerStreamCancelled,
            ..
        }]
    ));
    assert!(reduced[3].interaction_outcomes().is_empty());
    assert_ne!(reduced[2].stream(), reduced[3].stream());
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let cancellations = transition
        .interaction_events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                dockspace::interaction::InteractionEventKind::Cancelled {
                    reason: InteractionCancelReason::PointerStreamCancelled,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert_eq!(cancellations[0].cause(), reduced[2].cause());
}

fn assert_authoritative_capture_loss_terminates_active_drag(capture_owner: PointerCaptureOwner) {
    assert!(matches!(
        capture_owner,
        PointerCaptureOwner::None | PointerCaptureOwner::Foreign
    ));

    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source_tab, press) = point_inside_inactive_tab(projection);
    let (target_region, target) = point_inside_center_target(projection, target_tabs);

    let mut gesture_frame = host.begin(&engine);
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_tab),
    )
    .expect("source tab receives the press");
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                press_delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    )
    .expect("move hover is output-bound");
    submit_pointer_edge_with_observation(
        &mut gesture_frame,
        provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        ),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                target_hover,
            )])
            .expect("move answers hover"),
        ),
    );
    complete(&engine, &mut gesture_frame);
    let gesture_transition = host.finish(gesture_frame, &mut engine);

    assert!(matches!(
        gesture_transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated { .. }
        ]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let active_stream = gesture_transition.reduced_pointer_edges()[1].stream();

    let cancel_reason =
        dockspace::pointer_journal::PointerStreamCancelReason::ExplicitPlatformCancellation;
    let mut terminal_frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut terminal_frame,
        provider,
        pointer_edge_journal(
            2,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(capture_owner),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut terminal_frame,
        provider,
        pointer_edge_journal(
            3,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(capture_owner),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut terminal_frame,
        provider,
        pointer_edge_journal(
            4,
            PointerEdgeKind::StreamCancelled(cancel_reason),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(capture_owner),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut terminal_frame,
        provider,
        pointer_edge_journal(
            5,
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(target),
            },
            Authority::Known(capture_owner),
        ),
        PointerReceiverObservation::NotApplicable,
    );
    complete(&engine, &mut terminal_frame);
    let terminal_transition = host.finish(terminal_frame, &mut engine);

    let reduced = terminal_transition.reduced_pointer_edges();
    assert_eq!(reduced.len(), 4);
    assert_eq!(reduced[0].stream(), active_stream);
    assert_eq!(reduced[1].stream(), active_stream);
    assert_eq!(reduced[2].stream(), active_stream);
    assert_ne!(reduced[3].stream(), active_stream);
    assert!(reduced[3].stream().incarnation() > active_stream.incarnation());
    assert!(matches!(
        reduced[0].interaction_outcomes(),
        [InteractionOutcome::Cancelled {
            status: InteractionStatus::Dragging { .. },
            reason: InteractionCancelReason::CaptureLost,
        }]
    ));
    assert!(reduced[1].interaction_outcomes().is_empty());
    assert!(reduced[2].interaction_outcomes().is_empty());
    assert!(reduced[3].interaction_outcomes().is_empty());
    assert_eq!(
        reduced
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(outcome, InteractionOutcome::Cancelled { .. }))
            .count(),
        1
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let cancellations = terminal_transition
        .interaction_events()
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                dockspace::interaction::InteractionEventKind::Cancelled {
                    reason: InteractionCancelReason::CaptureLost,
                    ..
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations.len(), 1);
    assert_eq!(cancellations[0].cause(), reduced[0].cause());
}

#[test]
fn known_none_capture_loss_terminates_an_active_drag_exactly_once() {
    assert_authoritative_capture_loss_terminates_active_drag(PointerCaptureOwner::None);
}

#[test]
fn known_foreign_capture_loss_terminates_an_active_drag_exactly_once() {
    assert_authoritative_capture_loss_terminates_active_drag(PointerCaptureOwner::Foreign);
}

#[test]
fn missing_receipts_do_not_consume_the_journal_watermark() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut failed = host.begin(&engine);
    failed
        .submit_pointer_journal(provider, press_journal(0))
        .expect("journal prepares before receipts");
    complete(&engine, &mut failed);
    assert_eq!(
        failed.finish(&mut engine),
        Err(EngineError::HostFramePointerReceiverReceiptsMissing { provider })
    );

    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(provider, press_journal(0))
        .expect("failed frame did not advance the watermark");
    retry
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &retry))
        .expect("retry supplies the required receipts");
    complete(&engine, &mut retry);
    host.finish(retry, &mut engine);
}

#[test]
fn failed_frame_receipts_cannot_replay_into_a_new_attempt() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut abandoned = host.begin(&engine);
    abandoned
        .submit_pointer_journal(provider, press_journal(0))
        .expect("journal prepares before receipts");
    let stale_receipts = primary_press_receipts(&engine, &abandoned);
    complete(&engine, &mut abandoned);
    assert!(matches!(
        abandoned.finish(&mut engine),
        Err(EngineError::HostFramePointerReceiverReceiptsMissing { .. })
    ));

    let mut replay = host.begin(&engine);
    replay
        .submit_pointer_journal(provider, press_journal(0))
        .expect("retry has the same uncommitted provider sequence");
    assert_eq!(
        replay.submit_pointer_receiver_receipts(stale_receipts),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "foreign frame attempts fail at the private receipt boundary",
    );
    assert!(matches!(
        replay.finish(&mut engine),
        Err(EngineError::PointerReceiverReceipt {
            source: PointerReceiverReceiptValidationError::ForeignFrameAttempt { .. }
        })
    ));

    let mut retry = host.begin(&engine);
    retry
        .submit_pointer_journal(provider, press_journal(0))
        .expect("rejected replay left the provider watermark unchanged");
    retry
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &retry))
        .expect("new attempt supplies new receipt identities");
    complete(&engine, &mut retry);
    host.finish(retry, &mut engine);
}

#[test]
fn semantically_identical_new_emission_does_not_invalidate_frame_begin_receipt() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());

    // Produce a later concrete emission for the same ready candidate. The
    // following host frame observes that newer emission before it reduces the
    // pointer journal. The receipt still describes the exact frame-begin output
    // and remains valid because scene, coordinates, and endpoint did not change.
    let emit = host.begin(&engine);
    let emit = support::complete_host_frame_with_current_outputs(&engine, emit);
    host.finish_presentation(emit, &mut engine);
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_journal(0))
        .expect("journal prepares against the prior interactive authority");
    frame
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
        .expect("prior authority can form a structurally valid receipt");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
}

#[test]
fn retiring_a_surface_local_presentation_host_retires_its_pointer_provider() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let host = TestPresentationHost::new(&mut engine);
    create_local_provider(&mut engine, &host);
    let retired = engine.pointer_provider().expect("provider is live");

    host.close(&mut engine)
        .expect("presentation host retirement succeeds");
    assert_eq!(engine.pointer_provider(), None);

    let successor = TestPresentationHost::new(&mut engine);
    create_local_provider(&mut engine, &successor);
    let replacement = engine
        .pointer_provider()
        .expect("successor provider is live");
    assert_ne!(replacement, retired);
}

#[test]
fn native_surface_local_provider_retires_when_its_binding_is_reincarnated() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    let binding_a = register_native_viewport(&mut engine, &mut host);
    publish_native_snapshot(&mut engine, &mut host, binding_a);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let provider_a = create_native_provider(&mut engine, &host, binding_a);

    let mut arm = host.begin(&engine);
    arm.submit_pointer_journal(provider_a, press_journal(0))
        .expect("A1 press journal is valid");
    arm.submit_pointer_receiver_receipts(primary_press_receipts(&engine, &arm))
        .expect("A1 press receipt is exact");
    complete(&engine, &mut arm);
    host.finish(arm, &mut engine);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let mut rebind = host.begin(&engine);
    rebind
        .submit_pointer_journal(provider_a, empty_journal(1))
        .expect("the rebind frame preserves the A1 provider watermark");
    rebind
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("an empty journal has no receiver receipts"),
        )
        .expect("the empty A1 segment reduces before the rebind");
    support::TestInputStream::resume(&engine, WORKSPACE_REBIND_SOURCE)
        .append(&mut rebind, EngineInput::ReplaceWorkspace(workspace()))
        .expect("workspace reincarnation follows the last accepted A1 segment");
    complete(&engine, &mut rebind);
    let transition = host.finish(rebind, &mut engine);

    let binding_b = engine
        .viewport()
        .viewport(SURFACE)
        .expect("the native surface remains registered after workspace replacement")
        .binding();
    assert_ne!(binding_b, binding_a);
    assert_eq!(binding_b.surface(), binding_a.surface());
    assert_eq!(binding_b.token(), binding_a.token());
    assert!(binding_b.incarnation() > binding_a.incarnation());
    assert_eq!(engine.pointer_provider(), None);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(transition.reduced_pointer_edges().is_empty());
    assert!(matches!(
        engine.retire_pointer_provider(provider_a),
        Err(EngineError::PointerJournal {
            source: PointerJournalLedgerError::RetiredLease {
                lease,
                committed_through,
            },
        }) if lease == provider_a && committed_through == PointerEdgeSequence::new(1)
    ));

    publish_native_snapshot(&mut engine, &mut host, binding_b);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let provider_b = create_native_provider(&mut engine, &host, binding_b);
    assert_ne!(provider_b, provider_a);

    let mut successor = host.begin(&engine);
    successor
        .submit_pointer_journal(provider_b, press_journal(0))
        .expect("A2 press journal is valid");
    successor
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &successor))
        .expect("A2 press receipt is exact");
    complete(&engine, &mut successor);
    host.finish(successor, &mut engine);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
}

#[test]
fn retiring_a_pointer_provider_atomically_cancels_its_gesture_and_unblocks_the_successor() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let retired = engine.pointer_provider().expect("provider is live");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(retired, press_journal(0))
        .expect("press journal is valid");
    frame
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
        .expect("press receipt is exact");
    complete(&engine, &mut frame);
    host.finish(frame, &mut engine);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    let retirement = engine
        .retire_pointer_provider(retired)
        .expect("exact live provider retirement succeeds");
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(matches!(
        retirement.interaction_events(),
        [event]
            if matches!(
                event.cause(),
                ReductionCause::PointerProviderRetirement { provider, .. }
                    if provider == retired
            ) && matches!(
                event.kind(),
                dockspace::interaction::InteractionEventKind::Cancelled {
                    status: InteractionStatus::Armed { .. },
                    reason: InteractionCancelReason::PointerProviderRetired,
                }
            )
    ));

    let tick_after_retirement = engine.last_reducer_tick();
    assert!(matches!(
        engine.retire_pointer_provider(retired),
        Err(EngineError::PointerJournal {
            source: PointerJournalLedgerError::RetiredLease { lease, .. },
        }) if lease == retired
    ));
    assert_eq!(engine.last_reducer_tick(), tick_after_retirement);

    create_local_provider(&mut engine, &host);
    let successor = engine
        .pointer_provider()
        .expect("successor provider is live");
    assert_ne!(successor, retired);
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(successor, press_journal(0))
        .expect("successor press journal is valid");
    frame
        .submit_pointer_receiver_receipts(primary_press_receipts(&engine, &frame))
        .expect("successor press receipt is exact");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
}

#[test]
fn later_segment_candidates_follow_the_exact_reduced_receiver_prefix() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (_, press) = point_inside_inactive_tab(projection);

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, press_at_journal(0, press))
        .expect("press journal stages");
    let press_candidate = frame
        .pointer_receiver_candidates()
        .expect("press freezes one candidate")
        .candidates()[0]
        .clone();
    let no_receiver =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::NoReceiver)
            .expect("known receiver absence is bound to the presented output");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                exact_candidate_observation(&press_candidate, Some(no_receiver), None),
            )])
            .expect("press receipt set is exact"),
        )
        .expect("press receipt reduces the private input prefix");

    frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(press)))
        .expect("move follows the reduced press prefix");
    let move_candidate = &frame
        .pointer_receiver_candidates()
        .expect("move freezes one candidate")
        .candidates()[0];
    assert_eq!(
        move_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable,
        "a known receiver miss leaves the reducer idle, so a later move must not inherit a speculative armed branch",
    );
}

#[test]
fn idle_move_and_release_request_no_receiver_probes() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let point = LogicalPoint::new(96.0, 24.0).expect("idle point is finite");

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, moved_journal(0, Authority::Known(point)))
        .expect("idle move stages");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("idle move freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        move_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                move_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("idle move has an exact not-applicable receipt"),
        )
        .expect("idle move reduces");
    frame
        .submit_pointer_journal(provider, release_journal(1, Authority::Known(point)))
        .expect("idle release follows the move");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("idle release freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        release_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                release_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("idle release has an exact not-applicable receipt"),
        )
        .expect("idle release reduces");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(transition.reduced_pointer_edges().len(), 2);
    assert!(
        transition
            .reduced_pointer_edges()
            .iter()
            .all(|edge| edge.interaction_outcomes().is_empty())
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn dragging_move_and_release_request_only_hover() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source, press) = point_inside_inactive_tab(projection);
    let target = point_without_hover_receiver(projection, press);

    begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source,
        press,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    republish_surface_with_active_provider(&mut engine, &mut host, provider, 2);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, moved_journal(2, Authority::Known(target)))
        .expect("active drag move stages");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("active drag move freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        move_candidate.probes(),
        PointerReceiverProbeRequest::HoverHit
    );
    assert_eq!(move_candidate.hover_point(), Some(target));

    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("active drag frame retains interaction authority");
    let move_hover = exact_hover_receipt(&frame, target);
    let release_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    )
    .expect("release hover is output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([move_candidate.receipt(
                exact_candidate_observation(&move_candidate, None, Some(move_hover)),
            )])
            .expect("active drag move receipt is exact"),
        )
        .expect("active drag move reduces");
    frame
        .submit_pointer_journal(provider, release_journal(3, Authority::Known(target)))
        .expect("active drag release follows the move");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("active drag release freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        release_candidate.probes(),
        PointerReceiverProbeRequest::HoverHit
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                exact_candidate_observation(&release_candidate, None, Some(release_hover)),
            )])
            .expect("active drag release receipt is exact"),
        )
        .expect("active drag release reduces");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(transition.reduced_pointer_edges().len(), 2);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

type PendingDragReleaseFixture = (
    DockEngine,
    TestPresentationHost,
    dockspace::pointer_journal::PointerInputLease,
    Workspace,
    NodeId,
    (
        dockspace::interaction::DragSessionId,
        dockspace::interaction::PreviewToken,
    ),
);

fn pending_drag_release_fixture() -> PendingDragReleaseFixture {
    pending_drag_release_fixture_with_evidence(None)
        .expect("the fixture paints its exact frozen preview")
}

fn pending_drag_release_fixture_with_evidence(
    submitted_interaction: Option<HostInteractionPresentation>,
) -> Result<PendingDragReleaseFixture, CoreHostFrameError> {
    let (workspace, target_tabs) = split_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (source, press) = point_inside_tab(projection, ItemId::new(1));
    let first_target = point_without_hover_receiver(projection, press);

    begin_journal_drag(
        &mut engine,
        &mut host,
        provider,
        source,
        press,
        first_target,
        PointerReceiverHoverHitDisposition::NoReceiver,
    );
    paint_active_journal_drag(&mut engine, &mut host, provider);
    let before = engine.workspace().clone();

    let mut frame = host.begin(&engine);
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("release frame retains the presented hit graph");
    let (target_region, target) = point_inside_center_target(projection, target_tabs);
    let target_hover = PointerReceiverHoverHit::new(
        projection,
        target,
        PointerReceiverHoverHitDisposition::Dock(target_region),
    )
    .expect("new target hover is bound to the presented output");

    frame
        .submit_pointer_journal(provider, moved_journal(2, Authority::Known(target)))
        .expect("move to the new target stages");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("move freezes a hover challenge")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([move_candidate.receipt(
                exact_candidate_observation(&move_candidate, None, Some(target_hover)),
            )])
            .expect("move receipt is exact"),
        )
        .expect("move publishes the new preview");

    frame
        .submit_pointer_journal(provider, release_journal(3, Authority::Known(target)))
        .expect("release follows the move");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("release freezes a hover challenge")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([release_candidate.receipt(
                exact_candidate_observation(&release_candidate, None, Some(target_hover)),
            )])
            .expect("release receipt is exact"),
        )
        .expect("release reduces into a presentation obligation");
    let contribution = frame
        .view()
        .begin_surface_contribution(SURFACE)
        .expect("release surface remains in the roster");
    let mut frame = frame
        .into_presentation()
        .expect("release frame enters its presentation phase");
    let obligation = frame
        .take_presentation_obligations()
        .expect("release frame issues its exact physical roster")
        .into_iter()
        .find(|obligation| obligation.slot().surface() == SURFACE)
        .expect("release surface has one presentation obligation");
    let interaction = frame
        .view()
        .presentation_interaction(SURFACE)
        .expect("release paint retains interaction evidence");
    frame.record_painted_surface_contribution(
        obligation,
        contribution,
        submitted_interaction.unwrap_or(interaction),
    )?;
    let transition = host.finish_presentation(frame, &mut engine);

    let release_outcomes = transition.reduced_pointer_edges()[1].interaction_outcomes();
    assert!(matches!(
        release_outcomes,
        [
            InteractionOutcome::PreviewUpdated {
                preview: Some(_),
                status: PreviewResolutionStatus::Resolved,
                ..
            },
            InteractionOutcome::ReleasePending { session, preview }
        ] if engine.pending_release_preview() == Some((*session, *preview))
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let pending = engine
        .pending_release_preview()
        .expect("release remains pending until the emission is observed");
    let emitted_preview = transition
        .presentation_emissions()
        .iter()
        .find(|emission| emission.output().surface() == SURFACE)
        .and_then(|emission| emission.output().payload().interaction())
        .and_then(|interaction| interaction.drag_preview());
    assert_eq!(emitted_preview, Some(pending.1));

    Ok((engine, host, provider, before, target_tabs, pending))
}

#[test]
fn pending_release_rejects_a_painted_claim_without_its_preview_token() {
    let error = match pending_drag_release_fixture_with_evidence(Some(
        HostInteractionPresentation::default(),
    )) {
        Ok(_) => panic!("missing preview evidence must not publish the release output"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CoreHostFrameError::PresentationInteractionMismatch {
            surface: SURFACE,
            expected,
            submitted,
        } if expected.drag_preview().is_some()
            && submitted == HostInteractionPresentation::default()
    ));
}

#[test]
fn move_then_release_with_new_preview_waits_for_exact_presentation() {
    let (mut engine, mut host, provider, before, _, pending) = pending_drag_release_fixture();

    let mut unknown_prelude = engine
        .begin_host_frame(host.lease())
        .expect("unknown release-observation frame begins");
    host.submit_unknown_observation(&mut unknown_prelude);
    let mut unknown_frame = unknown_prelude
        .seal(&engine)
        .expect("unknown release-observation frame seals");
    unknown_frame
        .submit_pointer_journal(provider, empty_journal(4))
        .expect("unknown observation preserves the release watermark");
    unknown_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("unknown observation has an exact empty receipt set"),
        )
        .expect("unknown observation receipt set stages");
    complete(&engine, &mut unknown_frame);
    host.finish(unknown_frame, &mut engine);
    assert_eq!(engine.pending_release_preview(), Some(pending));
    assert_eq!(engine.workspace(), &before);

    let mut settle_frame = host.begin(&engine);
    settle_frame
        .submit_pointer_journal(provider, empty_journal(4))
        .expect("settlement frame preserves the consumed release watermark");
    settle_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty settlement journal has an exact empty receipt set"),
        )
        .expect("empty settlement receipt set stages");
    complete(&engine, &mut settle_frame);
    host.finish(settle_frame, &mut engine);

    assert_eq!(engine.pending_release_preview(), None);
    assert_ne!(engine.workspace(), &before);
}

#[test]
fn terminal_not_presented_output_cancels_the_release_obligation() {
    let (mut engine, mut host, provider, before, _, pending) = pending_drag_release_fixture();

    let mut prelude = engine
        .begin_host_frame(host.lease())
        .expect("not-presented release-observation frame begins");
    host.submit_not_presented_observation(&mut prelude);
    let mut frame = prelude
        .seal(&engine)
        .expect("not-presented release-observation frame seals");
    frame
        .submit_pointer_journal(provider, empty_journal(4))
        .expect("terminal observation preserves the release watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("terminal observation has an exact empty receipt set"),
        )
        .expect("terminal observation receipt set stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(engine.pending_release_preview(), None);
    assert_eq!(engine.workspace(), &before);
    assert_ne!(engine.pending_release_preview(), Some(pending));
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        InteractionEventKind::Cancelled {
            status: InteractionStatus::Idle,
            reason: InteractionCancelReason::SceneUnavailable,
        }
    )));
}

#[test]
fn presentation_host_retirement_terminally_cancels_pending_drag_release_once() {
    let (mut engine, host, _provider, before, _, pending) = pending_drag_release_fixture();
    let lease = host.lease();

    let retirement = host
        .close(&mut engine)
        .expect("presentation host retirement succeeds");
    let transition = retirement
        .transition()
        .expect("the first retirement publishes one atomic transition");

    assert_eq!(engine.pending_release_preview(), None);
    assert_eq!(engine.workspace(), &before);
    assert_ne!(engine.pending_release_preview(), Some(pending));
    assert_eq!(
        transition
            .interaction_events()
            .iter()
            .filter(|event| matches!(
                event.kind(),
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Idle,
                    reason: InteractionCancelReason::SceneUnavailable,
                }
            ))
            .count(),
        1,
        "host retirement must terminally fail the exact outstanding presentation obligation once",
    );
    let diagnostics = engine.presentation_ledger_diagnostics();
    assert_eq!(diagnostics.pending_outputs(), 0);
    assert_eq!(diagnostics.retired_hosts(), 0);
    assert_eq!(diagnostics.compacted_retired_hosts(), 1);

    let tick = engine.last_reducer_tick();
    assert!(matches!(
        engine
            .retire_presentation_host(
                lease,
                PresentationHostRetirementReason::ExplicitShutdown,
            )
            .expect("duplicate retirement remains idempotent"),
        PresentationHostRetirementOutcome::Compacted { host } if host == lease
    ));
    assert_eq!(engine.last_reducer_tick(), tick);
}

#[test]
fn policy_change_cancels_an_unpresented_release_obligation() {
    let (mut engine, mut host, provider, before, _, pending) = pending_drag_release_fixture();
    let mut prelude = engine
        .begin_host_frame(host.lease())
        .expect("policy frame begins");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("policy frame explicitly carries no presentation update");
    let mut frame = prelude.seal(&engine).expect("policy frame seals");
    let mut policy = engine.policy().clone();
    policy.set_close_capability(CloseCapability::Disabled);
    frame
        .submit_pointer_journal(provider, empty_journal(4))
        .expect("policy frame preserves the release watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("policy frame has an exact empty receipt set"),
        )
        .expect("policy frame receipt set stages");
    append_host_input(
        &mut frame,
        StableInputSourceId::new(0xC01),
        SourceSequence::new(1),
        EngineInput::ReplacePolicy {
            expected: engine.version(),
            policy,
        },
    )
    .expect("policy replacement stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(engine.pending_release_preview(), None);
    assert_eq!(engine.workspace(), &before);
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        InteractionEventKind::Cancelled {
            status: InteractionStatus::Idle,
            reason: InteractionCancelReason::PolicyChanged,
        }
    )));
    assert_ne!(engine.pending_release_preview(), Some(pending));
}

#[test]
fn new_authoritative_press_replaces_an_unpresented_release_obligation() {
    let (mut engine, mut host, provider, _, target_tabs, _) = pending_drag_release_fixture();
    let mut prelude = engine
        .begin_host_frame(host.lease())
        .expect("replacement press frame begins");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("replacement press frame explicitly carries no presentation update");
    let mut frame = prelude
        .seal(&engine)
        .expect("replacement press frame seals");
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("the prior presented hit graph remains authoritative");
    let (source, point) = point_inside_tab(projection, ItemId::new(2));
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(source))
            .expect("replacement tab receives the press");
    frame
        .submit_pointer_journal(provider, press_at_journal(4, point))
        .expect("replacement press journal stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("replacement press freezes a receiver challenge")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("replacement press answers delivery"),
                ),
            )])
            .expect("replacement press receipt set is exact"),
        )
        .expect("replacement press receipt stages");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(engine.pending_release_preview(), None);
    assert!(matches!(
        engine.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. }) if items == &[ItemId::new(3)]
    ));
    assert!(matches!(
        transition.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::Cancelled {
                status: InteractionStatus::Idle,
                reason: InteractionCancelReason::ReplacedByNewGesture,
            },
            InteractionOutcome::DragArmed { .. }
        ]
    ));
}

fn pending_contained_transform_release_fixture() -> (
    DockEngine,
    TestPresentationHost,
    dockspace::pointer_journal::PointerInputLease,
    LogicalRect,
) {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let projection = interaction(&engine);
    let (resize, press) = point_inside_contained_resize(
        projection,
        CONTAINED_FLOATING,
        ContainedResizeDirection::East,
    );
    let moved = LogicalPoint::new(press.x() + 32.0, press.y()).expect("move point is finite");
    let before = engine
        .workspace()
        .contained_floating(CONTAINED_FLOATING)
        .expect("contained root exists")
        .rect;

    let mut press_frame = host.begin(&engine);
    let projection = press_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("press frame has current interaction authority");
    let delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(resize))
            .expect("contained resize handle receives the press");
    submit_pointer_edge_with_observation(
        &mut press_frame,
        provider,
        press_at_journal(0, press),
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("press answers delivery"),
        ),
    );
    complete(&engine, &mut press_frame);
    host.finish(press_frame, &mut engine);

    let mut release_frame = host.begin(&engine);
    submit_pointer_edge_with_observation(
        &mut release_frame,
        provider,
        moved_journal(1, Authority::Known(moved)),
        PointerReceiverObservation::NotApplicable,
    );
    submit_pointer_edge_with_observation(
        &mut release_frame,
        provider,
        release_journal(2, Authority::Known(moved)),
        PointerReceiverObservation::NotApplicable,
    );
    let contribution = release_frame
        .view()
        .begin_surface_contribution(SURFACE)
        .expect("contained release surface remains in the roster");
    let mut release_frame = release_frame
        .into_presentation()
        .expect("contained release frame enters its presentation phase");
    let obligation = release_frame
        .take_presentation_obligations()
        .expect("contained release frame issues its exact physical roster")
        .into_iter()
        .find(|obligation| obligation.slot().surface() == SURFACE)
        .expect("contained release surface has one presentation obligation");
    let interaction = release_frame
        .view()
        .presentation_interaction(SURFACE)
        .expect("contained release paint retains interaction evidence");
    release_frame
        .record_painted_surface_contribution(obligation, contribution, interaction)
        .expect("the host paints the newly retained contained preview");
    let transition = host.finish_presentation(release_frame, &mut engine);

    assert!(matches!(
        transition.reduced_pointer_edges()[1].interaction_outcomes(),
        [
            InteractionOutcome::ContainedTransformPreviewUpdated { .. },
            InteractionOutcome::ContainedTransformReleasePending { session, preview }
        ] if engine.pending_contained_transform_release_preview() == Some((*session, *preview))
    ));
    assert_eq!(
        engine
            .workspace()
            .contained_floating(CONTAINED_FLOATING)
            .expect("contained root remains durable")
            .rect,
        before
    );

    let pending = engine
        .pending_contained_transform_release_preview()
        .expect("contained release waits for presentation");
    let emitted_preview = transition
        .presentation_emissions()
        .iter()
        .find(|emission| emission.output().surface() == SURFACE)
        .and_then(|emission| emission.output().payload().interaction())
        .and_then(|interaction| interaction.contained_transform_preview());
    assert_eq!(emitted_preview, Some(pending.1));

    (engine, host, provider, before)
}

#[test]
fn contained_move_then_release_with_new_preview_waits_for_exact_presentation() {
    let (mut engine, mut host, provider, before) = pending_contained_transform_release_fixture();

    let mut settle_frame = host.begin(&engine);
    settle_frame
        .submit_pointer_journal(provider, empty_journal(3))
        .expect("settlement frame preserves the release watermark");
    settle_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty settlement journal has exact receipts"),
        )
        .expect("empty settlement receipts stage");
    complete(&engine, &mut settle_frame);
    host.finish(settle_frame, &mut engine);

    assert_eq!(engine.pending_contained_transform_release_preview(), None);
    let after = engine
        .workspace()
        .contained_floating(CONTAINED_FLOATING)
        .expect("contained root remains durable")
        .rect;
    assert!(after.width() > before.width());
}

#[test]
fn presentation_host_retirement_terminally_cancels_pending_contained_release_once() {
    let (mut engine, host, _provider, before) = pending_contained_transform_release_fixture();
    let lease = host.lease();

    let retirement = host
        .close(&mut engine)
        .expect("presentation host retirement succeeds");
    let transition = retirement
        .transition()
        .expect("the first retirement publishes one atomic transition");

    assert_eq!(engine.pending_contained_transform_release_preview(), None);
    assert_eq!(
        engine
            .workspace()
            .contained_floating(CONTAINED_FLOATING)
            .expect("retirement preserves the contained root")
            .rect,
        before,
    );
    assert_eq!(
        transition
            .interaction_events()
            .iter()
            .filter(|event| matches!(
                event.kind(),
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Idle,
                    reason: InteractionCancelReason::SceneUnavailable,
                }
            ))
            .count(),
        1,
        "host retirement must terminally fail the exact contained presentation obligation once",
    );
    let diagnostics = engine.presentation_ledger_diagnostics();
    assert_eq!(diagnostics.pending_outputs(), 0);
    assert_eq!(diagnostics.retired_hosts(), 0);
    assert_eq!(diagnostics.compacted_retired_hosts(), 1);

    let tick = engine.last_reducer_tick();
    assert!(matches!(
        engine
            .retire_presentation_host(
                lease,
                PresentationHostRetirementReason::ExplicitShutdown,
            )
            .expect("duplicate retirement remains idempotent"),
        PresentationHostRetirementOutcome::Compacted { host } if host == lease
    ));
    assert_eq!(engine.last_reducer_tick(), tick);
}

#[test]
fn resizing_move_and_release_request_no_fresh_receiver_probes() {
    let (workspace, split) = splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (splitter, press) = point_inside_splitter(interaction(&engine), split);

    let mut press_frame = host.begin(&engine);
    press_frame
        .submit_pointer_journal(provider, press_at_journal(0, press))
        .expect("splitter press journal stages");
    let press_candidate = press_frame
        .pointer_receiver_candidates()
        .expect("splitter press freezes one candidate")
        .candidates()[0]
        .clone();
    let projection = press_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("splitter press retains interaction authority");
    let press_delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(splitter),
    )
    .expect("splitter press delivery is output-bound");
    press_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(press_delivery),
                    ])
                    .expect("splitter press answers delivery"),
                ),
            )])
            .expect("splitter press receipt set is exact"),
        )
        .expect("splitter press receipt stages");
    complete(&engine, &mut press_frame);
    host.finish(press_frame, &mut engine);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    let moved = LogicalPoint::new(press.x() + 24.0, press.y()).expect("move point is finite");
    let released = LogicalPoint::new(press.x() + 48.0, press.y()).expect("release point is finite");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(moved)))
        .expect("active resize move stages");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("active resize move freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        move_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                move_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("active resize move has an exact not-applicable receipt"),
        )
        .expect("active resize move reduces");
    frame
        .submit_pointer_journal(provider, release_journal(2, Authority::Known(released)))
        .expect("active resize release follows the move");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("active resize release freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        release_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                release_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("active resize release has an exact not-applicable receipt"),
        )
        .expect("active resize release reduces");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(transition.reduced_pointer_edges().len(), 2);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn contained_transform_move_and_release_request_no_fresh_receiver_probes() {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (resize, press) = point_inside_contained_resize(
        interaction(&engine),
        CONTAINED_FLOATING,
        ContainedResizeDirection::East,
    );

    let mut press_frame = host.begin(&engine);
    press_frame
        .submit_pointer_journal(provider, press_at_journal(0, press))
        .expect("contained resize press journal stages");
    let press_candidate = press_frame
        .pointer_receiver_candidates()
        .expect("contained resize press freezes one candidate")
        .candidates()[0]
        .clone();
    let projection = press_frame
        .view()
        .interaction_projection(SURFACE)
        .expect("contained resize press retains interaction authority");
    let press_delivery =
        PointerReceiverDelivery::new(projection, PointerReceiverDeliveryDisposition::Dock(resize))
            .expect("contained resize press delivery is output-bound");
    press_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([press_candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(press_delivery),
                    ])
                    .expect("contained resize press answers delivery"),
                ),
            )])
            .expect("contained resize press receipt set is exact"),
        )
        .expect("contained resize press receipt stages");
    complete(&engine, &mut press_frame);
    host.finish(press_frame, &mut engine);
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::ContainedTransforming { .. }
    ));

    let moved = LogicalPoint::new(press.x() + 24.0, press.y()).expect("move point is finite");
    let released = LogicalPoint::new(press.x() + 48.0, press.y()).expect("release point is finite");
    let mut frame = host.begin(&engine);
    frame
        .submit_pointer_journal(provider, moved_journal(1, Authority::Known(moved)))
        .expect("active contained transform move stages");
    let move_candidate = frame
        .pointer_receiver_candidates()
        .expect("active contained transform move freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        move_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                move_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("active contained transform move has an exact receipt"),
        )
        .expect("active contained transform move reduces");
    frame
        .submit_pointer_journal(provider, release_journal(2, Authority::Known(released)))
        .expect("active contained transform release follows the move");
    let release_candidate = frame
        .pointer_receiver_candidates()
        .expect("active contained transform release freezes one candidate")
        .candidates()[0]
        .clone();
    assert_eq!(
        release_candidate.probes(),
        PointerReceiverProbeRequest::NotApplicable
    );
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                release_candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("active contained transform release has an exact receipt"),
        )
        .expect("active contained transform release reduces");
    complete(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(transition.reduced_pointer_edges().len(), 2);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn unknown_capture_terminal_release_cancels_pressed_once_and_allows_next_press() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (close, point) = point_inside_tab_close(interaction(&engine), ItemId::new(1));

    let pressed = submit_dock_press_after(&mut engine, &mut host, provider, close, point, 0);
    assert!(
        pressed.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    let first_status = engine.interaction().status();
    assert!(matches!(first_status, InteractionStatus::Pressed { .. }));
    let before = engine.workspace().clone();

    let released = submit_unknown_capture_release_after(&mut engine, &mut host, provider, point, 1);
    assert_unknown_capture_release_cancelled_once(&released, first_status);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let next = submit_dock_press_after(&mut engine, &mut host, provider, close, point, 2);
    assert!(
        next.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Pressed { .. }
    ));
    assert_ne!(engine.interaction().status(), first_status);
    assert_eq!(engine.workspace(), &before);
}

#[test]
fn unknown_capture_terminal_release_cancels_armed_once_and_allows_next_press() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (tab, point) = point_inside_tab(interaction(&engine), ItemId::new(1));

    let armed = submit_dock_press_after(&mut engine, &mut host, provider, tab, point, 0);
    assert!(matches!(
        armed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    let first_status = engine.interaction().status();
    assert!(matches!(first_status, InteractionStatus::Armed { .. }));
    let before = engine.workspace().clone();

    let released = submit_unknown_capture_release_after(&mut engine, &mut host, provider, point, 1);
    assert_unknown_capture_release_cancelled_once(&released, first_status);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let next = submit_dock_press_after(&mut engine, &mut host, provider, tab, point, 2);
    assert!(matches!(
        next.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    assert_ne!(engine.interaction().status(), first_status);
    assert_eq!(engine.workspace(), &before);
}

#[test]
fn unknown_capture_terminal_release_cancels_resizing_once_and_allows_next_press() {
    let (workspace, split) = splitter_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (splitter, point) = point_inside_splitter(interaction(&engine), split);

    let resizing = submit_dock_press_after(&mut engine, &mut host, provider, splitter, point, 0);
    assert!(matches!(
        resizing.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeBegan { .. }]
    ));
    let first_status = engine.interaction().status();
    assert!(matches!(first_status, InteractionStatus::Resizing { .. }));
    let before = engine.workspace().clone();

    let released = submit_unknown_capture_release_after(&mut engine, &mut host, provider, point, 1);
    assert_unknown_capture_release_cancelled_once(&released, first_status);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let next = submit_dock_press_after(&mut engine, &mut host, provider, splitter, point, 2);
    assert!(matches!(
        next.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ResizeBegan { .. }]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Resizing { .. }
    ));
    assert_ne!(engine.interaction().status(), first_status);
    assert_eq!(engine.workspace(), &before);
}

#[test]
fn unknown_capture_terminal_release_cancels_contained_transform_once_and_allows_next_press() {
    let mut engine =
        DockEngine::new(contained_workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    create_local_provider(&mut engine, &host);
    let provider = engine.pointer_provider().expect("provider is live");
    let (resize, point) = point_inside_contained_resize(
        interaction(&engine),
        CONTAINED_FLOATING,
        ContainedResizeDirection::East,
    );

    let transforming = submit_dock_press_after(&mut engine, &mut host, provider, resize, point, 0);
    assert!(matches!(
        transforming.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ContainedTransformBegan { .. }]
    ));
    let first_status = engine.interaction().status();
    assert!(matches!(
        first_status,
        InteractionStatus::ContainedTransforming { .. }
    ));
    let before = engine.workspace().clone();

    let released = submit_unknown_capture_release_after(&mut engine, &mut host, provider, point, 1);
    assert_unknown_capture_release_cancelled_once(&released, first_status);
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);

    let next = submit_dock_press_after(&mut engine, &mut host, provider, resize, point, 2);
    assert!(matches!(
        next.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::ContainedTransformBegan { .. }]
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::ContainedTransforming { .. }
    ));
    assert_ne!(engine.interaction().status(), first_status);
    assert_eq!(engine.workspace(), &before);
}
