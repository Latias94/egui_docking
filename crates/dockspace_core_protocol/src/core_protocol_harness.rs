use std::collections::{BTreeMap, BTreeSet};

use dockspace::command::{DockTarget, Edge, WorkspaceCommand};
use dockspace::drop_target::DropTargetId;
use dockspace::effect::{
    DispatchFailureReason, EffectId, EffectIndeterminateReason, EffectInvalidation, EffectPhase,
    EffectUnsupportedReason, NativeCloseResolution, PlatformEffect,
};
use dockspace::engine::{
    CoreHostFrame, CoreHostFramePrelude, DockEngine, EngineInput, HostFrameView,
    HostPresentationDisposition, HostPresentationSlot, HostPresentationUnavailableReason,
    PreparedSurfaceContribution, SurfaceContributionToken,
};
use dockspace::event::ReductionCause;
use dockspace::frame::PanelFocus;
use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder,
};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::{
    EscapeDelivery, InteractionCancelReason, InteractionEventKind, InteractionOutcome,
    InteractionStatus, PreviewResolutionStatus, PreviewVisual, ScrollReductionOutcome,
    ScrollSuppressionReason, ScrollTerminationReason, WorkspaceDeliveryKind,
};
use dockspace::platform::{
    CapabilityRosterObservation, InputEffectAcknowledgement, ObservedWindow, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use dockspace::pointer_journal::{
    DesktopDockRoute, DesktopRouteFact, DesktopWorkAreaRoute, FiniteScrollVector,
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerEventDeliveryOwner, PointerInputLease, PointerProviderScope,
    PointerStreamCancelReason, ScrollCancelReason, ScrollDeliveryEndpoint, ScrollDelta,
    ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase, ScrollSequenceToken,
    SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation, PointerReceiverProbeReceipt,
    PointerReceiverReceiptBatch, PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionKind, PresentationPointerLane};
use dockspace::presentation_observation::{
    HostFrameKey, HostPresentationCaptureGeneration, HostPresentationEndpoint,
    HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationObservationOutcome, HostPresentationObservationRejection,
    HostPresentationOutput, HostPresentationProgress, HostPresentationStreamId,
    HostPresentationStreamObservation, PresentationHostLease, PresentedNativeStagingPresentation,
    PresentedSurfaceAuthority,
};
use dockspace::scene::TabBarSceneId;
use dockspace::scene_manifest::{
    Measurement, MeasurementUnavailableReason, SurfaceMeasurements, TabIntrinsic,
    TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement,
    TabStripMetrics,
};
use dockspace::tab_strip::TabStripControlId;
use dockspace::transition::{
    EngineTransition, InputOutcome, SurfaceContributionOutcome, SurfaceSceneStateKind,
    WorkspaceVersion,
};
use dockspace::viewport::{
    CapabilityObservationGeneration, CoordinateGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryObservationGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaObservationGeneration, WorkAreaToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, FocusValueChange, GlobalFocusedWindow,
    PaneFocusDisposition, PaneFocusIntent, PaneFocusIntentSource, PanelFocusRecord,
    PendingPlatformFocus, PendingViewportActivation, PlatformFocusEvidence,
    RecordedObserveOnlyActivation, SurfaceFocusState, ViewportActivationCause,
    ViewportActivationRequest, unknown_focus_observation,
};
use dockspace::{ClosePlanTarget, PlatformObservationLease, SurfaceCloseDisposition};

use crate::core_protocol_trace::{
    AuthorityUnavailableReasonSpec, AxisSpec, BoundaryId, CanonicalContained, CanonicalRoot,
    CanonicalSurface, CanonicalWorkspace, CoordinateGenerationIngress, CoreProtocolTrace,
    CoreProtocolTraceBoundary, CoreProtocolTraceCommand, CoreProtocolTraceError,
    CoreProtocolTraceInput, CoreProtocolTraceSuite, DesktopRouteIngress, EdgeSpec,
    ExpectedCanonicalSnapshot, ExpectedCloseTarget, ExpectedInteractionCancelReason,
    ExpectedInteractionOutcome, ExpectedInteractionState, ExpectedPointerEdge,
    ExpectedPointerEdgeCause, ExpectedPresentationObservationOutcome,
    ExpectedPresentationObservationRejection, ExpectedPreviewResolutionStatus,
    ExpectedReducedInteractionOutcome, ExpectedRetiredPresentation, ExpectedScrollBlocker,
    ExpectedScrollReceiver, ExpectedScrollSuppressionReason, ExpectedScrollTerminationReason,
    ExpectedSurfaceCloseDisposition, ExpectedSurfaceContributionOutcome, ExpectedTabStripControl,
    ExpectedTransition, FloatingPresentationKey, HostFrameEvent, IngressRef, InitialWorkspace,
    ItemCount, ItemKey, ItemLocation, LifecycleIngress, MeasurementUnavailableReasonSpec,
    NodeFixture, NodeLocation, ObservedWindowFixture, PlatformCapabilitiesFixture,
    PlatformObservationIngress, PlatformRequirementSpec, PlatformSnapshotFixture,
    PointAuthorityIngress, PointFixture, PointerCaptureIngress, PointerDeliveryIngress,
    PointerDropTargetIngress, PointerEdgeIngress, PointerEdgeKindSpec, PointerEventDeliveryIngress,
    PointerHoverIngress, PointerLocationIngress, PointerProviderIngress,
    PointerProviderScopeIngress, PointerReceiverIngress, PointerReceiverUnknownReasonSpec,
    PointerStreamCancelReasonSpec, PresentationDispositionIngress, PresentationDispositionSpec,
    PresentationEndpointRef, PresentationObservationIngress, PresentationStreamObservationIngress,
    PresentationStreamObservationSpec, PresentationStreamRef, PresentationUnavailableReasonSpec,
    ProducerId, RectFixture, ReducerTick, RetiredPresentationIngress, RootKey, RootOwner,
    ScrollCancelReasonSpec, ScrollDeliveryEndpointIngress, ScrollDeltaIngress, ScrollEdgeIngress,
    ScrollModifiersAuthorityIngress, ScrollMomentumAuthorityIngress, ScrollMomentumSpec,
    ScrollPhaseSpec, SurfaceContributionIngress, SurfaceKey, SurfaceMeasurementIngress,
    VersionExpectation, ViewportRoleSpec, WindowInputStateSpec, WindowPresentationStateSpec,
    WorkAreaRosterObservationFixture, validate_core_protocol_trace_suite,
    validate_provider_boundary_contract,
};
use crate::core_protocol_trace::{
    EffectKey, ExpectedAcknowledgedEffectAuthority, ExpectedDispatchFailureReason,
    ExpectedEffectIndeterminateReason, ExpectedEffectInvalidation, ExpectedEffectPhase,
    ExpectedEffectRef, ExpectedEffectUnsupportedReason, ExpectedFocusDelta,
    ExpectedFocusEffectChange, ExpectedFocusValueChange, ExpectedGlobalFocusAuthority,
    ExpectedGlobalFocusObservation, ExpectedInteractionEvent, ExpectedInteractionEventKind,
    ExpectedPaneFocusDisposition, ExpectedPaneFocusIntent, ExpectedPaneFocusIntentSource,
    ExpectedPaneFocusObservation, ExpectedPanelFocus, ExpectedPanelFocusRecord,
    ExpectedPendingPlatformFocus, ExpectedPendingViewportActivation, ExpectedPlatformEffect,
    ExpectedPlatformEffectEmission, ExpectedPlatformFocusEvidence, ExpectedPreviewVisual,
    ExpectedRecordedObserveOnlyActivation, ExpectedReductionCause, ExpectedSurfaceFocusChange,
    ExpectedSurfaceFocusState, ExpectedSurfaceSceneDelta, ExpectedSurfaceSceneState,
    ExpectedSurfaceSceneStateKind, ExpectedViewportActivationCause,
    ExpectedViewportActivationRequest, ExpectedWorkspaceDeliveryKind, NativeCloseResolutionSpec,
};

const PRODUCER_HASH_DOMAIN: &[u8] = b"dockspace.core-protocol-trace/1\0producer\0";
const FNV_1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_1A_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Default)]
struct ProducerSources {
    by_producer: BTreeMap<ProducerId, StableInputSourceId>,
    by_source: BTreeMap<StableInputSourceId, ProducerId>,
}

impl ProducerSources {
    fn from_trace(trace: &CoreProtocolTrace) -> Result<Self, CoreProtocolTraceError> {
        let producers = trace
            .boundaries
            .iter()
            .flat_map(|boundary| boundary.events.iter())
            .filter_map(|event| match event {
                HostFrameEvent::SemanticInput { producer, .. } => Some(producer.clone()),
                HostFrameEvent::PointerJournal { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let mut sources = Self::default();
        for producer in producers {
            sources.register(producer)?;
        }
        Ok(sources)
    }

    fn register(&mut self, producer: ProducerId) -> Result<(), CoreProtocolTraceError> {
        let source = stable_source_for_producer(&producer);
        if let Some(previous) = self.by_source.insert(source, producer.clone())
            && previous != producer
        {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "producer `{}` collides with `{}` at stable input source {}",
                producer.as_str(),
                previous.as_str(),
                source.get()
            )));
        }
        self.by_producer.insert(producer, source);
        Ok(())
    }

    fn source(&self, producer: &ProducerId) -> Option<StableInputSourceId> {
        self.by_producer.get(producer).copied()
    }

    fn ingress(
        &self,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
    ) -> Option<IngressRef> {
        self.by_source
            .get(&source)
            .cloned()
            .map(|producer| IngressRef {
                producer,
                source_sequence: source_sequence.get(),
            })
    }
}

fn stable_source_for_producer(producer: &ProducerId) -> StableInputSourceId {
    let mut hash = FNV_1A_OFFSET_BASIS;
    for byte in PRODUCER_HASH_DOMAIN
        .iter()
        .chain(producer.as_str().as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_1A_PRIME);
    }
    StableInputSourceId::new(hash)
}

/// Summary produced after every trace and boundary has passed exact comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreProtocolReplayReport {
    traces: usize,
    boundaries: usize,
}

impl CoreProtocolReplayReport {
    #[must_use]
    pub const fn traces(self) -> usize {
        self.traces
    }

    #[must_use]
    pub const fn boundaries(self) -> usize {
        self.boundaries
    }
}

#[derive(Debug)]
enum CompiledSurfaceContribution {
    Prepared(PreparedSurfaceContribution),
    Retained(SurfaceContributionToken),
}

#[derive(Debug)]
struct PresentationStreamSidecar {
    trace_ref: PresentationStreamRef,
    outputs: Vec<HostPresentationOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PresentationSlotKey {
    Surface(SurfaceId),
    NativeStaging(SurfaceId),
}

impl PresentationSlotKey {
    const fn surface(self) -> SurfaceId {
        match self {
            Self::Surface(surface) | Self::NativeStaging(surface) => surface,
        }
    }
}

/// UI-free harness that translates renderer-neutral fixture facts into core inputs.
///
/// It owns all opaque identities: bindings, provider leases, presentation streams,
/// candidate IDs, receiver receipts, and output tickets stay inside this type.
pub struct CoreProtocolHarness {
    engine: DockEngine,
    platform_provider: PlatformObservationLease,
    presentation_host: PresentationHostLease,
    producer_sources: ProducerSources,
    pointer_provider: Option<PointerInputLease>,
    bindings: BTreeMap<SurfaceKey, ViewportBinding>,
    binding_history: Vec<(ViewportBinding, SurfaceKey)>,
    effect_refs: BTreeMap<EffectId, ExpectedEffectRef>,
    last_effect_ref: u64,
    presentation_streams: BTreeMap<HostPresentationStreamId, PresentationStreamSidecar>,
    last_presentation_stream_sequence: BTreeMap<SurfaceKey, u64>,
}

impl CoreProtocolHarness {
    /// Constructs a real core engine from renderer-neutral initial state.
    ///
    /// # Errors
    ///
    /// Returns [`CoreProtocolTraceError`] when fixture topology or core invariants are invalid.
    pub fn new(initial: &InitialWorkspace) -> Result<Self, CoreProtocolTraceError> {
        let workspace = build_workspace(initial)?;
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(initial.policy.allow_native_surfaces);
        let mut engine = DockEngine::new(workspace, policy).map_err(|error| {
            CoreProtocolTraceError::Replay(format!("cannot construct DockEngine: {error}"))
        })?;
        let platform_provider = engine.create_platform_provider().map_err(|error| {
            CoreProtocolTraceError::Replay(format!(
                "cannot create core platform provider lease: {error}"
            ))
        })?;
        let presentation_host = engine.create_presentation_host().map_err(|error| {
            CoreProtocolTraceError::Replay(format!(
                "cannot create core presentation host lease: {error}"
            ))
        })?;
        Ok(Self {
            engine,
            platform_provider,
            presentation_host,
            producer_sources: ProducerSources::default(),
            pointer_provider: None,
            bindings: BTreeMap::new(),
            binding_history: Vec::new(),
            effect_refs: BTreeMap::new(),
            last_effect_ref: 0,
            presentation_streams: BTreeMap::new(),
            last_presentation_stream_sequence: BTreeMap::new(),
        })
    }

    fn from_trace(trace: &CoreProtocolTrace) -> Result<Self, CoreProtocolTraceError> {
        let mut harness = Self::new(&trace.initial_workspace)?;
        harness.producer_sources = ProducerSources::from_trace(trace)?;
        Ok(harness)
    }

    /// Returns the real engine owned by this harness.
    #[must_use]
    pub const fn engine(&self) -> &DockEngine {
        &self.engine
    }

    /// Replays one typed boundary against the harness's current state.
    ///
    /// This stepwise form is intended for protocol rollback tests which must
    /// observe a rejected boundary and then retry the same provider watermark.
    ///
    /// # Errors
    ///
    /// Returns [`CoreProtocolTraceError`] when the boundary cannot be reduced or
    /// its complete transition differs from the declared expectation.
    pub fn replay_boundary(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
    ) -> Result<(), CoreProtocolTraceError> {
        validate_provider_boundary_contract(trace, boundary)?;
        let provider_transition = self.apply_provider_operation(trace, boundary)?;
        let actual = if let Some(transition) = provider_transition {
            let compiled_pointer_edges = BTreeMap::new();
            self.observe_transition(trace, boundary, &[], &compiled_pointer_edges, &transition)?
        } else {
            self.reduce_boundary(trace, boundary)?
        };
        if actual != boundary.expected {
            return Err(replay_error(
                trace,
                &boundary.id,
                format!(
                    "transition mismatch\nexpected: {:#?}\nactual: {actual:#?}",
                    boundary.expected
                ),
            ));
        }
        Ok(())
    }

    fn apply_provider_operation(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
    ) -> Result<Option<EngineTransition>, CoreProtocolTraceError> {
        let Some(operation) = &boundary.provider else {
            return Ok(None);
        };
        match operation {
            PointerProviderIngress::Activate {
                scope,
                committed_through,
            } => {
                let scope = self.compile_pointer_provider_scope(*scope)?;
                let lease = self
                    .engine
                    .create_pointer_provider(scope, PointerEdgeSequence::new(*committed_through))
                    .map_err(|error| {
                        replay_error(
                            trace,
                            &boundary.id,
                            format!("cannot activate pointer provider: {error}"),
                        )
                    })?;
                self.pointer_provider = Some(lease);
                Ok(None)
            }
            PointerProviderIngress::Retire {} => {
                let lease = self.pointer_provider.ok_or_else(|| {
                    replay_error(
                        trace,
                        &boundary.id,
                        "cannot retire missing pointer provider",
                    )
                })?;
                let transition = self
                    .engine
                    .retire_pointer_provider(lease)
                    .map_err(|error| {
                        replay_error(
                            trace,
                            &boundary.id,
                            format!("cannot retire pointer provider: {error}"),
                        )
                    })?;
                self.pointer_provider = None;
                Ok(Some(transition))
            }
        }
    }

    fn reduce_boundary(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
    ) -> Result<ExpectedTransition, CoreProtocolTraceError> {
        let mut prelude = self
            .engine
            .begin_host_frame(self.presentation_host)
            .map_err(|error| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("cannot begin core host frame: {error}"),
                )
            })?;
        self.submit_presentation_observation(trace, boundary, &mut prelude)?;
        let mut frame = prelude.seal(&self.engine).map_err(|error| {
            replay_error(
                trace,
                &boundary.id,
                format!("cannot seal core host frame: {error}"),
            )
        })?;

        let event_causal_ordinals = Self::event_causal_ordinals(trace, boundary)?;
        let mut compiled_pointer_edges = BTreeMap::new();
        for (event_index, event) in boundary.events.iter().enumerate() {
            match event {
                HostFrameEvent::PointerJournal {
                    previous,
                    through,
                    edges,
                } => {
                    let provider = self.pointer_provider.ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "journal has no active pointer provider",
                        )
                    })?;
                    let journal = self
                        .compile_pointer_journal(frame.view(), *previous, *through, edges)
                        .map_err(|error| {
                            replay_error(
                                trace,
                                &boundary.id,
                                format!("cannot compile pointer journal: {error}"),
                            )
                        })?;
                    if journal.edges().is_empty() {
                        self.submit_pointer_segment(
                            trace, boundary, &mut frame, provider, journal, edges,
                        )?;
                        continue;
                    }

