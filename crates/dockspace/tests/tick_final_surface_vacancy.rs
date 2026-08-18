use super::support;

use dockspace::close::CloseDecision;
use dockspace::command::{
    ContainedPosition, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use dockspace::effect::{EffectPhase, PlatformEffect};
use dockspace::engine::{CoreHostFrame, DockEngine, EngineInput};
use dockspace::frame::{BindingRetirementStatus, NativeCreatePhase, NativeCreateRequest};
use dockspace::geometry::{LogicalPoint, LogicalRect, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, RootId, StableInputSourceId, SurfaceId, WorkspaceRevision,
};
use dockspace::intent::{
    Authority, AuthorityUnavailableReason, CloseSceneTarget, PointerButton, PointerId,
};
use dockspace::interaction::{InteractionDelivery, InteractionOutcome, PreviewResolutionStatus};
use dockspace::platform::{
    CloseEffectAcknowledgement, InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputObservation,
    WindowInputState, WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::{
    DesktopDockRoute, DesktopRouteFact, DesktopWorkAreaRoute, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerEventDeliveryOwner, PointerProviderScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PointerReceiverUnknownReason,
    PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::transaction::WorkspaceTransaction;
use dockspace::transition::{InputOutcome, WorkspaceVersion};
use dockspace::viewport::{
    CloseObservationGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    PresentationObservationGeneration, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use dockspace::viewport_registry::ViewportAdmission;
use support::{TestPresentationHost, submit_input};

const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const SOURCE_ROOT: RootId = RootId::new(11);
const TARGET_ROOT: RootId = RootId::new(12);
const SIBLING_ROOT: RootId = RootId::new(13);
const SOURCE_ITEM: ItemId = ItemId::new(21);
const TARGET_ITEM: ItemId = ItemId::new(22);
const SIBLING_ITEM: ItemId = ItemId::new(23);
const SOURCE_WINDOW: WindowToken = WindowToken::new(31);
const TARGET_WINDOW: WindowToken = WindowToken::new(32);
const REBOUND_SOURCE_WINDOW: WindowToken = WindowToken::new(33);
const MOVED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(45);
const SIBLING_FLOATING: FloatingPresentationId = FloatingPresentationId::new(42);
const NATIVE_SURFACE: SurfaceId = SurfaceId::new(3);
const NATIVE_ROOT: RootId = RootId::new(13);
const SOURCE_PEER_ITEM: ItemId = ItemId::new(24);
const NATIVE_DESTINATION_FLOATING: FloatingPresentationId = FloatingPresentationId::new(44);
const NATIVE_POINTER: PointerId = PointerId::new(51);
const NATIVE_WORK_AREA: WorkAreaToken = WorkAreaToken::new(61);

const CLOSE_SOURCE: StableInputSourceId = StableInputSourceId::new(100);
const COMMAND_SOURCE: StableInputSourceId = StableInputSourceId::new(200);
const LIFECYCLE_SOURCE: StableInputSourceId = StableInputSourceId::new(50);
const REGISTRATION_SOURCE: StableInputSourceId = StableInputSourceId::new(300);
const RENDERER_SOURCE: StableInputSourceId = StableInputSourceId::new(400);
const NATIVE_LIFECYCLE_SOURCE: StableInputSourceId = StableInputSourceId::new(500);
const NATIVE_PLATFORM_SOURCE: StableInputSourceId = StableInputSourceId::new(600);
const NATIVE_RENDERER_SOURCE: StableInputSourceId = StableInputSourceId::new(700);
const NATIVE_COMMAND_SOURCE: StableInputSourceId = StableInputSourceId::new(800);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("fixture rectangle is finite")
}

fn physical_rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("fixture rectangle is finite")
}

fn two_surface_workspace(with_contained_sibling: bool) -> Workspace {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    if with_contained_sibling {
        let sibling_tabs = builder.insert_node(Node::tabs([SIBLING_ITEM]));
        builder.set_root(SIBLING_ROOT, RootRecord::new(sibling_tabs));
        builder.set_contained_floating(
            SIBLING_FLOATING,
            ContainedFloating::new(SIBLING_ROOT, rect(20.0, 20.0, 240.0, 180.0)),
        );
        builder
            .attach_contained(SOURCE_SURFACE, SIBLING_FLOATING)
            .expect("source surface exists");
    }
    builder.build().expect("fixture workspace is valid")
}

fn target_only_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    builder.build().expect("fixture workspace is valid")
}

fn native_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM, SOURCE_PEER_ITEM]));
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    builder.build().expect("fixture workspace is valid")
}

struct NativeVacancyFixture {
    engine: DockEngine,
    presentation_host: TestPresentationHost,
    observation_generation: u64,
    pointer_sequence: u64,
}

