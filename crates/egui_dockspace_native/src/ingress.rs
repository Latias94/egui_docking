//! Exact translation from the fork event-loop protocol into dockspace ingress.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::PlatformObservationLease;
use dockspace::backend_ingress::{
    BackendIngressBatch, BackendIngressOrdinal, BackendIngressPayload, BackendIngressRecorder,
    BackendIngressSavepoint,
};
use dockspace::effect::{EffectId, EffectResult, PlatformEffectEmission};
use dockspace::engine::EngineInput;
use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::ids::SurfaceId;
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::EscapeDelivery;
use dockspace::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, InputEffectAcknowledgement,
    ObservedWindow, ObservedWorkArea, PlatformCapabilities, PlatformCapability,
    PlatformCapabilityReason, PlatformRequirement, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCloseObservation, WindowCloseState,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use dockspace::pointer_journal::{
    DesktopRouteFact, DesktopWorkAreaRoute, FiniteScrollVector, PhysicalScrollCoordinates,
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerEventDeliveryOwner, PointerStreamCancelReason, ScrollCancelReason,
    ScrollDeliveryEndpoint, ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers,
    ScrollMomentum, ScrollPhase, ScrollSequenceToken,
};
use dockspace::presentation_observation::PresentationHostLease;
use dockspace::semantic_input::{
    SemanticAccessibilityAction, SemanticDelivery, SemanticKey, SemanticReceiverAction,
    SemanticReceiverEvent,
};
use dockspace::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryObservationGeneration, PlatformSnapshotGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole, WorkAreaGeneration,
    WorkAreaObservationGeneration, WorkAreaToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
};
use dockspace::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
    NativeCloseEdge, SurfaceCloseRequest,
};
use eframe::{
    NativeAccessibilityAction, NativeAuthority, NativeBackendCapabilities, NativeBackendCapability,
    NativeCaptureOwner, NativeCloseState, NativeEffectAcknowledgement, NativeEffectProperty,
    NativeEffectSink, NativeFocusedWindow, NativeHostIngress, NativeHoveredWindow,
    NativeIngressEvent, NativeKey, NativeKeyEdgeKind, NativePhysicalPoint, NativePhysicalRect,
    NativePointerButton, NativePointerDeliveryOwner, NativePointerEdge, NativePointerEdgeKind,
    NativePointerIdentity, NativePointerInputState, NativePresentationState,
    NativeScrollCancelReason, NativeScrollDelta, NativeScrollEdge, NativeScrollMomentum,
    NativeScrollPhase, NativeUnavailableReason, NativeViewportBinding, NativeViewportCreateSink,
    NativeWindowSnapshot, NativeWorkAreaRosterObservation, NativeWorkAreaRoute,
};
use egui::ViewportId;
use egui_dockspace::{
    Dockspace, ExactNativeViewport, NativeBindingRoster, NativeCoreRoute,
    NativeViewportIncarnation, PaintReceiverFingerprint, PaintReceiverLookup,
};

use crate::effects::NativeEffectDriver;
use crate::presentation::{
    EdgePointerGraphs, NativePresentationLedger, NativePresentationPrepareSavepoint,
    PresentedNativePointerGraph,
};
use crate::{NativeRuntimeError, NativeViewportRoster};

#[derive(Clone, Copy, Debug)]
pub(crate) struct BoundNativeRoute {
    native_binding: NativeViewportBinding,
    exact: ExactNativeViewport,
    surface: SurfaceId,
    core: ViewportBinding,
}

impl BoundNativeRoute {
    pub(crate) const fn exact(self) -> ExactNativeViewport {
        self.exact
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn core(self) -> ViewportBinding {
        self.core
    }

    pub(crate) const fn native_binding(self) -> NativeViewportBinding {
        self.native_binding
    }

    const fn adapter_route(self) -> NativeCoreRoute {
        NativeCoreRoute::new(self.exact, self.surface, self.core)
    }
}

#[derive(Clone, Copy, Debug)]
struct RetiredNativeRoute {
    route: BoundNativeRoute,
    close_acknowledgement: CloseEffectAcknowledgement,
}

impl RetiredNativeRoute {
    const fn exact(self) -> ExactNativeViewport {
        self.route.exact
    }

    const fn surface(self) -> SurfaceId {
        self.route.surface
    }
}

pub(crate) struct PreparedNativeIngress {
    pub(crate) batch: BackendIngressBatch,
    pub(crate) bindings: NativeBindingRoster,
    pub(crate) routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pub(crate) pointer_edges: BTreeMap<u64, RetainedPointerEdge>,
    pub(crate) presentation_host: PresentationHostLease,
    pub(crate) transaction: NativeIngressTransaction,
}

struct PreparedNativeIngressPayload {
    batch: BackendIngressBatch,
    bindings: NativeBindingRoster,
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pointer_edges: BTreeMap<u64, RetainedPointerEdge>,
}

/// Affine rollback boundary for every bridge mutation staged by one host cycle.
#[must_use = "a native ingress transaction must be committed or rolled back"]
pub(crate) struct NativeIngressTransaction {
    recorder: BackendIngressSavepoint,
    state: NativeIngressStateSnapshot,
    presentation: NativePresentationPrepareSavepoint,
}

#[derive(Clone)]
pub(crate) struct RetainedPointerEdge {
    edge: NativePointerEdge,
    delivery_route: Option<BoundNativeRoute>,
    hovered_route: Option<BoundNativeRoute>,
    graphs: EdgePointerGraphs,
}

impl RetainedPointerEdge {
    pub(crate) const fn edge(&self) -> &NativePointerEdge {
        &self.edge
    }

    pub(crate) const fn hovered_route(&self) -> Option<BoundNativeRoute> {
        self.hovered_route
    }

    pub(crate) const fn delivery_route(&self) -> Option<BoundNativeRoute> {
        self.delivery_route
    }

    pub(crate) const fn graphs(&self) -> &EdgePointerGraphs {
        &self.graphs
    }
}

#[derive(Clone)]
struct BackendSidecarLedger<T> {
    entries: BTreeMap<u64, T>,
}

impl<T> Default for BackendSidecarLedger<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<T> BackendSidecarLedger<T> {
    fn insert(&mut self, ordinal: u64, sidecar: T) -> Option<T> {
        self.entries.insert(ordinal, sidecar)
    }

    fn get(&self, ordinal: u64) -> Option<&T> {
        self.entries.get(&ordinal)
    }

    fn latest_through(&self, committed: u64) -> Option<&T> {
        self.entries
            .range(..=committed)
            .next_back()
            .map(|(_, sidecar)| sidecar)
    }

    fn retire_through(&mut self, committed: u64) {
        self.entries.retain(|ordinal, _| *ordinal > committed);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Clone, Debug)]
struct NativeWorkAreaCommit {
    native_generation: u64,
    roster: Option<Vec<ObservedWorkArea>>,
}

#[derive(Clone, Debug)]
struct AcceptedWorkAreaAuthority {
    native_generation: u64,
    provider: PlatformObservationLease,
    generation: WorkAreaGeneration,
    tokens: BTreeSet<WorkAreaToken>,
}

/// Single-writer bridge between one fork coordinator and one dockspace provider.
pub(crate) struct NativeIngressBridge {
    recorder: Option<BackendIngressRecorder>,
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pending_registrations: BTreeMap<SurfaceId, BackendIngressOrdinal>,
    effects: NativeEffectDriver,
    pointer_ids: BTreeMap<(u64, u64), PointerId>,
    next_pointer_id: u64,
    observation_generation: u64,
    native_ingress_through: Option<u64>,
    pointer_sidecars: BackendSidecarLedger<RetainedPointerEdge>,
    work_area_sidecars: BackendSidecarLedger<NativeWorkAreaCommit>,
    accepted_work_areas: Option<AcceptedWorkAreaAuthority>,
    post_commit_records: Vec<PostCommitRecord>,
}

#[derive(Clone)]
enum PostCommitRecord {
    Effect(EffectResult),
    Semantic(EngineInput),
}

/// Cycle-local native effect state awaiting the host seal.
///
/// Requests may reserve fork sink lanes while this value is live, but the
/// bridge does not publish their correlation state until the application commit
/// hook runs. Dropping the value therefore leaves the live bridge unchanged.
pub(crate) struct NativeEffectCycle {
    effects: NativeEffectDriver,
    post_commit_records: Vec<PostCommitRecord>,
}

#[derive(Clone)]
struct NativeIngressStateSnapshot {
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pending_registrations: BTreeMap<SurfaceId, BackendIngressOrdinal>,
    effects: NativeEffectDriver,
    pointer_ids: BTreeMap<(u64, u64), PointerId>,
    next_pointer_id: u64,
    observation_generation: u64,
    native_ingress_through: Option<u64>,
    pointer_sidecars: BackendSidecarLedger<RetainedPointerEdge>,
    work_area_sidecars: BackendSidecarLedger<NativeWorkAreaCommit>,
    accepted_work_areas: Option<AcceptedWorkAreaAuthority>,
    post_commit_records: Vec<PostCommitRecord>,
}

impl NativeIngressBridge {
    pub(crate) fn new() -> Result<Self, NativeRuntimeError> {
        Ok(Self {
            recorder: None,
            routes: BTreeMap::new(),
            pending_registrations: BTreeMap::new(),
            effects: NativeEffectDriver::new()?,
            pointer_ids: BTreeMap::new(),
            next_pointer_id: 0,
            observation_generation: 0,
            native_ingress_through: None,
            pointer_sidecars: BackendSidecarLedger::default(),
            work_area_sidecars: BackendSidecarLedger::default(),
            accepted_work_areas: None,
            post_commit_records: Vec::new(),
        })
    }