                    let mut segment_previous = journal.previous();
                    for (edge_index, (fixture, edge)) in
                        edges.iter().zip(journal.edges()).enumerate()
                    {
                        let edge_ordinal = u64::try_from(edge_index)
                            .ok()
                            .and_then(|offset| {
                                event_causal_ordinals[event_index].checked_add(offset)
                            })
                            .ok_or_else(|| {
                                replay_error(
                                    trace,
                                    &boundary.id,
                                    "pointer edge causal ordinal is exhausted",
                                )
                            })?;
                        if compiled_pointer_edges
                            .insert(
                                edge.sequence().get(),
                                (event_index, edge_ordinal, edge.clone()),
                            )
                            .is_some()
                        {
                            return Err(replay_error(
                                trace,
                                &boundary.id,
                                format!(
                                    "pointer sequence {} appears in more than one ordered event",
                                    edge.sequence().get()
                                ),
                            ));
                        }
                        let segment = PointerEdgeJournal::new(
                            segment_previous,
                            edge.sequence(),
                            vec![edge.clone()],
                        )
                        .map_err(|error| {
                            replay_error(
                                trace,
                                &boundary.id,
                                format!(
                                    "cannot split pointer journal into an edge segment: {error}"
                                ),
                            )
                        })?;
                        self.submit_pointer_segment(
                            trace,
                            boundary,
                            &mut frame,
                            provider,
                            segment,
                            std::slice::from_ref(fixture),
                        )?;
                        segment_previous = edge.sequence();
                    }
                    debug_assert_eq!(segment_previous, journal.through());
                }
                HostFrameEvent::SemanticInput {
                    producer,
                    source_sequence,
                    input,
                } => {
                    let input = self.compile_input(frame.view(), input)?;
                    let source = self.producer_sources.source(producer).ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            format!("unknown producer `{}`", producer.as_str()),
                        )
                    })?;
                    frame
                        .append_input(source, SourceSequence::new(*source_sequence), input)
                        .map_err(|error| {
                            replay_error(
                                trace,
                                &boundary.id,
                                format!("cannot append host-frame input: {error}"),
                            )
                        })?;
                }
            }
        }

        let frozen_surfaces = frame.surfaces().collect::<BTreeSet<_>>();
        let mut submitted_surfaces = BTreeSet::new();
        let mut retained_contributions = BTreeMap::new();
        for contribution in &boundary.surface_contributions {
            match self.compile_surface_contribution(frame.view(), contribution)? {
                CompiledSurfaceContribution::Prepared(contribution) => {
                    submitted_surfaces.insert(contribution.surface());
                    frame
                        .push_surface_contribution(contribution)
                        .map_err(|error| {
                            replay_error(
                                trace,
                                &boundary.id,
                                format!("cannot attach prepared surface contribution: {error}"),
                            )
                        })?;
                }
                CompiledSurfaceContribution::Retained(token) => {
                    submitted_surfaces.insert(token.surface());
                    if retained_contributions
                        .insert(token.surface(), token)
                        .is_some()
                    {
                        return Err(replay_error(
                            trace,
                            &boundary.id,
                            format!(
                                "trace repeats retained contribution for surface {}",
                                contribution.surface.0
                            ),
                        ));
                    }
                }
            }
        }

        let mut frame = frame.into_presentation().map_err(|error| {
            replay_error(
                trace,
                &boundary.id,
                format!("cannot close host-frame input prefix: {error}"),
            )
        })?;
        let issued_obligations = frame.take_presentation_obligations().map_err(|error| {
            replay_error(
                trace,
                &boundary.id,
                format!("cannot issue physical presentation obligations: {error}"),
            )
        })?;
        let mut presentation_obligations = BTreeMap::new();
        for obligation in issued_obligations {
            let slot = presentation_slot_key(obligation.slot());
            if presentation_obligations.insert(slot, obligation).is_some() {
                return Err(replay_error(
                    trace,
                    &boundary.id,
                    format!("core issued duplicate physical presentation slot {slot:?}"),
                ));
            }
        }

        let mut presentation_dispositions = BTreeMap::new();
        for ingress in &boundary.presentation_dispositions {
            let slot = presentation_ingress_slot(*ingress);
            if presentation_dispositions
                .insert(slot, ingress.disposition())
                .is_some()
            {
                return Err(replay_error(
                    trace,
                    &boundary.id,
                    format!("trace repeats physical presentation slot {slot:?}"),
                ));
            }
        }
        validate_exact_presentation_roster(
            trace,
            boundary,
            presentation_obligations.keys().copied().collect(),
            presentation_dispositions.keys().copied().collect(),
        )?;

        for (slot, disposition) in presentation_dispositions {
            let obligation = presentation_obligations.remove(&slot).ok_or_else(|| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("trace names unavailable physical presentation slot {slot:?}"),
                )
            })?;
            let interaction = frame
                .view()
                .presentation_interaction(slot.surface())
                .unwrap_or_default();
            if let Some(token) = retained_contributions.remove(&slot.surface()) {
                if !matches!(slot, PresentationSlotKey::Surface(_)) {
                    return Err(replay_error(
                        trace,
                        &boundary.id,
                        format!(
                            "retained surface contribution cannot satisfy staging slot {slot:?}"
                        ),
                    ));
                }
                if !matches!(disposition, PresentationDispositionSpec::Painted {}) {
                    return Err(replay_error(
                        trace,
                        &boundary.id,
                        format!(
                            "retained contribution for surface {} requires an explicit painted disposition",
                            slot.surface().get()
                        ),
                    ));
                }
                frame
                    .record_painted_surface_contribution(obligation, token, interaction)
                    .map_err(|error| {
                        replay_error(
                            trace,
                            &boundary.id,
                            format!("cannot retain surface contribution: {error}"),
                        )
                    })?;
            } else {
                let disposition = compile_presentation_disposition(disposition, interaction);
                frame
                    .resolve_presentation_obligation(obligation, disposition)
                    .map_err(|error| {
                        replay_error(
                            trace,
                            &boundary.id,
                            format!("cannot resolve presentation disposition: {error}"),
                        )
                    })?;
            }
        }
        if let Some((surface, _)) = retained_contributions.first_key_value() {
            return Err(replay_error(
                trace,
                &boundary.id,
                format!(
                    "retained contribution for surface {} has no matching presentation disposition",
                    surface.get()
                ),
            ));
        }
        if let Some((slot, _)) = presentation_obligations.first_key_value() {
            return Err(replay_error(
                trace,
                &boundary.id,
                format!("trace omitted physical presentation slot {slot:?}"),
            ));
        }
        if submitted_surfaces != frozen_surfaces {
            return Err(replay_error(
                trace,
                &boundary.id,
                format!(
                    "trace did not submit complete frozen surface roster; expected {frozen_surfaces:?}, submitted {submitted_surfaces:?}"
                ),
            ));
        }

        let transition = frame.finish(&mut self.engine).map_err(|error| {
            replay_error(trace, &boundary.id, format!("reducer failed: {error}"))
        })?;
        self.capture_viewport_bindings(boundary)?;
        self.reconcile_presentation_sidecar(&transition)?;
        self.observe_transition(
            trace,
            boundary,
            &event_causal_ordinals,
            &compiled_pointer_edges,
            &transition,
        )
    }

    fn event_causal_ordinals(
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
    ) -> Result<Vec<u64>, CoreProtocolTraceError> {
        let mut next = 0_u64;
        boundary
            .events
            .iter()
            .map(|event| {
                let ordinal = next;
                let width = match event {
                    HostFrameEvent::PointerJournal { edges, .. } => edges.len(),
                    HostFrameEvent::SemanticInput { .. } => 1,
                };
                let width = u64::try_from(width).map_err(|_| {
                    replay_error(
                        trace,
                        &boundary.id,
                        "host-frame event width does not fit the core causal ordinal",
                    )
                })?;
                next = next.checked_add(width).ok_or_else(|| {
                    replay_error(
                        trace,
                        &boundary.id,
                        "host-frame causal ordinal is exhausted",
                    )
                })?;
                Ok(ordinal)
            })
            .collect()
    }

    /// Submits exactly one core pointer protocol segment and answers its
    /// receiver challenge before another segment can be observed. The trace
    /// may group contiguous edges into one host event, but each answer must be
    /// bound to the receiver state produced by the immediately preceding
    /// reduction prefix.
    fn submit_pointer_segment(
        &self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        frame: &mut CoreHostFrame,
        provider: PointerInputLease,
        journal: PointerEdgeJournal,
        edges: &[PointerEdgeIngress],
    ) -> Result<(), CoreProtocolTraceError> {
        frame
            .submit_pointer_journal(provider, journal)
            .map_err(|error| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("cannot submit pointer journal segment: {error}"),
                )
            })?;
        let receipts = self
            .compile_pointer_receiver_receipts(frame, edges)
            .map_err(|error| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("cannot compile pointer receiver receipts: {error}"),
                )
            })?;
        frame
            .submit_pointer_receiver_receipts(receipts)
            .map_err(|error| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("cannot submit core-minted receiver receipts: {error}"),
                )
            })
    }

    fn compile_pointer_provider_scope(
        &self,
        scope: PointerProviderScopeIngress,
    ) -> Result<PointerProviderScope, CoreProtocolTraceError> {
        match scope {
            PointerProviderScopeIngress::DesktopGlobal {} => {
                Ok(PointerProviderScope::DesktopGlobal)
            }
            PointerProviderScopeIngress::SurfaceLocal { surface } => {
                let surface_id = SurfaceId::new(surface.0);
                if self
                    .engine
                    .presentation_requirements()
                    .surface(surface_id)
                    .is_none()
                {
                    return Err(CoreProtocolTraceError::Replay(format!(
                        "surface {} has no current presentation requirements for local pointer provider",
                        surface.0
                    )));
                }
                Ok(PointerProviderScope::SurfaceLocal(
                    SurfaceLocalPointerScope::new(
                        self.presentation_host,
                        SurfaceLocalPointerEndpoint::Logical(surface_id),
                    ),
                ))
            }
        }
    }

    fn submit_presentation_observation(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        prelude: &mut CoreHostFramePrelude,
    ) -> Result<(), CoreProtocolTraceError> {
        let observation =
            self.compile_presentation_observation(&boundary.presentation_observation, prelude)?;
        prelude
            .submit_presentation_observation(observation)
            .map_err(|error| {
                replay_error(
                    trace,
                    &boundary.id,
                    format!("cannot submit presentation observation: {error}"),
                )
            })
    }

    fn compile_presentation_observation(
        &self,
        ingress: &PresentationObservationIngress,
        prelude: &CoreHostFramePrelude,
    ) -> Result<HostPresentationObservation, CoreProtocolTraceError> {
        match ingress {
            PresentationObservationIngress::NoUpdate {} => {
                Ok(HostPresentationObservation::NoUpdate)
            }
            PresentationObservationIngress::Batch { observations } => {
                self.compile_presentation_batch(prelude, observations)
            }
        }
    }

    fn compile_presentation_batch(
        &self,
        prelude: &CoreHostFramePrelude,
        observations: &[PresentationStreamObservationIngress],
    ) -> Result<HostPresentationObservation, CoreProtocolTraceError> {
        let frozen = prelude
            .pending_presentation_streams()
            .collect::<BTreeSet<_>>();
        let entries = observations
            .iter()
            .map(|observation| self.compile_presentation_stream_observation(observation))
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let submitted = entries
            .iter()
            .map(|entry| entry.stream())
            .collect::<BTreeSet<_>>();
        if submitted != frozen {
            return Err(CoreProtocolTraceError::Replay(format!(
                "presentation observation stream set differs from the frozen core scope; expected {frozen:?}, submitted {submitted:?}"
            )));
        }
        Ok(HostPresentationObservation::Batch(entries))
    }

    fn compile_presentation_stream_observation(
        &self,
        ingress: &PresentationStreamObservationIngress,
    ) -> Result<HostPresentationObservationEntry, CoreProtocolTraceError> {
        let (stream, sidecar) = self.presentation_stream(ingress.stream)?;
        let observation = match ingress.observation {
            PresentationStreamObservationSpec::NoUpdate => {
                HostPresentationStreamObservation::NoUpdate
            }
            PresentationStreamObservationSpec::CapturedUnknown { generation, reason } => {
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(generation),
                    progress: HostPresentationProgress::Unknown(compile_authority_reason(reason)),
                }
            }
            PresentationStreamObservationSpec::Retired {
                generation,
                settled_through,
                presented,
            } => {
                let settled_through = presentation_output(sidecar, settled_through)?.key();
                let presented = match presented {
                    RetiredPresentationIngress::Presented { emission } => {
                        Authority::Known(Some(presentation_output(sidecar, emission)?.key()))
                    }
                    RetiredPresentationIngress::None => Authority::Known(None),
                    RetiredPresentationIngress::Unknown { reason } => {
                        Authority::Unknown(compile_authority_reason(reason))
                    }
                };
                HostPresentationStreamObservation::Captured {
                    generation: HostPresentationCaptureGeneration::new(generation),
                    progress: HostPresentationProgress::Retired {
                        settled_through,
                        presented,
                    },
                }
            }
        };
        Ok(HostPresentationObservationEntry::new(stream, observation))
    }

    fn presentation_stream(
        &self,
        trace_ref: PresentationStreamRef,
    ) -> Result<(HostPresentationStreamId, &PresentationStreamSidecar), CoreProtocolTraceError>
    {
        self.presentation_streams
            .iter()
            .find(|(_, sidecar)| sidecar.trace_ref == trace_ref)
            .map(|(stream, sidecar)| (*stream, sidecar))
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(format!(
                    "trace names unknown presentation stream {trace_ref:?}"
                ))
            })
    }

    fn reconcile_presentation_sidecar(
        &mut self,
        transition: &EngineTransition,
    ) -> Result<(), CoreProtocolTraceError> {
        for emission in transition.presentation_emissions() {
            self.record_presentation_output(emission.output())?;
        }
        Ok(())
    }

    fn record_presentation_output(
        &mut self,
        output: HostPresentationOutput,
    ) -> Result<(), CoreProtocolTraceError> {
        let stream = output.stream();
        if let Some(sidecar) = self.presentation_streams.get_mut(&stream) {
            sidecar.outputs.push(output);
            return Ok(());
        }

        let surface = SurfaceKey(output.surface().get());
        let endpoint = self.presentation_endpoint_ref(output.endpoint())?;
        let previous = self
            .last_presentation_stream_sequence
            .get(&surface)
            .copied()
            .unwrap_or_default();
        let sequence = previous.checked_add(1).ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!(
                "presentation stream sequence exhausted for surface {}",
                surface.0
            ))
        })?;
        self.last_presentation_stream_sequence
            .insert(surface, sequence);
        self.presentation_streams.insert(
            stream,
            PresentationStreamSidecar {
                trace_ref: PresentationStreamRef {
                    surface,
                    endpoint,
                    sequence,
                },
                outputs: vec![output],
            },
        );
        Ok(())
    }

    fn presentation_endpoint_ref(
        &self,
        endpoint: HostPresentationEndpoint,
    ) -> Result<PresentationEndpointRef, CoreProtocolTraceError> {
        match endpoint {
            HostPresentationEndpoint::Headless => Ok(PresentationEndpointRef::Headless),
            HostPresentationEndpoint::Native(binding) => {
                let record = self
                    .engine
                    .viewport()
                    .registry()
                    .record(binding.surface())
                    .filter(|record| record.binding() == binding)
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(format!(
                            "native presentation output for surface {} has no exact viewport record",
                            binding.surface().get()
                        ))
                    })?;
                Ok(PresentationEndpointRef::Native {
                    role: observe_viewport_role(record.role()),
                })
            }
        }
    }

    fn compile_pointer_journal(
        &self,
        view: HostFrameView<'_>,
        previous: u64,
        through: u64,
        edges: &[PointerEdgeIngress],
    ) -> Result<PointerEdgeJournal, CoreProtocolTraceError> {
        let edges = edges
            .iter()
            .map(|edge| self.compile_pointer_edge(view, edge))
            .collect::<Result<Vec<_>, _>>()?;
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(previous),
            PointerEdgeSequence::new(through),
            edges,
        )
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid pointer journal: {error}"))
        })
    }

    fn compile_pointer_edge(
        &self,
        view: HostFrameView<'_>,
        fixture: &PointerEdgeIngress,
    ) -> Result<PointerEdge, CoreProtocolTraceError> {
        let edge = PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(fixture.sequence),
            PointerId::new(fixture.pointer),
            self.compile_pointer_edge_kind(view, &fixture.kind)?,
            self.compile_pointer_location(view, &fixture.location)?,
            self.compile_pointer_delivery(&fixture.delivery)?,
            self.compile_pointer_capture(&fixture.capture)?,
        );
        Ok(if fixture.ending_stream {
            edge.ending_stream()
        } else {
            edge
        })
    }

    fn compile_pointer_edge_kind(
        &self,
        view: HostFrameView<'_>,
        kind: &PointerEdgeKindSpec,
    ) -> Result<PointerEdgeKind, CoreProtocolTraceError> {
        Ok(match *kind {
            PointerEdgeKindSpec::Moved => PointerEdgeKind::Moved,
            PointerEdgeKindSpec::PrimaryPressed => {
                PointerEdgeKind::ButtonPressed(PointerButton::Primary)
            }
            PointerEdgeKindSpec::PrimaryReleased => {
                PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            }
            PointerEdgeKindSpec::SecondaryPressed => {
                PointerEdgeKind::ButtonPressed(PointerButton::Secondary)
            }
            PointerEdgeKindSpec::SecondaryReleased => {
                PointerEdgeKind::ButtonReleased(PointerButton::Secondary)
            }
            PointerEdgeKindSpec::CaptureChanged => PointerEdgeKind::CaptureChanged,
            PointerEdgeKindSpec::StreamCancelled { reason } => {
                PointerEdgeKind::StreamCancelled(compile_stream_cancel_reason(reason))
            }
            PointerEdgeKindSpec::Scrolled { scroll } => {
                PointerEdgeKind::Scrolled(self.compile_scroll_edge(view, scroll)?)
            }
        })
    }

    fn compile_scroll_edge(
        &self,
        view: HostFrameView<'_>,
        fixture: ScrollEdgeIngress,
    ) -> Result<ScrollEdge, CoreProtocolTraceError> {
        let delta = fixture
            .delta
            .map(|delta| Self::compile_scroll_delta(view, delta))
            .transpose()?;
        ScrollEdge::new(
            ScrollDeviceId::new(fixture.device),
            fixture.sequence.map(ScrollSequenceToken::new),
            compile_scroll_phase(fixture.phase),
            delta,
            compile_scroll_momentum(fixture.momentum),
            compile_scroll_modifiers(fixture.modifiers),
            self.compile_scroll_delivery_endpoint(view, fixture.delivery)?,
        )
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid scroll edge: {error}")))
    }

    fn compile_scroll_delta(
        view: HostFrameView<'_>,
        fixture: ScrollDeltaIngress,
    ) -> Result<ScrollDelta, CoreProtocolTraceError> {
        let vector =
            FiniteScrollVector::new(fixture.vector().x, fixture.vector().y).map_err(|error| {
                CoreProtocolTraceError::Replay(format!("invalid scroll delta: {error}"))
            })?;
        match fixture {
            ScrollDeltaIngress::PhysicalPixels {
                surface,
                coordinate_generation,
                ..
            } => {
                let projection = Self::current_projection(view, SurfaceId::new(surface.0))?;
                let authority = projection.authority();
                let binding = authority.binding().ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "physical scroll delta surface {} has no native binding",
                        surface.0
                    ))
                })?;
                Ok(ScrollDelta::PhysicalPixels {
                    delta: vector,
                    binding,
                    coordinate_generation: compile_coordinate_generation(
                        authority.coordinate_generation(),
                        coordinate_generation,
                    ),
                })
            }
            ScrollDeltaIngress::LogicalPoints { .. } => Ok(ScrollDelta::LogicalPoints(vector)),
            ScrollDeltaIngress::Lines { .. } => Ok(ScrollDelta::Lines(vector)),
            ScrollDeltaIngress::Pages { .. } => Ok(ScrollDelta::Pages(vector)),
        }
    }

    fn compile_scroll_delivery_endpoint(
        &self,
        view: HostFrameView<'_>,
        fixture: ScrollDeliveryEndpointIngress,
    ) -> Result<Authority<ScrollDeliveryEndpoint>, CoreProtocolTraceError> {
        let (surface, coordinate_generation, native) = match fixture {
            ScrollDeliveryEndpointIngress::Headless {
                surface,
                coordinate_generation,
            } => (surface, coordinate_generation, false),
            ScrollDeliveryEndpointIngress::Native {
                surface,
                coordinate_generation,
            } => (surface, coordinate_generation, true),
            ScrollDeliveryEndpointIngress::Unknown { reason } => {
                return Ok(Authority::Unknown(compile_authority_reason(reason)));
            }
        };
        let surface_id = SurfaceId::new(surface.0);
        let projection = Self::current_projection(view, surface_id)?;
        let authority = projection.authority();
        let binding = authority.binding();
        if native != binding.is_some() {
            return Err(CoreProtocolTraceError::Replay(format!(
                "scroll delivery endpoint kind differs from surface {} presentation authority",
                surface.0
            )));
        }
        ScrollDeliveryEndpoint::new(
            self.presentation_host,
            surface_id,
            binding,
            compile_coordinate_generation(authority.coordinate_generation(), coordinate_generation),
        )
        .map(Authority::Known)
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid scroll delivery endpoint: {error}"))
        })
    }

    fn compile_pointer_location(
        &self,
        view: HostFrameView<'_>,
        fixture: &PointerLocationIngress,
    ) -> Result<PointerEdgeLocation, CoreProtocolTraceError> {
        match fixture {
            PointerLocationIngress::SurfaceLocal { position } => {
                Ok(PointerEdgeLocation::SurfaceLocal {
                    position: compile_logical_point_authority(position)?,
                })
            }
            PointerLocationIngress::Desktop { route } => Ok(PointerEdgeLocation::Desktop {
                route: self.compile_desktop_route(view, route)?,
            }),
        }
    }

    fn compile_desktop_route(
        &self,
        view: HostFrameView<'_>,
        fixture: &DesktopRouteIngress,
    ) -> Result<DesktopRouteFact, CoreProtocolTraceError> {
        match fixture {
            DesktopRouteIngress::DockFromDesktop {
                surface,
                desktop_position,
            } => Ok(DesktopRouteFact::dock_from_desktop_position(
                self.binding(*surface)?,
                Authority::Known(compile_physical_point(*desktop_position)?),
            )),
            DesktopRouteIngress::Dock {
                surface,
                desktop_position,
                surface_position,
                coordinate_generation,
            } => {
                let surface_id = SurfaceId::new(surface.0);
                let projection = view.interaction_projection(surface_id).ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "desktop route target surface {} has no interactive projection",
                        surface.0
                    ))
                })?;
                let authority = projection.authority();
                let binding = authority.binding().ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "desktop route target surface {} has no native binding authority",
                        surface.0
                    ))
                })?;
                let current = authority.coordinate_generation();
                let coordinate_generation = match coordinate_generation {
                    CoordinateGenerationIngress::Current => current,
                    CoordinateGenerationIngress::Stale => {
                        CoordinateGeneration::new(current.get().saturating_sub(1))
                    }
                };
                Ok(DesktopRouteFact::dock(DesktopDockRoute::new(
                    binding,
                    coordinate_generation,
                    compile_physical_point(*desktop_position)?,
                    compile_logical_point(*surface_position)?,
                )))
            }
            DesktopRouteIngress::Foreign { desktop_position } => Ok(DesktopRouteFact::foreign(
                compile_physical_point_authority(desktop_position)?,
            )),
            DesktopRouteIngress::OutsideAll {
                desktop_position,
                work_area,
            } => {
                let work_area = match work_area {
                    Some(work_area) => {
                        let token = WorkAreaToken::new(work_area.0);
                        if self.engine.viewport().work_area(token).is_none() {
                            return Err(CoreProtocolTraceError::Replay(format!(
                                "desktop route names unavailable work area {}",
                                work_area.0
                            )));
                        }
                        Authority::Known(DesktopWorkAreaRoute::new(
                            self.platform_provider,
                            self.engine.viewport().work_area_generation(),
                            token,
                        ))
                    }
                    None => Authority::Unknown(AuthorityUnavailableReason::NotReported),
                };
                Ok(DesktopRouteFact::no_window(
                    Authority::Known(compile_physical_point(*desktop_position)?),
                    work_area,
                ))
            }
            DesktopRouteIngress::Unknown { reason } => Ok(DesktopRouteFact::unknown(
                Authority::Unknown(compile_authority_reason(*reason)),
                compile_authority_reason(*reason),
            )),
        }
    }

    fn compile_pointer_capture(
        &self,
        fixture: &PointerCaptureIngress,
    ) -> Result<Authority<PointerCaptureOwner>, CoreProtocolTraceError> {
        Ok(match fixture {
            PointerCaptureIngress::ProviderEndpoint => {
                Authority::Known(PointerCaptureOwner::ProviderEndpoint)
            }
            PointerCaptureIngress::Native { surface } => {
                Authority::Known(PointerCaptureOwner::Native(self.binding(*surface)?))
            }
            PointerCaptureIngress::Foreign => Authority::Known(PointerCaptureOwner::Foreign),
            PointerCaptureIngress::None => Authority::Known(PointerCaptureOwner::None),
            PointerCaptureIngress::Unknown { reason } => {
                Authority::Unknown(compile_authority_reason(*reason))
            }
        })
    }

    fn compile_pointer_delivery(
        &self,
        fixture: &PointerEventDeliveryIngress,
    ) -> Result<Authority<PointerEventDeliveryOwner>, CoreProtocolTraceError> {
        Ok(match fixture {
            PointerEventDeliveryIngress::ProviderEndpoint => {
                Authority::Known(PointerEventDeliveryOwner::ProviderEndpoint)
            }
            PointerEventDeliveryIngress::Native { surface } => {
                Authority::Known(PointerEventDeliveryOwner::Native(self.binding(*surface)?))
            }
            PointerEventDeliveryIngress::Foreign => {
                Authority::Known(PointerEventDeliveryOwner::Foreign)
            }
            PointerEventDeliveryIngress::None => Authority::Known(PointerEventDeliveryOwner::None),
            PointerEventDeliveryIngress::Unknown { reason } => {
                Authority::Unknown(compile_authority_reason(*reason))
            }
        })
    }

    fn compile_pointer_receiver_receipts(
        &self,
        frame: &CoreHostFrame,
        edges: &[PointerEdgeIngress],
    ) -> Result<PointerReceiverReceiptBatch, CoreProtocolTraceError> {
        let by_sequence = edges
            .iter()
            .map(|edge| (edge.sequence, edge))
            .collect::<BTreeMap<_, _>>();
        let candidates = frame
            .pointer_receiver_candidates()
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay("pointer journal did not freeze candidates".into())
            })?
            .candidates();
        let receipts = candidates
            .iter()
            .map(|candidate| {
                let fixture = by_sequence
                    .get(&candidate.id().sequence().get())
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(
                            "core candidate has no corresponding declarative journal edge".into(),
                        )
                    })?;
                self.compile_receiver_observation(frame.view(), candidate.hover_point(), fixture)
                    .map(|observation| candidate.receipt(observation))
            })
            .collect::<Result<Vec<_>, _>>()?;
        PointerReceiverReceiptBatch::new(receipts).map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid receipt batch: {error}"))
        })
    }

    fn compile_receiver_observation(
        &self,
        view: HostFrameView<'_>,
        hover_point: Option<LogicalPoint>,
        edge: &PointerEdgeIngress,
    ) -> Result<PointerReceiverObservation, CoreProtocolTraceError> {
        match &edge.receiver {
            PointerReceiverIngress::NotApplicable => Ok(PointerReceiverObservation::NotApplicable),
            PointerReceiverIngress::Unknown { reason } => Ok(PointerReceiverObservation::Unknown(
                compile_receiver_unknown_reason(*reason),
            )),
            PointerReceiverIngress::Presented { delivery, hover } => {
                let surface = self.receiver_surface(edge)?;
                let mut probes = Vec::new();
                if let Some(delivery) = delivery {
                    probes.push(PointerReceiverProbeReceipt::Delivery(
                        Self::compile_delivery(view, surface, delivery)?,
                    ));
                }
                if let Some(hover) = hover {
                    probes.push(PointerReceiverProbeReceipt::HoverHit(self.compile_hover(
                        view,
                        surface,
                        hover_point,
                        hover,
                    )?));
                }
                PresentedPointerReceiverObservation::new(probes)
                    .map(PointerReceiverObservation::Presented)
                    .map_err(|error| {
                        CoreProtocolTraceError::Replay(format!(
                            "invalid semantic receiver observation: {error}"
                        ))
                    })
            }
        }
    }

    fn compile_delivery(
        view: HostFrameView<'_>,
        surface: SurfaceId,
        fixture: &PointerDeliveryIngress,
    ) -> Result<PointerReceiverDelivery, CoreProtocolTraceError> {
        let projection = Self::current_projection(view, surface)?;
        let disposition = match fixture {
            PointerDeliveryIngress::Tab { item } => {
                let region = projection
                    .hit_manifest()
                    .regions()
                    .iter()
                    .find(|region| {
                        matches!(
                            region.id().kind(),
                            PresentationHitRegionKind::TabBody(tab) if tab.item == ItemId::new(item.0)
                        )
                    })
                    .map(|region| region.id())
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(format!(
                            "no visible tab receiver exists for item {}",
                            item.0
                        ))
                    })?;
                PointerReceiverDeliveryDisposition::Dock(region)
            }
            PointerDeliveryIngress::TabClose { item } => {
                let region = projection
                    .hit_manifest()
                    .regions()
                    .iter()
                    .find(|region| {
                        matches!(
                            region.id().kind(),
                            PresentationHitRegionKind::TabClose(tab)
                                if tab.item == ItemId::new(item.0)
                        )
                    })
                    .map(|region| region.id())
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(format!(
                            "no visible tab-close receiver exists for item {}",
                            item.0
                        ))
                    })?;
                PointerReceiverDeliveryDisposition::Dock(region)
            }
            PointerDeliveryIngress::Splitter { root, path, index } => {
                let split = resolve_node(
                    view.workspace(),
                    &NodeLocation {
                        root: *root,
                        path: path.clone(),
                    },
                )?;
                let root = RootId::new(root.0);
                let region = projection
                    .hit_manifest()
                    .regions()
                    .iter()
                    .find(|region| {
                        matches!(
                            region.id().kind(),
                            PresentationHitRegionKind::SplitterHandle(splitter)
                                if splitter.root == root
                                    && splitter.split == split
                                    && splitter.index == *index
                        )
                    })
                    .map(|region| region.id())
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(format!(
                            "no visible splitter receiver exists for root {}, path {:?}, gap {}",
                            root.get(),
                            path.0,
                            index
                        ))
                    })?;
                PointerReceiverDeliveryDisposition::Dock(region)
            }
            PointerDeliveryIngress::TabStripScroll { root, path }
            | PointerDeliveryIngress::TabListMenuScroll { root, path } => {
                PointerReceiverDeliveryDisposition::Dock(Self::compile_scroll_delivery_region(
                    view,
                    &projection,
                    *root,
                    path,
                    matches!(fixture, PointerDeliveryIngress::TabListMenuScroll { .. }),
                )?)
            }
            PointerDeliveryIngress::Canvas => PointerReceiverDeliveryDisposition::DockCanvas,
            PointerDeliveryIngress::Blocked => PointerReceiverDeliveryDisposition::Blocked,
            PointerDeliveryIngress::NoReceiver => PointerReceiverDeliveryDisposition::NoReceiver,
            PointerDeliveryIngress::Unknown { reason } => {
                PointerReceiverDeliveryDisposition::Unknown(compile_receiver_unknown_reason(
                    *reason,
                ))
            }
        };
        PointerReceiverDelivery::new(projection, disposition).map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid delivery receipt: {error}"))
        })
    }

    fn compile_scroll_delivery_region(
        view: HostFrameView<'_>,
        projection: &dockspace::scene::SurfaceInteractionProjection<'_>,
        root: RootKey,
        path: &crate::core_protocol_trace::StructuralPath,
        menu: bool,
    ) -> Result<dockspace::presentation_hit::PresentationHitRegionId, CoreProtocolTraceError> {
        let tabs = resolve_node(
            view.workspace(),
            &NodeLocation {
                root,
                path: path.clone(),
            },
        )?;
        let root_id = RootId::new(root.0);
        let region = projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| match region.id().kind() {
                PresentationHitRegionKind::TabListMenuScroll(session) if menu => {
                    session.key().bar().root == root_id && session.key().bar().tabs == tabs
                }
                PresentationHitRegionKind::TabStripScroll(bar) if !menu => {
                    bar.root == root_id && bar.tabs == tabs
                }
                _ => false,
            })
            .map(|region| region.id())
            .ok_or_else(|| {
                let receiver = if menu { "tab-list menu" } else { "tab-strip" };
                CoreProtocolTraceError::Replay(format!(
                    "no {receiver} scroll receiver exists for root {}, path {:?}",
                    root.0, path.0
                ))
            })?;
        Ok(region)
    }

    fn compile_hover(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        point: Option<LogicalPoint>,
        fixture: &PointerHoverIngress,
    ) -> Result<PointerReceiverHoverHit, CoreProtocolTraceError> {
        let projection = Self::current_projection(view, surface)?;
        let point = point.ok_or_else(|| {
            CoreProtocolTraceError::Replay(
                "known hover receipt has no core-frozen logical point".into(),
            )
        })?;
        let disposition = match fixture {
            PointerHoverIngress::DockTarget {
                surface: declared_surface,
                target,
            } => {
                let declared_surface = SurfaceId::new(declared_surface.0);
                if declared_surface != surface {
                    return Err(CoreProtocolTraceError::Replay(format!(
                        "declared hover receiver surface {} differs from routed surface {}",
                        declared_surface.get(),
                        surface.get()
                    )));
                }
                let expected = self.compile_pointer_drop_target(view, declared_surface, target)?;
                let region = projection
                    .hit_manifest()
                    .regions()
                    .iter()
                    .copied()
                    .find(|region| {
                        !region.is_passive()
                            && region.lanes().contains(PresentationPointerLane::HoverDrop)
                            && region.hit().contains(point)
                            && matches!(
                                region.id().kind(),
                                PresentationHitRegionKind::DropTarget(actual) if actual == expected
                            )
                    })
                    .ok_or_else(|| {
                        CoreProtocolTraceError::Replay(format!(
                            "declared hover target {expected:?} is not a current point-bound receiver"
                        ))
                    })?;
                PointerReceiverHoverHitDisposition::Dock(region.id())
            }
            PointerHoverIngress::Blocked => PointerReceiverHoverHitDisposition::Blocked,
            PointerHoverIngress::NoReceiver => PointerReceiverHoverHitDisposition::NoReceiver,
            PointerHoverIngress::Unknown { reason } => PointerReceiverHoverHitDisposition::Unknown(
                compile_receiver_unknown_reason(*reason),
            ),
        };
        PointerReceiverHoverHit::new(projection, point, disposition).map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid hover receipt: {error}"))
        })
    }

    fn compile_pointer_drop_target(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        fixture: &PointerDropTargetIngress,
    ) -> Result<DropTargetId, CoreProtocolTraceError> {
        let resolve = |root: RootKey, path: &crate::core_protocol_trace::StructuralPath| {
            resolve_node(
                view.workspace(),
                &NodeLocation {
                    root,
                    path: path.clone(),
                },
            )
        };
        match fixture {
            PointerDropTargetIngress::TabGap { root, path, index } => Ok(DropTargetId::TabGap {
                surface,
                root: RootId::new(root.0),
                tabs: resolve(*root, path)?,
                index: *index,
            }),
            PointerDropTargetIngress::Center { root, path } => Ok(DropTargetId::Center {
                surface,
                root: RootId::new(root.0),
                tabs: resolve(*root, path)?,
            }),
            PointerDropTargetIngress::InnerEdge { root, path, edge } => {
                Ok(DropTargetId::InnerEdge {
                    surface,
                    root: RootId::new(root.0),
                    node: resolve(*root, path)?,
                    edge: compile_edge(*edge),
                })
            }
            PointerDropTargetIngress::OuterEdge { root, path, edge } => {
                Ok(DropTargetId::OuterEdge {
                    surface,
                    root: RootId::new(root.0),
                    node: resolve(*root, path)?,
                    edge: compile_edge(*edge),
                })
            }
            PointerDropTargetIngress::SurfaceBackground => {
                Ok(DropTargetId::SurfaceBackground { surface })
            }
        }
    }

    fn receiver_surface(
        &self,
        edge: &PointerEdgeIngress,
    ) -> Result<SurfaceId, CoreProtocolTraceError> {
        match &edge.location {
            PointerLocationIngress::SurfaceLocal { .. } => self
                .pointer_provider
                .and_then(|provider| provider.scope().surface_local())
                .map(|scope| scope.surface())
                .ok_or_else(|| {
                    CoreProtocolTraceError::Replay(
                        "surface-local receipt has no matching local pointer provider".into(),
                    )
                }),
            PointerLocationIngress::Desktop {
                route:
                    DesktopRouteIngress::Dock { surface, .. }
                    | DesktopRouteIngress::DockFromDesktop { surface, .. },
            } => Ok(SurfaceId::new(surface.0)),
            PointerLocationIngress::Desktop { .. } => Err(CoreProtocolTraceError::Replay(
                "a non-dock desktop route cannot claim a presented receiver".into(),
            )),
        }
    }

    fn current_projection(
        view: HostFrameView<'_>,
        surface: SurfaceId,
    ) -> Result<dockspace::scene::SurfaceInteractionProjection<'_>, CoreProtocolTraceError> {
        view.interaction_projection(surface).ok_or_else(|| {
            CoreProtocolTraceError::Replay("delivery surface is not interactive".into())
        })
    }

    fn compile_surface_contribution(
        &self,
        view: HostFrameView<'_>,
        ingress: &SurfaceContributionIngress,
    ) -> Result<CompiledSurfaceContribution, CoreProtocolTraceError> {
        let surface = SurfaceId::new(ingress.surface.0);
        let token = view.begin_surface_contribution(surface).map_err(|error| {
            CoreProtocolTraceError::Replay(format!(
                "cannot begin contribution for surface {}: {}",
                ingress.surface.0, error
            ))
        })?;
        if matches!(
            &ingress.measurements,
            SurfaceMeasurementIngress::Retained {}
        ) {
            return Ok(CompiledSurfaceContribution::Retained(token));
        }
        let measurements = compile_surface_measurements(view, surface, &ingress.measurements)?;
        view.prepare_surface_contribution(token, measurements)
            .map(CompiledSurfaceContribution::Prepared)
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!(
                    "cannot prepare contribution for surface {}: {}",
                    ingress.surface.0, error
                ))
            })
    }

    fn compile_input(
        &self,
        view: HostFrameView<'_>,
        input: &CoreProtocolTraceInput,
    ) -> Result<EngineInput, CoreProtocolTraceError> {
        match input {
            CoreProtocolTraceInput::WorkspaceCommand { command } => {
                Ok(EngineInput::WorkspaceCommand {
                    expected: view.version(),
                    command: compile_command(view.workspace(), command)?,
                })
            }
            CoreProtocolTraceInput::CancelActiveClickWithEscape {} => {
                let Some(click) = view.interaction().active_click_view() else {
                    return Err(CoreProtocolTraceError::Replay(
                        "Escape click cancellation requires one active Pressed session".into(),
                    ));
                };
                Ok(EngineInput::CancelActiveInteractionWithEscape {
                    expected: view.version(),
                    delivery: EscapeDelivery::Surface(click.surface()),
                })
            }
            CoreProtocolTraceInput::OpenTabListMenu { surface, bar } => {
                let tabs = resolve_node(view.workspace(), bar)?;
                let bar = TabBarSceneId {
                    root: RootId::new(bar.root.0),
                    tabs,
                };
                let prepared = view
                    .prepare_tab_strip_control_activation(
                        SurfaceId::new(surface.0),
                        TabStripControlId::TabListMenu(bar),
                    )
                    .map_err(|error| {
                        CoreProtocolTraceError::Replay(format!(
                            "cannot prepare tab-list menu activation for surface {}: {error:?}",
                            surface.0
                        ))
                    })?;
                Ok(EngineInput::ActivateTabStripControl { prepared })
            }
            CoreProtocolTraceInput::ValidateWorkspace => Ok(EngineInput::ValidateWorkspace),
            CoreProtocolTraceInput::Lifecycle {
                action:
                    LifecycleIngress::RegisterViewport {
                        surface,
                        token,
                        role,
                    },
            } => Ok(EngineInput::RegisterViewport {
                provider: self.platform_provider,
                expected: view.version(),
                surface: SurfaceId::new(surface.0),
                token: WindowToken::new(*token),
                role: compile_viewport_role(*role),
                recovery_target: None,
            }),
            CoreProtocolTraceInput::PlatformObservation {
                observation: PlatformObservationIngress::PublishSnapshot { snapshot },
            } => Ok(EngineInput::PublishPlatformSnapshot {
                provider: self.platform_provider,
                expected_epoch: view.version().epoch(),
                snapshot: self.compile_platform_snapshot(snapshot)?,
            }),
        }
    }

    fn compile_platform_snapshot(
        &self,
        fixture: &PlatformSnapshotFixture,
    ) -> Result<PlatformSnapshot, CoreProtocolTraceError> {
        let windows = fixture
            .windows
            .iter()
            .map(|window| self.compile_observed_window(window))
            .collect::<Result<Vec<_>, _>>()?;
        let work_area_observation = match &fixture.work_area_observation {
            WorkAreaRosterObservationFixture::Known {
                generation,
                work_areas,
            } => {
                let work_areas = work_areas
                    .iter()
                    .map(|work_area| {
                        Ok(ObservedWorkArea::new(
                            WorkAreaToken::new(work_area.token.0),
                            compile_physical_rect(work_area.bounds)?,
                            ScaleFactor::new(work_area.scale_factor).map_err(|error| {
                                CoreProtocolTraceError::Replay(format!(
                                    "invalid work-area scale factor: {error}"
                                ))
                            })?,
                        ))
                    })
                    .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
                WorkAreaRosterObservation::new(
                    WorkAreaObservationGeneration::new(*generation),
                    Authority::Known(work_areas),
                )
            }
            WorkAreaRosterObservationFixture::Unknown { generation, reason } => {
                Ok(WorkAreaRosterObservation::unknown(
                    WorkAreaObservationGeneration::new(*generation),
                    compile_authority_reason(*reason),
                ))
            }
        }
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid work-area observation: {error}"))
        })?;
        let capability_observation = CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(fixture.capability_generation),
            Authority::Known(compile_platform_capabilities(&fixture.capabilities)),
        );
        let inventory_observation = WindowInventoryObservation::new(
            InventoryObservationGeneration::new(fixture.inventory_generation),
            Authority::Known(windows.iter().map(ObservedWindow::binding).collect()),
        )
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid inventory observation: {error}"))
        })?;
        PlatformSnapshot::new(
            dockspace::viewport::PlatformSnapshotGeneration::new(fixture.focus_generation),
            capability_observation,
            unknown_focus_observation(
                FocusObservationGeneration::new(fixture.focus_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            inventory_observation,
            windows,
            Vec::new(),
            work_area_observation,
        )
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("invalid platform snapshot: {error}"))
        })
    }

    fn compile_observed_window(
        &self,
        fixture: &ObservedWindowFixture,
    ) -> Result<ObservedWindow, CoreProtocolTraceError> {
        let binding = self.binding(fixture.surface)?;
        let native_scale_factor =
            ScaleFactor::new(fixture.native_scale_factor).map_err(|error| {
                CoreProtocolTraceError::Replay(format!(
                    "invalid native window scale factor: {error}"
                ))
            })?;
        let presentation_scale_factor = ScaleFactor::new(fixture.presentation_scale_factor)
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!(
                    "invalid presentation scale factor: {error}"
                ))
            })?;
        Ok(ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(fixture.coordinate_observation_generation),
                Authority::Known(compile_physical_rect(fixture.content_bounds)?),
                Authority::Known(compile_physical_rect(fixture.outer_bounds)?),
                Authority::Known(native_scale_factor),
                Authority::Known(presentation_scale_factor),
            ))
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(fixture.input_observation_generation),
                Authority::Known(compile_window_input_state(fixture.input_state)),
                InputEffectAcknowledgement::known(None),
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(fixture.presentation_generation),
                Authority::Known(compile_window_presentation_state(
                    fixture.presentation_state,
                )),
                PresentationEffectAcknowledgement::known(
                    fixture
                        .acknowledged_presentation_effect
                        .map(|effect| self.effect_id(effect))
                        .transpose()?,
                ),
            )))
    }

    fn capture_viewport_bindings(
        &mut self,
        boundary: &CoreProtocolTraceBoundary,
    ) -> Result<(), CoreProtocolTraceError> {
        for event in &boundary.events {
            let HostFrameEvent::SemanticInput {
                input:
                    CoreProtocolTraceInput::Lifecycle {
                        action: LifecycleIngress::RegisterViewport { surface, .. },
                    },
                ..
            } = event
            else {
                continue;
            };
            let binding = self
                .engine
                .viewport()
                .registry()
                .record(SurfaceId::new(surface.0))
                .map(|record| record.binding())
                .ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "registered surface {} has no current core-minted binding",
                        surface.0
                    ))
                })?;
            self.bindings.insert(*surface, binding);
            if !self
                .binding_history
                .iter()
                .any(|(known, _)| *known == binding)
            {
                self.binding_history.push((binding, *surface));
            }
        }
        Ok(())
    }

    fn binding(&self, surface: SurfaceKey) -> Result<ViewportBinding, CoreProtocolTraceError> {
        self.bindings.get(&surface).copied().ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!(
                "surface {} has no captured core-minted viewport binding",
                surface.0
            ))
        })
    }

    fn observe_transition(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        event_causal_ordinals: &[u64],
        compiled_pointer_edges: &BTreeMap<u64, (usize, u64, PointerEdge)>,
        transition: &EngineTransition,
    ) -> Result<ExpectedTransition, CoreProtocolTraceError> {
        let reduced = transition
            .reduced_inputs()
            .iter()
            .map(|input| {
                let ingress = self
                    .producer_sources
                    .ingress(input.source(), input.source_sequence())
                    .ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "reduced input names an unknown stable source",
                        )
                    })?;
                let expected_ordinal = boundary
                    .events
                    .iter()
                    .position(|event| event.semantic_reference().as_ref() == Some(&ingress))
                    .ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "reduced input has no corresponding boundary ingress",
                        )
                    })?;
                let expected_ordinal = *event_causal_ordinals.get(expected_ordinal).ok_or_else(|| {
                    replay_error(
                        trace,
                        &boundary.id,
                        "semantic event has no core causal ordinal",
                    )
                })?;
                if input.causal_ordinal().get() != expected_ordinal {
                    return Err(replay_error(
                        trace,
                        &boundary.id,
                        format!(
                            "reduced input causal ordinal mismatch: expected {expected_ordinal}, got {}",
                            input.causal_ordinal().get()
                        ),
                    ));
                }
                Ok(ingress)
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let reduced_interaction_outcomes = transition
            .reduced_inputs()
            .iter()
            .zip(&reduced)
            .filter_map(|(input, ingress)| match input.outcome() {
                InputOutcome::InteractionProcessed { outcome, .. } => Some(
                    observe_interaction_outcome(self.engine.workspace(), outcome).map(|outcome| {
                        ExpectedReducedInteractionOutcome {
                            ingress: ingress.clone(),
                            outcome,
                        }
                    }),
                ),
                _ => None,
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let reduced_pointer_edges = transition
            .reduced_pointer_edges()
            .iter()
            .map(|edge| {
                let (event_index, ingress) = boundary
                    .events
                    .iter()
                    .enumerate()
                    .find_map(|(event_index, event)| match event {
                        HostFrameEvent::PointerJournal { edges, .. } => edges
                            .iter()
                            .find(|candidate| candidate.sequence == edge.edge().sequence().get())
                            .map(|ingress| (event_index, ingress)),
                        HostFrameEvent::SemanticInput { .. } => None,
                    })
                    .ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "reduced pointer edge has no corresponding ordered journal event",
                        )
                    })?;
                let (compiled_event_index, expected_ordinal, expected_edge) =
                    compiled_pointer_edges
                        .get(&ingress.sequence)
                        .ok_or_else(|| {
                            replay_error(
                                trace,
                                &boundary.id,
                                "reduced pointer edge has no event-time compiled ingress",
                            )
                        })?;
                if *compiled_event_index != event_index {
                    return Err(replay_error(
                        trace,
                        &boundary.id,
                        "compiled pointer edge was rebound to another ordered event",
                    ));
                }
                validate_pointer_edge_fidelity(edge.edge(), expected_edge).map_err(|field| {
                    replay_error(
                        trace,
                        &boundary.id,
                        format!("reduced pointer edge changed its ingress {field}"),
                    )
                })?;
                let cause = match edge.cause() {
                    ReductionCause::PointerEdge {
                        tick,
                        ordinal,
                        stream,
                        ticket,
                    } if tick == transition.tick()
                        && tick == edge.tick()
                        && ordinal == edge.causal_ordinal()
                        && ordinal.get() == *expected_ordinal
                        && stream == edge.stream()
                        && ticket == edge.ticket() =>
                    {
                        ExpectedPointerEdgeCause::PointerEdge
                    }
                    _ => {
                        return Err(replay_error(
                            trace,
                            &boundary.id,
                            format!(
                                "reduced pointer edge has an incoherent non-journal cause: expected ordinal {expected_ordinal}, actual cause {:?}, edge ordinal {:?}",
                                edge.cause(),
                                edge.causal_ordinal(),
                            ),
                        ));
                    }
                };
                Ok(ExpectedPointerEdge {
                    cause,
                    provider_incarnation: edge.stream().lease().incarnation(),
                    stream_incarnation: edge.stream().incarnation(),
                    sequence: edge.edge().sequence().get(),
                    outcomes: edge
                        .interaction_outcomes()
                        .iter()
                        .map(|outcome| observe_interaction_outcome(self.engine.workspace(), outcome))
                        .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?,
                })
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let surface_contributions = transition
            .surface_contributions()
            .iter()
            .map(observe_surface_contribution_outcome)
            .collect();
        let presentation_observations = transition
            .presentation_observations()
            .iter()
            .map(|outcome| self.observe_presentation_outcome(outcome))
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let interactive_surface_roster = self
            .engine
            .scene()
            .surfaces()
            .filter_map(|(surface, _)| {
                self.engine
                    .interaction_authority(*surface)
                    .map(|_| SurfaceKey(surface.get()))
            })
            .collect();
        let platform_effects = self.observe_platform_effects(trace, boundary, transition)?;
        let interaction_events = self.observe_interaction_events(trace, boundary, transition)?;
        let focus_delta = self.observe_focus_delta(trace, boundary, transition)?;
        let surface_scene_deltas = self.observe_surface_scene_deltas(transition)?;
        Ok(ExpectedTransition {
            tick: ReducerTick(transition.tick().get()),
            before: observe_version(transition.before()),
            after: observe_version(transition.after()),
            reduced,
            reduced_interaction_outcomes,
            reduced_pointer_edges,
            presentation_observations,
            presentation_emissions: transition.presentation_emissions().len(),
            surface_contributions,
            interaction_events,
            platform_effects,
            focus_delta,
            surface_scene_deltas,
            interaction: self.observe_interaction_state(),
            interactive_surface_roster,
            published_state_changed: transition.published_state_changed(),
        })
    }

    fn effect_ref(
        &mut self,
        effect: EffectId,
    ) -> Result<ExpectedEffectRef, CoreProtocolTraceError> {
        if let Some(reference) = self.effect_refs.get(&effect) {
            return Ok(*reference);
        }
        self.last_effect_ref = self.last_effect_ref.checked_add(1).ok_or_else(|| {
            CoreProtocolTraceError::Replay("trace-local effect identity exhausted".into())
        })?;
        let reference = EffectKey(self.last_effect_ref);
        self.effect_refs.insert(effect, reference);
        Ok(reference)
    }

    fn effect_id(&self, reference: ExpectedEffectRef) -> Result<EffectId, CoreProtocolTraceError> {
        self.effect_refs
            .iter()
            .find_map(|(effect, candidate)| (*candidate == reference).then_some(*effect))
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(format!(
                    "trace names unknown platform effect {}",
                    reference.0
                ))
            })
    }

    fn remember_dynamic_binding(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<SurfaceKey, CoreProtocolTraceError> {
        let surface = SurfaceKey(binding.surface().get());
        if let Some(existing) = self.bindings.get(&surface)
            && *existing != binding
        {
            return Err(CoreProtocolTraceError::Replay(format!(
                "core-created surface {} conflicts with an existing viewport binding",
                surface.0
            )));
        }
        if let Some((_, existing_surface)) = self
            .binding_history
            .iter()
            .find(|(candidate, _)| *candidate == binding)
            && *existing_surface != surface
        {
            return Err(CoreProtocolTraceError::Replay(format!(
                "core-created viewport binding is already assigned to surface {}",
                existing_surface.0
            )));
        }
        self.bindings.insert(surface, binding);
        if !self
            .binding_history
            .iter()
            .any(|(known, _)| *known == binding)
        {
            self.binding_history.push((binding, surface));
        }
        Ok(surface)
    }

    fn surface_for_binding(
        &self,
        binding: ViewportBinding,
    ) -> Result<SurfaceKey, CoreProtocolTraceError> {
        self.binding_history
            .iter()
            .find_map(|(candidate, surface)| (*candidate == binding).then_some(*surface))
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(format!(
                    "adapter output names unknown viewport binding {binding:?}"
                ))
            })
    }

    fn observe_platform_effects(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        transition: &EngineTransition,
    ) -> Result<Vec<ExpectedPlatformEffectEmission>, CoreProtocolTraceError> {
        let mut effects = Vec::with_capacity(transition.platform_effects().len());
        for emission in transition.platform_effects() {
            if emission.provider() != self.platform_provider {
                return Err(replay_error(
                    trace,
                    &boundary.id,
                    "platform effect was emitted to a non-current provider",
                ));
            }
            let effect = self.observe_platform_effect(emission.effect())?;
            let id = self.effect_ref(emission.id())?;
            effects.push(ExpectedPlatformEffectEmission {
                id,
                epoch: emission.epoch().get(),
                inventory_generation: emission.delivery().inventory_generation().get(),
                effect,
            });
        }
        Ok(effects)
    }

    fn observe_platform_effect(
        &mut self,
        effect: &PlatformEffect,
    ) -> Result<ExpectedPlatformEffect, CoreProtocolTraceError> {
        Ok(match effect {
            PlatformEffect::CreateWindow {
                binding,
                placement,
                role,
            } => ExpectedPlatformEffect::CreateWindow {
                surface: self.remember_dynamic_binding(*binding)?,
                placement: observe_physical_rect(*placement),
                role: observe_viewport_role(*role),
            },
            PlatformEffect::ShowWindow {
                binding,
                after_hidden,
                after_pre_show,
            } => {
                let (after_pre_show_stream, after_pre_show_emission) =
                    self.presented_staging_output_ref(*after_pre_show)?;
                ExpectedPlatformEffect::ShowWindow {
                    surface: self.surface_for_binding(*binding)?,
                    after_hidden_generation: after_hidden.get(),
                    after_pre_show_stream,
                    after_pre_show_emission,
                }
            }
            PlatformEffect::CompensatingClose {
                binding,
                compensates,
            } => ExpectedPlatformEffect::CompensatingClose {
                surface: self.surface_for_binding(*binding)?,
                compensates: self.effect_ref(*compensates)?,
            },
            PlatformEffect::CancelRootClose { binding } => {
                ExpectedPlatformEffect::CancelRootClose {
                    surface: self.surface_for_binding(*binding)?,
                }
            }
            PlatformEffect::RetainChild { binding } => ExpectedPlatformEffect::RetainChild {
                surface: self.surface_for_binding(*binding)?,
            },
            PlatformEffect::ReleaseChild { binding } => ExpectedPlatformEffect::ReleaseChild {
                surface: self.surface_for_binding(*binding)?,
            },
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after,
            } => ExpectedPlatformEffect::ContinueCleanup {
                surface: self.surface_for_binding(*binding)?,
                predecessor: self.effect_ref(*predecessor)?,
                after: after.map(|effect| self.effect_ref(effect)).transpose()?,
            },
            PlatformEffect::RequestRootClose { binding } => {
                ExpectedPlatformEffect::RequestRootClose {
                    surface: self.surface_for_binding(*binding)?,
                }
            }
            PlatformEffect::SetPointerPassthrough {
                binding,
                enabled,
                after,
            } => ExpectedPlatformEffect::SetPointerPassthrough {
                surface: self.surface_for_binding(*binding)?,
                enabled: *enabled,
                after: after.map(|effect| self.effect_ref(effect)).transpose()?,
            },
            PlatformEffect::RequestFocus { binding, after } => {
                ExpectedPlatformEffect::RequestFocus {
                    surface: self.surface_for_binding(*binding)?,
                    after: after.map(|effect| self.effect_ref(effect)).transpose()?,
                }
            }
            PlatformEffect::RequestReplacement {
                binding,
                placement,
                role,
            } => ExpectedPlatformEffect::RequestReplacement {
                surface: self.surface_for_binding(*binding)?,
                placement: observe_physical_rect(*placement),
                role: observe_viewport_role(*role),
            },
            PlatformEffect::ResolveNativeClose {
                edge, resolution, ..
            } => ExpectedPlatformEffect::ResolveNativeClose {
                surface: self.surface_for_binding(edge.binding())?,
                observed_at: edge.observed_at().get(),
                received_at: edge.received_at().get(),
                resolution: match resolution {
                    NativeCloseResolution::Accept => NativeCloseResolutionSpec::Accept,
                    NativeCloseResolution::Cancel => NativeCloseResolutionSpec::Cancel,
                },
            },
        })
    }

    fn observe_interaction_events(
        &mut self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        transition: &EngineTransition,
    ) -> Result<Vec<ExpectedInteractionEvent>, CoreProtocolTraceError> {
        transition
            .interaction_events()
            .iter()
            .map(|event| {
                Ok(ExpectedInteractionEvent {
                    cause: self.observe_reduction_cause(
                        trace,
                        boundary,
                        transition,
                        event.cause(),
                    )?,
                    version: observe_version(event.version()),
                    event: self.observe_interaction_event_kind(event.kind())?,
                })
            })
            .collect()
    }

    fn observe_reduction_cause(
        &self,
        trace: &CoreProtocolTrace,
        boundary: &CoreProtocolTraceBoundary,
        transition: &EngineTransition,
        cause: ReductionCause,
    ) -> Result<ExpectedReductionCause, CoreProtocolTraceError> {
        let tick = match cause {
            ReductionCause::Input { tick, .. }
            | ReductionCause::PointerEdge { tick, .. }
            | ReductionCause::PointerProviderRetirement { tick, .. }
            | ReductionCause::PlatformProviderReplacement { tick, .. }
            | ReductionCause::SurfaceContribution { tick, .. }
            | ReductionCause::SurfacePresentationObservationBatch { tick }
            | ReductionCause::PresentationObservation { tick, .. }
            | ReductionCause::SurfaceContributionBatch { tick }
            | ReductionCause::PresentationHostRetirement { tick, .. } => tick,
        };
        if tick != transition.tick() {
            return Err(replay_error(
                trace,
                &boundary.id,
                "interaction event cause belongs to another reducer tick",
            ));
        }
        Ok(match cause {
            ReductionCause::Input {
                source,
                source_sequence,
                ..
            } => ExpectedReductionCause::Input {
                ingress: self
                    .producer_sources
                    .ingress(source, source_sequence)
                    .ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "interaction event input cause names an unknown stable source",
                        )
                    })?,
            },
            ReductionCause::PointerEdge { stream, ticket, .. } => {
                let sequence = transition
                    .reduced_pointer_edges()
                    .iter()
                    .find(|edge| edge.stream() == stream && edge.ticket() == ticket)
                    .map(|edge| edge.edge().sequence().get())
                    .ok_or_else(|| {
                        replay_error(
                            trace,
                            &boundary.id,
                            "interaction event pointer cause has no reduced edge",
                        )
                    })?;
                ExpectedReductionCause::PointerEdge { sequence }
            }
            ReductionCause::PointerProviderRetirement { .. } => {
                ExpectedReductionCause::PointerProviderRetirement
            }
            ReductionCause::PlatformProviderReplacement { .. } => {
                ExpectedReductionCause::PlatformProviderReplacement
            }
            ReductionCause::SurfaceContribution { surface, .. } => {
                ExpectedReductionCause::SurfaceContribution {
                    surface: SurfaceKey(surface.get()),
                }
            }
            ReductionCause::SurfacePresentationObservationBatch { .. } => {
                ExpectedReductionCause::SurfacePresentationObservationBatch
            }
            ReductionCause::PresentationObservation { stream, .. } => {
                ExpectedReductionCause::PresentationObservation {
                    stream: self.presentation_stream_ref(stream)?,
                }
            }
            ReductionCause::SurfaceContributionBatch { .. } => {
                ExpectedReductionCause::SurfaceContributionBatch
            }
            ReductionCause::PresentationHostRetirement { .. } => {
                ExpectedReductionCause::PresentationHostRetirement
            }
        })
    }

    fn observe_interaction_event_kind(
        &mut self,
        event: &InteractionEventKind,
    ) -> Result<ExpectedInteractionEventKind, CoreProtocolTraceError> {
        Ok(match event {
            InteractionEventKind::ScrollTerminated {
                receiver, reason, ..
            } => ExpectedInteractionEventKind::ScrollTerminated {
                receiver: receiver
                    .map(|receiver| observe_scroll_receiver(self.engine.workspace(), receiver))
                    .transpose()?,
                reason: observe_scroll_termination(*reason),
            },
            InteractionEventKind::SemanticFocusRequested { tab } => {
                let root = self.engine.workspace().root(tab.root).ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "semantic focus event names missing root {}",
                        tab.root.get()
                    ))
                })?;
                let path = path_in_root(self.engine.workspace(), root.node, tab.tabs)?.ok_or_else(
                    || {
                        CoreProtocolTraceError::Replay(
                            "semantic focus event names a tab outside its root".into(),
                        )
                    },
                )?;
                ExpectedInteractionEventKind::SemanticFocusRequested {
                    root: RootKey(tab.root.get()),
                    path,
                    item: ItemKey(tab.item.get()),
                }
            }
            InteractionEventKind::PreviewPublished { preview } => {
                ExpectedInteractionEventKind::PreviewPublished {
                    visual: self.observe_preview_visual(preview.visual())?,
                }
            }
            InteractionEventKind::PreviewCleared { .. } => {
                ExpectedInteractionEventKind::PreviewCleared
            }
            InteractionEventKind::Cancelled { status, reason } => {
                ExpectedInteractionEventKind::Cancelled {
                    status: observe_interaction_status(*status),
                    reason: observe_cancel_reason(*reason),
                }
            }
            InteractionEventKind::Delivered { kind, .. } => {
                ExpectedInteractionEventKind::Delivered {
                    delivery: match kind {
                        WorkspaceDeliveryKind::Dock => ExpectedWorkspaceDeliveryKind::Dock,
                        WorkspaceDeliveryKind::Contained => {
                            ExpectedWorkspaceDeliveryKind::Contained
                        }
                        WorkspaceDeliveryKind::ContainedFallback => {
                            ExpectedWorkspaceDeliveryKind::ContainedFallback
                        }
                    },
                }
            }
            InteractionEventKind::ResizeDelivered { .. } => {
                ExpectedInteractionEventKind::ResizeDelivered
            }
            InteractionEventKind::ContainedTransformPreviewPublished { preview } => {
                ExpectedInteractionEventKind::ContainedTransformPreviewPublished {
                    surface: SurfaceKey(preview.surface().get()),
                    root: RootKey(preview.root().get()),
                    floating: FloatingPresentationKey(preview.floating().get()),
                    rect: observe_rect(preview.rect()),
                }
            }
            InteractionEventKind::ContainedTransformDelivered { .. } => {
                ExpectedInteractionEventKind::ContainedTransformDelivered
            }
            InteractionEventKind::NativePresentationRequested(request) => {
                ExpectedInteractionEventKind::NativePresentationRequested {
                    surface: self.surface_for_binding(request.binding())?,
                    effect: self.effect_ref(request.effect())?,
                }
            }
        })
    }

    fn observe_preview_visual(
        &self,
        visual: &PreviewVisual,
    ) -> Result<ExpectedPreviewVisual, CoreProtocolTraceError> {
        Ok(match visual {
            PreviewVisual::Dock {
                surface,
                target,
                rect,
            } => ExpectedPreviewVisual::Dock {
                surface: SurfaceKey(surface.get()),
                target: observe_drop_target(self.engine.workspace(), *target)?,
                rect: observe_rect(*rect),
            },
            PreviewVisual::Contained {
                surface,
                rect,
                fallback,
            } => ExpectedPreviewVisual::Contained {
                surface: SurfaceKey(surface.get()),
                rect: observe_rect(*rect),
                fallback: *fallback,
            },
            PreviewVisual::Native {
                host_surface,
                target_surface,
                placement,
            } => ExpectedPreviewVisual::Native {
                host_surface: SurfaceKey(host_surface.get()),
                target_surface: SurfaceKey(target_surface.get()),
                placement: observe_physical_rect(*placement),
            },
        })
    }

    fn observe_focus_delta(
        &mut self,
        _trace: &CoreProtocolTrace,
        _boundary: &CoreProtocolTraceBoundary,
        transition: &EngineTransition,
    ) -> Result<ExpectedFocusDelta, CoreProtocolTraceError> {
        let delta = transition.focus_delta();
        let global_observation = delta
            .global_observation()
            .map(|change| {
                map_focus_change(change, |observation| {
                    self.observe_global_focus_observation(*observation)
                })
            })
            .transpose()?;
        let pending_activation = delta
            .pending_activation()
            .map(|change| {
                map_focus_change(change, |activation| {
                    self.observe_pending_activation(*activation)
                })
            })
            .transpose()?;
        let observe_only_activation = delta
            .observe_only_activation()
            .map(|change| {
                map_focus_change(change, |activation| {
                    self.observe_recorded_activation(*activation)
                })
            })
            .transpose()?;
        let pane_intent = delta
            .pane_intent()
            .map(|change| {
                map_focus_change(change, |intent| self.observe_pane_focus_intent(*intent))
            })
            .transpose()?;
        let surface_focus = delta
            .surface_focus()
            .iter()
            .map(|change| {
                let surface = SurfaceKey(change.surface().get());
                Ok(ExpectedSurfaceFocusChange {
                    surface,
                    state: map_focus_change(change.state(), |state| {
                        self.observe_surface_focus_state(*state, surface)
                    })?,
                })
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        let effects = delta
            .effects()
            .iter()
            .map(|change| {
                let effect = self.effect_ref(change.request().id())?;
                let surface = self.surface_for_binding(change.request().effect().binding())?;
                let phase =
                    map_focus_change(change.phase(), |phase| Ok(observe_effect_phase(*phase)))?;
                let observed = change
                    .observed()
                    .map(|observed| {
                        Ok::<_, CoreProtocolTraceError>(
                            crate::core_protocol_trace::ExpectedObservedPlatformFocusEffect {
                                surface: self.surface_for_binding(observed.binding())?,
                                generation: observed.generation().get(),
                                evidence: match observed.evidence() {
                                    PlatformFocusEvidence::ExactEffectAcknowledgement => {
                                        ExpectedPlatformFocusEvidence::ExactEffectAcknowledgement
                                    }
                                    PlatformFocusEvidence::NewerMatchingObservation => {
                                        ExpectedPlatformFocusEvidence::NewerMatchingObservation
                                    }
                                },
                            },
                        )
                    })
                    .transpose()?;
                Ok(ExpectedFocusEffectChange {
                    effect,
                    surface,
                    phase,
                    observed,
                })
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        Ok(ExpectedFocusDelta {
            global_observation,
            pending_activation,
            observe_only_activation,
            pane_intent,
            surface_focus,
            effects,
        })
    }

    fn observe_global_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
    ) -> Result<ExpectedGlobalFocusObservation, CoreProtocolTraceError> {
        let focused = match *observation.focused() {
            Authority::Known(GlobalFocusedWindow::Dock(binding)) => {
                ExpectedGlobalFocusAuthority::Dock {
                    surface: self.surface_for_binding(binding)?,
                }
            }
            Authority::Known(GlobalFocusedWindow::Foreign) => ExpectedGlobalFocusAuthority::Foreign,
            Authority::Known(GlobalFocusedWindow::None) => ExpectedGlobalFocusAuthority::None,
            Authority::Unknown(reason) => ExpectedGlobalFocusAuthority::Unknown {
                reason: observe_authority_reason(reason),
            },
        };
        let acknowledged_effect = match *observation.acknowledged_effect() {
            Authority::Known(effect) => ExpectedAcknowledgedEffectAuthority::Known {
                effect: effect.map(|effect| self.effect_ref(effect)).transpose()?,
            },
            Authority::Unknown(reason) => ExpectedAcknowledgedEffectAuthority::Unknown {
                reason: observe_authority_reason(reason),
            },
        };
        Ok(ExpectedGlobalFocusObservation {
            generation: observation.generation().get(),
            focused,
            acknowledged_effect,
        })
    }

    fn observe_pending_activation(
        &mut self,
        activation: PendingViewportActivation,
    ) -> Result<ExpectedPendingViewportActivation, CoreProtocolTraceError> {
        let platform_focus = match activation.platform_focus() {
            PendingPlatformFocus::EffectRequired => ExpectedPendingPlatformFocus::EffectRequired,
            PendingPlatformFocus::Requested { effect } => ExpectedPendingPlatformFocus::Requested {
                effect: self.effect_ref(effect)?,
            },
            PendingPlatformFocus::Indeterminate { effect, reason } => {
                ExpectedPendingPlatformFocus::Indeterminate {
                    effect: self.effect_ref(effect)?,
                    reason: observe_effect_indeterminate_reason(reason),
                }
            }
            PendingPlatformFocus::ObservedAwaitingTarget {
                effect,
                acknowledged_at,
            } => ExpectedPendingPlatformFocus::ObservedAwaitingTarget {
                effect: self.effect_ref(effect)?,
                acknowledged_at: acknowledged_at.get(),
            },
        };
        Ok(ExpectedPendingViewportActivation {
            request: self.observe_activation_request(activation.request())?,
            observation_baseline: activation.observation_baseline().get(),
            platform_focus,
        })
    }

    fn observe_recorded_activation(
        &self,
        activation: RecordedObserveOnlyActivation,
    ) -> Result<ExpectedRecordedObserveOnlyActivation, CoreProtocolTraceError> {
        Ok(ExpectedRecordedObserveOnlyActivation {
            request: self.observe_activation_request(activation.request())?,
            observation_baseline: activation.observation_baseline().map(|value| value.get()),
        })
    }

    fn observe_activation_request(
        &self,
        request: ViewportActivationRequest,
    ) -> Result<ExpectedViewportActivationRequest, CoreProtocolTraceError> {
        Ok(ExpectedViewportActivationRequest {
            target: self.surface_for_binding(request.target())?,
            pane: observe_pane_focus_disposition(request.pane()),
            cause: observe_viewport_activation_cause(request.cause()),
        })
    }

    fn observe_pane_focus_intent(
        &self,
        intent: PaneFocusIntent,
    ) -> Result<ExpectedPaneFocusIntent, CoreProtocolTraceError> {
        Ok(ExpectedPaneFocusIntent {
            target: self.surface_for_binding(intent.target())?,
            focus: observe_panel_focus(intent.focus()),
            source: match intent.source() {
                PaneFocusIntentSource::PlatformActivation => {
                    ExpectedPaneFocusIntentSource::PlatformActivation
                }
                PaneFocusIntentSource::ExplicitViewportActivation => {
                    ExpectedPaneFocusIntentSource::ExplicitViewportActivation
                }
                PaneFocusIntentSource::PointerTabGesture => {
                    ExpectedPaneFocusIntentSource::PointerTabGesture
                }
                PaneFocusIntentSource::CloseRecovery => {
                    ExpectedPaneFocusIntentSource::CloseRecovery
                }
            },
            cause: intent.cause().map(observe_viewport_activation_cause),
            focus_observation_baseline: intent.focus_observation_baseline().get(),
            pane_observation_baseline: intent.pane_observation_baseline().map(|value| value.get()),
        })
    }

    fn observe_surface_focus_state(
        &self,
        state: SurfaceFocusState,
        surface: SurfaceKey,
    ) -> Result<ExpectedSurfaceFocusState, CoreProtocolTraceError> {
        let panel = match state.panel() {
            PanelFocusRecord::NoHistory => ExpectedPanelFocusRecord::NoHistory,
            PanelFocusRecord::Item(item) => ExpectedPanelFocusRecord::Item {
                item: ItemKey(item.get()),
            },
            PanelFocusRecord::None => ExpectedPanelFocusRecord::None,
        };
        let observation = state
            .observation()
            .map(|observation| {
                if self.surface_for_binding(observation.binding())? != surface {
                    return Err(CoreProtocolTraceError::Replay(
                        "surface focus observation was published under another surface".into(),
                    ));
                }
                Ok(ExpectedPaneFocusObservation {
                    generation: observation.generation().get(),
                    focus: observe_panel_focus(observation.focus()),
                    acknowledges_intent: observation.acknowledges().is_some(),
                })
            })
            .transpose()?;
        Ok(ExpectedSurfaceFocusState { panel, observation })
    }

    fn observe_surface_scene_deltas(
        &self,
        transition: &EngineTransition,
    ) -> Result<Vec<ExpectedSurfaceSceneDelta>, CoreProtocolTraceError> {
        transition
            .surface_scene_deltas()
            .iter()
            .map(|delta| {
                Ok(ExpectedSurfaceSceneDelta {
                    surface: SurfaceKey(delta.surface().get()),
                    before: delta
                        .before()
                        .map(|(_, state, authority)| {
                            self.observe_surface_scene_state(state, authority)
                        })
                        .transpose()?,
                    after: delta
                        .after()
                        .map(|(_, state, authority)| {
                            self.observe_surface_scene_state(state, authority)
                        })
                        .transpose()?,
                })
            })
            .collect()
    }

    fn observe_surface_scene_state(
        &self,
        state: SurfaceSceneStateKind,
        authority: Option<PresentedSurfaceAuthority>,
    ) -> Result<ExpectedSurfaceSceneState, CoreProtocolTraceError> {
        Ok(ExpectedSurfaceSceneState {
            state: match state {
                SurfaceSceneStateKind::Ready => ExpectedSurfaceSceneStateKind::Ready,
                SurfaceSceneStateKind::Stale => ExpectedSurfaceSceneStateKind::Stale,
                SurfaceSceneStateKind::Bootstrap => ExpectedSurfaceSceneStateKind::Bootstrap,
            },
            presented: authority
                .map(|authority| {
                    Ok::<_, CoreProtocolTraceError>(
                        crate::core_protocol_trace::ExpectedPresentedSurfaceAuthority {
                            stream: self.presentation_stream_ref(authority.stream())?,
                            emission: self.presentation_output_sequence(
                                authority.stream(),
                                authority.emission(),
                            )?,
                            coordinate_generation: authority.coordinate_generation().get(),
                        },
                    )
                })
                .transpose()?,
        })
    }

    fn observe_presentation_outcome(
        &self,
        outcome: &HostPresentationObservationOutcome,
    ) -> Result<ExpectedPresentationObservationOutcome, CoreProtocolTraceError> {
        match outcome {
            HostPresentationObservationOutcome::NoUpdate { stream } => {
                Ok(ExpectedPresentationObservationOutcome::NoUpdate {
                    stream: self.presentation_stream_ref(*stream)?,
                })
            }
            HostPresentationObservationOutcome::CapturedUnknown {
                stream,
                generation,
                reason,
            } => Ok(ExpectedPresentationObservationOutcome::CapturedUnknown {
                stream: self.presentation_stream_ref(*stream)?,
                generation: generation.get(),
                reason: observe_authority_reason(*reason),
            }),
            HostPresentationObservationOutcome::Retired {
                stream,
                generation,
                settled_through,
                presented,
                retired_output_count,
                promotion_eligible,
            } => {
                let trace_ref = self.presentation_stream_ref(*stream)?;
                let settled_through =
                    self.presentation_output_sequence(*stream, *settled_through)?;
                match *presented {
                    Authority::Known(Some(presented)) => {
                        Ok(ExpectedPresentationObservationOutcome::Presented {
                            stream: trace_ref,
                            generation: generation.get(),
                            settled_through,
                            presented: self.presentation_output_sequence(*stream, presented)?,
                            retired_output_count: *retired_output_count,
                            promotion_eligible: *promotion_eligible,
                        })
                    }
                    Authority::Known(None) => Ok(ExpectedPresentationObservationOutcome::Retired {
                        stream: trace_ref,
                        generation: generation.get(),
                        settled_through,
                        presented: ExpectedRetiredPresentation::None,
                        retired_output_count: *retired_output_count,
                        promotion_eligible: *promotion_eligible,
                    }),
                    Authority::Unknown(reason) => {
                        Ok(ExpectedPresentationObservationOutcome::Retired {
                            stream: trace_ref,
                            generation: generation.get(),
                            settled_through,
                            presented: ExpectedRetiredPresentation::Unknown {
                                reason: observe_authority_reason(reason),
                            },
                            retired_output_count: *retired_output_count,
                            promotion_eligible: *promotion_eligible,
                        })
                    }
                }
            }
            HostPresentationObservationOutcome::Rejected { stream, reason } => {
                Ok(ExpectedPresentationObservationOutcome::Rejected {
                    stream: self.presentation_stream_ref(*stream)?,
                    reason: observe_presentation_rejection(*reason),
                })
            }
        }
    }

    fn presentation_stream_ref(
        &self,
        stream: HostPresentationStreamId,
    ) -> Result<PresentationStreamRef, CoreProtocolTraceError> {
        self.presentation_streams
            .get(&stream)
            .map(|sidecar| sidecar.trace_ref)
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(
                    "core reported an unregistered presentation stream".into(),
                )
            })
    }

    fn presentation_output_sequence(
        &self,
        stream: HostPresentationStreamId,
        key: HostFrameKey,
    ) -> Result<u64, CoreProtocolTraceError> {
        let sidecar = self.presentation_streams.get(&stream).ok_or_else(|| {
            CoreProtocolTraceError::Replay(
                "core reported an output for an unregistered presentation stream".into(),
            )
        })?;
        sidecar
            .outputs
            .iter()
            .position(|output| output.key() == key)
            .and_then(|index| u64::try_from(index).ok())
            .and_then(|index| index.checked_add(1))
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(format!(
                    "core reported an unknown presentation emission for {:?}",
                    sidecar.trace_ref
                ))
            })
    }

    fn presented_staging_output_ref(
        &self,
        presented: PresentedNativeStagingPresentation,
    ) -> Result<(PresentationStreamRef, u64), CoreProtocolTraceError> {
        self.presentation_streams
            .values()
            .find_map(|sidecar| {
                sidecar
                    .outputs
                    .iter()
                    .position(|output| presented.matches_output(*output))
                    .and_then(|index| u64::try_from(index).ok())
                    .and_then(|index| index.checked_add(1))
                    .map(|emission| (sidecar.trace_ref, emission))
            })
            .ok_or_else(|| {
                CoreProtocolTraceError::Replay(
                    "show-window effect names an unrecorded pre-show staging output".into(),
                )
            })
    }

    fn canonical_snapshot(&self) -> Result<ExpectedCanonicalSnapshot, CoreProtocolTraceError> {
        Ok(ExpectedCanonicalSnapshot {
            workspace: canonical_workspace(self.engine.workspace(), self.engine.version())?,
        })
    }

    fn observe_interaction_state(&self) -> ExpectedInteractionState {
        match self.engine.interaction().status() {
            InteractionStatus::Idle => ExpectedInteractionState::Idle,
            InteractionStatus::Pressed { .. } => ExpectedInteractionState::Pressed,
            InteractionStatus::Armed { .. } => ExpectedInteractionState::Armed,
            InteractionStatus::Dragging { .. } => ExpectedInteractionState::Dragging,
            InteractionStatus::Resizing { .. } => ExpectedInteractionState::Resizing,
            InteractionStatus::ContainedTransforming { .. } => {
                ExpectedInteractionState::ContainedTransforming
            }
        }
    }
}