impl NativeVacancyFixture {
    fn new() -> Self {
        let workspace = native_workspace();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut engine = DockEngine::new(workspace, policy).expect("fixture engine is valid");
        let presentation_host = TestPresentationHost::new(&mut engine);
        Self {
            engine,
            presentation_host,
            observation_generation: 0,
            pointer_sequence: 0,
        }
    }

    fn snapshot(
        &mut self,
        native: Option<(NativeCreateRequest, WindowPresentationState)>,
    ) -> PlatformSnapshot {
        self.snapshot_with_source_input(
            native,
            WindowInputState::ReceivesInput,
            InputEffectAcknowledgement::known(None),
        )
    }

    fn snapshot_with_source_input(
        &mut self,
        native: Option<(NativeCreateRequest, WindowPresentationState)>,
        source_state: WindowInputState,
        acknowledged_effect: InputEffectAcknowledgement,
    ) -> PlatformSnapshot {
        self.observation_generation += 1;
        let generation = self.observation_generation;
        let source_binding = self
            .engine
            .viewport()
            .viewport(SOURCE_SURFACE)
            .expect("source viewport remains registered")
            .binding();
        let source = observed_source_window(source_binding, generation)
            .with_input_observation(WindowInputObservation::new(
                source_binding,
                InputObservationGeneration::new(generation),
                Authority::Known(source_state),
                acknowledged_effect,
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                source_binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
        let target_binding = self
            .engine
            .viewport()
            .viewport(TARGET_SURFACE)
            .expect("target viewport remains registered")
            .binding();
        let target = observed_target_window(target_binding, generation)
            .with_presentation_observation(WindowPresentationObservation::new(
                target_binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ));
        let mut windows = vec![source, target];
        if let Some((request, state)) = native {
            let phase = self
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .expect("native saga remains queryable")
                .phase();
            let acknowledged_effect = phase.presentation_correlation_effect();
            windows.push(
                observed_native_window(request, generation).with_presentation_observation(
                    WindowPresentationObservation::new(
                        request.binding(),
                        PresentationObservationGeneration::new(generation),
                        Authority::Known(state),
                        PresentationEffectAcknowledgement::known(Some(acknowledged_effect)),
                    ),
                ),
            );
        }
        let inventory_observation = support::known_inventory_observation(generation, &windows);
        PlatformSnapshot::new(
            dockspace::viewport::PlatformSnapshotGeneration::new(generation),
            support::known_capability_observation(generation, native_platform_capabilities()),
            unknown_focus_observation(
                FocusObservationGeneration::new(generation),
                AuthorityUnavailableReason::NotReported,
            ),
            inventory_observation,
            windows,
            Vec::new(),
            support::known_work_area_observation(
                generation,
                vec![ObservedWorkArea::new(
                    NATIVE_WORK_AREA,
                    physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
                    ScaleFactor::new(1.0).expect("fixture scale factor is valid"),
                )],
            ),
        )
        .expect("fixture platform snapshot is canonical")
    }

    fn publish_snapshot(&mut self, native: Option<(NativeCreateRequest, WindowPresentationState)>) {
        let snapshot = self.snapshot(native);
        let expected_epoch = self.engine.version().epoch();
        let provider = self.presentation_host.platform_provider();
        let transition = submit_input(
            &mut self.engine,
            &mut self.presentation_host,
            NATIVE_PLATFORM_SOURCE,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch,
                snapshot,
            },
        )
        .expect("platform snapshot reduces");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::PlatformSnapshotPublished { .. }
        ));
    }
}

fn native_platform_capabilities() -> PlatformCapabilities {
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
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    capabilities.set_close_cancellation(PlatformCapability::Supported);
    capabilities
}

fn observed_source_window(
    binding: dockspace::viewport::ViewportBinding,
    generation: u64,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_rect(0.0, 0.0, 900.0, 700.0)),
            Authority::Known(physical_rect(-8.0, -30.0, 916.0, 738.0)),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::PassThrough))
}