    pub(crate) fn prepare_cycle(
        &mut self,
        dockspace: &mut Dockspace,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
        presentations: &mut NativePresentationLedger,
    ) -> Result<PreparedNativeIngress, NativeRuntimeError> {
        self.ensure_provider(dockspace, ingress)?;
        self.reclaim_committed_prefix(dockspace)?;
        self.validate_native_order(ingress)?;

        let recorder = self
            .recorder
            .as_ref()
            .expect("the provider was enrolled above")
            .savepoint();
        let presentation = presentations.prepare_savepoint();
        let state = self.state_snapshot();
        let presentation_host = self
            .recorder
            .as_ref()
            .expect("the provider was enrolled above")
            .lease()
            .presentation_host();
        let result = self.prepare_cycle_inner(dockspace, ingress, configured, presentations);
        match result {
            Ok(prepared) => Ok(PreparedNativeIngress {
                batch: prepared.batch,
                bindings: prepared.bindings,
                routes: prepared.routes,
                pointer_edges: prepared.pointer_edges,
                presentation_host,
                transaction: NativeIngressTransaction {
                    recorder,
                    state,
                    presentation,
                },
            }),
            Err(error) => {
                dockspace.rollback_backend_ingress_to(
                    self.recorder
                        .as_mut()
                        .expect("the provider was enrolled above"),
                    recorder,
                )?;
                self.restore_state(state);
                presentations.rollback_prepare(presentation);
                Err(error)
            }
        }
    }

