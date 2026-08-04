use std::collections::BTreeMap;

use dockspace::engine::{
    BackendIngressProgress, EngineInput, HostPresentationDisposition, HostPresentationSlot,
    HostPresentationUnavailableReason,
};
use dockspace::frame::PanelFocus;
use dockspace::geometry::{LogicalPoint, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{InteractionDelivery, InteractionEventKind, InteractionOutcome};
use dockspace::platform::{
    CapabilityRosterObservation, ObservedWindow, ObservedWorkArea, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCoordinateObservation, WindowInputState, WindowInventoryObservation,
    WindowPresentationObservation, WindowPresentationState, WorkAreaRosterObservation,
};
use dockspace::pointer_journal::{
    DesktopDockRoute, DesktopRouteFact, DesktopWorkAreaRoute, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerEventDeliveryOwner,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PointerReceiverUnknownReason,
    PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::{
    HostPresentationCaptureGeneration, HostPresentationObservationEntry,
    HostPresentationOutputPayload, HostPresentationProgress, HostPresentationStreamId,
    HostPresentationStreamObservation,
};
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::viewport::{
    CapabilityObservationGeneration, CoordinateObservationGeneration,
    InventoryObservationGeneration, PlatformSnapshotGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaObservationGeneration, WorkAreaToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow, PanelFocusRecord,
    ViewportActivationRequest,
};
use egui::accesskit::{Action, Role};
use egui::{Color32, Context, Id, Pos2, RawInput, Rect, TextEdit, Ui, ViewportId, vec2};
use egui_dockspace::{
    Dockspace, EguiFrameScheduleKey, EguiOuterSurfaceOutput, EguiPresentationResult,
    ExactNativeViewport, NativeBindingRoster, NativeCoreRoute, NativeViewportIncarnation, PaneView,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM: ItemId = ItemId::new(3);
const POINTER: PointerId = PointerId::new(1);
const WORK_AREA: WorkAreaToken = WorkAreaToken::new(1);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

struct FocusPanes {
    target: Id,
    text: String,
}

impl FocusPanes {
    fn new() -> Self {
        Self {
            target: Id::new("native-pane-focus-target"),
            text: String::new(),
        }
    }
}

impl PaneView for FocusPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        ui.add(TextEdit::singleline(&mut self.text).id(self.target));
    }

    fn focus_target(&self, item: ItemId) -> Option<Id> {
        (item == ITEM).then_some(self.target)
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("test workspace must be valid")
}

fn context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    context
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        ..RawInput::default()
    }
}

fn native_input(viewport_id: ViewportId) -> RawInput {
    let mut input = RawInput {
        viewport_id,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        ..RawInput::default()
    };
    input
        .viewports
        .entry(viewport_id)
        .or_insert_with(|| egui::ViewportInfo {
            parent: Some(ViewportId::ROOT),
            ..Default::default()
        });
    input
}

fn focused_native_input(viewport_id: ViewportId) -> RawInput {
    let mut input = native_input(viewport_id);
    input
        .viewports
        .get_mut(&viewport_id)
        .expect("the native viewport info exists")
        .focused = Some(true);
    input
}

fn backend_capabilities() -> PlatformCapabilities {
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
    capabilities
}

fn observed_window(
    binding: ViewportBinding,
    presentation: WindowPresentationState,
    acknowledged_effect: Option<dockspace::effect::EffectId>,
    x: f64,
) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(
                PhysicalRect::new(x, 0.0, 640.0, 480.0).expect("test client bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(x - 8.0, -30.0, 656.0, 518.0)
                    .expect("test outer bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(presentation),
            PresentationEffectAcknowledgement::known(acknowledged_effect),
        ))
        .with_close_requested(Authority::Known(false))
}

fn platform_snapshot(
    generation: u64,
    source: ViewportBinding,
    target: Option<(
        ViewportBinding,
        WindowPresentationState,
        Option<dockspace::effect::EffectId>,
    )>,
) -> PlatformSnapshot {
    let mut windows = vec![observed_window(
        source,
        WindowPresentationState::Visible,
        None,
        0.0,
    )];
    if let Some((binding, presentation, acknowledgement)) = target {
        windows.push(observed_window(
            binding,
            presentation,
            acknowledgement,
            800.0,
        ));
    }
    let inventory = windows.iter().map(ObservedWindow::binding).collect();
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(backend_capabilities()),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Dock(source)),
            Authority::Known(None),
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(inventory),
        )
        .expect("test inventory must be canonical"),
        windows,
        Vec::new(),
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Known(vec![ObservedWorkArea::new(
                WORK_AREA,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid"),
                ScaleFactor::new(1.0).expect("test work-area scale must be valid"),
            )]),
        )
        .expect("test work-area roster must be canonical"),
    )
    .expect("test platform snapshot must be canonical")
}

fn pointer_journal(
    previous: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    delivery: ViewportBinding,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new_with_delivery(
            sequence,
            POINTER,
            kind,
            location,
            Authority::Known(PointerEventDeliveryOwner::Native(delivery)),
            capture,
        )],
    )
    .expect("test pointer journal must be contiguous")
}

fn outside_all_route(dockspace: &Dockspace, position: PhysicalPoint) -> DesktopRouteFact {
    DesktopRouteFact::no_window(
        Authority::Known(position),
        Authority::Known(DesktopWorkAreaRoute::new(
            dockspace
                .engine()
                .platform_provider()
                .expect("native test platform provider remains active"),
            dockspace.engine().viewport().work_area_generation(),
            WORK_AREA,
        )),
    )
}

fn unavailable_receiver_receipts(
    candidates: &dockspace::pointer_receiver::PointerReceiverCandidateRoster,
) -> PointerReceiverReceiptBatch {
    PointerReceiverReceiptBatch::new(candidates.candidates().iter().map(|candidate| {
        let observation = if candidate.receiver_is_applicable() {
            PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            )
        } else {
            PointerReceiverObservation::NotApplicable
        };
        candidate.receipt(observation)
    }))
    .expect("test receiver receipts must answer the exact roster")
}