fn observed_target_window(
    binding: dockspace::viewport::ViewportBinding,
    generation: u64,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_rect(900.0, 0.0, 900.0, 700.0)),
            Authority::Known(physical_rect(892.0, -30.0, 916.0, 738.0)),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn observed_native_window(request: NativeCreateRequest, generation: u64) -> ObservedWindow {
    ObservedWindow::new(request.binding())
        .with_coordinate_observation(WindowCoordinateObservation::new(
            request.binding(),
            CoordinateObservationGeneration::new(generation),
            Authority::Known(physical_rect(100.0, 120.0, 640.0, 480.0)),
            Authority::Known(physical_rect(92.0, 90.0, 656.0, 518.0)),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("fixture scale factor is valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn source_snapshot(
    binding: dockspace::viewport::ViewportBinding,
    generation: u64,
    close: Option<WindowCloseState>,
) -> PlatformSnapshot {
    let windows = if close == Some(WindowCloseState::Destroyed) {
        Vec::new()
    } else {
        vec![
            observed_source_window(binding, generation).with_presentation_observation(
                WindowPresentationObservation::new(
                    binding,
                    PresentationObservationGeneration::new(generation),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
        ]
    };
    let close = close
        .map(|state| {
            WindowCloseObservation::new(
                binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                CloseEffectAcknowledgement::known(None),
            )
        })
        .into_iter()
        .collect();
    let inventory_observation = support::known_inventory_observation(generation, &windows);
    PlatformSnapshot::new(
        dockspace::viewport::PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, native_platform_capabilities()),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        inventory_observation,
        windows,
        close,
        support::known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                NATIVE_WORK_AREA,
                physical_rect(-1920.0, -200.0, 3840.0, 1400.0),
                ScaleFactor::new(1.0).expect("fixture scale factor is valid"),
            )],
        ),
    )
    .expect("fixture platform snapshot is canonical")
}

fn register_source(engine: &mut DockEngine, presentation_host: &mut TestPresentationHost) {
    let expected = engine.version();
    let provider = presentation_host.platform_provider();
    submit_input(
        engine,
        presentation_host,
        REGISTRATION_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: SOURCE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration reduces");
}

fn rehome_source_to_target(engine: &DockEngine) -> WorkspaceCommand {
    let source = engine
        .workspace()
        .root(SOURCE_ROOT)
        .expect("source root exists");
    WorkspaceCommand::RehomeRoot {
        source: engine
            .workspace()
            .capture_node_source(SOURCE_ROOT, source.node)
            .expect("source root is capturable"),
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: MOVED_FLOATING,
            rect: rect(40.0, 40.0, 260.0, 190.0),
            position: ContainedPosition::Front,
        },
    }
}

fn pointer_journal(
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
            sequence,
            NATIVE_POINTER,
            kind,
            location,
            delivery,
            capture,
        )],
    )
    .expect("fixture pointer journal is contiguous")
}

fn outside_all_route(fixture: &NativeVacancyFixture, position: PhysicalPoint) -> DesktopRouteFact {
    DesktopRouteFact::no_window(
        Authority::Known(position),
        Authority::Known(DesktopWorkAreaRoute::new(
            fixture.presentation_host.platform_provider(),
            fixture.engine.viewport().work_area_generation(),
            NATIVE_WORK_AREA,
        )),
    )
}

fn unknown_receiver_receipts(frame: &CoreHostFrame) -> PointerReceiverReceiptBatch {
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
    .expect("receiver receipts answer the exact candidate roster")
}