    fn prepare_cycle_inner(
        &mut self,
        dockspace: &mut Dockspace,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
        presentations: &mut NativePresentationLedger,
    ) -> Result<PreparedNativeIngressPayload, NativeRuntimeError> {
        {
            let recorder = self
                .recorder
                .as_mut()
                .expect("the provider was enrolled before preparing native ingress");
            let _ = dockspace.record_ready_backend_pane_focus_observations(recorder)?;
        }
        self.flush_post_commit_records()?;
        let mut routes = self.routes.clone();
        let mut retirements = Vec::new();
        let mut observed_snapshot = false;
        let mut submitted_pointer_segment = false;

        for record in ingress.ordered().records() {
            match record.event() {
                NativeIngressEvent::PointerEdge(edge) => {
                    let sidecar = RetainedPointerEdge {
                        edge: edge.clone(),
                        delivery_route: delivery_route(edge, &routes),
                        hovered_route: hovered_route(edge, &routes),
                        graphs: presentations.capture_edge(edge),
                    };
                    let edge = self.translate_pointer_edge(&sidecar, &routes)?;
                    let previous = PointerEdgeSequence::new(
                        edge.sequence()
                            .get()
                            .checked_sub(1)
                            .ok_or(NativeRuntimeError::PointerSequenceMismatch)?,
                    );
                    let journal = PointerEdgeJournal::new(previous, edge.sequence(), vec![edge])?;
                    let ordinal = self
                        .recorder
                        .as_mut()
                        .expect("the provider was enrolled above")
                        .record_pointer_segment(journal)?;
                    if self
                        .pointer_sidecars
                        .insert(ordinal.get(), sidecar)
                        .is_some()
                    {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "one backend ordinal named multiple pointer sidecars",
                        ));
                    }
                    submitted_pointer_segment = true;
                }
                NativeIngressEvent::KeyEdge(edge) => {
                    let native = edge.binding();
                    let route = routes
                        .get(&native.viewport_id())
                        .filter(|route| route.native_binding() == native)
                        .copied()
                        .ok_or(NativeRuntimeError::IngressUnavailable(
                            "native key edge has no exact current core binding route",
                        ))?;
                    if edge.key() == NativeKey::Escape {
                        if edge.kind() == NativeKeyEdgeKind::Pressed {
                            self.recorder
                                .as_mut()
                                .expect("the provider was enrolled above")
                                .record_semantic_input(
                                    EngineInput::CancelActiveInteractionWithEscape {
                                        expected: dockspace.engine().version(),
                                        delivery: EscapeDelivery::NativeBinding(route.core()),
                                    },
                                )?;
                        }
                        continue;
                    }
                    if edge.kind() == NativeKeyEdgeKind::Released
                        || edge.kind() == NativeKeyEdgeKind::Repeated
                            && matches!(edge.key(), NativeKey::Enter | NativeKey::Space)
                    {
                        continue;
                    }
                    let Some(graph) = edge.presentation().value() else {
                        continue;
                    };
                    let Some(receiver) = graph.focused_receiver() else {
                        continue;
                    };
                    let Some(key) = semantic_key(edge.key()) else {
                        continue;
                    };
                    let Some(event) = resolve_semantic_receiver_event(
                        dockspace,
                        presentations,
                        route,
                        graph,
                        receiver,
                        SemanticReceiverAction::Key(key),
                    ) else {
                        continue;
                    };
                    self.recorder
                        .as_mut()
                        .expect("the provider was enrolled above")
                        .record_semantic_input(EngineInput::ActivateSemanticReceiver {
                            expected: dockspace.engine().version(),
                            event,
                        })?;
                }
                NativeIngressEvent::AccessibilityEdge(edge) => {
                    let native = edge.binding();
                    let route = routes
                        .get(&native.viewport_id())
                        .filter(|route| route.native_binding() == native)
                        .copied()
                        .ok_or(NativeRuntimeError::IngressUnavailable(
                            "native accessibility edge has no exact current core binding route",
                        ))?;
                    let Some(graph) = edge.presentation().value() else {
                        continue;
                    };
                    let Some(receiver) = graph.receiver_for_accesskit_node(edge.target()) else {
                        continue;
                    };
                    let action = match edge.action() {
                        NativeAccessibilityAction::Click => SemanticAccessibilityAction::Click,
                        NativeAccessibilityAction::Focus => SemanticAccessibilityAction::Focus,
                        NativeAccessibilityAction::Increment => {
                            SemanticAccessibilityAction::Increment
                        }
                        NativeAccessibilityAction::Decrement => {
                            SemanticAccessibilityAction::Decrement
                        }
                        NativeAccessibilityAction::ScrollIntoView => {
                            SemanticAccessibilityAction::ScrollIntoView
                        }
                    };
                    let Some(event) = resolve_semantic_receiver_event(
                        dockspace,
                        presentations,
                        route,
                        graph,
                        receiver,
                        SemanticReceiverAction::Accessibility(action),
                    ) else {
                        continue;
                    };
                    self.recorder
                        .as_mut()
                        .expect("the provider was enrolled above")
                        .record_semantic_input(EngineInput::ActivateSemanticReceiver {
                            expected: dockspace.engine().version(),
                            event,
                        })?;
                }
                NativeIngressEvent::CloseObservation(observation) => {
                    let native = observation.binding();
                    let route = routes
                        .values()
                        .find(|route| route.native_binding == native)
                        .copied()
                        .or_else(|| {
                            self.effects
                                .route_for_new_binding(native)
                                .map(|(surface, core)| BoundNativeRoute {
                                    native_binding: native,
                                    exact: exact_native(native),
                                    surface,
                                    core,
                                })
                        })
                        .or_else(|| {
                            let spec = configured.get(native.viewport_id())?;
                            if spec.role() != ViewportRole::Child
                                || spec.bootstrap_token().is_none()
                            {
                                return None;
                            }
                            let core = dockspace.native_viewport_binding(spec.surface())?;
                            (Some(core.token()) == spec.bootstrap_token()).then(|| {
                                BoundNativeRoute {
                                    native_binding: native,
                                    exact: exact_native(native),
                                    surface: spec.surface(),
                                    core,
                                }
                            })
                        })
                        .ok_or(NativeRuntimeError::IngressUnavailable(
                            "native close observation has no exact core binding route",
                        ))?;
                    if route.exact().viewport() == ViewportId::ROOT {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "root process shutdown is outside the dockspace child-close lane",
                        ));
                    }
                    routes.entry(route.exact().viewport()).or_insert(route);
                    let generation = self.next_observation_generation()?;
                    let close = WindowCloseObservation::new(
                        route.core(),
                        CloseObservationGeneration::new(generation),
                        translate_authority(observation.value(), |state| match state {
                            NativeCloseState::LiveClear => WindowCloseState::LiveClear,
                            NativeCloseState::LiveRequested => WindowCloseState::LiveRequested,
                            NativeCloseState::Destroyed => WindowCloseState::Destroyed,
                        }),
                        translate_close_acknowledgement(
                            &self.effects,
                            observation.acknowledgement(),
                            native,
                        ),
                    );
                    self.recorder
                        .as_mut()
                        .expect("the provider was enrolled above")
                        .record_native_close_observation(
                            dockspace.engine().version().epoch(),
                            close,
                        )?;
                }
                NativeIngressEvent::PlatformSnapshot(_) => {
                    if observed_snapshot {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "one native ingress batch contained multiple platform snapshots",
                        ));
                    }
                    observed_snapshot = true;
                    self.reconcile_snapshot_routes(
                        dockspace,
                        ingress,
                        configured,
                        &retirements,
                        &mut routes,
                    )?;
                    let (snapshot, work_area_commit) = self.translate_platform_snapshot(
                        dockspace,
                        ingress,
                        &routes,
                        &retirements,
                    )?;
                    let ordinal = self
                        .recorder
                        .as_mut()
                        .expect("the provider was enrolled above")
                        .record_platform_snapshot(dockspace.engine().version().epoch(), snapshot)?;
                    if self
                        .work_area_sidecars
                        .insert(ordinal.get(), work_area_commit)
                        .is_some()
                    {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "one backend ordinal named multiple work-area snapshots",
                        ));
                    }
                }
                NativeIngressEvent::PresentationResult(result) => {
                    let recorder = self.recorder.as_mut().expect("provider enrolled above");
                    presentations.consume_ordered(dockspace, recorder, result)?;
                }
                NativeIngressEvent::EffectResult(result) => {
                    let recorder = self.recorder.as_mut().expect("provider enrolled above");
                    self.effects.consume_effect_result(result, recorder)?;
                }
                NativeIngressEvent::ViewportCreateResult(result) => {
                    let recorder = self.recorder.as_mut().expect("provider enrolled above");
                    self.effects.consume_create_result(result, recorder)?;
                }
                NativeIngressEvent::Retirement(tombstone) => {
                    let native = tombstone.binding();
                    if let Some((viewport, route)) = routes
                        .iter()
                        .find(|(_, route)| route.native_binding == native)
                        .map(|(viewport, route)| (*viewport, *route))
                    {
                        routes.remove(&viewport);
                        let close_acknowledgement = translate_lifecycle_acknowledgement(
                            &self.effects,
                            tombstone.acknowledgement(),
                            native,
                        );
                        self.effects.forget_native(route.exact);
                        let recorder = self.recorder.as_mut().expect("provider enrolled above");
                        presentations.retire(dockspace, recorder, native)?;
                        retirements.push(RetiredNativeRoute {
                            route,
                            close_acknowledgement,
                        });
                    }
                }
                NativeIngressEvent::RetirementQuiesced(quiesced) => {
                    // This proof closes only the fork renderer's presentation
                    // lane. Core route, effect, and close tombstones have
                    // independent producers and therefore independent
                    // retention boundaries.
                    presentations.retirement_quiesced(exact_native(quiesced.binding()));
                }
            }
        }

        if !observed_snapshot {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress batch omitted its atomic platform snapshot",
            ));
        }
        // The restored surface already owns workspace content. Failing inside this savepoint keeps
        // the document intact for explicit recovery instead of committing a windowless runtime.
        if let Some(error) = self.effects.restored_create_terminal_error() {
            return Err(error);
        }
        let watermark = PointerEdgeSequence::new(ingress.pointer_journal().through().get());
        if let Some(journal) = empty_pointer_interval(submitted_pointer_segment, watermark)? {
            self.recorder
                .as_mut()
                .expect("the provider was enrolled above")
                .record_pointer_segment(journal)?;
        }
        self.native_ingress_through = Some(ingress.ordered().through().get());
        self.effects.reconcile_restored_routes(&routes);
        let live_routes = routes.clone();
        self.routes = routes;

        let bindings = NativeBindingRoster::new(
            live_routes
                .values()
                .copied()
                .map(BoundNativeRoute::adapter_route),
            retirements.iter().copied().map(RetiredNativeRoute::exact),
        );
        let batch = self
            .recorder
            .as_ref()
            .expect("the provider was enrolled above")
            .pending_batch()?;
        let pointer_edges = self.pending_pointer_sidecars(&batch)?;
        Ok(PreparedNativeIngressPayload {
            batch,
            bindings,
            routes: live_routes,
            pointer_edges,
        })
    }

    fn state_snapshot(&self) -> NativeIngressStateSnapshot {
        NativeIngressStateSnapshot {
            routes: self.routes.clone(),
            pending_registrations: self.pending_registrations.clone(),
            effects: self.effects.clone(),
            pointer_ids: self.pointer_ids.clone(),
            next_pointer_id: self.next_pointer_id,
            observation_generation: self.observation_generation,
            native_ingress_through: self.native_ingress_through,
            pointer_sidecars: self.pointer_sidecars.clone(),
            work_area_sidecars: self.work_area_sidecars.clone(),
            accepted_work_areas: self.accepted_work_areas.clone(),
            post_commit_records: self.post_commit_records.clone(),
        }
    }

    fn restore_state(&mut self, snapshot: NativeIngressStateSnapshot) {
        self.routes = snapshot.routes;
        self.pending_registrations = snapshot.pending_registrations;
        self.effects = snapshot.effects;
        self.pointer_ids = snapshot.pointer_ids;
        self.next_pointer_id = snapshot.next_pointer_id;
        self.observation_generation = snapshot.observation_generation;
        self.native_ingress_through = snapshot.native_ingress_through;
        self.pointer_sidecars = snapshot.pointer_sidecars;
        self.work_area_sidecars = snapshot.work_area_sidecars;
        self.accepted_work_areas = snapshot.accepted_work_areas;
        self.post_commit_records = snapshot.post_commit_records;
    }

    pub(crate) fn commit_transaction(
        &self,
        presentations: &NativePresentationLedger,
        transaction: NativeIngressTransaction,
    ) {
        debug_assert_eq!(
            self.recorder.as_ref().map(BackendIngressRecorder::lease),
            Some(transaction.recorder.lease())
        );
        presentations.commit_prepare(transaction.presentation);
    }

    pub(crate) fn rollback_transaction(
        &mut self,
        dockspace: &mut Dockspace,
        presentations: &mut NativePresentationLedger,
        transaction: NativeIngressTransaction,
    ) {
        dockspace
            .rollback_backend_ingress_to(
                self.recorder
                    .as_mut()
                    .expect("an active native transaction retains its recorder"),
                transaction.recorder,
            )
            .expect("an aborted native transaction cannot reclaim its recorder prefix");
        self.restore_state(transaction.state);
        presentations.rollback_prepare(transaction.presentation);
    }

    pub(crate) fn begin_effect_cycle(&self) -> NativeEffectCycle {
        NativeEffectCycle {
            effects: self.effects.clone(),
            post_commit_records: Vec::new(),
        }
    }

    pub(crate) fn dispatch_effects(
        cycle: &mut NativeEffectCycle,
        emissions: &[PlatformEffectEmission],
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
        effect_sink: &NativeEffectSink,
        create_sink: &NativeViewportCreateSink,
    ) {
        let terminal = cycle
            .effects
            .dispatch(emissions, routes, catalog, effect_sink, create_sink);
        cycle
            .post_commit_records
            .extend(terminal.into_iter().map(PostCommitRecord::Effect));
    }

    pub(crate) fn schedule_restored_viewports(
        cycle: &mut NativeEffectCycle,
        pending_viewports: &BTreeSet<ViewportId>,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        catalog: &NativeViewportRoster,
        create_sink: &NativeViewportCreateSink,
    ) -> Result<(), NativeRuntimeError> {
        cycle
            .effects
            .schedule_restored_creates(pending_viewports, routes, catalog, create_sink)
    }

    pub(crate) fn commit_effect_cycle(&mut self, cycle: NativeEffectCycle) {
        self.effects = cycle.effects;
        self.post_commit_records.extend(cycle.post_commit_records);
    }

    pub(crate) fn restored_viewport_is_materialized(&self, viewport: ViewportId) -> bool {
        self.effects.restored_create_is_materialized(viewport)
    }

    pub(crate) fn has_post_commit_records(&self) -> bool {
        !self.post_commit_records.is_empty()
    }

    pub(crate) fn queue_surface_close_request(
        &mut self,
        dockspace: &Dockspace,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
    ) {
        self.post_commit_records.push(PostCommitRecord::Semantic(
            EngineInput::RequestSurfaceClose {
                expected: dockspace.engine().version(),
                edge,
                request,
            },
        ));
    }

    pub(crate) fn queue_close_resolution(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) {
        self.post_commit_records
            .push(PostCommitRecord::Semantic(EngineInput::ResolveClose {
                request,
                token,
                decision,
            }));
    }

    pub(crate) fn queue_deferred_close_resolution(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) {
        self.post_commit_records.push(PostCommitRecord::Semantic(
            EngineInput::ContinueDeferredClose {
                request,
                token,
                decision,
            },
        ));
    }

    fn flush_post_commit_records(&mut self) -> Result<(), NativeRuntimeError> {
        let records = std::mem::take(&mut self.post_commit_records);
        let recorder = self
            .recorder
            .as_mut()
            .expect("the provider was enrolled before flushing the native outbox");
        for record in records {
            match record {
                PostCommitRecord::Effect(result) => {
                    recorder.record_platform_effect_result(result)?;
                }
                PostCommitRecord::Semantic(input) => {
                    recorder.record_semantic_input(input)?;
                }
            }
        }
        Ok(())
    }

    fn ensure_provider(
        &mut self,
        dockspace: &mut Dockspace,
        ingress: &NativeHostIngress,
    ) -> Result<(), NativeRuntimeError> {
        if self.recorder.is_none() {
            let previous = PointerEdgeSequence::new(ingress.pointer_journal().previous().get());
            self.recorder = Some(dockspace.create_backend_ingress_provider(previous)?);
        }
        Ok(())
    }

    fn reclaim_committed_prefix(
        &mut self,
        dockspace: &Dockspace,
    ) -> Result<(), NativeRuntimeError> {
        let Some(recorder) = self.recorder.as_mut() else {
            return Ok(());
        };
        let committed_through = dockspace
            .engine()
            .backend_ingress_commit_watermark()
            .map(|watermark| watermark.through());
        if let Some(watermark) = dockspace.engine().backend_ingress_commit_watermark() {
            let work_area_commit = self
                .work_area_sidecars
                .latest_through(watermark.through().get())
                .cloned();
            recorder.retire_committed_prefix(watermark)?;
            self.pointer_sidecars
                .retire_through(watermark.through().get());
            if let Some(work_area_commit) = work_area_commit {
                self.accepted_work_areas =
                    accepted_work_area_authority(dockspace, &work_area_commit);
            }
            self.work_area_sidecars
                .retire_through(watermark.through().get());
        }
        self.pending_registrations.retain(|surface, ordinal| {
            dockspace.native_viewport_binding(*surface).is_none()
                && committed_through.is_none_or(|committed| *ordinal > committed)
        });
        Ok(())
    }

    fn pending_pointer_sidecars(
        &self,
        batch: &BackendIngressBatch,
    ) -> Result<BTreeMap<u64, RetainedPointerEdge>, NativeRuntimeError> {
        let mut pending = BTreeMap::new();
        for record in batch.records() {
            let BackendIngressPayload::PointerSegment(segment) = record.payload() else {
                continue;
            };
            let Some(edge) = segment.edges().first() else {
                continue;
            };
            let sidecar = self
                .pointer_sidecars
                .get(record.ordinal().get())
                .cloned()
                .ok_or(NativeRuntimeError::IngressUnavailable(
                    "a pending pointer edge lost its event-time receiver sidecar",
                ))?;
            if pending.insert(edge.sequence().get(), sidecar).is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "one pending batch repeated a pointer edge sequence",
                ));
            }
        }
        Ok(pending)
    }

    fn validate_native_order(&self, ingress: &NativeHostIngress) -> Result<(), NativeRuntimeError> {
        let submitted = ingress.ordered().previous().get();
        if let Some(expected) = self.native_ingress_through
            && submitted != expected
        {
            return Err(NativeRuntimeError::NativeIngressOrderMismatch {
                expected,
                submitted,
            });
        }
        Ok(())
    }

    fn reconcile_snapshot_routes(
        &mut self,
        dockspace: &Dockspace,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
        retirements: &[RetiredNativeRoute],
        routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<(), NativeRuntimeError> {
        let live = ingress
            .platform()
            .inventory()
            .iter()
            .map(|binding| (binding.viewport_id(), *binding))
            .collect::<BTreeMap<_, _>>();

        for (viewport, route) in routes.iter() {
            if live.get(viewport) != Some(&route.native_binding) {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "atomic native snapshot changed a routed lifetime without an ordered retirement",
                ));
            }
        }

        if let Some(native_binding) = live.get(&ViewportId::ROOT).copied()
            && !routes.contains_key(&ViewportId::ROOT)
        {
            let root = configured.root();
            if !retirements
                .iter()
                .any(|retired| retired.surface() == root.surface())
            {
                if let Some(core) = dockspace.native_viewport_binding(root.surface()) {
                    if Some(core.token()) != root.bootstrap_token() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "core root binding token differs from the bootstrap token",
                        ));
                    }
                    routes.insert(
                        ViewportId::ROOT,
                        BoundNativeRoute {
                            native_binding,
                            exact: exact_native(native_binding),
                            surface: root.surface(),
                            core,
                        },
                    );
                } else if !self.pending_registrations.contains_key(&root.surface()) {
                    let ordinal = dockspace.record_backend_viewport_registration(
                        self.recorder
                            .as_mut()
                            .expect("the provider was enrolled above"),
                        root.surface(),
                        root.bootstrap_token()
                            .ok_or(NativeRuntimeError::MissingRootViewport)?,
                        ViewportRole::Root,
                        None,
                    )?;
                    self.pending_registrations.insert(root.surface(), ordinal);
                }
            }
        }

        for (viewport, native_binding) in live {
            if viewport == ViewportId::ROOT {
                continue;
            }
            if let Some(route) = routes.get(&viewport) {
                if route.native_binding != native_binding {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native viewport incarnation changed without retirement",
                    ));
                }
                continue;
            }

            let routed = self.effects.route_for_new_binding(native_binding);
            let restored = configured.get(viewport).filter(|spec| {
                spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some()
            });
            let Some((surface, core)) = routed.or_else(|| {
                let spec = restored?;
                dockspace
                    .native_viewport_binding(spec.surface())
                    .map(|binding| (spec.surface(), binding))
            }) else {
                if let Some(spec) = restored
                    && !retirements
                        .iter()
                        .any(|retired| retired.surface() == spec.surface())
                    && !self.pending_registrations.contains_key(&spec.surface())
                {
                    let recovery =
                        spec.recovery_bootstrap()
                            .ok_or(NativeRuntimeError::IngressUnavailable(
                                "restored child omitted its recovery bootstrap",
                            ))?;
                    let ordinal = dockspace.record_backend_child_viewport_bootstrap(
                        self.recorder
                            .as_mut()
                            .expect("the provider was enrolled above"),
                        spec.surface(),
                        spec.bootstrap_token().expect("restored child has a token"),
                        recovery,
                    )?;
                    self.pending_registrations.insert(spec.surface(), ordinal);
                }
                // Unknown physical children remain outside dockspace authority. A restored
                // child joins only after its core registration commits on a prior cycle.
                continue;
            };
            if let Some(spec) = restored
                && routed.is_none()
                && Some(core.token()) != spec.bootstrap_token()
            {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "core child binding token differs from the restored token",
                ));
            }
            routes.insert(
                viewport,
                BoundNativeRoute {
                    native_binding,
                    exact: exact_native(native_binding),
                    surface,
                    core,
                },
            );
        }
        Ok(())
    }

    fn translate_platform_snapshot(
        &mut self,
        dockspace: &Dockspace,
        ingress: &NativeHostIngress,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        retirements: &[RetiredNativeRoute],
    ) -> Result<(PlatformSnapshot, NativeWorkAreaCommit), NativeRuntimeError> {
        let generation = self.next_observation_generation()?;
        let by_native = routes
            .values()
            .map(|route| (route.native_binding, *route))
            .collect::<BTreeMap<_, _>>();
        let focus_routes = by_native
            .values()
            .filter_map(|route| {
                (dockspace.engine().viewport_focus_binding(route.surface()) == Some(route.core()))
                    .then_some(route.native_binding())
            })
            .collect::<BTreeSet<_>>();

        let capabilities = translate_capability_roster(
            ingress.platform().capabilities(),
            work_area_capability(ingress.platform().work_areas()),
        );
        let capability_observation = CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(capabilities),
        );

        let mut windows = Vec::new();
        let mut closes = Vec::new();
        for native_window in ingress.platform().windows() {
            let Some(route) = by_native.get(&native_window.binding()).copied() else {
                continue;
            };
            let mut window = ObservedWindow::new(route.core);
            window = window.with_coordinate_observation(translate_coordinates(
                route.core,
                generation,
                native_window.geometry().value(),
            )?);
            let input = WindowInputObservation::new(
                route.core,
                InputObservationGeneration::new(generation),
                translate_authority(native_window.pointer_input().value(), |state| match state {
                    NativePointerInputState::ReceivesInput => WindowInputState::ReceivesInput,
                    NativePointerInputState::PassThrough => WindowInputState::PassThrough,
                }),
                translate_input_acknowledgement(
                    &self.effects,
                    native_window.pointer_input().acknowledgement(),
                    native_window.binding(),
                ),
            );
            window = window.with_input_observation(input);
            let presentation = WindowPresentationObservation::new(
                route.core,
                PresentationObservationGeneration::new(generation),
                translate_authority(native_window.presentation().value(), |state| match state {
                    NativePresentationState::Visible => WindowPresentationState::Visible,
                    NativePresentationState::Hidden => WindowPresentationState::Hidden,
                    NativePresentationState::Minimized => WindowPresentationState::Minimized,
                }),
                translate_presentation_acknowledgement(
                    &self.effects,
                    native_window.presentation().acknowledgement(),
                    native_window.binding(),
                ),
            );
            window = window.with_presentation_observation(presentation);
            let close_state =
                translate_authority(native_window.close().value(), |state| match state {
                    NativeCloseState::LiveClear => WindowCloseState::LiveClear,
                    NativeCloseState::LiveRequested => WindowCloseState::LiveRequested,
                    NativeCloseState::Destroyed => WindowCloseState::Destroyed,
                });
            window = window.with_close_requested(match close_state {
                Authority::Known(WindowCloseState::LiveClear) => Authority::Known(false),
                Authority::Known(WindowCloseState::LiveRequested) => Authority::Known(true),
                Authority::Known(WindowCloseState::Destroyed) => {
                    Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
                }
                Authority::Unknown(reason) => Authority::Unknown(reason),
            });
            closes.push(WindowCloseObservation::new(
                route.core,
                CloseObservationGeneration::new(generation),
                close_state,
                translate_close_acknowledgement(
                    &self.effects,
                    native_window.close().acknowledgement(),
                    native_window.binding(),
                ),
            ));
            windows.push(window);
        }
        closes.extend(retirements.iter().map(|retired| {
            WindowCloseObservation::new(
                retired.route.core,
                CloseObservationGeneration::new(generation),
                Authority::Known(WindowCloseState::Destroyed),
                retired.close_acknowledgement,
            )
        }));

        let inventory = WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(windows.iter().map(ObservedWindow::binding).collect()),
        )?;
        let focus = FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            translate_focus(ingress.platform().focused(), &by_native, &focus_routes),
            translate_focus_acknowledgement(
                &self.effects,
                ingress.platform().focused(),
                ingress.platform().windows(),
                &focus_routes,
            ),
        );
        let (work_areas, work_area_commit) =
            translate_work_area_roster(ingress.platform().work_areas(), generation)?;
        let snapshot = PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(generation),
            capability_observation,
            focus,
            inventory,
            windows,
            closes,
            work_areas,
        )?;
        Ok((snapshot, work_area_commit))
    }

    fn translate_pointer_edge(
        &mut self,
        retained: &RetainedPointerEdge,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<PointerEdge, NativeRuntimeError> {
        let edge = retained.edge();
        let sequence = PointerEdgeSequence::new(edge.sequence().get());
        let pointer = self.pointer_id(edge.identity())?;
        let kind = match edge.kind() {
            NativePointerEdgeKind::Moved => PointerEdgeKind::Moved,
            NativePointerEdgeKind::ButtonPressed(button) => {
                PointerEdgeKind::ButtonPressed(translate_button(button))
            }
            NativePointerEdgeKind::ButtonReleased(button) => {
                PointerEdgeKind::ButtonReleased(translate_button(button))
            }
            NativePointerEdgeKind::CaptureChanged => PointerEdgeKind::CaptureChanged,
            NativePointerEdgeKind::Cancelled => PointerEdgeKind::StreamCancelled(
                PointerStreamCancelReason::ExplicitPlatformCancellation,
            ),
            NativePointerEdgeKind::Scrolled(scroll) => {
                PointerEdgeKind::Scrolled(translate_scroll_edge(
                    self.recorder
                        .as_ref()
                        .expect("the provider was enrolled above")
                        .lease()
                        .presentation_host(),
                    scroll,
                    edge.delivery_owner(),
                    routes,
                    retained.graphs().delivery(),
                    event_time_physical_scroll_coordinates(retained),
                )?)
            }
        };
        let position = transpose_geometry(translate_authority(edge.position(), translate_point))?;
        let route = match edge.hovered().value() {
            Some(NativeHoveredWindow::Viewport(binding)) => routes
                .get(&binding.viewport_id())
                .copied()
                .filter(|route| route.native_binding == *binding)
                .map_or_else(
                    || {
                        DesktopRouteFact::unknown(
                            position,
                            AuthorityUnavailableReason::SurfaceUnavailable,
                        )
                    },
                    |route| DesktopRouteFact::dock_from_desktop_position(route.core, position),
                ),
            Some(NativeHoveredWindow::Foreign) => DesktopRouteFact::foreign(position),
            Some(NativeHoveredWindow::None) => DesktopRouteFact::no_window(
                position,
                translate_work_area_route(edge.work_area(), self.accepted_work_areas.as_ref()),
            ),
            None => DesktopRouteFact::unknown(
                position,
                map_unavailable(
                    edge.hovered()
                        .unavailable_reason()
                        .unwrap_or(NativeUnavailableReason::NotObserved),
                ),
            ),
        };
        let capture_owner = match edge.capture().value() {
            Some(NativeCaptureOwner::Viewport(binding)) => routes
                .get(&binding.viewport_id())
                .copied()
                .filter(|route| route.native_binding == *binding)
                .map_or(
                    Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
                    |route| Authority::Known(PointerCaptureOwner::Native(route.core)),
                ),
            Some(NativeCaptureOwner::Foreign) => Authority::Known(PointerCaptureOwner::Foreign),
            Some(NativeCaptureOwner::None) => Authority::Known(PointerCaptureOwner::None),
            None => Authority::Unknown(map_unavailable(
                edge.capture()
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            )),
        };
        let delivery_owner = translate_delivery_owner(edge.delivery_owner(), routes);
        let stream_terminal =
            edge.ends_stream() || matches!(kind, PointerEdgeKind::StreamCancelled(_));
        let mut translated = PointerEdge::new_with_delivery(
            sequence,
            pointer,
            kind,
            PointerEdgeLocation::Desktop { route },
            delivery_owner,
            capture_owner,
        );
        if stream_terminal {
            translated = translated.ending_stream();
            self.pointer_ids.remove(&(
                edge.identity().device_id().get(),
                edge.identity().pointer_id().get(),
            ));
        }
        Ok(translated)
    }

    fn pointer_id(
        &mut self,
        identity: NativePointerIdentity,
    ) -> Result<PointerId, NativeRuntimeError> {
        let key = (identity.device_id().get(), identity.pointer_id().get());
        if let Some(pointer) = self.pointer_ids.get(&key) {
            return Ok(*pointer);
        }
        self.next_pointer_id = self
            .next_pointer_id
            .checked_add(1)
            .ok_or(NativeRuntimeError::IdentityExhausted)?;
        let pointer = PointerId::new(self.next_pointer_id);
        self.pointer_ids.insert(key, pointer);
        Ok(pointer)
    }

    fn next_observation_generation(&mut self) -> Result<u64, NativeRuntimeError> {
        self.observation_generation = self
            .observation_generation
            .checked_add(1)
            .ok_or(NativeRuntimeError::IdentityExhausted)?;
        Ok(self.observation_generation)
    }
}