fn presentation_slot_key(slot: HostPresentationSlot) -> PresentationSlotKey {
    match slot {
        HostPresentationSlot::Surface { surface } => PresentationSlotKey::Surface(surface),
        HostPresentationSlot::NativeStaging { presentation } => {
            PresentationSlotKey::NativeStaging(presentation.binding().surface())
        }
    }
}

fn presentation_ingress_slot(ingress: PresentationDispositionIngress) -> PresentationSlotKey {
    match ingress {
        PresentationDispositionIngress::Surface { surface, .. } => {
            PresentationSlotKey::Surface(SurfaceId::new(surface.0))
        }
        PresentationDispositionIngress::NativeStaging { surface, .. } => {
            PresentationSlotKey::NativeStaging(SurfaceId::new(surface.0))
        }
    }
}

fn validate_exact_presentation_roster(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    expected: BTreeSet<PresentationSlotKey>,
    submitted: BTreeSet<PresentationSlotKey>,
) -> Result<(), CoreProtocolTraceError> {
    if expected == submitted {
        return Ok(());
    }

    if let Some((expected_slot, submitted_slot)) = expected
        .iter()
        .filter(|expected_slot| !submitted.contains(expected_slot))
        .find_map(|expected_slot| {
            submitted
                .iter()
                .find(|submitted_slot| submitted_slot.surface() == expected_slot.surface())
                .map(|submitted_slot| (*expected_slot, *submitted_slot))
        })
    {
        return Err(replay_error(
            trace,
            &boundary.id,
            format!(
                "presentation slot role mismatch for surface {}: core requires {expected_slot:?}, trace supplied {submitted_slot:?}",
                expected_slot.surface().get()
            ),
        ));
    }

    let missing = expected.difference(&submitted).copied().collect::<Vec<_>>();
    let extra = submitted.difference(&expected).copied().collect::<Vec<_>>();
    Err(replay_error(
        trace,
        &boundary.id,
        format!(
            "presentation disposition roster is not exact; missing {missing:?}, extra {extra:?}"
        ),
    ))
}