fn start_pending_native_create(fixture: &mut NativeVacancyFixture) -> NativeCreateRequest {
    let expected = fixture.engine.version();
    let provider = fixture.presentation_host.platform_provider();
    submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        NATIVE_LIFECYCLE_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: SOURCE_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("source viewport registration reduces");
    let expected = fixture.engine.version();
    submit_input(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        NATIVE_LIFECYCLE_SOURCE,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: TARGET_SURFACE,
            token: TARGET_WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("target viewport registration reduces");
    fixture.publish_snapshot(None);
    let _ = support::publish_surfaces_batch(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        [
            (SOURCE_SURFACE, rect(0.0, 0.0, 900.0, 700.0)),
            (TARGET_SURFACE, rect(900.0, 0.0, 900.0, 700.0)),
        ],
    );

    let source_binding = fixture
        .engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source viewport remains registered")
        .binding();
    let source_projection = fixture
        .engine
        .interaction_projection(SOURCE_SURFACE)
        .expect("source surface has current interaction authority");
    let source_region = source_projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == SOURCE_ITEM
            )
        })
        .expect("source tab has an exact receiver");
    let source_rect = source_region.hit().rect();
    let source_point = LogicalPoint::new(
        source_rect.x() + source_rect.width() * 0.5,
        source_rect.y() + source_rect.height() * 0.5,
    )
    .expect("source tab midpoint is finite");
    let source_receiver = source_region.id();
    let source_route = DesktopRouteFact::dock(DesktopDockRoute::new(
        source_binding,
        source_projection.authority().coordinate_generation(),
        PhysicalPoint::new(source_point.x(), source_point.y())
            .expect("source desktop point is finite"),
        source_point,
    ));
    let pointer_provider = fixture
        .engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(fixture.pointer_sequence),
        )
        .expect("desktop-global pointer provider is admitted");

    let mut press_frame = fixture.presentation_host.begin(&fixture.engine);
    press_frame
        .submit_pointer_journal(
            pointer_provider,
            pointer_journal(
                fixture.pointer_sequence,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                PointerEdgeLocation::Desktop {
                    route: source_route,
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            ),
        )
        .expect("source press stages");
    let candidate = press_frame
        .pointer_receiver_candidates()
        .expect("source press freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let projection = press_frame
        .view()
        .interaction_projection(SOURCE_SURFACE)
        .expect("sealed press frame retains source authority");
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(source_receiver),
    )
    .expect("source press binds to the presented tab");
    press_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("source press answers delivery"),
                ),
            )])
            .expect("source press receipt set is exact"),
        )
        .expect("source press receipt stages");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut press_frame);
    let pressed = fixture
        .presentation_host
        .finish(press_frame, &mut fixture.engine);
    assert!(matches!(
        pressed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    fixture.pointer_sequence += 1;

    let outside = PhysicalPoint::new(1_850.0, 900.0).expect("outside-all desktop point is finite");
    let mut move_frame = fixture.presentation_host.begin(&fixture.engine);
    move_frame
        .submit_pointer_journal(
            pointer_provider,
            pointer_journal(
                fixture.pointer_sequence,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: outside_all_route(fixture, outside),
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            ),
        )
        .expect("outside-all move stages");
    move_frame
        .submit_pointer_receiver_receipts(unknown_receiver_receipts(&move_frame))
        .expect("outside-all move receiver facts stage");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut move_frame);
    let moved = fixture
        .presentation_host
        .finish(move_frame, &mut fixture.engine);
    assert!(matches!(
        moved.reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated {
                status: PreviewResolutionStatus::Rejected,
                preview: None,
                ..
            }
        ]
    ));
    fixture.pointer_sequence += 1;

    let enable_effect = fixture
        .engine
        .viewport()
        .effects()
        .records()
        .filter_map(|(effect, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::SetPointerPassthrough {
                    binding,
                    enabled: true,
                    ..
                } if *binding == source_binding
            )
            .then_some(effect)
        })
        .max()
        .expect("physical drag requests source pointer pass-through");
    let snapshot = fixture.snapshot_with_source_input(
        None,
        WindowInputState::PassThrough,
        InputEffectAcknowledgement::known(Some(enable_effect)),
    );
    let expected_epoch = fixture.engine.version().epoch();
    let platform_provider = fixture.presentation_host.platform_provider();
    let mut confirmation_frame = fixture.presentation_host.begin(&fixture.engine);
    let mut input_stream =
        support::TestInputStream::resume(&fixture.engine, NATIVE_PLATFORM_SOURCE);
    input_stream
        .append(
            &mut confirmation_frame,
            EngineInput::PublishPlatformSnapshot {
                provider: platform_provider,
                expected_epoch,
                snapshot,
            },
        )
        .expect("pointer pass-through confirmation stages");
    let pointer_sequence = PointerEdgeSequence::new(fixture.pointer_sequence);
    confirmation_frame
        .submit_pointer_journal(
            pointer_provider,
            PointerEdgeJournal::new(pointer_sequence, pointer_sequence, Vec::new())
                .expect("confirmation preserves the pointer watermark"),
        )
        .expect("confirmation pointer checkpoint stages");
    confirmation_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::new())
                .expect("confirmation has no pointer receiver receipts"),
        )
        .expect("confirmation receipt checkpoint stages");
    support::complete_host_frame_with_retained_or_unavailable(
        &fixture.engine,
        &mut confirmation_frame,
    );
    let confirmed = fixture
        .presentation_host
        .finish(confirmation_frame, &mut fixture.engine);
    assert!(matches!(
        confirmed.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(enable_effect)
            .map(|record| record.phase()),
        Some(EffectPhase::ObservedApplied { .. })
    ));

    let mut routed_move_frame = fixture.presentation_host.begin(&fixture.engine);
    routed_move_frame
        .submit_pointer_journal(
            pointer_provider,
            pointer_journal(
                fixture.pointer_sequence,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: outside_all_route(fixture, outside),
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                Authority::Known(PointerCaptureOwner::Native(source_binding)),
            ),
        )
        .expect("confirmed outside-all move stages");
    routed_move_frame
        .submit_pointer_receiver_receipts(unknown_receiver_receipts(&routed_move_frame))
        .expect("confirmed outside-all move receiver facts stage");
    support::complete_host_frame_with_retained_or_unavailable(
        &fixture.engine,
        &mut routed_move_frame,
    );
    let routed = fixture
        .presentation_host
        .finish(routed_move_frame, &mut fixture.engine);
    assert!(matches!(
        routed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::PreviewUpdated {
            status: PreviewResolutionStatus::Resolved,
            preview: Some(_),
            ..
        }]
    ));
    fixture.pointer_sequence += 1;

    let acknowledgement = fixture
        .engine
        .interaction()
        .preview()
        .expect("outside-all move publishes a native preview")
        .acknowledgement();
    let mut release_frame = fixture.presentation_host.begin(&fixture.engine);
    let mut input_stream =
        support::TestInputStream::resume(&fixture.engine, NATIVE_RENDERER_SOURCE);
    input_stream
        .append(
            &mut release_frame,
            EngineInput::AcknowledgePreview {
                expected: fixture.engine.version(),
                acknowledgement,
            },
        )
        .expect("native preview acknowledgement stages before release");
    release_frame
        .submit_pointer_journal(
            pointer_provider,
            pointer_journal(
                fixture.pointer_sequence,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                PointerEdgeLocation::Desktop {
                    route: outside_all_route(fixture, outside),
                },
                Authority::Known(PointerEventDeliveryOwner::Native(source_binding)),
                Authority::Known(PointerCaptureOwner::None),
            ),
        )
        .expect("outside-all release stages");
    release_frame
        .submit_pointer_receiver_receipts(unknown_receiver_receipts(&release_frame))
        .expect("outside-all release receiver facts stage");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut release_frame);
    let released = fixture
        .presentation_host
        .finish(release_frame, &mut fixture.engine);
    fixture.pointer_sequence += 1;

    let request = released.reduced_pointer_edges()[0]
        .interaction_outcomes()
        .iter()
        .find_map(|outcome| match outcome {
            InteractionOutcome::DragDelivered {
                delivery: InteractionDelivery::NativeRequested(request),
                ..
            } => Some(*request),
            _ => None,
        })
        .expect("outside-all release starts one native create saga");
    fixture
        .engine
        .retire_pointer_provider(pointer_provider)
        .expect("idle pointer provider retires after native release");
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_some(),
        "pointer-provider retirement must not cancel the native create saga"
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .expect("native reservation is queryable")
            .admission(),
        ViewportAdmission::Pending
    );
    request
}