fn accepted_work_area_authority(
    dockspace: &Dockspace,
    commit: &NativeWorkAreaCommit,
) -> Option<AcceptedWorkAreaAuthority> {
    let expected = commit.roster.as_ref()?;
    let viewport = dockspace.engine().viewport();
    if !viewport.capabilities().work_area().is_supported() {
        return None;
    }
    let mut actual = viewport
        .work_areas()
        .map(|(_, work_area)| work_area)
        .collect::<Vec<_>>();
    actual.sort_by_key(|work_area| work_area.token());
    if &actual != expected {
        return None;
    }
    Some(AcceptedWorkAreaAuthority {
        native_generation: commit.native_generation,
        provider: dockspace.engine().platform_provider()?,
        generation: viewport.work_area_generation(),
        tokens: actual.into_iter().map(ObservedWorkArea::token).collect(),
    })
}

fn translate_work_area_roster(
    observation: &NativeWorkAreaRosterObservation,
    core_observation_generation: u64,
) -> Result<(WorkAreaRosterObservation, NativeWorkAreaCommit), NativeRuntimeError> {
    let native_generation = observation.generation().get();
    let Some(native_roster) = observation.roster().value() else {
        let reason = map_unavailable(
            observation
                .roster()
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        );
        return Ok((
            WorkAreaRosterObservation::unknown(
                WorkAreaObservationGeneration::new(core_observation_generation),
                reason,
            ),
            NativeWorkAreaCommit {
                native_generation,
                roster: None,
            },
        ));
    };

    let mut roster = Vec::with_capacity(native_roster.len());
    for native in native_roster {
        roster.push(ObservedWorkArea::new(
            WorkAreaToken::new(native.token().get()),
            translate_rect(&native.bounds())?,
            ScaleFactor::new(native.scale_factor())?,
        ));
    }
    roster.sort_by_key(|work_area| work_area.token());
    let translated = WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(core_observation_generation),
        Authority::Known(roster.clone()),
    )?;
    Ok((
        translated,
        NativeWorkAreaCommit {
            native_generation,
            roster: Some(roster),
        },
    ))
}