const fn compile_presentation_disposition(
    disposition: PresentationDispositionSpec,
    interaction: dockspace::presentation_observation::HostInteractionPresentation,
) -> HostPresentationDisposition {
    match disposition {
        PresentationDispositionSpec::Painted {} => {
            HostPresentationDisposition::Painted(interaction)
        }
        PresentationDispositionSpec::Unavailable { reason } => {
            HostPresentationDisposition::Unavailable(match reason {
                PresentationUnavailableReasonSpec::FinalPresentationUnobservable => {
                    HostPresentationUnavailableReason::FinalPresentationUnobservable
                }
                PresentationUnavailableReasonSpec::OutputNotProduced => {
                    HostPresentationUnavailableReason::OutputNotProduced
                }
                PresentationUnavailableReasonSpec::SupersededBeforePublication => {
                    HostPresentationUnavailableReason::SupersededBeforePublication
                }
                PresentationUnavailableReasonSpec::RetainedResourceUnavailable => {
                    HostPresentationUnavailableReason::RetainedResourceUnavailable
                }
                PresentationUnavailableReasonSpec::TransientVisualNotPainted => {
                    HostPresentationUnavailableReason::TransientVisualNotPainted
                }
                PresentationUnavailableReasonSpec::BackendFailure => {
                    HostPresentationUnavailableReason::BackendFailure
                }
            })
        }
    }
}