fn reclaim_backend_prefix(
    dockspace: &mut Dockspace,
    recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
) {
    let committed = dockspace
        .engine()
        .backend_ingress_commit_watermark()
        .expect("a committed native frame exposes a reclaimable prefix");
    let receipt = recorder
        .retire_committed_prefix(committed)
        .expect("the recorder may reclaim the core-proven prefix");
    if let Some(mut receipt) = receipt {
        dockspace
            .adapter_settle_backend_ingress_prefix_retirement(&mut receipt)
            .expect("the reclaimed prefix must settle against the same core authority");
    }
}

#[derive(Default)]
struct PresentationCaptureClock {
    generations: BTreeMap<HostPresentationStreamId, HostPresentationCaptureGeneration>,
}

impl PresentationCaptureClock {
    fn settle_presented(
        &mut self,
        outputs: Vec<EguiOuterSurfaceOutput>,
    ) -> Vec<HostPresentationObservationEntry> {
        outputs
            .into_iter()
            .filter_map(|output| {
                let (_, _, settlement) = output.into_parts();
                let presentation = settlement.presentation_output();
                settlement.settle(EguiPresentationResult::Presented);
                presentation.map(|output| {
                    let stream = output.stream();
                    let generation = self.generations.get(&stream).copied().map_or(
                        HostPresentationCaptureGeneration::new(1),
                        |previous| {
                            previous
                                .checked_next()
                                .expect("test presentation generation must not exhaust")
                        },
                    );
                    self.generations.insert(stream, generation);
                    HostPresentationObservationEntry::new(
                        stream,
                        HostPresentationStreamObservation::Captured {
                            generation,
                            progress: HostPresentationProgress::Retired {
                                settled_through: output.key(),
                                presented: Authority::Known(Some(output.key())),
                            },
                        },
                    )
                })
            })
            .collect()
    }

    fn record(
        dockspace: &Dockspace,
        recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
        entries: impl IntoIterator<Item = HostPresentationObservationEntry>,
    ) {
        for entry in entries {
            dockspace
                .record_backend_presentation_observation(recorder, entry)
                .expect("renderer observation must enter the joined ingress order");
        }
    }

    fn settle_and_record(
        &mut self,
        dockspace: &Dockspace,
        recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
        outputs: Vec<EguiOuterSurfaceOutput>,
    ) {
        let entries = self.settle_presented(outputs);
        Self::record(dockspace, recorder, entries);
    }
}

fn empty_native_bindings() -> NativeBindingRoster {
    NativeBindingRoster::empty()
}

fn native_roster(route: NativeCoreRoute) -> NativeBindingRoster {
    NativeBindingRoster::new([route], [])
}

fn bootstrap_native_root(
    dockspace: &mut Dockspace,
    recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
) -> NativeCoreRoute {
    dockspace
        .record_backend_viewport_registration(
            recorder,
            SURFACE,
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .expect("bootstrap registration must enter the joined ingress stream");
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty bootstrap pointer checkpoint must be canonical"),
        )
        .expect("bootstrap pointer checkpoint must be recorded");
    let batch = recorder
        .pending_batch()
        .expect("bootstrap ingress must freeze");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("bootstrap native cycle must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("bootstrap ingress must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty bootstrap receipts must be canonical"),
            )
            .expect("bootstrap pointer checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = input_session
        .into_presentation()
        .expect("bootstrap input must enter presentation");
    presentation
        .mark_surface_unavailable(dockspace, SURFACE, MeasurementUnavailableReason::Deferred)
        .expect("bootstrap may explicitly omit paint");
    let commit = presentation
        .finish(dockspace)
        .expect("bootstrap registration must publish atomically");
    for output in commit.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
    let committed = dockspace
        .engine()
        .backend_ingress_commit_watermark()
        .expect("bootstrap must publish a reclaimable prefix");
    let receipt = recorder
        .retire_committed_prefix(committed)
        .expect("bootstrap prefix must be reclaimable");
    if let Some(mut receipt) = receipt {
        dockspace
            .adapter_settle_backend_ingress_prefix_retirement(&mut receipt)
            .expect("bootstrap prefix retirement must settle");
    }
    let binding = dockspace
        .native_viewport_binding(SURFACE)
        .expect("bootstrap registration must mint a core binding");
    NativeCoreRoute::new(
        ExactNativeViewport::new(ViewportId::ROOT, NativeViewportIncarnation::new(1)),
        SURFACE,
        binding,
    )
}

fn paint_native_frame(
    dockspace: &mut Dockspace,
    recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
    route: NativeCoreRoute,
    sequence: u64,
) -> egui_dockspace::EguiOuterFrameCommit {
    paint_native_frame_with_context(dockspace, recorder, route, sequence, &context())
}

fn paint_native_frame_with_context(
    dockspace: &mut Dockspace,
    recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
    route: NativeCoreRoute,
    sequence: u64,
    context: &Context,
) -> egui_dockspace::EguiOuterFrameCommit {
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty native pointer checkpoint must be canonical"),
        )
        .expect("empty native pointer checkpoint must be recorded");
    let batch = recorder
        .pending_batch()
        .expect("native frame ingress must freeze");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(sequence, 0), native_roster(route))
        .expect("native cycle must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("native ingress must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty native receipts must be canonical"),
            )
            .expect("empty native checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = input_session
        .into_presentation()
        .expect("native input must enter presentation");
    presentation
        .run_native_surface(dockspace, route.native(), context, input(), &mut TestPanes)
        .expect("exact native surface must paint");
    let commit = presentation
        .finish(dockspace)
        .expect("native frame must publish atomically");
    let committed = dockspace
        .engine()
        .backend_ingress_commit_watermark()
        .expect("native frame must publish a reclaimable ingress prefix");
    let receipt = recorder
        .retire_committed_prefix(committed)
        .expect("committed native ingress must be reclaimable");
    if let Some(mut receipt) = receipt {
        dockspace
            .adapter_settle_backend_ingress_prefix_retirement(&mut receipt)
            .expect("native prefix retirement must settle");
    }
    commit
}