fn work_area_capability(observation: &NativeWorkAreaRosterObservation) -> PlatformCapability {
    if observation.roster().value().is_some() {
        return PlatformCapability::Supported;
    }
    let reason = observation
        .roster()
        .unavailable_reason()
        .unwrap_or(NativeUnavailableReason::NotObserved);
    match reason {
        NativeUnavailableReason::Unsupported => PlatformCapability::unsupported(
            PlatformRequirement::WorkArea,
            PlatformCapabilityReason::BackendUnsupported,
        ),
        NativeUnavailableReason::NotObserved => PlatformCapability::unknown(
            PlatformRequirement::WorkArea,
            PlatformCapabilityReason::NotReported,
        ),
        NativeUnavailableReason::StaleSource | NativeUnavailableReason::Retired => {
            PlatformCapability::unknown(
                PlatformRequirement::WorkArea,
                PlatformCapabilityReason::EnvironmentUnavailable,
            )
        }
    }
}

fn native_window_lifecycle_capability(
    capabilities: NativeBackendCapabilities,
) -> PlatformCapability {
    translate_backend_capability(
        intersect_backend_capability(
            capabilities.window_lifecycle(),
            capabilities.window_visibility(),
        ),
        PlatformRequirement::NativeWindowLifecycle,
    )
}