fn presentation_output(
    sidecar: &PresentationStreamSidecar,
    sequence: u64,
) -> Result<HostPresentationOutput, CoreProtocolTraceError> {
    let index = sequence
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!(
                "invalid presentation emission sequence {sequence} for {:?}",
                sidecar.trace_ref
            ))
        })?;
    sidecar.outputs.get(index).copied().ok_or_else(|| {
        CoreProtocolTraceError::Replay(format!(
            "presentation emission sequence {sequence} is unavailable for {:?}",
            sidecar.trace_ref
        ))
    })
}

fn validate_pointer_edge_fidelity(
    actual: &PointerEdge,
    ingress: &PointerEdge,
) -> Result<(), &'static str> {
    if actual.sequence() != ingress.sequence() {
        return Err("sequence");
    }
    if actual.pointer() != ingress.pointer() {
        return Err("pointer identity");
    }
    if actual.kind() != ingress.kind() {
        return Err("kind/button transition");
    }
    if actual.ends_stream() != ingress.ends_stream() {
        return Err("pointer stream terminality");
    }
    if actual.location() != ingress.location() {
        return Err("location/route authority");
    }
    if actual.delivery_owner() != ingress.delivery_owner() {
        return Err("event delivery authority");
    }
    if actual.capture_owner() != ingress.capture_owner() {
        return Err("capture authority");
    }
    Ok(())
}