fn paint_native_focus_frame(
    dockspace: &mut Dockspace,
    recorder: &mut dockspace::backend_ingress::BackendIngressRecorder,
    route: NativeCoreRoute,
    sequence: u64,
    context: &Context,
    panes: &mut FocusPanes,
) -> egui_dockspace::EguiOuterFrameCommit {
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty native pointer checkpoint must be canonical"),
        )
        .expect("empty native pointer checkpoint must be recorded");
    let batch = recorder
        .pending_batch()
        .expect("native focus ingress must freeze");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(sequence, 0), native_roster(route))
        .expect("native focus cycle must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("native focus ingress must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty native receipts must be canonical"),
            )
            .expect("empty native checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = input_session
        .into_presentation()
        .expect("native focus input must enter presentation");
    presentation
        .run_native_surface(
            dockspace,
            route.native(),
            context,
            focused_native_input(route.native().viewport()),
            panes,
        )
        .expect("the native focus surface must paint");
    presentation
        .finish(dockspace)
        .expect("the native focus frame must publish atomically")
}

#[test]
fn native_pane_focus_is_requested_sampled_and_acknowledged_across_three_cycles() {
    let mut dockspace = Dockspace::builder("native-pane-focus", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);
    let context = context();
    let mut panes = FocusPanes::new();
    let mut presentation_clock = PresentationCaptureClock::default();

    recorder
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(1, route.core(), None),
        )
        .expect("global focus authority must enter the native stream");
    let mut sequence = 2;
    loop {
        let commit = paint_native_focus_frame(
            &mut dockspace,
            &mut recorder,
            route,
            sequence,
            &context,
            &mut panes,
        );
        let (_, outputs) = commit.into_parts();
        let _ = dockspace
            .record_ready_backend_pane_focus_observations(&mut recorder)
            .expect("completed focus samples must enter the backend recorder");
        presentation_clock.settle_and_record(&dockspace, &mut recorder, outputs);
        reclaim_backend_prefix(&mut dockspace, &mut recorder);
        sequence += 1;
        if dockspace.engine().interaction_projection(SURFACE).is_some() {
            break;
        }
        assert!(sequence < 8, "native presentation authority must converge");
    }

    recorder
        .record_semantic_input(EngineInput::ActivateViewport {
            expected: dockspace.engine().version(),
            request: ViewportActivationRequest::explicit(route.core(), PanelFocus::Item(ITEM)),
        })
        .expect("the explicit pane-focus request must retain backend order");
    let requested = paint_native_focus_frame(
        &mut dockspace,
        &mut recorder,
        route,
        sequence,
        &context,
        &mut panes,
    );
    sequence += 1;
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "the first post-intent paint must request the pane's real egui focus target",
    );
    assert!(
        dockspace
            .record_ready_backend_pane_focus_observations(&mut recorder)
            .expect("same-paint focus output remains queryable")
            .is_empty(),
        "a request cannot acknowledge itself in the paint that issued it",
    );
    presentation_clock.settle_and_record(&dockspace, &mut recorder, requested.into_parts().1);
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    let intent = dockspace
        .engine()
        .viewport_focus()
        .pending_pane_intent()
        .expect("the request remains pending until a later observation");

    let sampled = paint_native_focus_frame(
        &mut dockspace,
        &mut recorder,
        route,
        sequence,
        &context,
        &mut panes,
    );
    sequence += 1;
    let observations = dockspace
        .record_ready_backend_pane_focus_observations(&mut recorder)
        .expect("the later focus sample must enter backend ingress");
    assert_eq!(observations.len(), 1);
    assert_eq!(
        dockspace.engine().viewport_focus().pending_pane_intent(),
        Some(intent),
        "recording an observation is not the same as core acknowledgement",
    );
    presentation_clock.settle_and_record(&dockspace, &mut recorder, sampled.into_parts().1);
    reclaim_backend_prefix(&mut dockspace, &mut recorder);

    let acknowledged = paint_native_focus_frame(
        &mut dockspace,
        &mut recorder,
        route,
        sequence,
        &context,
        &mut panes,
    );
    assert_eq!(
        dockspace.engine().viewport_focus().pending_pane_intent(),
        None,
    );
    assert_eq!(
        dockspace.engine().viewport_focus().panel_focus(SURFACE),
        PanelFocusRecord::Item(ITEM),
    );
    assert!(
        dockspace
            .record_ready_backend_pane_focus_observations(&mut recorder)
            .expect("unchanged acknowledged focus remains stable")
            .is_empty(),
        "an acknowledged unchanged focus sample must not grow the recorder",
    );
    for output in acknowledged.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn uncommitted_native_pane_focus_observation_replays_after_backend_provider_replacement() {
    let mut dockspace = Dockspace::builder("native-pane-focus-provider-replacement", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("predecessor backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);
    let context = context();
    let mut panes = FocusPanes::new();
    let mut presentation_clock = PresentationCaptureClock::default();

    recorder
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(1, route.core(), None),
        )
        .expect("predecessor focus authority must enter the native stream");
    let mut sequence = 2;
    loop {
        let commit = paint_native_focus_frame(
            &mut dockspace,
            &mut recorder,
            route,
            sequence,
            &context,
            &mut panes,
        );
        let (_, outputs) = commit.into_parts();
        let _ = dockspace
            .record_ready_backend_pane_focus_observations(&mut recorder)
            .expect("bootstrap focus samples must enter the predecessor recorder");
        presentation_clock.settle_and_record(&dockspace, &mut recorder, outputs);
        reclaim_backend_prefix(&mut dockspace, &mut recorder);
        sequence += 1;
        if dockspace.engine().interaction_projection(SURFACE).is_some() {
            break;
        }
        assert!(sequence < 8, "native presentation authority must converge");
    }

    context.memory_mut(|memory| memory.request_focus(panes.target));
    let sampled = paint_native_focus_frame(
        &mut dockspace,
        &mut recorder,
        route,
        sequence,
        &context,
        &mut panes,
    );
    sequence += 1;
    assert!(
        context.memory(|memory| memory.has_focus(panes.target)),
        "the predecessor paint must sample the pane's real egui focus target",
    );
    let predecessor_ordinals = dockspace
        .record_ready_backend_pane_focus_observations(&mut recorder)
        .expect("the sampled focus must reserve a predecessor ingress position");
    assert_eq!(predecessor_ordinals.len(), 1);
    for output in sampled.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    assert!(
        recorder.recorded_through() > dockspace.engine().backend_ingress_committed_through(),
        "recorder acceptance must not masquerade as a core commit",
    );
    assert_ne!(
        dockspace.engine().viewport_focus().panel_focus(SURFACE),
        PanelFocusRecord::Item(ITEM),
        "the uncommitted predecessor sample must not affect core focus history",
    );

    let predecessor = recorder.lease();
    let mut drained = recorder.drain();
    let replacement = dockspace
        .begin_backend_ingress_provider_replacement(&mut drained)
        .expect("the exact predecessor may begin a joined replacement");
    assert!(drained.is_consumed());
    let (mut ticket, _) = replacement.into_parts();
    let mut successor = dockspace
        .finish_backend_ingress_provider_replacement(&mut ticket)
        .expect("the successor lanes must activate atomically");
    assert!(ticket.is_consumed());
    assert_ne!(successor.lease(), predecessor);

    successor
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(1, route.core(), None),
        )
        .expect("the successor must establish a fresh platform baseline");
    let replayed_ordinals = dockspace
        .record_ready_backend_pane_focus_observations(&mut successor)
        .expect("the successor recorder must recover the predecessor reservation");
    assert_eq!(replayed_ordinals.len(), 1);
    assert_ne!(
        dockspace.engine().viewport_focus().panel_focus(SURFACE),
        PanelFocusRecord::Item(ITEM),
        "re-recording still must not bypass the core frame boundary",
    );

    let acknowledged = paint_native_focus_frame(
        &mut dockspace,
        &mut successor,
        route,
        sequence,
        &context,
        &mut panes,
    );
    assert_eq!(
        dockspace.engine().viewport_focus().panel_focus(SURFACE),
        PanelFocusRecord::Item(ITEM),
        "the successor commit must acknowledge the replayed focus sample",
    );
    let committed = dockspace
        .engine()
        .backend_ingress_commit_watermark()
        .expect("the successor frame must publish a commit watermark");
    assert_eq!(committed.lease(), successor.lease());
    assert!(committed.through() >= replayed_ordinals[0]);
    assert!(
        dockspace
            .record_ready_backend_pane_focus_observations(&mut successor)
            .expect("the acknowledged sample must remain quiescent")
            .is_empty(),
        "the committed replay must not be emitted a third time",
    );
    for output in acknowledged.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn native_session_is_owned_and_blocks_competing_facade_mutations() {
    fn assert_static<T: 'static>() {}

    assert_static::<egui_dockspace::EguiNativeInputSession>();
    assert_static::<egui_dockspace::EguiNativePresentationSession>();

    let mut dockspace = Dockspace::builder("native-session-lease", workspace())
        .build()
        .expect("fixture must build");
    dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let version = dockspace.engine().version();
    let session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("owned native session must begin");

    assert!(matches!(
        dockspace.set_policy(DockPolicy::default()),
        Err(egui_dockspace::DockspaceError::NativeSessionAlreadyActive)
    ));
    assert!(matches!(
        dockspace.begin_native_cycle(EguiFrameScheduleKey::new(2, 0), empty_native_bindings()),
        Err(egui_dockspace::DockspaceError::NativeSessionAlreadyActive)
    ));
    assert_eq!(dockspace.engine().version(), version);

    drop(session);
    let retry = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("dropping the owner must make the same uncommitted key retryable");
    drop(retry);
}

#[test]
fn native_presentation_session_rejects_a_different_dockspace_instance() {
    let mut dockspace = Dockspace::builder("native-session-owner", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("empty checkpoint must enter the joined ingress stream");
    let batch = recorder
        .pending_batch()
        .expect("the complete empty checkpoint must be replayable");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("owned native session must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("batch must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receipt batch must be canonical"),
            )
            .expect("empty checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = input_session
        .into_presentation()
        .expect("completed input must enter presentation");
    let mut other = Dockspace::builder("native-session-other", workspace())
        .build()
        .expect("second fixture must build");

    assert!(matches!(
        presentation.mark_surface_unavailable(
            &mut other,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        ),
        Err(egui_dockspace::DockspaceError::NativeSessionLeaseMismatch)
    ));
    presentation
        .mark_surface_unavailable(
            &mut dockspace,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("the exact facade must accept its owned session");
    let commit = presentation
        .finish(&mut dockspace)
        .expect("the exact facade must commit its owned session");
    for output in commit.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn native_renderer_settlement_rejects_another_incarnation_and_returns_the_capability() {
    let mut dockspace = Dockspace::builder("native-settlement-incarnation", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);

    let committed = paint_native_frame(&mut dockspace, &mut recorder, route, 2);
    let (_, mut outputs) = committed.into_parts();
    assert_eq!(outputs.len(), 1);
    assert!(!outputs[0].has_presentation_obligation());
    let output = outputs.pop().expect("one native output must exist");
    assert_eq!(output.native_route(), Some(route));
    let (_, _, settlement) = output.into_parts();
    assert_eq!(settlement.native_route(), Some(route));
    assert_eq!(settlement.presentation_output(), None);

    let stale_result = ExactNativeViewport::new(
        route.native().viewport(),
        route
            .native()
            .incarnation()
            .checked_next()
            .expect("test incarnation advances"),
    );
    let mismatch = settlement
        .settle_native(stale_result, EguiPresentationResult::Presented)
        .expect_err("another native incarnation must not settle A1 output");
    assert_eq!(mismatch.expected(), Some(route.native()));
    assert_eq!(mismatch.submitted(), stale_result);
    let settlement = mismatch.into_settlement();
    assert_eq!(settlement.native_route(), Some(route));
    settlement
        .settle_native(route.native(), EguiPresentationResult::Presented)
        .expect("the exact A1 result may consume the returned capability");
}

#[test]
fn core_backend_publishes_actionable_accesskit_tree() {
    let mut dockspace = Dockspace::builder("native-accesskit-tree", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);
    let context = context();
    context.enable_accesskit();
    let mut presentation_clock = PresentationCaptureClock::default();
    let mut actionable_tab = false;

    recorder
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(1, route.core(), None),
        )
        .expect("platform authority must enter the native stream");

    for sequence in 2..8 {
        let commit = paint_native_frame_with_context(
            &mut dockspace,
            &mut recorder,
            route,
            sequence,
            &context,
        );
        let (_, outputs) = commit.into_parts();
        actionable_tab |= outputs.iter().any(|output| {
            output
                .full_output()
                .platform_output
                .accesskit_update
                .as_ref()
                .is_some_and(|update| {
                    update.nodes.iter().any(|(_, node)| {
                        node.role() == Role::Tab
                            && !node.is_disabled()
                            && node.supports_action(Action::Click)
                    })
                })
        });
        presentation_clock.settle_and_record(&dockspace, &mut recorder, outputs);
        reclaim_backend_prefix(&mut dockspace, &mut recorder);
        if actionable_tab && dockspace.engine().interaction_projection(SURFACE).is_some() {
            break;
        }
    }

    assert!(
        actionable_tab,
        "CoreBackend must publish enabled AccessKit controls while ordered ingress owns actions",
    );
}

#[derive(Clone, Copy)]
enum ReleasePresentationOrder {
    PresentationBeforeRelease,
    ReleaseBeforePresentation,
}

fn run_native_staging_request(order: ReleasePresentationOrder) {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut dockspace = Dockspace::builder("native-staging-output", workspace())
        .policy(policy)
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let mut presentation_clock = PresentationCaptureClock::default();
    let source_route = bootstrap_native_root(&mut dockspace, &mut recorder);

    recorder
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(1, source_route.core(), None),
        )
        .expect("source platform snapshot must be ordered");
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty source checkpoint must be canonical"),
        )
        .expect("source checkpoint must be ordered");
    let batch = recorder
        .pending_batch()
        .expect("source platform batch must freeze");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(2, 0), native_roster(source_route))
        .expect("source platform cycle must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("source platform facts must reduce"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let receipts = unavailable_receiver_receipts(
        input_session
            .pointer_receiver_candidates()
            .expect("empty checkpoint freezes an empty receiver roster"),
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(receipts)
            .expect("source checkpoint receipts must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = input_session
        .into_presentation()
        .expect("source platform cycle enters presentation");
    presentation
        .run_native_surface(
            &mut dockspace,
            source_route.native(),
            &context(),
            native_input(source_route.native().viewport()),
            &mut TestPanes,
        )
        .expect("source surface must paint after platform publication");
    let commit = presentation
        .finish(&mut dockspace)
        .expect("source platform cycle must commit");
    presentation_clock.settle_and_record(&dockspace, &mut recorder, commit.into_parts().1);
    reclaim_backend_prefix(&mut dockspace, &mut recorder);

    let mut sequence = 3;
    while dockspace.engine().interaction_projection(SURFACE).is_none() {
        let commit = paint_native_frame(&mut dockspace, &mut recorder, source_route, sequence);
        presentation_clock.settle_and_record(&dockspace, &mut recorder, commit.into_parts().1);
        sequence += 1;
        assert!(
            sequence < 8,
            "source presentation authority must converge before pointer input"
        );
    }

    let projection = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("source interaction projection must be current");
    let tab = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabBody(tab) if tab.item == ITEM
            )
        })
        .expect("source tab must expose a pointer receiver");
    let tab_id = tab.id();
    let tab_rect = tab.hit().rect();
    let source_point = LogicalPoint::new(
        tab_rect.x() + tab_rect.width() * 0.5,
        tab_rect.y() + tab_rect.height() * 0.5,
    )
    .expect("source tab midpoint must be valid");
    let source_route_fact = DesktopRouteFact::dock(DesktopDockRoute::new(
        source_route.core(),
        projection.authority().coordinate_generation(),
        PhysicalPoint::new(source_point.x(), source_point.y())
            .expect("source desktop point must be valid"),
        source_point,
    ));

    recorder
        .record_pointer_segment(pointer_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: source_route_fact,
            },
            source_route.core(),
            Authority::Known(PointerCaptureOwner::Native(source_route.core())),
        ))
        .expect("source press must be ordered");
    let batch = recorder
        .pending_batch()
        .expect("source press batch freezes");
    let mut press = dockspace
        .begin_native_cycle(
            EguiFrameScheduleKey::new(sequence, 0),
            native_roster(source_route),
        )
        .expect("source press cycle begins");
    assert_eq!(
        press
            .submit_ingress(batch)
            .expect("source press must pause for its receiver"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let candidate = press
        .pointer_receiver_candidates()
        .expect("source press freezes a receiver challenge")
        .candidates()[0]
        .clone();
    let sealed_projection = press
        .receiver_view()
        .interaction_projection(SURFACE)
        .expect("source press retains its presented projection");
    let delivery = PointerReceiverDelivery::new(
        sealed_projection,
        PointerReceiverDeliveryDisposition::Dock(tab_id),
    )
    .expect("source press delivery must match the exact projection");
    let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("source press delivery must be canonical"),
        ),
    )])
    .expect("source press receipt batch must be exact");
    assert_eq!(
        press
            .submit_pointer_receiver_receipts(receipts)
            .expect("source press receiver must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = press
        .into_presentation()
        .expect("source press enters presentation");
    presentation
        .run_native_surface(
            &mut dockspace,
            source_route.native(),
            &context(),
            native_input(source_route.native().viewport()),
            &mut TestPanes,
        )
        .expect("source press frame paints");
    let press = presentation
        .finish(&mut dockspace)
        .expect("source press frame commits");
    assert!(matches!(
        press.host().transition().reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    presentation_clock.settle_and_record(&dockspace, &mut recorder, press.into_parts().1);
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    sequence += 1;

    let outside = PhysicalPoint::new(1_700.0, 900.0).expect("outside point must be valid");
    recorder
        .record_pointer_segment(pointer_journal(
            1,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::Desktop {
                route: outside_all_route(&dockspace, outside),
            },
            source_route.core(),
            Authority::Known(PointerCaptureOwner::Native(source_route.core())),
        ))
        .expect("outside move must be ordered");
    let batch = recorder
        .pending_batch()
        .expect("outside move batch freezes");
    let mut moved = dockspace
        .begin_native_cycle(
            EguiFrameScheduleKey::new(sequence, 0),
            native_roster(source_route),
        )
        .expect("outside move cycle begins");
    assert_eq!(
        moved
            .submit_ingress(batch)
            .expect("outside move pauses for receiver facts"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let receipts = unavailable_receiver_receipts(
        moved
            .pointer_receiver_candidates()
            .expect("outside move freezes a receiver challenge"),
    );
    assert_eq!(
        moved
            .submit_pointer_receiver_receipts(receipts)
            .expect("outside move receiver facts reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = moved
        .into_presentation()
        .expect("outside move enters presentation");
    presentation
        .run_native_surface(
            &mut dockspace,
            source_route.native(),
            &context(),
            native_input(source_route.native().viewport()),
            &mut TestPanes,
        )
        .expect("outside move frame paints its preview");
    let moved = presentation
        .finish(&mut dockspace)
        .expect("outside move frame commits");
    assert!(matches!(
        moved.host().transition().reduced_pointer_edges()[0].interaction_outcomes(),
        [
            InteractionOutcome::DragBegan { .. },
            InteractionOutcome::PreviewUpdated { .. }
        ]
    ));
    let preview_presented = presentation_clock.settle_presented(moved.into_parts().1);
    assert!(
        !preview_presented.is_empty(),
        "the moved preview must create an ordered presentation fact"
    );
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    sequence += 1;

    if matches!(order, ReleasePresentationOrder::PresentationBeforeRelease) {
        PresentationCaptureClock::record(
            &dockspace,
            &mut recorder,
            preview_presented.iter().copied(),
        );
    }
    recorder
        .record_pointer_segment(pointer_journal(
            2,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeLocation::Desktop {
                route: outside_all_route(&dockspace, outside),
            },
            source_route.core(),
            Authority::Known(PointerCaptureOwner::None),
        ))
        .expect("outside release must be ordered");
    if matches!(order, ReleasePresentationOrder::ReleaseBeforePresentation) {
        PresentationCaptureClock::record(&dockspace, &mut recorder, preview_presented);
    }
    let batch = recorder
        .pending_batch()
        .expect("outside release batch freezes");
    let mut released = dockspace
        .begin_native_cycle(
            EguiFrameScheduleKey::new(sequence, 0),
            native_roster(source_route),
        )
        .expect("outside release cycle begins");
    assert_eq!(
        released
            .submit_ingress(batch)
            .expect("outside release pauses for receiver facts"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let receipts = unavailable_receiver_receipts(
        released
            .pointer_receiver_candidates()
            .expect("outside release freezes a receiver challenge"),
    );
    assert_eq!(
        released
            .submit_pointer_receiver_receipts(receipts)
            .expect("outside release receiver facts reduce"),
        BackendIngressProgress::Complete,
    );
    let mut presentation = released
        .into_presentation()
        .expect("outside release enters presentation");
    presentation
        .run_native_surface(
            &mut dockspace,
            source_route.native(),
            &context(),
            native_input(source_route.native().viewport()),
            &mut TestPanes,
        )
        .expect("outside release frame paints");
    let released = presentation
        .finish(&mut dockspace)
        .expect("outside release frame commits");
    let transition = released.host().transition();
    let release_outcomes = transition.reduced_pointer_edges()[0].interaction_outcomes();
    match order {
        ReleasePresentationOrder::PresentationBeforeRelease => assert!(
            matches!(
                release_outcomes,
                [
                    InteractionOutcome::PreviewUpdated { .. },
                    InteractionOutcome::DragDelivered {
                        delivery: InteractionDelivery::NativeRequested(_),
                        ..
                    }
                ]
            ),
            "unexpected release outcomes: {release_outcomes:?}"
        ),
        ReleasePresentationOrder::ReleaseBeforePresentation => assert!(
            matches!(
                release_outcomes,
                [
                    InteractionOutcome::PreviewUpdated { .. },
                    InteractionOutcome::ReleasePending { .. }
                ]
            ),
            "unexpected release outcomes: {release_outcomes:?}"
        ),
    }
    let request = transition
        .interaction_events()
        .iter()
        .find_map(|event| match event.kind() {
            InteractionEventKind::NativePresentationRequested(request) => Some(*request),
            _ => None,
        })
        .expect("both causal orders must eventually create one native saga");
    presentation_clock.settle_and_record(&dockspace, &mut recorder, released.into_parts().1);
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    sequence += 1;

    let target_native = ExactNativeViewport::new(
        ViewportId::from_hash_of("native-staging-target"),
        NativeViewportIncarnation::new(1),
    );
    let target_route = NativeCoreRoute::new(
        target_native,
        request.binding().surface(),
        request.binding(),
    );
    recorder
        .record_platform_snapshot(
            dockspace.engine().version().epoch(),
            platform_snapshot(
                2,
                source_route.core(),
                Some((
                    request.binding(),
                    WindowPresentationState::Hidden,
                    Some(request.effect()),
                )),
            ),
        )
        .expect("hidden target observation must be ordered");
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(3),
                PointerEdgeSequence::new(3),
                Vec::new(),
            )
            .expect("post-release pointer checkpoint must be canonical"),
        )
        .expect("post-release checkpoint must be ordered");
    let batch = recorder
        .pending_batch()
        .expect("hidden staging batch must freeze");
    let mut staging = dockspace
        .begin_native_cycle(
            EguiFrameScheduleKey::new(sequence, 0),
            NativeBindingRoster::new([source_route, target_route], []),
        )
        .expect("hidden staging cycle begins");
    assert_eq!(
        staging
            .submit_ingress(batch)
            .expect("hidden observation must reduce before paint"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let receipts = unavailable_receiver_receipts(
        staging
            .pointer_receiver_candidates()
            .expect("empty staging checkpoint freezes its receiver roster"),
    );
    assert_eq!(
        staging
            .submit_pointer_receiver_receipts(receipts)
            .expect("empty staging checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut staging = staging
        .into_presentation()
        .expect("hidden observation must enter presentation");
    let requests = staging.native_staging_requests().collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].binding(), request.binding());
    staging
        .mark_surface_unavailable(
            &mut dockspace,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("source surface may be omitted from the staging test frame");
    let unavailable = staging
        .finish(&mut dockspace)
        .expect("omitted staging must commit an explicit unavailable disposition");
    assert!(
        unavailable
            .host()
            .transition()
            .presentation_dispositions()
            .iter()
            .any(|outcome| {
                outcome.slot()
                    == HostPresentationSlot::NativeStaging {
                        presentation: requests[0],
                    }
                    && outcome.disposition()
                        == HostPresentationDisposition::Unavailable(
                            HostPresentationUnavailableReason::RetainedResourceUnavailable,
                        )
            })
    );
    assert!(
        unavailable
            .host()
            .transition()
            .presentation_emissions()
            .iter()
            .all(|emission| !matches!(
                emission.output().payload(),
                HostPresentationOutputPayload::NativeStaging { .. }
            ))
    );
    for output in unavailable.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
    reclaim_backend_prefix(&mut dockspace, &mut recorder);
    sequence += 1;

    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(3),
                PointerEdgeSequence::new(3),
                Vec::new(),
            )
            .expect("staging retry pointer checkpoint must be canonical"),
        )
        .expect("staging retry checkpoint must be ordered");
    let batch = recorder
        .pending_batch()
        .expect("staging retry batch must freeze");
    let mut staging = dockspace
        .begin_native_cycle(
            EguiFrameScheduleKey::new(sequence, 0),
            NativeBindingRoster::new([source_route, target_route], []),
        )
        .expect("staging retry cycle begins");
    assert_eq!(
        staging
            .submit_ingress(batch)
            .expect("staging retry ingress must reduce"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    let receipts = unavailable_receiver_receipts(
        staging
            .pointer_receiver_candidates()
            .expect("staging retry freezes its receiver roster"),
    );
    assert_eq!(
        staging
            .submit_pointer_receiver_receipts(receipts)
            .expect("staging retry receipts must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut staging = staging
        .into_presentation()
        .expect("staging retry enters presentation");
    let requests = staging.native_staging_requests().collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].binding(), request.binding());
    staging
        .mark_surface_unavailable(
            &mut dockspace,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("source surface may remain omitted during staging retry");
    let painted = staging
        .run_native_staging(
            &mut dockspace,
            target_native,
            &context(),
            native_input(target_native.viewport()),
        )
        .expect("exact hidden target must paint its non-interactive staging fill");
    assert_eq!(painted, requests[0]);
    let staging = staging
        .finish(&mut dockspace)
        .expect("staging output and source omission must publish atomically");
    let emissions = staging
        .host()
        .transition()
        .presentation_emissions()
        .iter()
        .filter(|emission| {
            matches!(
                emission.output().payload(),
                HostPresentationOutputPayload::NativeStaging { presentation }
                    if presentation == painted
            )
        })
        .count();
    assert_eq!(emissions, 1);
    let (_, mut outputs) = staging.into_parts();
    assert_eq!(outputs.len(), 1);
    let output = outputs
        .pop()
        .expect("one staging FullOutput must be retained");
    assert_eq!(output.native_route(), Some(target_route));
    assert!(output.has_presentation_obligation());
    assert!(matches!(
        output
            .presentation_output()
            .expect("staging output must carry a core obligation")
            .payload(),
        HostPresentationOutputPayload::NativeStaging { presentation }
            if presentation == painted
    ));
    output
        .into_parts()
        .2
        .settle_native(target_native, EguiPresentationResult::Presented)
        .expect("the exact target lifetime settles its staging output");
}

#[test]
fn presented_preview_before_release_delivers_in_the_release_edge() {
    run_native_staging_request(ReleasePresentationOrder::PresentationBeforeRelease);
}

#[test]
fn release_before_presented_preview_waits_then_delivers_in_the_same_batch() {
    run_native_staging_request(ReleasePresentationOrder::ReleaseBeforePresentation);
}

#[test]
fn backend_batch_retries_after_aborted_presentation_and_commits_with_paint() {
    let mut dockspace = Dockspace::builder("backend-frame", workspace())
        .build()
        .expect("fixture must build");
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("empty checkpoint must enter the joined ingress stream");
    let committed = dockspace.engine().backend_ingress_committed_through();
    let version_before = dockspace.engine().version();
    let tick_before = dockspace.engine().last_reducer_tick();
    let batch = recorder
        .batch_after(committed)
        .expect("the captured suffix must be retryable");

    let mut aborted = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(2, 0), native_roster(route))
        .expect("backend input phase must begin");
    assert_eq!(
        aborted
            .submit_ingress(batch.clone())
            .expect("batch must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert!(
        aborted
            .pointer_receiver_candidates()
            .is_some_and(|roster| roster.candidates().is_empty())
    );
    assert_eq!(
        aborted
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receiver receipt batch must be canonical"),
            )
            .expect("empty checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut aborted = aborted
        .into_presentation()
        .expect("completed input must enter presentation");
    aborted
        .mark_surface_unavailable(
            &mut dockspace,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("the aborted frame still prepares a complete surface roster");
    let prepared = aborted
        .prepare_finish(&mut dockspace)
        .expect("the outer frame must prepare without publishing");
    assert_eq!(
        dockspace.engine().backend_ingress_committed_through(),
        committed,
        "preparing must not publish the ingress watermark",
    );
    assert_eq!(dockspace.engine().version(), version_before);
    assert_eq!(dockspace.engine().last_reducer_tick(), tick_before);
    prepared.abort(&mut dockspace);
    assert_eq!(
        dockspace.engine().backend_ingress_committed_through(),
        committed,
        "aborting after host seal failure must preserve the live core watermark",
    );
    assert_eq!(dockspace.engine().version(), version_before);
    assert_eq!(dockspace.engine().last_reducer_tick(), tick_before);

    let mut input_frame = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(2, 0), native_roster(route))
        .expect("the same host key remains retryable after rollback");
    assert_eq!(
        input_frame
            .submit_ingress(batch)
            .expect("the identical batch must remain valid"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receiver receipt batch must be canonical"),
            )
            .expect("retry checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );

    let mut presentation = input_frame
        .into_presentation()
        .expect("completed input must freeze the post-input roster");
    let mut panes = TestPanes;
    presentation
        .run_native_surface(
            &mut dockspace,
            route.native(),
            &context(),
            input(),
            &mut panes,
        )
        .expect("post-input surface must paint and bind its full output");
    let prepared = presentation
        .prepare_finish(&mut dockspace)
        .expect("painted backend frame must prepare atomically");
    assert_eq!(
        dockspace.engine().backend_ingress_committed_through(),
        committed,
        "a prepared frame remains invisible until the host commits it",
    );
    let commit = prepared
        .commit(&mut dockspace)
        .expect("the sealed backend frame must publish exactly once");
    assert_eq!(
        dockspace.engine().backend_ingress_committed_through(),
        recorder.recorded_through(),
    );
    let committed = dockspace
        .engine()
        .backend_ingress_commit_watermark()
        .expect("the active provider must expose its exact committed prefix");
    let receipt = recorder
        .retire_committed_prefix(committed)
        .expect("the recorder may reclaim only the core-proven prefix");
    if let Some(mut receipt) = receipt {
        dockspace
            .adapter_settle_backend_ingress_prefix_retirement(&mut receipt)
            .expect("the reclaimed prefix must settle against the committed frame");
    }
    assert_eq!(recorder.retained_record_count(), 0);
    assert!(
        recorder
            .pending_batch()
            .expect("the post-retirement empty suffix must remain canonical")
            .is_empty()
    );
    let (_, outputs) = commit.into_parts();
    assert!(
        outputs
            .iter()
            .all(|output| !output.has_presentation_obligation()),
        "a prepared-only first paint must not be relabelled as the frozen bootstrap output",
    );
    for output in outputs {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn backend_terminal_configuration_commits_policy_and_style_atomically() {
    let mut dockspace = Dockspace::builder("backend-configuration", workspace())
        .build()
        .expect("fixture must build");
    let original_style = dockspace.style().clone();
    let mut replacement_style = original_style.clone();
    replacement_style.tab_bar_height += 2.0;
    replacement_style.tab_active_fill = Color32::from_rgb(12, 34, 56);
    let mut replacement_policy = DockPolicy::default();
    replacement_policy.set_allow_contained_floating(false);

    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    let route = bootstrap_native_root(&mut dockspace, &mut recorder);
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("empty checkpoint must enter the joined ingress stream");
    let batch = recorder
        .pending_batch()
        .expect("the configuration frame ingress must be replayable");
    let mut input_session = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(2, 0), native_roster(route))
        .expect("native input session must begin");
    assert_eq!(
        input_session
            .submit_ingress(batch)
            .expect("backend prefix must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        input_session
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receipt batch must be canonical"),
            )
            .expect("empty checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );

    let mut configuration = input_session
        .into_configuration()
        .expect("completed ingress must enter terminal configuration");
    configuration
        .set_policy(&dockspace, replacement_policy)
        .expect("policy must stage against the frame candidate");
    configuration
        .set_style(&dockspace, replacement_style.clone())
        .expect("style geometry and renderer sidecar must stage together");
    assert_eq!(dockspace.style(), &original_style);
    assert!(dockspace.engine().policy().allows_contained_floating());

    let mut presentation = configuration
        .into_presentation()
        .expect("terminal configuration must freeze presentation");
    let mut panes = TestPanes;
    presentation
        .run_native_surface(
            &mut dockspace,
            route.native(),
            &context(),
            input(),
            &mut panes,
        )
        .expect("the current old-style pass may paint non-authoritatively");
    let commit = presentation
        .finish(&mut dockspace)
        .expect("core and renderer configuration must publish atomically");

    assert_eq!(dockspace.style(), &replacement_style);
    assert!(!dockspace.engine().policy().allows_contained_floating());
    assert!(
        commit
            .host()
            .transition()
            .reduced_inputs()
            .iter()
            .any(|input| {
                matches!(
                    input.outcome(),
                    dockspace::transition::InputOutcome::PolicyReplaced { changed: true, .. }
                )
            })
    );
    assert!(
        commit
            .outputs()
            .all(|output| !output.has_presentation_obligation()),
        "the old-style paint must not become authoritative after terminal configuration"
    );
    for output in commit.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn dropped_backend_configuration_rolls_back_and_replays_the_same_ingress() {
    let mut dockspace = Dockspace::builder("backend-configuration-rollback", workspace())
        .build()
        .expect("fixture must build");
    let original_style = dockspace.style().clone();
    let mut replacement_style = original_style.clone();
    replacement_style.tab_active_fill = Color32::from_rgb(90, 80, 70);
    let mut recorder = dockspace
        .create_backend_ingress_provider(PointerEdgeSequence::new(0))
        .expect("joined backend provider must enroll");
    recorder
        .record_pointer_segment(
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(0),
                PointerEdgeSequence::new(0),
                Vec::new(),
            )
            .expect("empty pointer checkpoint must be canonical"),
        )
        .expect("empty checkpoint must enter the joined ingress stream");
    let committed = dockspace.engine().backend_ingress_committed_through();
    let batch = recorder
        .batch_after(committed)
        .expect("the exact ingress suffix must be replayable");

    let mut aborted = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("first native session must begin");
    assert_eq!(
        aborted
            .submit_ingress(batch.clone())
            .expect("backend prefix must validate"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        aborted
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receipt batch must be canonical"),
            )
            .expect("empty checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut configuration = aborted
        .into_configuration()
        .expect("completed ingress must enter terminal configuration");
    configuration
        .set_style(&dockspace, replacement_style.clone())
        .expect("style must stage speculatively");
    drop(configuration);

    assert_eq!(dockspace.style(), &original_style);
    assert_eq!(
        dockspace.engine().backend_ingress_committed_through(),
        committed,
    );

    let mut retry = dockspace
        .begin_native_cycle(EguiFrameScheduleKey::new(1, 0), empty_native_bindings())
        .expect("the abandoned session must leave the exact host key retryable");
    assert_eq!(
        retry
            .submit_ingress(batch)
            .expect("the identical backend suffix must replay"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        retry
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([])
                    .expect("empty receipt batch must be canonical"),
            )
            .expect("retry checkpoint must reduce"),
        BackendIngressProgress::Complete,
    );
    let mut configuration = retry
        .into_configuration()
        .expect("retry must enter terminal configuration");
    configuration
        .set_style(&dockspace, replacement_style.clone())
        .expect("retry style must stage");
    let mut presentation = configuration
        .into_presentation()
        .expect("retry configuration must enter presentation");
    presentation
        .mark_surface_unavailable(
            &mut dockspace,
            SURFACE,
            MeasurementUnavailableReason::Deferred,
        )
        .expect("retry may explicitly omit paint");
    let commit = presentation
        .finish(&mut dockspace)
        .expect("retry must publish atomically");
    assert_eq!(dockspace.style(), &replacement_style);
    for output in commit.into_parts().1 {
        output.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}