fn translate_capability_roster(
    backend: NativeBackendCapabilities,
    work_area: PlatformCapability,
) -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(native_window_lifecycle_capability(backend));
    capabilities.set_authoritative_inventory(translate_backend_capability(
        backend.authoritative_inventory(),
        PlatformRequirement::AuthoritativeInventory,
    ));
    capabilities.set_hovered_window(translate_backend_capability(
        backend.hovered_window(),
        PlatformRequirement::HoveredWindow,
    ));
    capabilities.set_desktop_pointer_position(translate_backend_capability(
        backend.desktop_pointer_position(),
        PlatformRequirement::DesktopPointerPosition,
    ));
    capabilities.set_authoritative_button_state(translate_backend_capability(
        backend.authoritative_button_state(),
        PlatformRequirement::AuthoritativeButtonState,
    ));
    capabilities.set_global_window_placement(translate_backend_capability(
        backend.global_window_placement(),
        PlatformRequirement::GlobalWindowPlacement,
    ));
    capabilities.set_work_area(work_area);
    capabilities.set_pointer_hit_test_observation(translate_backend_capability(
        backend.pointer_hit_test_observation(),
        PlatformRequirement::PointerHitTestObservation,
    ));
    capabilities.set_pointer_hit_test_control(translate_backend_capability(
        backend.pointer_hit_test_control(),
        PlatformRequirement::PointerHitTestControl,
    ));
    capabilities.set_global_focus_observation(translate_backend_capability(
        backend.global_focus_observation(),
        PlatformRequirement::GlobalFocusObservation,
    ));
    capabilities.set_window_activation_control(translate_backend_capability(
        backend.window_activation_control(),
        PlatformRequirement::WindowActivationControl,
    ));
    capabilities.set_close_cancellation(translate_backend_capability(
        backend.close_cancellation(),
        PlatformRequirement::CloseCancellation,
    ));
    capabilities
}

const fn intersect_backend_capability(
    left: NativeBackendCapability,
    right: NativeBackendCapability,
) -> NativeBackendCapability {
    match (left, right) {
        (NativeBackendCapability::Unsupported, _) | (_, NativeBackendCapability::Unsupported) => {
            NativeBackendCapability::Unsupported
        }
        (NativeBackendCapability::Unknown, _) | (_, NativeBackendCapability::Unknown) => {
            NativeBackendCapability::Unknown
        }
        (NativeBackendCapability::Supported, NativeBackendCapability::Supported) => {
            NativeBackendCapability::Supported
        }
    }
}

fn translate_backend_capability(
    capability: NativeBackendCapability,
    requirement: PlatformRequirement,
) -> PlatformCapability {
    match capability {
        NativeBackendCapability::Supported => PlatformCapability::Supported,
        NativeBackendCapability::Unsupported => PlatformCapability::unsupported(
            requirement,
            PlatformCapabilityReason::BackendUnsupported,
        ),
        NativeBackendCapability::Unknown => PlatformCapability::unknown(
            requirement,
            PlatformCapabilityReason::EnvironmentUnavailable,
        ),
    }
}

fn translate_work_area_route(
    route: &NativeAuthority<NativeWorkAreaRoute>,
    accepted: Option<&AcceptedWorkAreaAuthority>,
) -> Authority<DesktopWorkAreaRoute> {
    let Some(route) = route.value().copied() else {
        return Authority::Unknown(map_unavailable(
            route
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        ));
    };
    let Some(accepted) = accepted else {
        return Authority::Unknown(AuthorityUnavailableReason::NotReported);
    };
    let token = WorkAreaToken::new(route.token().get());
    if !native_work_area_route_is_current(
        route.generation().get(),
        token,
        accepted.native_generation,
        &accepted.tokens,
    ) {
        return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
    }
    Authority::Known(DesktopWorkAreaRoute::new(
        accepted.provider,
        accepted.generation,
        token,
    ))
}

fn native_work_area_route_is_current(
    native_generation: u64,
    token: WorkAreaToken,
    accepted_generation: u64,
    accepted_tokens: &BTreeSet<WorkAreaToken>,
) -> bool {
    native_generation == accepted_generation && accepted_tokens.contains(&token)
}

fn empty_pointer_interval(
    submitted_pointer_segment: bool,
    watermark: PointerEdgeSequence,
) -> Result<Option<PointerEdgeJournal>, dockspace::pointer_journal::PointerJournalError> {
    if submitted_pointer_segment {
        return Ok(None);
    }
    Ok(Some(PointerEdgeJournal::new(
        watermark,
        watermark,
        Vec::new(),
    )?))
}

fn hovered_route(
    edge: &NativePointerEdge,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Option<BoundNativeRoute> {
    let NativeHoveredWindow::Viewport(binding) = edge.hovered().value()? else {
        return None;
    };
    routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding == *binding)
}

fn delivery_route(
    edge: &NativePointerEdge,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Option<BoundNativeRoute> {
    let NativePointerDeliveryOwner::Viewport(binding) = edge.delivery_owner().value()? else {
        return None;
    };
    routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding == *binding)
}

fn translate_scroll_edge(
    host: dockspace::presentation_observation::PresentationHostLease,
    scroll: NativeScrollEdge,
    native_delivery: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    presented: Option<&PresentedNativePointerGraph>,
    physical_coordinates: Authority<PhysicalScrollCoordinates>,
) -> Result<ScrollEdge, NativeRuntimeError> {
    let delivery = translate_scroll_delivery(host, native_delivery, routes, presented)?;
    let delta = scroll
        .delta()
        .map(|delta| translate_scroll_delta(delta, physical_coordinates))
        .transpose()?;
    Ok(ScrollEdge::new(
        ScrollDeviceId::new(scroll.device().get()),
        scroll
            .sequence()
            .map(|sequence| ScrollSequenceToken::new(sequence.get())),
        match scroll.phase() {
            NativeScrollPhase::Discrete => ScrollPhase::Discrete,
            NativeScrollPhase::Begin => ScrollPhase::Begin,
            NativeScrollPhase::Update => ScrollPhase::Update,
            NativeScrollPhase::End => ScrollPhase::End,
            NativeScrollPhase::Cancel(reason) => ScrollPhase::Cancel(match reason {
                NativeScrollCancelReason::PlatformCancelled => {
                    ScrollCancelReason::PlatformCancelled
                }
                NativeScrollCancelReason::DeviceRemoved => ScrollCancelReason::DeviceRemoved,
            }),
        },
        delta,
        translate_authority(scroll.momentum(), |momentum| match momentum {
            NativeScrollMomentum::Direct => ScrollMomentum::Direct,
            NativeScrollMomentum::Momentum => ScrollMomentum::Momentum,
        }),
        translate_authority(scroll.modifiers(), |modifiers| {
            ScrollModifiers::new(
                modifiers.shift(),
                modifiers.control(),
                modifiers.alt(),
                modifiers.command(),
            )
        }),
        delivery,
    )?)
}

fn translate_scroll_delivery(
    host: dockspace::presentation_observation::PresentationHostLease,
    native: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
    presented: Option<&PresentedNativePointerGraph>,
) -> Result<Authority<ScrollDeliveryEndpoint>, NativeRuntimeError> {
    let Some(NativePointerDeliveryOwner::Viewport(binding)) = native.value() else {
        return Ok(Authority::Unknown(map_unavailable(
            native
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        )));
    };
    let Some(route) = routes
        .get(&binding.viewport_id())
        .copied()
        .filter(|route| route.native_binding() == *binding)
    else {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    };
    let Some(presented) = presented else {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    };
    if presented.native() != route.exact() || presented.scene().surface() != route.surface() {
        return Ok(Authority::Unknown(
            AuthorityUnavailableReason::SurfaceUnavailable,
        ));
    }
    Ok(Authority::Known(ScrollDeliveryEndpoint::new(
        host,
        route.surface(),
        Some(route.core()),
        presented.coordinate_generation(),
    )?))
}

fn translate_scroll_delta(
    delta: NativeScrollDelta,
    physical_coordinates: Authority<PhysicalScrollCoordinates>,
) -> Result<ScrollDelta, NativeRuntimeError> {
    let native = delta.vector();
    let vector = FiniteScrollVector::new(native.x(), native.y())?;
    match delta {
        NativeScrollDelta::Lines(_) => Ok(ScrollDelta::Lines(vector)),
        NativeScrollDelta::PhysicalPixels(_) => Ok(ScrollDelta::PhysicalPixels {
            delta: vector,
            coordinates: physical_coordinates,
        }),
    }
}

fn event_time_physical_scroll_coordinates(
    retained: &RetainedPointerEdge,
) -> Authority<PhysicalScrollCoordinates> {
    let edge = retained.edge();
    let Some(capture) = edge.delivery_coordinates().value().copied() else {
        return Authority::Unknown(map_unavailable(
            edge.delivery_coordinates()
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        ));
    };
    let (Some(route), Some(graph)) = (retained.delivery_route(), retained.graphs().delivery())
    else {
        return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
    };
    if capture.binding() != route.native_binding()
        || graph.native() != route.exact()
        || graph.scene().surface() != route.surface()
        || capture.native_scale_factor() as f32 != graph.graph().native_pixels_per_point()
        || capture.presentation_scale_factor() as f32 != graph.graph().pixels_per_point()
    {
        return Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable);
    }
    Authority::Known(PhysicalScrollCoordinates::new(
        route.core(),
        graph.coordinate_generation(),
    ))
}