/// Replays every trace against a fresh core protocol harness.
///
/// # Errors
///
/// Returns [`CoreProtocolTraceError`] at the first schema, transition, or final-state mismatch.
pub fn replay_core_protocol_trace_suite(
    suite: &CoreProtocolTraceSuite,
) -> Result<CoreProtocolReplayReport, CoreProtocolTraceError> {
    validate_core_protocol_trace_suite(suite)?;
    let mut boundaries = 0;
    for trace in &suite.traces {
        let mut harness = CoreProtocolHarness::from_trace(trace)?;
        for boundary in &trace.boundaries {
            harness.replay_boundary(trace, boundary)?;
            boundaries += 1;
        }
        let actual = harness.canonical_snapshot()?;
        if actual != trace.expected_final {
            return Err(CoreProtocolTraceError::Replay(format!(
                "trace `{}` final snapshot mismatch\nexpected: {:#?}\nactual: {actual:#?}",
                trace.id.as_str(),
                trace.expected_final
            )));
        }
    }
    Ok(CoreProtocolReplayReport {
        traces: suite.traces.len(),
        boundaries,
    })
}

fn compile_surface_measurements(
    view: HostFrameView<'_>,
    surface: SurfaceId,
    ingress: &SurfaceMeasurementIngress,
) -> Result<SurfaceMeasurements, CoreProtocolTraceError> {
    let requirements = view
        .presentation_requirements()
        .surface(surface)
        .ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!(
                "surface {} has no presentation requirements",
                surface.get()
            ))
        })?;
    if let SurfaceMeasurementIngress::Unavailable { reason } = ingress {
        return Ok(SurfaceMeasurements::unavailable(
            requirements,
            compile_measurement_unavailable_reason(*reason),
        ));
    }
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    let bounds = match ingress {
        SurfaceMeasurementIngress::Complete { bounds }
        | SurfaceMeasurementIngress::BoundsOnly { bounds } => {
            Measurement::Measured(compile_rect(*bounds)?)
        }
        SurfaceMeasurementIngress::Retained {} | SurfaceMeasurementIngress::Unavailable { .. } => {
            unreachable!("retained and unavailable do not compile a measurement set")
        }
    };
    measurements
        .set_bounds(requirements.bounds(), bounds)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("duplicate bounds: {error}")))?;
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, bounds)
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("duplicate popup-plane bounds: {error}"))
            })?;
    }
    if matches!(ingress, SurfaceMeasurementIngress::BoundsOnly { .. }) {
        return Ok(measurements);
    }
    let minimum = LogicalSize::new(0.0, 0.0).map_err(|error| {
        CoreProtocolTraceError::Replay(format!("invalid minimum size: {error}"))
    })?;
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("duplicate pane measurement: {error}"))
            })?;
    }
    let tab = TabIntrinsic::new(56.0).map_err(|error| {
        CoreProtocolTraceError::Replay(format!("invalid tab intrinsic: {error}"))
    })?;
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(key, Measurement::Measured(tab))
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("duplicate tab measurement: {error}"))
            })?;
    }
    let controls =
        TabStripControlMetrics::new(4.0)
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("invalid tab-strip controls: {error}"))
            })?
            .with_scroll_backward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                    .map_err(|error| {
                        CoreProtocolTraceError::Replay(format!(
                            "invalid backward tab-strip control: {error}"
                        ))
                    })?,
            )
            .with_scroll_forward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                    .map_err(|error| {
                        CoreProtocolTraceError::Replay(format!(
                            "invalid forward tab-strip control: {error}"
                        ))
                    })?,
            )
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .map_err(|error| {
                        CoreProtocolTraceError::Replay(format!(
                            "invalid tab-list menu control: {error}"
                        ))
                    })?,
            );
    let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).map_err(|error| {
        CoreProtocolTraceError::Replay(format!("invalid tab-list menu: {error}"))
    })?;
    let strip = TabStripMetrics::new(0.0, 0.0)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid tab strip: {error}")))?
        .with_controls(controls)
        .with_tab_list_menu(menu);
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(key, Measurement::Measured(strip))
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("duplicate tab strip measurement: {error}"))
            })?;
    }
    Ok(measurements)
}

fn compile_command(
    workspace: &Workspace,
    command: &CoreProtocolTraceCommand,
) -> Result<WorkspaceCommand, CoreProtocolTraceError> {
    match command {
        CoreProtocolTraceCommand::Select { source } => Ok(WorkspaceCommand::Select {
            source: capture_item(workspace, source)?,
        }),
        CoreProtocolTraceCommand::OpenCenter { item, target } => Ok(WorkspaceCommand::Open {
            item: ItemId::new(item.0),
            target: DockTarget::Center(capture_tab_target(workspace, target)?),
        }),
        CoreProtocolTraceCommand::OpenTabGap {
            item,
            target,
            insertion_index,
        } => Ok(WorkspaceCommand::Open {
            item: ItemId::new(item.0),
            target: DockTarget::TabGap {
                target: capture_tab_target(workspace, target)?,
                index: *insertion_index,
            },
        }),
        CoreProtocolTraceCommand::Reorder {
            source,
            insertion_index,
        } => Ok(WorkspaceCommand::Reorder {
            source: capture_item(workspace, source)?,
            insertion_index: *insertion_index,
        }),
    }
}

fn capture_tab_target(
    workspace: &Workspace,
    target: &NodeLocation,
) -> Result<dockspace::command::TabTarget, CoreProtocolTraceError> {
    let node = resolve_node(workspace, target)?;
    workspace
        .capture_tab_target(RootId::new(target.root.0), node)
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("cannot capture tab target: {error}"))
        })
}

fn capture_item(
    workspace: &Workspace,
    source: &ItemLocation,
) -> Result<dockspace::command::ItemSource, CoreProtocolTraceError> {
    let node = resolve_node(
        workspace,
        &NodeLocation {
            root: source.root,
            path: source.path.clone(),
        },
    )?;
    workspace
        .capture_item_source(RootId::new(source.root.0), node, ItemId::new(source.item.0))
        .map_err(|error| {
            CoreProtocolTraceError::Replay(format!("cannot capture item source: {error}"))
        })
}

fn resolve_node(
    workspace: &Workspace,
    location: &NodeLocation,
) -> Result<NodeId, CoreProtocolTraceError> {
    let root = workspace
        .root(RootId::new(location.root.0))
        .ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!("missing root {}", location.root.0))
        })?;
    let mut node = root.node;
    for &index in &location.path.0 {
        let Node::Split { children, .. } = workspace.node(node).ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!("missing runtime node {node:?}"))
        })?
        else {
            return Err(CoreProtocolTraceError::Replay(format!(
                "path {:?} traverses a tabs leaf",
                location.path.0
            )));
        };
        node = *children.get(index).ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!("path {:?} is out of bounds", location.path.0))
        })?;
    }
    Ok(node)
}

fn build_workspace(initial: &InitialWorkspace) -> Result<Workspace, CoreProtocolTraceError> {
    let mut builder = WorkspaceBuilder::new();
    for root in &initial.roots {
        let mut locations = BTreeMap::new();
        let node = insert_fixture_node(&mut builder, &root.node, &mut Vec::new(), &mut locations)?;
        let mut record = RootRecord::new(node);
        if let Some(path) = &root.central_path {
            let central = locations.get(path).copied().ok_or_else(|| {
                CoreProtocolTraceError::Replay(format!(
                    "root {} central path {:?} is absent",
                    root.id.0, path.0
                ))
            })?;
            record = record.with_central(central);
        }
        builder.set_root(RootId::new(root.id.0), record);
    }
    for surface in &initial.surfaces {
        let mut presentation = surface
            .main_root
            .map_or_else(SurfacePresentation::rootless, |root| {
                SurfacePresentation::with_main(RootId::new(root.0))
            });
        for contained in &surface.contained {
            presentation
                .contained
                .push(FloatingPresentationId::new(contained.id.0));
            builder.set_contained_floating(
                FloatingPresentationId::new(contained.id.0),
                ContainedFloating::new(
                    RootId::new(contained.root.0),
                    compile_rect(contained.rect)?,
                ),
            );
        }
        builder.set_surface(SurfaceId::new(surface.id.0), presentation);
    }
    builder.build().map_err(|error| {
        CoreProtocolTraceError::Replay(format!("invalid initial workspace: {error}"))
    })
}

fn insert_fixture_node(
    builder: &mut WorkspaceBuilder,
    fixture: &NodeFixture,
    path: &mut Vec<usize>,
    locations: &mut BTreeMap<crate::core_protocol_trace::StructuralPath, NodeId>,
) -> Result<NodeId, CoreProtocolTraceError> {
    let (node, mru) = match fixture {
        NodeFixture::Tabs {
            items,
            selected,
            mru,
        } => (
            Node::tabs_with_selection(
                items.iter().map(|item| ItemId::new(item.0)),
                selected.map(|item| ItemId::new(item.0)),
            ),
            Some(mru),
        ),
        NodeFixture::Split {
            axis,
            weights,
            children,
        } => {
            let mut child_ids = Vec::with_capacity(children.len());
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                child_ids.push(insert_fixture_node(builder, child, path, locations)?);
                path.pop();
            }
            (
                Node::split(compile_axis(*axis), child_ids, weights.iter().copied()).map_err(
                    |error| {
                        CoreProtocolTraceError::Replay(format!("invalid fixture split: {error}"))
                    },
                )?,
                None,
            )
        }
    };
    let id = builder.insert_node(node);
    if let Some(mru) = mru {
        builder
            .set_tab_mru(id, mru.iter().map(|item| ItemId::new(item.0)))
            .map_err(|error| {
                CoreProtocolTraceError::Replay(format!("cannot set fixture MRU: {error}"))
            })?;
    }
    locations.insert(crate::core_protocol_trace::StructuralPath(path.clone()), id);
    Ok(id)
}

fn canonical_workspace(
    workspace: &Workspace,
    version: WorkspaceVersion,
) -> Result<CanonicalWorkspace, CoreProtocolTraceError> {
    let mut roots = Vec::new();
    let mut item_owners = Vec::new();
    for (root_id, record) in workspace.roots() {
        let owner = root_owner(workspace, root_id)?;
        let central_path = record
            .central
            .map(|central| path_in_root(workspace, record.node, central))
            .transpose()?
            .flatten();
        collect_item_owners(
            workspace,
            root_id,
            record.node,
            owner,
            &mut Vec::new(),
            &mut item_owners,
        )?;
        roots.push(CanonicalRoot {
            id: RootKey(root_id.get()),
            central_path,
            owner,
            node: canonical_node(workspace, record.node)?,
        });
    }
    let mut surfaces = Vec::new();
    for (surface_id, presentation) in workspace.surfaces() {
        let contained = presentation
            .contained
            .iter()
            .map(|floating| {
                let record = workspace.contained_floating(*floating).ok_or_else(|| {
                    CoreProtocolTraceError::Replay(format!(
                        "surface {} references missing contained floating {}",
                        surface_id.get(),
                        floating.get()
                    ))
                })?;
                Ok(CanonicalContained {
                    id: FloatingPresentationKey(floating.get()),
                    root: RootKey(record.root.get()),
                    rect: observe_rect(record.rect),
                })
            })
            .collect::<Result<Vec<_>, CoreProtocolTraceError>>()?;
        surfaces.push(CanonicalSurface {
            id: SurfaceKey(surface_id.get()),
            main_root: presentation.main_root.map(|root| RootKey(root.get())),
            contained,
        });
    }
    let item_multiset = workspace
        .item_multiset()
        .into_iter()
        .map(|(item, count)| ItemCount {
            item: ItemKey(item.get()),
            count,
        })
        .collect();
    Ok(CanonicalWorkspace {
        version: observe_version(version),
        roots,
        surfaces,
        item_multiset,
        item_owners,
    })
}

fn canonical_node(
    workspace: &Workspace,
    node: NodeId,
) -> Result<NodeFixture, CoreProtocolTraceError> {
    match workspace
        .node(node)
        .ok_or_else(|| CoreProtocolTraceError::Replay(format!("missing runtime node {node:?}")))?
    {
        Node::Tabs { items, selected } => Ok(NodeFixture::Tabs {
            items: items.iter().map(|item| ItemKey(item.get())).collect(),
            selected: selected.map(|item| ItemKey(item.get())),
            mru: workspace
                .tab_mru(node)
                .ok_or_else(|| CoreProtocolTraceError::Replay("tabs node has no MRU".into()))?
                .iter()
                .map(|item| ItemKey(item.get()))
                .collect(),
        }),
        Node::Split {
            axis,
            children,
            weights,
        } => Ok(NodeFixture::Split {
            axis: observe_axis(*axis),
            weights: weights.iter().map(|weight| weight.get()).collect(),
            children: children
                .iter()
                .map(|child| canonical_node(workspace, *child))
                .collect::<Result<Vec<_>, _>>()?,
        }),
    }
}

fn collect_item_owners(
    workspace: &Workspace,
    root: RootId,
    node: NodeId,
    owner: RootOwner,
    path: &mut Vec<usize>,
    output: &mut Vec<crate::core_protocol_trace::ItemOwner>,
) -> Result<(), CoreProtocolTraceError> {
    match workspace
        .node(node)
        .ok_or_else(|| CoreProtocolTraceError::Replay(format!("missing runtime node {node:?}")))?
    {
        Node::Tabs { items, .. } => {
            output.extend(
                items
                    .iter()
                    .map(|item| crate::core_protocol_trace::ItemOwner {
                        item: ItemKey(item.get()),
                        root: RootKey(root.get()),
                        path: crate::core_protocol_trace::StructuralPath(path.clone()),
                        owner,
                    }),
            )
        }
        Node::Split { children, .. } => {
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                collect_item_owners(workspace, root, *child, owner, path, output)?;
                path.pop();
            }
        }
    }
    Ok(())
}

fn root_owner(workspace: &Workspace, root: RootId) -> Result<RootOwner, CoreProtocolTraceError> {
    for (surface, presentation) in workspace.surfaces() {
        if presentation.main_root == Some(root) {
            return Ok(RootOwner::Main {
                surface: SurfaceKey(surface.get()),
            });
        }
        for floating in &presentation.contained {
            if workspace
                .contained_floating(*floating)
                .is_some_and(|record| record.root == root)
            {
                return Ok(RootOwner::Contained {
                    surface: SurfaceKey(surface.get()),
                    floating: FloatingPresentationKey(floating.get()),
                });
            }
        }
    }
    Err(CoreProtocolTraceError::Replay(format!(
        "root {} has no presentation owner",
        root.get()
    )))
}

fn path_in_root(
    workspace: &Workspace,
    current: NodeId,
    needle: NodeId,
) -> Result<Option<crate::core_protocol_trace::StructuralPath>, CoreProtocolTraceError> {
    fn visit(
        workspace: &Workspace,
        current: NodeId,
        needle: NodeId,
        path: &mut Vec<usize>,
    ) -> Result<Option<crate::core_protocol_trace::StructuralPath>, CoreProtocolTraceError> {
        if current == needle {
            return Ok(Some(crate::core_protocol_trace::StructuralPath(
                path.clone(),
            )));
        }
        let node = workspace.node(current).ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!("missing runtime node {current:?}"))
        })?;
        if let Node::Split { children, .. } = node {
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                if let Some(found) = visit(workspace, *child, needle, path)? {
                    return Ok(Some(found));
                }
                path.pop();
            }
        }
        Ok(None)
    }
    visit(workspace, current, needle, &mut Vec::new())
}

fn compile_rect(rect: RectFixture) -> Result<LogicalRect, CoreProtocolTraceError> {
    LogicalRect::new(rect.x, rect.y, rect.width, rect.height)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid logical rect: {error}")))
}

fn compile_physical_rect(rect: RectFixture) -> Result<PhysicalRect, CoreProtocolTraceError> {
    PhysicalRect::new(rect.x, rect.y, rect.width, rect.height)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid physical rect: {error}")))
}

fn compile_logical_point(point: PointFixture) -> Result<LogicalPoint, CoreProtocolTraceError> {
    LogicalPoint::new(point.x, point.y)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid logical point: {error}")))
}

fn compile_physical_point(point: PointFixture) -> Result<PhysicalPoint, CoreProtocolTraceError> {
    PhysicalPoint::new(point.x, point.y)
        .map_err(|error| CoreProtocolTraceError::Replay(format!("invalid physical point: {error}")))
}

fn compile_logical_point_authority(
    fixture: &PointAuthorityIngress,
) -> Result<Authority<LogicalPoint>, CoreProtocolTraceError> {
    match fixture {
        PointAuthorityIngress::Known { point } => {
            compile_logical_point(*point).map(Authority::Known)
        }
        PointAuthorityIngress::Unknown { reason } => {
            Ok(Authority::Unknown(compile_authority_reason(*reason)))
        }
    }
}

fn compile_physical_point_authority(
    fixture: &PointAuthorityIngress,
) -> Result<Authority<PhysicalPoint>, CoreProtocolTraceError> {
    match fixture {
        PointAuthorityIngress::Known { point } => {
            compile_physical_point(*point).map(Authority::Known)
        }
        PointAuthorityIngress::Unknown { reason } => {
            Ok(Authority::Unknown(compile_authority_reason(*reason)))
        }
    }
}