fn native_vacate_command(
    fixture: &NativeVacancyFixture,
    request: NativeCreateRequest,
) -> WorkspaceCommand {
    let create = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("native saga remains queryable")
        .prepared()
        .command()
        .clone();
    let mut future = fixture.engine.workspace().clone();
    WorkspaceTransaction::from_commands([create.clone()])
        .apply(&mut future, fixture.engine.policy_snapshot())
        .expect("native create command produces the staged workspace");
    let root = future.root(NATIVE_ROOT).expect("staged native root exists");
    WorkspaceCommand::RehomeRoot {
        source: future
            .capture_node_source(NATIVE_ROOT, root.node)
            .expect("staged native root is capturable"),
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: NATIVE_DESTINATION_FLOATING,
            rect: rect(80.0, 80.0, 320.0, 240.0),
            position: ContainedPosition::Front,
        },
    }
}

#[test]
fn same_tick_close_then_repopulate_preserves_the_existing_surface_binding() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(builder.build().expect("fixture workspace is valid"), policy)
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    support::publish_surface(
        &mut engine,
        &mut presentation_host,
        SOURCE_SURFACE,
        rect(0.0, 0.0, 640.0, 420.0),
    );

    let ready = engine
        .scene()
        .ready_surface(SOURCE_SURFACE)
        .expect("source surface is painted");
    let scene = ready.stamp();
    let tab = ready
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == SOURCE_ITEM)
        .expect("source tab is painted");
    let tab = *tab.id();
    let expected = engine.version();
    let requested = submit_input(
        &mut engine,
        &mut presentation_host,
        RENDERER_SOURCE,
        EngineInput::RequestSceneClose {
            expected,
            scene,
            target: CloseSceneTarget::Tab(tab),
        },
    )
    .expect("close request reduces");
    let plan = match requested.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::CloseRequested { plan, .. },
            ..
        } => plan.clone(),
        outcome => panic!("unexpected close request outcome: {outcome:?}"),
    };

    register_source(&mut engine, &mut presentation_host);
    let binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source binding exists")
        .binding();
    let expected = engine.version();
    let mut semantic_writer = support::TestInputStream::resume(&engine, CLOSE_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::ResolveClose {
                request: plan.request(),
                token: plan.items()[0].token(),
                decision: CloseDecision::Allow,
            },
        )
        .expect("close resolution belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            COMMAND_SOURCE,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::CreateSurfaceRoot {
                    surface: SOURCE_SURFACE,
                    root: RootId::new(99),
                    content: RootContent::OpenItem(ItemId::new(99)),
                },
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    assert_eq!(transition.reduced_inputs().len(), 2);
    assert_eq!(
        engine
            .viewport()
            .viewport(SOURCE_SURFACE)
            .expect("tick-final surface keeps its existing binding")
            .binding(),
        binding
    );
    assert!(engine.workspace().surface(SOURCE_SURFACE).is_some());
    assert!(
        transition
            .platform_effects()
            .iter()
            .all(|request| { !matches!(request.effect(), PlatformEffect::ReleaseChild { .. }) })
    );
}