fn translate_delivery_owner(
    owner: &NativeAuthority<NativePointerDeliveryOwner>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Authority<PointerEventDeliveryOwner> {
    translate_delivery_owner_fact(owner.value(), owner.unavailable_reason(), routes)
}

fn translate_delivery_owner_fact(
    owner: Option<&NativePointerDeliveryOwner>,
    unavailable: Option<NativeUnavailableReason>,
    routes: &BTreeMap<ViewportId, BoundNativeRoute>,
) -> Authority<PointerEventDeliveryOwner> {
    match owner {
        Some(NativePointerDeliveryOwner::Viewport(binding)) => routes
            .get(&binding.viewport_id())
            .copied()
            .filter(|route| route.native_binding == *binding)
            .map_or(
                Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
                |route| Authority::Known(PointerEventDeliveryOwner::Native(route.core)),
            ),
        Some(NativePointerDeliveryOwner::Foreign) => {
            Authority::Known(PointerEventDeliveryOwner::Foreign)
        }
        Some(NativePointerDeliveryOwner::None) => Authority::Known(PointerEventDeliveryOwner::None),
        None => Authority::Unknown(map_unavailable(
            unavailable.unwrap_or(NativeUnavailableReason::NotObserved),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::effect::{DispatchFailureReason, EffectDispatchResult, EffectId};
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch};
    use dockspace::policy::DockPolicy;
    use dockspace::viewport::{ViewportRole, WindowToken};

    fn test_dockspace() -> Dockspace {
        let surface = SurfaceId::new(1);
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        Dockspace::builder(
            "native-ingress-transaction",
            builder.build().expect("the test workspace is valid"),
        )
        .policy(DockPolicy::default())
        .build()
        .expect("the test dockspace is valid")
    }

    #[test]
    fn an_idle_native_cycle_still_submits_its_pointer_watermark() {
        let watermark = PointerEdgeSequence::new(41);
        let interval = empty_pointer_interval(false, watermark)
            .expect("an unchanged watermark is structurally valid")
            .expect("an idle provider cycle requires an explicit interval");

        assert_eq!(interval.previous(), watermark);
        assert_eq!(interval.through(), watermark);
        assert!(interval.edges().is_empty());
        assert!(
            empty_pointer_interval(true, watermark)
                .expect("a prior segment is structurally valid")
                .is_none()
        );
    }

    #[test]
    fn unavailable_delivery_authority_is_never_inferred_from_the_event_source() {
        assert_eq!(
            translate_delivery_owner_fact(
                None,
                Some(NativeUnavailableReason::StaleSource),
                &BTreeMap::new(),
            ),
            Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable)
        );
        assert_eq!(
            translate_delivery_owner_fact(
                Some(&NativePointerDeliveryOwner::Foreign),
                None,
                &BTreeMap::new(),
            ),
            Authority::Known(PointerEventDeliveryOwner::Foreign)
        );
    }

    #[test]
    fn native_scroll_delta_reaches_core_without_f32_normalization() {
        let raw = NativeScrollDelta::Lines(
            eframe::NativeFiniteScrollVector::new(16_777_217.25, -16_777_216.0)
                .expect("the native vector is finite"),
        );
        let translated = translate_scroll_delta(
            raw,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("line deltas do not require coordinate conversion");

        assert_eq!(translated.vector().x(), 16_777_217.25);
        assert_eq!(translated.vector().y(), -16_777_216.0);
    }

    #[test]
    fn unknown_physical_scroll_coordinates_preserve_the_journal_sample() {
        let raw = NativeScrollDelta::PhysicalPixels(
            eframe::NativeFiniteScrollVector::new(2.5, -7.25).expect("the native vector is finite"),
        );
        let translated = translate_scroll_delta(
            raw,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("missing coordinate authority is a typed fact, not a cycle failure");

        assert!(matches!(
            translated,
            ScrollDelta::PhysicalPixels {
                delta,
                coordinates: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            } if delta.x() == 2.5 && delta.y() == -7.25
        ));
    }

    #[test]
    fn unsupported_visibility_fails_the_combined_native_lifecycle_closed() {
        let capability = translate_backend_capability(
            intersect_backend_capability(
                NativeBackendCapability::Supported,
                NativeBackendCapability::Unsupported,
            ),
            PlatformRequirement::NativeWindowLifecycle,
        );

        assert!(matches!(
            capability,
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::NativeWindowLifecycle
                    && issue.reason() == PlatformCapabilityReason::BackendUnsupported
        ));
    }

    #[test]
    fn unknown_backend_placement_never_becomes_supported() {
        let capability = translate_backend_capability(
            NativeBackendCapability::Unknown,
            PlatformRequirement::GlobalWindowPlacement,
        );

        assert!(matches!(
            capability,
            PlatformCapability::Unknown(issue)
                if issue.requirement() == PlatformRequirement::GlobalWindowPlacement
                    && issue.reason() == PlatformCapabilityReason::EnvironmentUnavailable
        ));
    }

    #[test]
    fn complete_backend_capability_roster_is_translated_without_implicit_support() {
        let capabilities = translate_capability_roster(
            NativeBackendCapabilities::default(),
            PlatformCapability::Supported,
        );
        let translated = [
            (
                capabilities.native_window_lifecycle(),
                PlatformRequirement::NativeWindowLifecycle,
            ),
            (
                capabilities.authoritative_inventory(),
                PlatformRequirement::AuthoritativeInventory,
            ),
            (
                capabilities.hovered_window(),
                PlatformRequirement::HoveredWindow,
            ),
            (
                capabilities.desktop_pointer_position(),
                PlatformRequirement::DesktopPointerPosition,
            ),
            (
                capabilities.authoritative_button_state(),
                PlatformRequirement::AuthoritativeButtonState,
            ),
            (
                capabilities.global_window_placement(),
                PlatformRequirement::GlobalWindowPlacement,
            ),
            (
                capabilities.pointer_hit_test_observation(),
                PlatformRequirement::PointerHitTestObservation,
            ),
            (
                capabilities.pointer_hit_test_control(),
                PlatformRequirement::PointerHitTestControl,
            ),
            (
                capabilities.global_focus_observation(),
                PlatformRequirement::GlobalFocusObservation,
            ),
            (
                capabilities.window_activation_control(),
                PlatformRequirement::WindowActivationControl,
            ),
            (
                capabilities.close_cancellation(),
                PlatformRequirement::CloseCancellation,
            ),
        ];

        for (capability, requirement) in translated {
            assert!(matches!(
                capability,
                PlatformCapability::Unknown(issue)
                    if issue.requirement() == requirement
                        && issue.reason() == PlatformCapabilityReason::EnvironmentUnavailable
            ));
        }
        assert_eq!(capabilities.work_area(), PlatformCapability::Supported);
    }

    #[test]
    fn backend_sidecars_survive_retry_and_retire_only_after_commit() {
        let mut ledger = BackendSidecarLedger::default();
        assert_eq!(ledger.insert(3, "edge-three"), None);
        assert_eq!(ledger.insert(5, "edge-five"), None);

        assert_eq!(ledger.get(3), Some(&"edge-three"));
        assert_eq!(ledger.get(5), Some(&"edge-five"));
        ledger.retire_through(2);
        assert_eq!(ledger.len(), 2, "an aborted prefix is retained for replay");

        ledger.retire_through(3);
        assert_eq!(ledger.get(3), None);
        assert_eq!(ledger.get(5), Some(&"edge-five"));
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn post_commit_outbox_is_restored_with_the_native_prepare_snapshot() {
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        let effect = EffectResult::new(
            EffectId::new(7),
            WorkspaceEpoch::new(3),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        );
        bridge
            .post_commit_records
            .push(PostCommitRecord::Effect(effect));
        let snapshot = bridge.state_snapshot();

        bridge.post_commit_records.clear();
        bridge.restore_state(snapshot);

        assert!(matches!(
            bridge.post_commit_records.as_slice(),
            [PostCommitRecord::Effect(restored)] if *restored == effect
        ));
    }

    #[test]
    fn post_commit_outbox_only_requires_follow_up_when_it_contains_work() {
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        assert!(!bridge.has_post_commit_records());

        bridge
            .post_commit_records
            .push(PostCommitRecord::Effect(EffectResult::new(
                EffectId::new(7),
                WorkspaceEpoch::new(3),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            )));

        assert!(bridge.has_post_commit_records());
    }

    #[test]
    fn aborted_outer_transaction_restores_registration_and_recorder_state() {
        let surface = SurfaceId::new(1);
        let mut dockspace = test_dockspace();
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        bridge.recorder = Some(
            dockspace
                .create_backend_ingress_provider(PointerEdgeSequence::new(0))
                .expect("the test backend provider is available"),
        );
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");
        let transaction = NativeIngressTransaction {
            recorder: bridge
                .recorder
                .as_ref()
                .expect("the recorder is enrolled")
                .savepoint(),
            state: bridge.state_snapshot(),
            presentation: presentations.prepare_savepoint(),
        };

        let ordinal = dockspace
            .record_backend_viewport_registration(
                bridge.recorder.as_mut().expect("the recorder is enrolled"),
                surface,
                WindowToken::new(7),
                ViewportRole::Root,
                None,
            )
            .expect("the registration is staged");
        bridge.pending_registrations.insert(surface, ordinal);
        bridge.observation_generation = 19;

        bridge.rollback_transaction(&mut dockspace, &mut presentations, transaction);

        assert!(bridge.pending_registrations.is_empty());
        assert_eq!(bridge.observation_generation, 0);
        assert!(
            bridge
                .recorder
                .as_ref()
                .expect("the recorder remains enrolled")
                .pending_batch()
                .expect("the rolled-back recorder remains valid")
                .records()
                .is_empty()
        );
    }

    #[test]
    fn outside_all_route_requires_the_exact_committed_roster_generation_and_token() {
        let selected = WorkAreaToken::new(7);
        let accepted = BTreeSet::from([selected]);

        assert!(native_work_area_route_is_current(4, selected, 4, &accepted));
        assert!(!native_work_area_route_is_current(
            3, selected, 4, &accepted
        ));
        assert!(!native_work_area_route_is_current(
            4,
            WorkAreaToken::new(8),
            4,
            &accepted,
        ));
    }
}

fn exact_native(binding: NativeViewportBinding) -> ExactNativeViewport {
    ExactNativeViewport::new(
        binding.viewport_id(),
        NativeViewportIncarnation::new(binding.incarnation().get()),
    )
}

fn resolve_semantic_receiver_event(
    dockspace: &Dockspace,
    presentations: &NativePresentationLedger,
    route: BoundNativeRoute,
    graph: &egui::PointerHitGraphSnapshot,
    receiver: egui::WidgetReceiver,
    action: SemanticReceiverAction,
) -> Option<SemanticReceiverEvent> {
    if !receiver.enabled {
        return None;
    }
    let presented = presentations.semantic_graph(route.native_binding(), graph)?;
    let fingerprint = PaintReceiverFingerprint::new(
        receiver.id,
        receiver.layer_id,
        receiver.interact_rect,
        receiver.sense,
        receiver.enabled,
    );
    let PaintReceiverLookup::Dock(target) = dockspace.resolve_retained_receiver(
        presented.scene(),
        presented.emission(),
        route.exact().viewport(),
        graph.widget_pass_nr(),
        fingerprint,
    ) else {
        return None;
    };
    if !dockspace.retained_semantic_receiver_supports(
        presented.scene(),
        presented.emission(),
        target,
        action,
    ) {
        return None;
    }
    Some(SemanticReceiverEvent::new(
        presented.scene(),
        presented.emission(),
        SemanticDelivery::Native(route.core()),
        target,
        action,
    ))
}

fn translate_coordinates(
    binding: ViewportBinding,
    generation: u64,
    geometry: &NativeAuthority<eframe::NativeWindowGeometry>,
) -> Result<WindowCoordinateObservation, NativeRuntimeError> {
    let unavailable = geometry
        .unavailable_reason()
        .map(map_unavailable)
        .unwrap_or(AuthorityUnavailableReason::NotReported);
    let (content, outer, native_scale, presentation_scale) = geometry.value().map_or_else(
        || {
            (
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
                Authority::Unknown(unavailable),
            )
        },
        |geometry| {
            (
                translate_authority(geometry.content_rect(), translate_rect),
                translate_authority(geometry.outer_rect(), translate_rect),
                translate_authority(geometry.native_scale_factor(), |scale| {
                    ScaleFactor::new(*scale)
                }),
                translate_authority(geometry.presentation_scale_factor(), |scale| {
                    ScaleFactor::new(*scale)
                }),
            )
        },
    );
    Ok(WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(generation),
        transpose_geometry(content)?,
        transpose_geometry(outer)?,
        transpose_geometry(native_scale)?,
        transpose_geometry(presentation_scale)?,
    ))
}

fn translate_input_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> InputEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::PointerInput,
    ) {
        return InputEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || InputEffectAcknowledgement::known(None),
        |reason| InputEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

fn translate_presentation_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> PresentationEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Presentation,
    ) {
        return PresentationEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || PresentationEffectAcknowledgement::known(None),
        |reason| PresentationEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

fn translate_close_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> CloseEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Close,
    ) {
        return CloseEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || CloseEffectAcknowledgement::known(None),
        |reason| CloseEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

fn translate_lifecycle_acknowledgement(
    effects: &NativeEffectDriver,
    acknowledgement: &NativeEffectAcknowledgement,
    native: NativeViewportBinding,
) -> CloseEffectAcknowledgement {
    if let Some(effect) = effects.acknowledged_effect(
        acknowledgement.correlation(),
        native,
        NativeEffectProperty::Lifecycle,
    ) {
        return CloseEffectAcknowledgement::known(Some(effect));
    }
    acknowledgement.unavailable_reason().map_or_else(
        || CloseEffectAcknowledgement::known(None),
        |reason| CloseEffectAcknowledgement::unknown(map_unavailable(reason)),
    )
}

fn transpose_geometry<T>(
    authority: Authority<Result<T, dockspace::geometry::GeometryError>>,
) -> Result<Authority<T>, NativeRuntimeError> {
    match authority {
        Authority::Known(value) => Ok(Authority::Known(value?)),
        Authority::Unknown(reason) => Ok(Authority::Unknown(reason)),
    }
}

fn translate_focus(
    focused: &NativeAuthority<NativeFocusedWindow>,
    routes: &BTreeMap<NativeViewportBinding, BoundNativeRoute>,
    focus_routes: &BTreeSet<NativeViewportBinding>,
) -> Authority<GlobalFocusedWindow> {
    match focused.value() {
        Some(NativeFocusedWindow::Viewport(binding)) => routes
            .get(binding)
            .filter(|route| route.native_binding() == *binding && focus_routes.contains(binding))
            .map_or(
                Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable),
                |route| Authority::Known(GlobalFocusedWindow::Dock(route.core)),
            ),
        Some(NativeFocusedWindow::Foreign) => Authority::Known(GlobalFocusedWindow::Foreign),
        Some(NativeFocusedWindow::None) => Authority::Known(GlobalFocusedWindow::None),
        None => Authority::Unknown(map_unavailable(
            focused
                .unavailable_reason()
                .unwrap_or(NativeUnavailableReason::NotObserved),
        )),
    }
}

fn translate_focus_acknowledgement(
    effects: &NativeEffectDriver,
    focused: &NativeAuthority<NativeFocusedWindow>,
    windows: &[NativeWindowSnapshot],
    focus_routes: &BTreeSet<NativeViewportBinding>,
) -> Authority<Option<EffectId>> {
    let expected = match focused.value() {
        Some(NativeFocusedWindow::Viewport(binding)) if focus_routes.contains(binding) => {
            Some(*binding)
        }
        Some(NativeFocusedWindow::Viewport(_)) => {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        }
        Some(NativeFocusedWindow::Foreign | NativeFocusedWindow::None) => None,
        None => {
            return Authority::Unknown(map_unavailable(
                focused
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ));
        }
    };

    let mut acknowledged = None;
    for window in windows
        .iter()
        .filter(|window| focus_routes.contains(&window.binding()))
    {
        let observation = window.focus();
        let Some(observed_focused) = observation.value().value().copied() else {
            return Authority::Unknown(map_unavailable(
                observation
                    .value()
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ));
        };
        let acknowledgement = observation.acknowledgement();
        if let Some(reason) = acknowledgement.unavailable_reason() {
            return Authority::Unknown(map_unavailable(reason));
        }
        let Some(correlation) = acknowledgement.correlation() else {
            continue;
        };
        if expected != Some(window.binding()) || !observed_focused || acknowledged.is_some() {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        }
        let Some(effect) = effects.acknowledged_effect(
            Some(correlation),
            window.binding(),
            NativeEffectProperty::Focus,
        ) else {
            return Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        };
        acknowledged = Some(effect);
    }
    Authority::Known(acknowledged)
}

fn translate_authority<T, U>(
    authority: &NativeAuthority<T>,
    map: impl FnOnce(&T) -> U,
) -> Authority<U> {
    authority.value().map_or_else(
        || {
            Authority::Unknown(map_unavailable(
                authority
                    .unavailable_reason()
                    .unwrap_or(NativeUnavailableReason::NotObserved),
            ))
        },
        |value| Authority::Known(map(value)),
    )
}

const fn map_unavailable(reason: NativeUnavailableReason) -> AuthorityUnavailableReason {
    match reason {
        NativeUnavailableReason::NotObserved => AuthorityUnavailableReason::NotReported,
        NativeUnavailableReason::Unsupported => AuthorityUnavailableReason::ProviderUnavailable,
        NativeUnavailableReason::StaleSource | NativeUnavailableReason::Retired => {
            AuthorityUnavailableReason::SurfaceUnavailable
        }
    }
}

fn translate_point(
    point: &NativePhysicalPoint,
) -> Result<PhysicalPoint, dockspace::geometry::GeometryError> {
    PhysicalPoint::new(f64::from(point.x()), f64::from(point.y()))
}

fn translate_rect(
    rect: &NativePhysicalRect,
) -> Result<PhysicalRect, dockspace::geometry::GeometryError> {
    PhysicalRect::from_min_max(translate_point(&rect.min())?, translate_point(&rect.max())?)
}

const fn translate_button(button: NativePointerButton) -> PointerButton {
    match button {
        NativePointerButton::Primary => PointerButton::Primary,
        NativePointerButton::Secondary => PointerButton::Secondary,
        NativePointerButton::Middle => PointerButton::Middle,
        NativePointerButton::Back => PointerButton::Other(4),
        NativePointerButton::Forward => PointerButton::Other(5),
        NativePointerButton::Other(button) => PointerButton::Other(button),
    }
}

const fn semantic_key(key: NativeKey) -> Option<SemanticKey> {
    match key {
        NativeKey::Escape => None,
        NativeKey::ArrowLeft => Some(SemanticKey::ArrowLeft),
        NativeKey::ArrowRight => Some(SemanticKey::ArrowRight),
        NativeKey::ArrowUp => Some(SemanticKey::ArrowUp),
        NativeKey::ArrowDown => Some(SemanticKey::ArrowDown),
        NativeKey::Home => Some(SemanticKey::Home),
        NativeKey::End => Some(SemanticKey::End),
        NativeKey::Enter => Some(SemanticKey::Enter),
        NativeKey::Space => Some(SemanticKey::Space),
    }
}