fn compile_platform_capabilities(fixture: &PlatformCapabilitiesFixture) -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    for requirement in &fixture.supported {
        match requirement {
            PlatformRequirementSpec::NativeWindowLifecycle => {
                capabilities.set_native_window_lifecycle(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::AuthoritativeInventory => {
                capabilities.set_authoritative_inventory(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::HoveredWindow => {
                capabilities.set_hovered_window(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::DesktopPointerPosition => {
                capabilities.set_desktop_pointer_position(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::AuthoritativeButtonState => {
                capabilities.set_authoritative_button_state(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::GlobalWindowPlacement => {
                capabilities.set_global_window_placement(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::WorkArea => {
                capabilities.set_work_area(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::PointerHitTestObservation => {
                capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::PointerHitTestControl => {
                capabilities.set_pointer_hit_test_control(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::GlobalFocusObservation => {
                capabilities.set_global_focus_observation(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::WindowActivationControl => {
                capabilities.set_window_activation_control(PlatformCapability::Supported)
            }
            PlatformRequirementSpec::CloseCancellation => {
                capabilities.set_close_cancellation(PlatformCapability::Supported)
            }
        }
    }
    capabilities
}

const fn compile_authority_reason(
    reason: AuthorityUnavailableReasonSpec,
) -> AuthorityUnavailableReason {
    match reason {
        AuthorityUnavailableReasonSpec::ProviderUnavailable => {
            AuthorityUnavailableReason::ProviderUnavailable
        }
        AuthorityUnavailableReasonSpec::PermissionDenied => {
            AuthorityUnavailableReason::PermissionDenied
        }
        AuthorityUnavailableReasonSpec::SurfaceUnavailable => {
            AuthorityUnavailableReason::SurfaceUnavailable
        }
        AuthorityUnavailableReasonSpec::CoordinateUnavailable => {
            AuthorityUnavailableReason::CoordinateUnavailable
        }
        AuthorityUnavailableReasonSpec::NotReported => AuthorityUnavailableReason::NotReported,
    }
}

const fn observe_authority_reason(
    reason: AuthorityUnavailableReason,
) -> AuthorityUnavailableReasonSpec {
    match reason {
        AuthorityUnavailableReason::ProviderUnavailable => {
            AuthorityUnavailableReasonSpec::ProviderUnavailable
        }
        AuthorityUnavailableReason::PermissionDenied => {
            AuthorityUnavailableReasonSpec::PermissionDenied
        }
        AuthorityUnavailableReason::SurfaceUnavailable => {
            AuthorityUnavailableReasonSpec::SurfaceUnavailable
        }
        AuthorityUnavailableReason::CoordinateUnavailable => {
            AuthorityUnavailableReasonSpec::CoordinateUnavailable
        }
        AuthorityUnavailableReason::NotReported => AuthorityUnavailableReasonSpec::NotReported,
    }
}

const fn observe_presentation_rejection(
    rejection: HostPresentationObservationRejection,
) -> ExpectedPresentationObservationRejection {
    match rejection {
        HostPresentationObservationRejection::CaptureGenerationNotIncreasing { .. } => {
            ExpectedPresentationObservationRejection::CaptureGenerationNotIncreasing
        }
        HostPresentationObservationRejection::SettlementFromDifferentStream { .. } => {
            ExpectedPresentationObservationRejection::SettlementFromDifferentStream
        }
        HostPresentationObservationRejection::SettlementNotIncreasing { .. } => {
            ExpectedPresentationObservationRejection::SettlementNotIncreasing
        }
        HostPresentationObservationRejection::SettlementNotPending { .. } => {
            ExpectedPresentationObservationRejection::SettlementNotPending
        }
        HostPresentationObservationRejection::PresentedFromDifferentStream { .. } => {
            ExpectedPresentationObservationRejection::PresentedFromDifferentStream
        }
        HostPresentationObservationRejection::PresentedOutsideRetirementRange { .. } => {
            ExpectedPresentationObservationRejection::PresentedOutsideRetirementRange
        }
        HostPresentationObservationRejection::PresentedNotPending { .. } => {
            ExpectedPresentationObservationRejection::PresentedNotPending
        }
    }
}

const fn compile_receiver_unknown_reason(
    reason: PointerReceiverUnknownReasonSpec,
) -> PointerReceiverUnknownReason {
    match reason {
        PointerReceiverUnknownReasonSpec::FrameworkDeliveryUnavailable => {
            PointerReceiverUnknownReason::FrameworkDeliveryUnavailable
        }
        PointerReceiverUnknownReasonSpec::EventCorrelationUnavailable => {
            PointerReceiverUnknownReason::EventCorrelationUnavailable
        }
        PointerReceiverUnknownReasonSpec::LayerAuthorityUnavailable => {
            PointerReceiverUnknownReason::LayerAuthorityUnavailable
        }
        PointerReceiverUnknownReasonSpec::PresentationAuthorityUnavailable => {
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable
        }
        PointerReceiverUnknownReasonSpec::NotReported => PointerReceiverUnknownReason::NotReported,
    }
}

const fn compile_coordinate_generation(
    current: CoordinateGeneration,
    fixture: CoordinateGenerationIngress,
) -> CoordinateGeneration {
    match fixture {
        CoordinateGenerationIngress::Current => current,
        CoordinateGenerationIngress::Stale => {
            CoordinateGeneration::new(current.get().saturating_sub(1))
        }
    }
}

const fn compile_scroll_phase(phase: ScrollPhaseSpec) -> ScrollPhase {
    match phase {
        ScrollPhaseSpec::Discrete => ScrollPhase::Discrete,
        ScrollPhaseSpec::Begin => ScrollPhase::Begin,
        ScrollPhaseSpec::Update => ScrollPhase::Update,
        ScrollPhaseSpec::End => ScrollPhase::End,
        ScrollPhaseSpec::Cancel { reason } => {
            ScrollPhase::Cancel(compile_scroll_cancel_reason(reason))
        }
    }
}

const fn compile_scroll_cancel_reason(reason: ScrollCancelReasonSpec) -> ScrollCancelReason {
    match reason {
        ScrollCancelReasonSpec::PlatformCancelled => ScrollCancelReason::PlatformCancelled,
        ScrollCancelReasonSpec::DeviceRemoved => ScrollCancelReason::DeviceRemoved,
        ScrollCancelReasonSpec::ProviderReset => ScrollCancelReason::ProviderReset,
    }
}

const fn compile_scroll_momentum(
    momentum: ScrollMomentumAuthorityIngress,
) -> Authority<ScrollMomentum> {
    match momentum {
        ScrollMomentumAuthorityIngress::Known {
            momentum: ScrollMomentumSpec::Direct,
        } => Authority::Known(ScrollMomentum::Direct),
        ScrollMomentumAuthorityIngress::Known {
            momentum: ScrollMomentumSpec::Momentum,
        } => Authority::Known(ScrollMomentum::Momentum),
        ScrollMomentumAuthorityIngress::Unknown { reason } => {
            Authority::Unknown(compile_authority_reason(reason))
        }
    }
}

const fn compile_scroll_modifiers(
    modifiers: ScrollModifiersAuthorityIngress,
) -> Authority<ScrollModifiers> {
    match modifiers {
        ScrollModifiersAuthorityIngress::Known {
            shift,
            control,
            alt,
            command,
        } => Authority::Known(ScrollModifiers::new(shift, control, alt, command)),
        ScrollModifiersAuthorityIngress::Unknown { reason } => {
            Authority::Unknown(compile_authority_reason(reason))
        }
    }
}

const fn compile_stream_cancel_reason(
    reason: PointerStreamCancelReasonSpec,
) -> PointerStreamCancelReason {
    match reason {
        PointerStreamCancelReasonSpec::DeviceRemoved => PointerStreamCancelReason::DeviceRemoved,
        PointerStreamCancelReasonSpec::ProviderShutdown => {
            PointerStreamCancelReason::ProviderShutdown
        }
        PointerStreamCancelReasonSpec::ExplicitPlatformCancellation => {
            PointerStreamCancelReason::ExplicitPlatformCancellation
        }
        PointerStreamCancelReasonSpec::ProviderReset => PointerStreamCancelReason::ProviderReset,
    }
}

const fn compile_measurement_unavailable_reason(
    reason: MeasurementUnavailableReasonSpec,
) -> MeasurementUnavailableReason {
    match reason {
        MeasurementUnavailableReasonSpec::Unsupported => MeasurementUnavailableReason::Unsupported,
        MeasurementUnavailableReasonSpec::SurfaceUnavailable => {
            MeasurementUnavailableReason::SurfaceUnavailable
        }
        MeasurementUnavailableReasonSpec::ContentUnavailable => {
            MeasurementUnavailableReason::ContentUnavailable
        }
        MeasurementUnavailableReasonSpec::TextMetricsUnavailable => {
            MeasurementUnavailableReason::TextMetricsUnavailable
        }
        MeasurementUnavailableReasonSpec::Deferred => MeasurementUnavailableReason::Deferred,
    }
}

const fn compile_window_input_state(state: WindowInputStateSpec) -> WindowInputState {
    match state {
        WindowInputStateSpec::ReceivesInput => WindowInputState::ReceivesInput,
        WindowInputStateSpec::PassThrough => WindowInputState::PassThrough,
    }
}

const fn compile_window_presentation_state(
    state: WindowPresentationStateSpec,
) -> WindowPresentationState {
    match state {
        WindowPresentationStateSpec::Visible => WindowPresentationState::Visible,
        WindowPresentationStateSpec::Hidden => WindowPresentationState::Hidden,
        WindowPresentationStateSpec::Minimized => WindowPresentationState::Minimized,
    }
}

const fn compile_viewport_role(role: ViewportRoleSpec) -> ViewportRole {
    match role {
        ViewportRoleSpec::Root => ViewportRole::Root,
        ViewportRoleSpec::Child => ViewportRole::Child,
    }
}

const fn observe_viewport_role(role: ViewportRole) -> ViewportRoleSpec {
    match role {
        ViewportRole::Root => ViewportRoleSpec::Root,
        ViewportRole::Child => ViewportRoleSpec::Child,
    }
}

fn map_focus_change<T, U>(
    change: &FocusValueChange<T>,
    mut observe: impl FnMut(&T) -> Result<U, CoreProtocolTraceError>,
) -> Result<ExpectedFocusValueChange<U>, CoreProtocolTraceError> {
    let before = change.before().as_ref().map(&mut observe).transpose()?;
    let after = change.after().as_ref().map(observe).transpose()?;
    Ok(ExpectedFocusValueChange { before, after })
}

const fn observe_pane_focus_disposition(
    disposition: PaneFocusDisposition,
) -> ExpectedPaneFocusDisposition {
    match disposition {
        PaneFocusDisposition::Preserve => ExpectedPaneFocusDisposition::Preserve,
        PaneFocusDisposition::Set(item) => ExpectedPaneFocusDisposition::Set {
            item: ItemKey(item.get()),
        },
        PaneFocusDisposition::Clear => ExpectedPaneFocusDisposition::Clear,
    }
}

const fn observe_viewport_activation_cause(
    cause: ViewportActivationCause,
) -> ExpectedViewportActivationCause {
    match cause {
        ViewportActivationCause::Explicit => ExpectedViewportActivationCause::Explicit,
        ViewportActivationCause::DropCommitted => ExpectedViewportActivationCause::DropCommitted,
        ViewportActivationCause::TearOffCommitted => {
            ExpectedViewportActivationCause::TearOffCommitted
        }
        ViewportActivationCause::PointerTabGesture => {
            ExpectedViewportActivationCause::PointerTabGesture
        }
        ViewportActivationCause::CloseRecovery { .. } => {
            ExpectedViewportActivationCause::CloseRecovery
        }
        ViewportActivationCause::RecoveryReplacement => {
            ExpectedViewportActivationCause::RecoveryReplacement
        }
        ViewportActivationCause::PlatformObservation => {
            ExpectedViewportActivationCause::PlatformObservation
        }
    }
}

const fn observe_panel_focus(focus: PanelFocus) -> ExpectedPanelFocus {
    match focus {
        PanelFocus::Item(item) => ExpectedPanelFocus::Item {
            item: ItemKey(item.get()),
        },
        PanelFocus::None => ExpectedPanelFocus::None,
    }
}

const fn observe_effect_phase(phase: EffectPhase) -> ExpectedEffectPhase {
    match phase {
        EffectPhase::Requested => ExpectedEffectPhase::Requested,
        EffectPhase::DispatchFailed(reason) => ExpectedEffectPhase::DispatchFailed {
            reason: observe_dispatch_failure_reason(reason),
        },
        EffectPhase::ObservationDispatchFailed(reason) => {
            ExpectedEffectPhase::ObservationDispatchFailed {
                reason: observe_dispatch_failure_reason(reason),
            }
        }
        EffectPhase::ObservedApplied {
            inventory_generation,
        } => ExpectedEffectPhase::ObservedApplied {
            inventory_generation: inventory_generation.get(),
        },
        EffectPhase::Unsupported(reason) => ExpectedEffectPhase::Unsupported {
            reason: observe_effect_unsupported_reason(reason),
        },
        EffectPhase::ObservationUnsupported(reason) => {
            ExpectedEffectPhase::ObservationUnsupported {
                reason: observe_effect_unsupported_reason(reason),
            }
        }
        EffectPhase::Indeterminate(reason) => ExpectedEffectPhase::Indeterminate {
            reason: observe_effect_indeterminate_reason(reason),
        },
        EffectPhase::Destroyed {
            inventory_generation,
        } => ExpectedEffectPhase::Destroyed {
            inventory_generation: inventory_generation.get(),
        },
        EffectPhase::Invalidated { cause } => ExpectedEffectPhase::Invalidated {
            reason: match cause {
                EffectInvalidation::WorkspaceReplaced { replacement_epoch } => {
                    ExpectedEffectInvalidation::WorkspaceReplaced {
                        replacement_epoch: replacement_epoch.get(),
                    }
                }
                EffectInvalidation::NativeCreateAborted => {
                    ExpectedEffectInvalidation::NativeCreateAborted
                }
                EffectInvalidation::StagingCloseCleared => {
                    ExpectedEffectInvalidation::StagingCloseCleared
                }
                EffectInvalidation::PlatformProviderReplaced { .. } => {
                    ExpectedEffectInvalidation::PlatformProviderReplaced
                }
            },
        },
    }
}

const fn observe_dispatch_failure_reason(
    reason: DispatchFailureReason,
) -> ExpectedDispatchFailureReason {
    match reason {
        DispatchFailureReason::AdapterRejected => ExpectedDispatchFailureReason::AdapterRejected,
        DispatchFailureReason::WindowUnavailable => {
            ExpectedDispatchFailureReason::WindowUnavailable
        }
        DispatchFailureReason::ProviderStopped => ExpectedDispatchFailureReason::ProviderStopped,
    }
}

const fn observe_effect_unsupported_reason(
    reason: EffectUnsupportedReason,
) -> ExpectedEffectUnsupportedReason {
    match reason {
        EffectUnsupportedReason::BackendUnsupported => {
            ExpectedEffectUnsupportedReason::BackendUnsupported
        }
        EffectUnsupportedReason::CapabilityRevoked => {
            ExpectedEffectUnsupportedReason::CapabilityRevoked
        }
    }
}

const fn observe_effect_indeterminate_reason(
    reason: EffectIndeterminateReason,
) -> ExpectedEffectIndeterminateReason {
    match reason {
        EffectIndeterminateReason::AcknowledgementLost => {
            ExpectedEffectIndeterminateReason::AcknowledgementLost
        }
        EffectIndeterminateReason::ProviderRestarted => {
            ExpectedEffectIndeterminateReason::ProviderRestarted
        }
    }
}

const fn observe_interaction_status(status: InteractionStatus) -> ExpectedInteractionState {
    match status {
        InteractionStatus::Idle => ExpectedInteractionState::Idle,
        InteractionStatus::Pressed { .. } => ExpectedInteractionState::Pressed,
        InteractionStatus::Armed { .. } => ExpectedInteractionState::Armed,
        InteractionStatus::Dragging { .. } => ExpectedInteractionState::Dragging,
        InteractionStatus::Resizing { .. } => ExpectedInteractionState::Resizing,
        InteractionStatus::ContainedTransforming { .. } => {
            ExpectedInteractionState::ContainedTransforming
        }
    }
}

fn observe_drop_target(
    workspace: &Workspace,
    target: DropTargetId,
) -> Result<PointerDropTargetIngress, CoreProtocolTraceError> {
    let path = |root: RootId, node: NodeId| {
        let root_record = workspace.root(root).ok_or_else(|| {
            CoreProtocolTraceError::Replay(format!(
                "preview target names missing root {}",
                root.get()
            ))
        })?;
        path_in_root(workspace, root_record.node, node)?.ok_or_else(|| {
            CoreProtocolTraceError::Replay("preview target node is outside its root".into())
        })
    };
    Ok(match target {
        DropTargetId::TabGap {
            root, tabs, index, ..
        } => PointerDropTargetIngress::TabGap {
            root: RootKey(root.get()),
            path: path(root, tabs)?,
            index,
        },
        DropTargetId::Center { root, tabs, .. } => PointerDropTargetIngress::Center {
            root: RootKey(root.get()),
            path: path(root, tabs)?,
        },
        DropTargetId::InnerEdge {
            root, node, edge, ..
        } => PointerDropTargetIngress::InnerEdge {
            root: RootKey(root.get()),
            path: path(root, node)?,
            edge: observe_edge(edge),
        },
        DropTargetId::OuterEdge {
            root, node, edge, ..
        } => PointerDropTargetIngress::OuterEdge {
            root: RootKey(root.get()),
            path: path(root, node)?,
            edge: observe_edge(edge),
        },
        DropTargetId::SurfaceBackground { .. } => PointerDropTargetIngress::SurfaceBackground,
    })
}

const fn observe_edge(edge: Edge) -> EdgeSpec {
    match edge {
        Edge::Left => EdgeSpec::Left,
        Edge::Right => EdgeSpec::Right,
        Edge::Top => EdgeSpec::Top,
        Edge::Bottom => EdgeSpec::Bottom,
    }
}

const fn compile_axis(axis: AxisSpec) -> Axis {
    match axis {
        AxisSpec::Horizontal => Axis::Horizontal,
        AxisSpec::Vertical => Axis::Vertical,
    }
}

const fn compile_edge(edge: EdgeSpec) -> Edge {
    match edge {
        EdgeSpec::Left => Edge::Left,
        EdgeSpec::Right => Edge::Right,
        EdgeSpec::Top => Edge::Top,
        EdgeSpec::Bottom => Edge::Bottom,
    }
}

const fn observe_axis(axis: Axis) -> AxisSpec {
    match axis {
        Axis::Horizontal => AxisSpec::Horizontal,
        Axis::Vertical => AxisSpec::Vertical,
    }
}

fn observe_interaction_outcome(
    workspace: &Workspace,
    outcome: &InteractionOutcome,
) -> Result<ExpectedInteractionOutcome, CoreProtocolTraceError> {
    Ok(match outcome {
        InteractionOutcome::Scroll(outcome) => observe_scroll_outcome(workspace, *outcome)?,
        InteractionOutcome::CloseRequested { plan, reused } => {
            ExpectedInteractionOutcome::CloseRequested {
                target: observe_close_target(plan.target()),
                items: plan
                    .items()
                    .iter()
                    .map(|item| ItemKey(item.item().get()))
                    .collect(),
                reused: *reused,
            }
        }
        InteractionOutcome::TabStripControlActivated {
            control,
            changed,
            menu,
        } => ExpectedInteractionOutcome::TabStripControlActivated {
            control: match control {
                TabStripControlId::ScrollBackward(_) => ExpectedTabStripControl::ScrollBackward,
                TabStripControlId::ScrollForward(_) => ExpectedTabStripControl::ScrollForward,
                TabStripControlId::TabListMenu(_) => ExpectedTabStripControl::TabListMenu,
            },
            changed: *changed,
            menu_open: menu.is_some(),
        },
        InteractionOutcome::TabListMenuItemSelected { tab, changed, .. } => {
            ExpectedInteractionOutcome::TabListMenuItemSelected {
                item: ItemKey(tab.item.get()),
                changed: *changed,
            }
        }
        InteractionOutcome::TabListMenuDismissed { .. } => {
            ExpectedInteractionOutcome::TabListMenuDismissed
        }
        InteractionOutcome::TabListMenuFrameConsumed { .. } => {
            ExpectedInteractionOutcome::TabListMenuFrameConsumed
        }
        InteractionOutcome::TabStripScrolled { changed, .. } => {
            ExpectedInteractionOutcome::TabStripScrolled { changed: *changed }
        }
        InteractionOutcome::TabListMenuScrolled { changed, .. } => {
            ExpectedInteractionOutcome::TabListMenuScrolled { changed: *changed }
        }
        InteractionOutcome::TabListMenuFocusMoved { changed, .. } => {
            ExpectedInteractionOutcome::TabListMenuFocusMoved { changed: *changed }
        }
        InteractionOutcome::DragArmed { .. } => ExpectedInteractionOutcome::DragArmed,
        InteractionOutcome::DragBegan { .. } => ExpectedInteractionOutcome::DragBegan,
        InteractionOutcome::PreviewUpdated { status, .. } => ExpectedInteractionOutcome::Preview {
            status: observe_preview_status(*status),
        },
        InteractionOutcome::PreviewAcknowledged { changed, .. } => {
            ExpectedInteractionOutcome::PreviewAcknowledged { changed: *changed }
        }
        InteractionOutcome::DragDelivered { .. } => ExpectedInteractionOutcome::DragDelivered,
        InteractionOutcome::ReleasePending { .. } => ExpectedInteractionOutcome::PendingDragRelease,
        InteractionOutcome::Cancelled { reason, .. } => ExpectedInteractionOutcome::Cancelled {
            reason: observe_cancel_reason(*reason),
        },
        InteractionOutcome::ResizeBegan { .. } => ExpectedInteractionOutcome::ResizeBegan,
        InteractionOutcome::ResizeUpdated { .. } => ExpectedInteractionOutcome::ResizeUpdated,
        InteractionOutcome::SplitterAdjusted { changed, .. } => {
            ExpectedInteractionOutcome::SplitterAdjusted { changed: *changed }
        }
        InteractionOutcome::ResizeDelivered { changed, .. } => {
            ExpectedInteractionOutcome::ResizeDelivered { changed: *changed }
        }
        InteractionOutcome::ContainedPlacementApplied { changed, .. } => {
            ExpectedInteractionOutcome::ContainedPlacementApplied { changed: *changed }
        }
        InteractionOutcome::ContainedTransformBegan { .. } => {
            ExpectedInteractionOutcome::ContainedTransformBegan
        }
        InteractionOutcome::ContainedTransformPreviewUpdated { .. } => {
            ExpectedInteractionOutcome::ContainedTransformPreviewUpdated
        }
        InteractionOutcome::ContainedTransformPreviewAcknowledged { changed, .. } => {
            ExpectedInteractionOutcome::ContainedTransformPreviewAcknowledged { changed: *changed }
        }
        InteractionOutcome::ContainedTransformReleasePending { .. } => {
            ExpectedInteractionOutcome::PendingContainedTransformRelease
        }
        InteractionOutcome::ContainedTransformDelivered { changed, .. } => {
            ExpectedInteractionOutcome::ContainedTransformDelivered { changed: *changed }
        }
        InteractionOutcome::Rejected(_) => ExpectedInteractionOutcome::Rejected,
    })
}

fn observe_scroll_outcome(
    workspace: &Workspace,
    outcome: ScrollReductionOutcome,
) -> Result<ExpectedInteractionOutcome, CoreProtocolTraceError> {
    match outcome {
        ScrollReductionOutcome::AwaitingFirstDelta {
            sequence, phase, ..
        } => Ok(ExpectedInteractionOutcome::ScrollAwaitingFirstDelta {
            sequence: sequence.get(),
            phase: observe_scroll_phase(phase),
        }),
        ScrollReductionOutcome::Began { receiver, .. } => {
            Ok(ExpectedInteractionOutcome::ScrollBegan {
                receiver: observe_scroll_receiver(workspace, receiver)?,
            })
        }
        ScrollReductionOutcome::Applied(application) => {
            Ok(ExpectedInteractionOutcome::ScrollApplied {
                session: application.session().is_some(),
                receiver: observe_scroll_receiver(workspace, application.receiver())?,
                phase: observe_scroll_phase(application.phase()),
                requested_delta: application.requested_delta(),
                applied_delta: application.applied_delta(),
                unapplied_delta: application.unapplied_delta(),
                offset: application.offset(),
            })
        }
        ScrollReductionOutcome::Suppressed {
            session,
            sequence,
            phase,
            reason,
        } => Ok(ExpectedInteractionOutcome::ScrollSuppressed {
            session: session.is_some(),
            sequence: sequence.map(ScrollSequenceToken::get),
            phase: observe_scroll_phase(phase),
            reason: observe_scroll_suppression(workspace, reason)?,
        }),
        ScrollReductionOutcome::Terminated {
            receiver, reason, ..
        } => Ok(ExpectedInteractionOutcome::ScrollTerminated {
            receiver: receiver
                .map(|receiver| observe_scroll_receiver(workspace, receiver))
                .transpose()?,
            reason: observe_scroll_termination(reason),
        }),
    }
}

fn observe_scroll_receiver(
    workspace: &Workspace,
    receiver: dockspace::presentation_hit::PresentationHitRegionId,
) -> Result<ExpectedScrollReceiver, CoreProtocolTraceError> {
    match receiver.kind() {
        PresentationHitRegionKind::TabStripScroll(bar) => {
            observe_scroll_bar(workspace, receiver.surface(), bar, false)
        }
        PresentationHitRegionKind::TabListMenuScroll(session) => {
            observe_scroll_bar(workspace, receiver.surface(), session.key().bar(), true)
        }
        kind => Err(CoreProtocolTraceError::Replay(format!(
            "scroll outcome names non-scroll receiver {kind:?}"
        ))),
    }
}

fn observe_scroll_bar(
    workspace: &Workspace,
    surface: SurfaceId,
    bar: TabBarSceneId,
    menu: bool,
) -> Result<ExpectedScrollReceiver, CoreProtocolTraceError> {
    let root = workspace.root(bar.root).ok_or_else(|| {
        CoreProtocolTraceError::Replay(format!(
            "scroll receiver names missing root {}",
            bar.root.get()
        ))
    })?;
    let path = path_in_root(workspace, root.node, bar.tabs)?.ok_or_else(|| {
        CoreProtocolTraceError::Replay(format!(
            "scroll receiver tab node {:?} is outside root {}",
            bar.tabs,
            bar.root.get()
        ))
    })?;
    if menu {
        Ok(ExpectedScrollReceiver::TabListMenu {
            surface: SurfaceKey(surface.get()),
            root: RootKey(bar.root.get()),
            path,
        })
    } else {
        Ok(ExpectedScrollReceiver::TabStrip {
            surface: SurfaceKey(surface.get()),
            root: RootKey(bar.root.get()),
            path,
        })
    }
}

fn observe_scroll_suppression(
    workspace: &Workspace,
    reason: ScrollSuppressionReason,
) -> Result<ExpectedScrollSuppressionReason, CoreProtocolTraceError> {
    Ok(match reason {
        ScrollSuppressionReason::ReceiverUnknown => {
            ExpectedScrollSuppressionReason::ReceiverUnknown
        }
        ScrollSuppressionReason::FrameworkBlocked => {
            ExpectedScrollSuppressionReason::FrameworkBlocked
        }
        ScrollSuppressionReason::NoReceiver => ExpectedScrollSuppressionReason::NoReceiver,
        ScrollSuppressionReason::DockCanvas => ExpectedScrollSuppressionReason::DockCanvas,
        ScrollSuppressionReason::DockBlocker(blocker) => {
            ExpectedScrollSuppressionReason::DockBlocker {
                blocker: observe_scroll_blocker(workspace, blocker)?,
            }
        }
    })
}

fn observe_scroll_blocker(
    workspace: &Workspace,
    blocker: dockspace::presentation_hit::PresentationHitRegionId,
) -> Result<ExpectedScrollBlocker, CoreProtocolTraceError> {
    let observe_menu = |session: dockspace::tab_strip::TabListMenuSessionId| {
        let receiver = observe_scroll_bar(workspace, blocker.surface(), session.key().bar(), true)?;
        let ExpectedScrollReceiver::TabListMenu {
            surface,
            root,
            path,
        } = receiver
        else {
            unreachable!("menu scroll bar projection returns a menu receiver")
        };
        Ok::<_, CoreProtocolTraceError>((surface, root, path))
    };
    match blocker.kind() {
        PresentationHitRegionKind::TabListMenuBlocker(session) => {
            let (surface, root, path) = observe_menu(session)?;
            Ok(ExpectedScrollBlocker::TabListMenuFrame {
                surface,
                root,
                path,
            })
        }
        PresentationHitRegionKind::TabListMenuBackdrop(session) => {
            let (surface, root, path) = observe_menu(session)?;
            Ok(ExpectedScrollBlocker::TabListMenuBackdrop {
                surface,
                root,
                path,
            })
        }
        PresentationHitRegionKind::ContainedFrameBlocker(floating) => {
            Ok(ExpectedScrollBlocker::ContainedFrame {
                surface: SurfaceKey(blocker.surface().get()),
                floating: FloatingPresentationKey(floating.get()),
            })
        }
        kind => Err(CoreProtocolTraceError::Replay(format!(
            "scroll suppression names unsupported blocker {kind:?}"
        ))),
    }
}

const fn observe_scroll_phase(phase: ScrollPhase) -> ScrollPhaseSpec {
    match phase {
        ScrollPhase::Discrete => ScrollPhaseSpec::Discrete,
        ScrollPhase::Begin => ScrollPhaseSpec::Begin,
        ScrollPhase::Update => ScrollPhaseSpec::Update,
        ScrollPhase::End => ScrollPhaseSpec::End,
        ScrollPhase::Cancel(reason) => ScrollPhaseSpec::Cancel {
            reason: observe_scroll_cancel_reason(reason),
        },
    }
}

const fn observe_scroll_cancel_reason(reason: ScrollCancelReason) -> ScrollCancelReasonSpec {
    match reason {
        ScrollCancelReason::PlatformCancelled => ScrollCancelReasonSpec::PlatformCancelled,
        ScrollCancelReason::DeviceRemoved => ScrollCancelReasonSpec::DeviceRemoved,
        ScrollCancelReason::ProviderReset => ScrollCancelReasonSpec::ProviderReset,
    }
}

const fn observe_scroll_termination(
    reason: ScrollTerminationReason,
) -> ExpectedScrollTerminationReason {
    match reason {
        ScrollTerminationReason::Completed => ExpectedScrollTerminationReason::Completed,
        ScrollTerminationReason::Cancelled(reason) => ExpectedScrollTerminationReason::Cancelled {
            reason: observe_scroll_cancel_reason(reason),
        },
        ScrollTerminationReason::StreamCancelled => {
            ExpectedScrollTerminationReason::StreamCancelled
        }
        ScrollTerminationReason::ProviderRetired => {
            ExpectedScrollTerminationReason::ProviderRetired
        }
        ScrollTerminationReason::ReceiverLost => ExpectedScrollTerminationReason::ReceiverLost,
        ScrollTerminationReason::DeliveryEndpointChanged => {
            ExpectedScrollTerminationReason::DeliveryEndpointChanged
        }
        ScrollTerminationReason::BindingRetired => ExpectedScrollTerminationReason::BindingRetired,
        ScrollTerminationReason::PresentationHostRetired => {
            ExpectedScrollTerminationReason::PresentationHostRetired
        }
        ScrollTerminationReason::SurfaceRemoved => ExpectedScrollTerminationReason::SurfaceRemoved,
        ScrollTerminationReason::PopupRoutingChanged => {
            ExpectedScrollTerminationReason::PopupRoutingChanged
        }
        ScrollTerminationReason::PolicyChanged => ExpectedScrollTerminationReason::PolicyChanged,
        ScrollTerminationReason::PresentationConfigChanged => {
            ExpectedScrollTerminationReason::PresentationConfigChanged
        }
    }
}

const fn observe_close_target(target: ClosePlanTarget) -> ExpectedCloseTarget {
    match target {
        ClosePlanTarget::Item { item } => ExpectedCloseTarget::Item {
            item: ItemKey(item.get()),
        },
        ClosePlanTarget::Root { root } => ExpectedCloseTarget::Root {
            root: RootKey(root.get()),
        },
        ClosePlanTarget::Surface {
            surface,
            disposition,
        } => ExpectedCloseTarget::Surface {
            surface: SurfaceKey(surface.get()),
            disposition: match disposition {
                SurfaceCloseDisposition::RetainLayout => {
                    ExpectedSurfaceCloseDisposition::RetainLayout
                }
                SurfaceCloseDisposition::RehomeAll { target } => {
                    ExpectedSurfaceCloseDisposition::RehomeAll {
                        target: SurfaceKey(target.get()),
                    }
                }
                SurfaceCloseDisposition::CloseContent => {
                    ExpectedSurfaceCloseDisposition::CloseContent
                }
            },
        },
    }
}

const fn observe_preview_status(
    status: PreviewResolutionStatus,
) -> ExpectedPreviewResolutionStatus {
    match status {
        PreviewResolutionStatus::Resolved => ExpectedPreviewResolutionStatus::Resolved,
        PreviewResolutionStatus::KnownNone => ExpectedPreviewResolutionStatus::KnownNone,
        PreviewResolutionStatus::Rejected => ExpectedPreviewResolutionStatus::Rejected,
        PreviewResolutionStatus::Unavailable => ExpectedPreviewResolutionStatus::Unavailable,
        PreviewResolutionStatus::UnknownAuthority => {
            ExpectedPreviewResolutionStatus::UnknownAuthority
        }
        PreviewResolutionStatus::OpaqueBlocker => ExpectedPreviewResolutionStatus::OpaqueBlocker,
        PreviewResolutionStatus::NativeCapabilityUnknown => {
            ExpectedPreviewResolutionStatus::NativeCapabilityUnknown
        }
        PreviewResolutionStatus::NativePlacementUnavailable => {
            ExpectedPreviewResolutionStatus::NativePlacementUnavailable
        }
    }
}

const fn observe_cancel_reason(reason: InteractionCancelReason) -> ExpectedInteractionCancelReason {
    match reason {
        InteractionCancelReason::Escape => ExpectedInteractionCancelReason::Escape,
        InteractionCancelReason::ReleasedBeforeDrag => {
            ExpectedInteractionCancelReason::ReleasedBeforeDrag
        }
        InteractionCancelReason::CaptureLost => ExpectedInteractionCancelReason::CaptureLost,
        InteractionCancelReason::CaptureAuthorityUnavailable => {
            ExpectedInteractionCancelReason::CaptureAuthorityUnavailable
        }
        InteractionCancelReason::DeliveryAuthorityUnavailable => {
            ExpectedInteractionCancelReason::DeliveryAuthorityUnavailable
        }
        InteractionCancelReason::DeliveryOwnerLost => {
            ExpectedInteractionCancelReason::DeliveryOwnerLost
        }
        InteractionCancelReason::PointerStreamCancelled => {
            ExpectedInteractionCancelReason::PointerStreamCancelled
        }
        InteractionCancelReason::UnknownButtonState => {
            ExpectedInteractionCancelReason::UnknownButtonState
        }
        InteractionCancelReason::UnknownTargetAuthority => {
            ExpectedInteractionCancelReason::UnknownTargetAuthority
        }
        InteractionCancelReason::OpaquePointerBlocker => {
            ExpectedInteractionCancelReason::OpaquePointerBlocker
        }
        InteractionCancelReason::ClickReceiverMismatch => {
            ExpectedInteractionCancelReason::ClickReceiverMismatch
        }
        InteractionCancelReason::NativeCapabilityUnknown => {
            ExpectedInteractionCancelReason::NativeCapabilityUnknown
        }
        InteractionCancelReason::NativeCapabilityUnavailable => {
            ExpectedInteractionCancelReason::NativeCapabilityUnavailable
        }
        InteractionCancelReason::NativePlacementUnavailable => {
            ExpectedInteractionCancelReason::NativePlacementUnavailable
        }
        InteractionCancelReason::SourceVanished => ExpectedInteractionCancelReason::SourceVanished,
        InteractionCancelReason::WorkspaceChanged => {
            ExpectedInteractionCancelReason::WorkspaceChanged
        }
        InteractionCancelReason::PolicyChanged => ExpectedInteractionCancelReason::PolicyChanged,
        InteractionCancelReason::SurfaceClosed => ExpectedInteractionCancelReason::SurfaceClosed,
        InteractionCancelReason::SceneUnavailable => {
            ExpectedInteractionCancelReason::SceneUnavailable
        }
        InteractionCancelReason::PointerProviderRetired => {
            ExpectedInteractionCancelReason::PointerProviderRetired
        }
        InteractionCancelReason::ReplacedByNewGesture => {
            ExpectedInteractionCancelReason::ReplacedByNewGesture
        }
        InteractionCancelReason::WorkspaceRestored => {
            ExpectedInteractionCancelReason::WorkspaceRestored
        }
    }
}

fn observe_surface_contribution_outcome(
    outcome: &SurfaceContributionOutcome,
) -> ExpectedSurfaceContributionOutcome {
    match outcome {
        SurfaceContributionOutcome::Ready { surface, .. } => {
            ExpectedSurfaceContributionOutcome::Ready {
                surface: SurfaceKey(surface.get()),
            }
        }
        SurfaceContributionOutcome::Retained { surface, .. } => {
            ExpectedSurfaceContributionOutcome::Retained {
                surface: SurfaceKey(surface.get()),
            }
        }
        SurfaceContributionOutcome::Unavailable { surface, .. } => {
            ExpectedSurfaceContributionOutcome::Unavailable {
                surface: SurfaceKey(surface.get()),
            }
        }
        SurfaceContributionOutcome::Rejected { surface, .. } => {
            ExpectedSurfaceContributionOutcome::Rejected {
                surface: SurfaceKey(surface.get()),
            }
        }
    }
}

const fn observe_version(version: WorkspaceVersion) -> VersionExpectation {
    VersionExpectation {
        epoch: version.epoch().get(),
        revision: version.revision().get(),
    }
}

fn observe_rect(rect: LogicalRect) -> RectFixture {
    RectFixture {
        x: rect.x(),
        y: rect.y(),
        width: rect.width(),
        height: rect.height(),
    }
}

fn observe_physical_rect(rect: PhysicalRect) -> RectFixture {
    RectFixture {
        x: rect.x(),
        y: rect.y(),
        width: rect.width(),
        height: rect.height(),
    }
}

fn replay_error(
    trace: &CoreProtocolTrace,
    boundary: &BoundaryId,
    message: impl std::fmt::Display,
) -> CoreProtocolTraceError {
    CoreProtocolTraceError::Replay(format!(
        "trace `{}` boundary `{}`: {message}",
        trace.id.as_str(),
        boundary.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(
        sequence: u64,
        pointer: u64,
        kind: PointerEdgeKind,
        x: f64,
        capture: PointerCaptureOwner,
    ) -> PointerEdge {
        PointerEdge::new(
            PointerEdgeSequence::new(sequence),
            PointerId::new(pointer),
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(
                    LogicalPoint::new(x, 24.0).expect("test point must be finite"),
                ),
            },
            Authority::Known(capture),
        )
    }

    fn scroll_edge(
        device: u64,
        token: u64,
        x: f64,
        momentum: ScrollMomentum,
        shift: bool,
        delivery_reason: AuthorityUnavailableReason,
    ) -> PointerEdge {
        let scroll = ScrollEdge::new(
            ScrollDeviceId::new(device),
            Some(ScrollSequenceToken::new(token)),
            ScrollPhase::Update,
            Some(ScrollDelta::Lines(
                FiniteScrollVector::new(x, 0.0).expect("test scroll vector is finite"),
            )),
            Authority::Known(momentum),
            Authority::Known(ScrollModifiers::new(shift, false, false, false)),
            Authority::Unknown(delivery_reason),
        )
        .expect("test scroll shape is valid");
        PointerEdge::new_with_delivery(
            PointerEdgeSequence::new(1),
            PointerId::new(7),
            PointerEdgeKind::Scrolled(scroll),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(
                    LogicalPoint::new(96.0, 24.0).expect("test point must be finite"),
                ),
            },
            Authority::Known(PointerEventDeliveryOwner::ProviderEndpoint),
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )
    }

    #[test]
    fn pointer_edge_fidelity_rejects_each_lossless_ingress_field_mismatch() {
        let ingress = edge(
            1,
            7,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            96.0,
            PointerCaptureOwner::ProviderEndpoint,
        );
        assert_eq!(validate_pointer_edge_fidelity(&ingress, &ingress), Ok(()));

        let cases = [
            (
                edge(
                    2,
                    7,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    96.0,
                    PointerCaptureOwner::ProviderEndpoint,
                ),
                "sequence",
            ),
            (
                edge(
                    1,
                    8,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    96.0,
                    PointerCaptureOwner::ProviderEndpoint,
                ),
                "pointer identity",
            ),
            (
                edge(
                    1,
                    7,
                    PointerEdgeKind::ButtonPressed(PointerButton::Secondary),
                    96.0,
                    PointerCaptureOwner::ProviderEndpoint,
                ),
                "kind/button transition",
            ),
            (
                edge(
                    1,
                    7,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    96.0,
                    PointerCaptureOwner::ProviderEndpoint,
                )
                .ending_stream(),
                "pointer stream terminality",
            ),
            (
                edge(
                    1,
                    7,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    97.0,
                    PointerCaptureOwner::ProviderEndpoint,
                ),
                "location/route authority",
            ),
            (
                edge(
                    1,
                    7,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    96.0,
                    PointerCaptureOwner::Foreign,
                ),
                "capture authority",
            ),
        ];
        for (actual, expected_field) in cases {
            assert_eq!(
                validate_pointer_edge_fidelity(&actual, &ingress),
                Err(expected_field)
            );
        }

        let different_delivery = PointerEdge::new_with_delivery(
            ingress.sequence(),
            ingress.pointer(),
            ingress.kind(),
            ingress.location(),
            Authority::Known(PointerEventDeliveryOwner::Foreign),
            ingress.capture_owner(),
        );
        assert_eq!(
            validate_pointer_edge_fidelity(&different_delivery, &ingress),
            Err("event delivery authority")
        );
    }

    #[test]
    fn pointer_edge_fidelity_preserves_every_scroll_payload_authority() {
        let ingress = scroll_edge(
            9,
            41,
            -1.0,
            ScrollMomentum::Direct,
            false,
            AuthorityUnavailableReason::NotReported,
        );
        assert_eq!(validate_pointer_edge_fidelity(&ingress, &ingress), Ok(()));

        let cases = [
            scroll_edge(
                10,
                41,
                -1.0,
                ScrollMomentum::Direct,
                false,
                AuthorityUnavailableReason::NotReported,
            ),
            scroll_edge(
                9,
                42,
                -1.0,
                ScrollMomentum::Direct,
                false,
                AuthorityUnavailableReason::NotReported,
            ),
            scroll_edge(
                9,
                41,
                -2.0,
                ScrollMomentum::Direct,
                false,
                AuthorityUnavailableReason::NotReported,
            ),
            scroll_edge(
                9,
                41,
                -1.0,
                ScrollMomentum::Momentum,
                false,
                AuthorityUnavailableReason::NotReported,
            ),
            scroll_edge(
                9,
                41,
                -1.0,
                ScrollMomentum::Direct,
                true,
                AuthorityUnavailableReason::NotReported,
            ),
            scroll_edge(
                9,
                41,
                -1.0,
                ScrollMomentum::Direct,
                false,
                AuthorityUnavailableReason::ProviderUnavailable,
            ),
        ];
        for actual in cases {
            assert_eq!(
                validate_pointer_edge_fidelity(&actual, &ingress),
                Err("kind/button transition")
            );
        }
    }
}