#[test]
fn an_ordinary_workspace_command_unbinds_a_truly_vacant_external_surface() {
    let mut engine = DockEngine::new(two_surface_workspace(false), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_source(&mut engine, &mut presentation_host);
    let expected = engine.version();
    let command = rehome_source_to_target(&engine);

    let mut semantic_writer = support::TestInputStream::resume(&engine, COMMAND_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand { expected, command },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    assert!(engine.workspace().surface(SOURCE_SURFACE).is_none());
    assert!(engine.viewport().viewport(SOURCE_SURFACE).is_none());
    assert!(
        transition.platform_effects().is_empty(),
        "external bindings unbind without a platform cleanup effect"
    );
}

#[test]
fn same_tick_added_admitted_then_vacated_external_surface_is_unbound() {
    const ADMITTED_SURFACE: SurfaceId = SurfaceId::new(101);
    const ADMITTED_ROOT: RootId = RootId::new(101);
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([SOURCE_ITEM]));
    let target_tabs = builder.insert_node(Node::tabs([TARGET_ITEM]));
    builder.set_root(ADMITTED_ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(
        ADMITTED_SURFACE,
        SurfacePresentation::with_main(ADMITTED_ROOT),
    );
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let replacement = builder.build().expect("replacement workspace is valid");
    let source = replacement
        .root(ADMITTED_ROOT)
        .expect("replacement source root exists");
    let command = WorkspaceCommand::RehomeRoot {
        source: replacement
            .capture_node_source(ADMITTED_ROOT, source.node)
            .expect("replacement source root is capturable"),
        target: RootPresentationTarget::Contained {
            surface: TARGET_SURFACE,
            floating: MOVED_FLOATING,
            rect: rect(40.0, 40.0, 260.0, 190.0),
            position: ContainedPosition::Front,
        },
    };
    let mut engine = DockEngine::new(target_only_workspace(), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let replacement_version = WorkspaceVersion::new(
        engine
            .version()
            .epoch()
            .checked_next()
            .expect("fixture epoch can advance"),
        WorkspaceRevision::default(),
    );
    let provider = presentation_host.platform_provider();

    let mut semantic_writer = support::TestInputStream::resume(&engine, LIFECYCLE_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(&mut frame, EngineInput::ReplaceWorkspace(replacement))
        .expect("workspace replacement belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            REGISTRATION_SOURCE,
            EngineInput::RegisterViewport {
                provider,
                expected: replacement_version,
                surface: ADMITTED_SURFACE,
                token: SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        )
        .expect("viewport registration belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            COMMAND_SOURCE,
            EngineInput::WorkspaceCommand {
                expected: replacement_version,
                command,
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::ViewportRegistered { binding }
            if binding.surface() == ADMITTED_SURFACE && binding.token() == SOURCE_WINDOW
    ));
    assert!(engine.workspace().surface(ADMITTED_SURFACE).is_none());
    assert!(engine.viewport().viewport(ADMITTED_SURFACE).is_none());
    assert!(
        transition.platform_effects().is_empty(),
        "external bindings unbind without a platform cleanup effect"
    );
}

#[test]
fn pending_native_reservation_is_not_settled_as_an_admitted_vacancy() {
    let mut fixture = NativeVacancyFixture::new();
    let request = start_pending_native_create(&mut fixture);
    let target = fixture
        .engine
        .workspace()
        .root(TARGET_ROOT)
        .expect("target root exists");
    let vacate = WorkspaceCommand::RehomeRoot {
        source: fixture
            .engine
            .workspace()
            .capture_node_source(TARGET_ROOT, target.node)
            .expect("target root is capturable"),
        target: RootPresentationTarget::Contained {
            surface: SOURCE_SURFACE,
            floating: NATIVE_DESTINATION_FLOATING,
            rect: rect(80.0, 80.0, 320.0, 240.0),
            position: ContainedPosition::Front,
        },
    };
    let expected = fixture.engine.version();

    let mut semantic_writer =
        support::TestInputStream::resume(&fixture.engine, NATIVE_COMMAND_SOURCE);
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: vacate,
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert!(fixture.engine.workspace().surface(TARGET_SURFACE).is_none());
    assert!(fixture.engine.viewport().viewport(TARGET_SURFACE).is_none());
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    let viewport = fixture
        .engine
        .viewport()
        .viewport(NATIVE_SURFACE)
        .expect("pending native reservation must remain owned by its saga");
    assert_eq!(viewport.admission(), ViewportAdmission::Pending);
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("pending native saga remains queryable")
            .phase(),
        NativeCreatePhase::AwaitingHidden { .. }
    ));
    assert!(transition.platform_effects().iter().all(|effect| {
        !matches!(
            effect.effect(),
            PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
        )
    }));
}

#[test]
fn same_tick_native_first_live_admission_then_vacancy_releases_the_child() {
    let mut fixture = NativeVacancyFixture::new();
    let request = start_pending_native_create(&mut fixture);
    let vacate = native_vacate_command(&fixture, request);
    fixture.publish_snapshot(Some((request, WindowPresentationState::Hidden)));
    let pre_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPreShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("hidden native create must request pre-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        pre_show,
    );
    fixture.publish_snapshot(Some((request, WindowPresentationState::Hidden)));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("native saga remains queryable")
            .phase(),
        NativeCreatePhase::AwaitingVisible { .. }
    ));

    fixture.publish_snapshot(Some((request, WindowPresentationState::Visible)));
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("transferred native saga remains queryable")
            .phase(),
        NativeCreatePhase::AwaitingPostShowPresentation { .. }
    ));
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    let post_show = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .and_then(|saga| match saga.phase() {
            NativeCreatePhase::AwaitingPostShowPresentation { presentation, .. } => {
                Some(presentation)
            }
            _ => None,
        })
        .expect("visible native create must request post-show staging");
    let _ = support::present_native_staging(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        post_show,
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("transferred native saga remains queryable")
            .phase(),
        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(NATIVE_SURFACE)
            .map(|record| record.admission()),
        Some(ViewportAdmission::Pending)
    );

    support::install_surface_projection(
        &mut fixture.engine,
        &mut fixture.presentation_host,
        NATIVE_SURFACE,
        rect(1_200.0, 0.0, 800.0, 600.0),
    );
    let paint_frame = fixture.presentation_host.begin(&fixture.engine);
    let paint_frame =
        support::complete_host_frame_with_current_outputs(&fixture.engine, paint_frame);
    let painted = fixture
        .presentation_host
        .finish_presentation(paint_frame, &mut fixture.engine);
    assert_eq!(
        painted
            .presentation_emissions()
            .iter()
            .filter(|emission| emission.output().surface() == NATIVE_SURFACE)
            .count(),
        1
    );

    let expected = fixture.engine.version();
    let mut semantic_writer =
        support::TestInputStream::resume(&fixture.engine, NATIVE_COMMAND_SOURCE);
    let mut frame = fixture.presentation_host.begin(&fixture.engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: vacate,
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.presentation_host.finish(frame, &mut fixture.engine);

    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert!(fixture.engine.workspace().surface(NATIVE_SURFACE).is_none());
    assert!(fixture.engine.viewport().viewport(NATIVE_SURFACE).is_none());
    let releases = transition
        .platform_effects()
        .iter()
        .filter(|effect| {
            matches!(
                effect.effect(),
                PlatformEffect::ReleaseChild { binding } if *binding == request.binding()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(releases.len(), 1);
    let retirement = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == request.binding()).then_some(retirement))
        .expect("retiring runtime binding remains auditable through exact retirement state");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: releases[0].id(),
        }
    );
}

#[test]
fn a_rootless_surface_with_a_contained_sibling_is_not_vacant() {
    let mut engine = DockEngine::new(two_surface_workspace(true), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_source(&mut engine, &mut presentation_host);
    let binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source binding exists")
        .binding();
    let expected = engine.version();
    let command = rehome_source_to_target(&engine);

    let mut semantic_writer = support::TestInputStream::resume(&engine, COMMAND_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand { expected, command },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    let source = engine
        .workspace()
        .surface(SOURCE_SURFACE)
        .expect("contained sibling keeps the surface alive");
    assert_eq!(source.main_root, None);
    assert_eq!(source.contained, vec![SIBLING_FLOATING]);
    assert_eq!(
        engine
            .viewport()
            .viewport(SOURCE_SURFACE)
            .expect("rootless surface keeps its binding")
            .binding(),
        binding
    );
    assert!(transition.platform_effects().is_empty());
}

#[test]
fn validate_then_single_replacement_and_vacancy_settles_the_new_binding() {
    let replacement = two_surface_workspace(false);
    let mut engine = DockEngine::new(replacement.clone(), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_source(&mut engine, &mut presentation_host);
    let old_binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source binding exists")
        .binding();
    let before_version = engine.version();
    let replacement_version = WorkspaceVersion::new(
        before_version
            .epoch()
            .checked_next()
            .expect("fixture epoch can advance"),
        WorkspaceRevision::default(),
    );
    let command = rehome_source_to_target(&engine);

    let mut semantic_writer = support::TestInputStream::resume(&engine, LIFECYCLE_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(&mut frame, EngineInput::ValidateWorkspace)
        .expect("validation belongs to the semantic host-frame phase");
    semantic_writer
        .append(&mut frame, EngineInput::ReplaceWorkspace(replacement))
        .expect("workspace replacement belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            COMMAND_SOURCE,
            EngineInput::WorkspaceCommand {
                expected: replacement_version,
                command,
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    let rebound = transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation
                .rebound()
                .iter()
                .find_map(|(previous, current)| (*previous == old_binding).then_some(*current)),
            _ => None,
        })
        .expect("workspace replacement must publish the exact rebound binding");
    assert_ne!(rebound, old_binding);
    assert_eq!(rebound.surface(), SOURCE_SURFACE);
    assert!(engine.workspace().surface(SOURCE_SURFACE).is_none());
    assert!(
        engine.viewport().viewport(SOURCE_SURFACE).is_none(),
        "tick-final vacancy must detach the rebound incarnation, not ignore it after filtering the old one"
    );
    assert!(transition.platform_effects().is_empty());
}

#[test]
fn validate_then_double_replacement_and_vacancy_settles_the_latest_binding() {
    let replacement = two_surface_workspace(false);
    let mut engine = DockEngine::new(replacement.clone(), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_source(&mut engine, &mut presentation_host);
    let old_binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source binding exists")
        .binding();
    let first_epoch = engine
        .version()
        .epoch()
        .checked_next()
        .expect("fixture epoch can advance once");
    let second_replacement_version = WorkspaceVersion::new(
        first_epoch
            .checked_next()
            .expect("fixture epoch can advance twice"),
        WorkspaceRevision::default(),
    );
    let command = rehome_source_to_target(&engine);

    let mut semantic_writer = support::TestInputStream::resume(&engine, LIFECYCLE_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(&mut frame, EngineInput::ValidateWorkspace)
        .expect("validation belongs to the semantic host-frame phase");
    semantic_writer
        .append(
            &mut frame,
            EngineInput::ReplaceWorkspace(replacement.clone()),
        )
        .expect("first workspace replacement belongs to the semantic host-frame phase");
    semantic_writer
        .append(&mut frame, EngineInput::ReplaceWorkspace(replacement))
        .expect("second workspace replacement belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            COMMAND_SOURCE,
            EngineInput::WorkspaceCommand {
                expected: second_replacement_version,
                command,
            },
        )
        .expect("workspace command belongs to the semantic host-frame phase");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    let rebound_chain = transition
        .reduced_inputs()
        .iter()
        .flat_map(|input| match input.outcome() {
            InputOutcome::WorkspaceReplaced { reconciliation, .. } => {
                reconciliation.rebound().iter().copied().collect::<Vec<_>>()
            }
            _ => Vec::new(),
        })
        .collect::<Vec<_>>();
    assert_eq!(rebound_chain.len(), 2);
    let (first_previous, first_binding) = rebound_chain[0];
    let (second_previous, latest_binding) = rebound_chain[1];
    assert_eq!(first_previous, old_binding);
    assert_eq!(second_previous, first_binding);
    assert_ne!(first_binding, old_binding);
    assert_ne!(latest_binding, first_binding);
    assert_eq!(latest_binding.surface(), SOURCE_SURFACE);
    assert!(engine.workspace().surface(SOURCE_SURFACE).is_none());
    assert!(
        engine.viewport().viewport(SOURCE_SURFACE).is_none(),
        "tick-final vacancy must detach the latest rebound incarnation rather than filtering an earlier one"
    );
    assert!(transition.platform_effects().is_empty());
}

#[test]
fn destroyed_then_registered_rebound_settles_the_new_exact_binding_at_tick_final_vacancy() {
    let mut engine = DockEngine::new(two_surface_workspace(false), DockPolicy::default())
        .expect("fixture engine is valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    register_source(&mut engine, &mut presentation_host);
    let old_binding = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("source binding exists")
        .binding();

    let expected_epoch = engine.version().epoch();
    let provider = presentation_host.platform_provider();
    submit_input(
        &mut engine,
        &mut presentation_host,
        NATIVE_PLATFORM_SOURCE,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: source_snapshot(old_binding, 1, None),
        },
    )
    .expect("old external binding must be observably live before destruction");

    let expected = engine.version();
    let command = rehome_source_to_target(&engine);
    let mut semantic_writer = support::TestInputStream::resume(&engine, NATIVE_PLATFORM_SOURCE);
    let mut frame = presentation_host.begin(&engine);
    semantic_writer
        .append(
            &mut frame,
            EngineInput::PublishPlatformSnapshot {
                provider,
                expected_epoch: expected.epoch(),
                snapshot: source_snapshot(old_binding, 2, Some(WindowCloseState::Destroyed)),
            },
        )
        .expect("terminal observation belongs to the semantic host-frame phase");
    semantic_writer
        .append_as(
            &mut frame,
            REGISTRATION_SOURCE,
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SOURCE_SURFACE,
                token: REBOUND_SOURCE_WINDOW,
                role: ViewportRole::Root,
                recovery_target: None,
            },
        )
        .expect("new exact external binding belongs to the same host frame");
    semantic_writer
        .append_as(
            &mut frame,
            COMMAND_SOURCE,
            EngineInput::WorkspaceCommand { expected, command },
        )
        .expect("surface removal belongs to the same host frame");
    support::complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = presentation_host.finish(frame, &mut engine);

    let rebound = transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::ViewportRegistered { binding }
                if binding.surface() == SOURCE_SURFACE
                    && binding.token() == REBOUND_SOURCE_WINDOW =>
            {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the new external incarnation must register after the terminal old binding");
    assert_ne!(rebound, old_binding);
    assert!(engine.workspace().surface(SOURCE_SURFACE).is_none());
    assert!(
        engine.viewport().viewport(SOURCE_SURFACE).is_none(),
        "tick-final vacancy must settle the new exact binding, not retain it behind the old tombstone"
    );
    assert!(transition.platform_effects().is_empty());
}
