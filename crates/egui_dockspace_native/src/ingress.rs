//! Exact translation from the fork event-loop protocol into dockspace ingress.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::PlatformObservationLease;
use dockspace::backend_ingress::{
    BackendIngressBatch, BackendIngressOrdinal, BackendIngressPayload,
    BackendIngressPrefixRetirementReceipt, BackendIngressRecorder, BackendIngressSavepoint,
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

/// Keeps one exact native lifetime in the host viewport roster without granting core authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteLessNativeKeepalive {
    exact: ExactNativeViewport,
    surface: SurfaceId,
}

impl RouteLessNativeKeepalive {
    pub(crate) const fn new(exact: ExactNativeViewport, surface: SurfaceId) -> Self {
        Self { exact, surface }
    }

    pub(crate) const fn exact(self) -> ExactNativeViewport {
        self.exact
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }
}

#[derive(Clone, Copy, Debug)]
struct RetiredNativeRoute {
    exact: ExactNativeViewport,
    surface: SurfaceId,
    core: ViewportBinding,
    // An unpublished bootstrap has no entry in egui's route registry to retire.
    adapter_route_was_published: bool,
    close_acknowledgement: CloseEffectAcknowledgement,
}

#[derive(Clone, Copy, Debug)]
enum NativeRetirementAuthority {
    Published(BoundNativeRoute),
    PendingRegistration(PendingNativeRegistration, ViewportBinding),
    Provisional(crate::effects::PendingEffectRoute),
    AdoptedProvisional(
        PendingNativeRegistration,
        crate::effects::PendingEffectRoute,
    ),
}

impl NativeRetirementAuthority {
    const fn surface(self) -> SurfaceId {
        match self {
            Self::Published(route) => route.surface(),
            Self::PendingRegistration(pending, _) => pending.surface,
            Self::Provisional(route) => route.surface(),
            Self::AdoptedProvisional(pending, _) => pending.surface,
        }
    }

    const fn core(self) -> ViewportBinding {
        match self {
            Self::Published(route) => route.core(),
            Self::PendingRegistration(_, binding) => binding,
            Self::Provisional(route) => route.core(),
            Self::AdoptedProvisional(_, route) => route.core(),
        }
    }

    const fn adapter_route_was_published(self) -> bool {
        matches!(self, Self::Published(_))
    }
}

impl RetiredNativeRoute {
    const fn surface(self) -> SurfaceId {
        self.surface
    }

    const fn adapter_retirement(self) -> Option<ExactNativeViewport> {
        if self.adapter_route_was_published {
            Some(self.exact)
        } else {
            None
        }
    }
}

fn existing_restored_core_route(
    dockspace: &Dockspace,
    surface: SurfaceId,
    retirements: &[RetiredNativeRoute],
) -> Option<(SurfaceId, ViewportBinding)> {
    if retirements
        .iter()
        .any(|retired| retired.surface() == surface)
    {
        return None;
    }
    dockspace
        .native_viewport_binding(surface)
        .map(|binding| (surface, binding))
}

fn configured_child_accepts_predecessor(
    spec: &crate::NativeSurfaceSpec,
    surface: SurfaceId,
    predecessor: ViewportBinding,
) -> bool {
    spec.role() == ViewportRole::Child
        && spec.surface() == surface
        && spec
            .bootstrap_token()
            .is_none_or(|token| token == predecessor.token())
}

#[derive(Debug, Default)]
struct DeferredReplacementLineagePlan {
    successor_after_retirement: BTreeMap<ExactNativeViewport, PendingNativeRegistration>,
    intermediate_retirements: BTreeSet<ExactNativeViewport>,
    route_less_lifetimes: BTreeSet<ExactNativeViewport>,
    live_keepalives: BTreeMap<ViewportId, RouteLessNativeKeepalive>,
    terminal_viewports: BTreeSet<ViewportId>,
}

impl DeferredReplacementLineagePlan {
    fn take_successor(
        &mut self,
        retired: ExactNativeViewport,
    ) -> Option<PendingNativeRegistration> {
        self.successor_after_retirement.remove(&retired)
    }

    fn take_intermediate(&mut self, retired: ExactNativeViewport) -> bool {
        self.intermediate_retirements.remove(&retired)
    }

    fn is_route_less(&self, exact: ExactNativeViewport) -> bool {
        self.route_less_lifetimes.contains(&exact)
    }

    fn finish(
        self,
    ) -> Result<
        (
            BTreeMap<ViewportId, RouteLessNativeKeepalive>,
            BTreeSet<ViewportId>,
        ),
        NativeRuntimeError,
    > {
        if !self.successor_after_retirement.is_empty() || !self.intermediate_retirements.is_empty()
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native replacement lineage omitted an ordered retirement",
            ));
        }
        Ok((self.live_keepalives, self.terminal_viewports))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingNativeRegistrationPhase {
    /// The replacement native lifetime is live, but its predecessor still owns the core binding.
    AwaitingPredecessorRetirement(ViewportBinding),
    /// Core committed its replacement effect and the existing native lifetime adopted that binding.
    AdoptedReplacement {
        predecessor: ViewportBinding,
        replacement: ViewportBinding,
    },
    /// The registration record exists only in the recorder's uncommitted suffix.
    Staged(BackendIngressOrdinal),
    /// Core committed the exact registration, but no adapter route has been published yet.
    Committed(ViewportBinding),
}

/// Correlates a bootstrap window lifetime with its core registration boundary.
///
/// This sidecar must survive registration commit until either the first route is published or an
/// ordered retirement provides the window's terminal fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingNativeRegistration {
    exact: ExactNativeViewport,
    surface: SurfaceId,
    phase: PendingNativeRegistrationPhase,
}

impl PendingNativeRegistration {
    const fn staged(
        exact: ExactNativeViewport,
        surface: SurfaceId,
        registration_ordinal: BackendIngressOrdinal,
    ) -> Self {
        Self {
            exact,
            surface,
            phase: PendingNativeRegistrationPhase::Staged(registration_ordinal),
        }
    }

    const fn awaiting_predecessor(
        exact: ExactNativeViewport,
        surface: SurfaceId,
        predecessor: ViewportBinding,
    ) -> Self {
        Self {
            exact,
            surface,
            phase: PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(predecessor),
        }
    }

    const fn predecessor_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding) => Some(binding),
            PendingNativeRegistrationPhase::AdoptedReplacement { predecessor, .. } => {
                Some(predecessor)
            }
            PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    const fn awaiting_predecessor_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding) => Some(binding),
            PendingNativeRegistrationPhase::AdoptedReplacement { .. }
            | PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    const fn adopted_replacement_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AdoptedReplacement { replacement, .. } => {
                Some(replacement)
            }
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(_)
            | PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    const fn committed_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(_)
            | PendingNativeRegistrationPhase::Staged(_) => None,
            PendingNativeRegistrationPhase::AdoptedReplacement { replacement, .. } => {
                Some(replacement)
            }
            PendingNativeRegistrationPhase::Committed(binding) => Some(binding),
        }
    }
}

pub(crate) struct PreparedNativeIngress {
    pub(crate) batch: BackendIngressBatch,
    pub(crate) bindings: NativeBindingRoster,
    pub(crate) routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pub(crate) effect_routes: BTreeMap<ViewportId, BoundNativeRoute>,
    pub(crate) route_less_keepalives: BTreeMap<ViewportId, RouteLessNativeKeepalive>,
    pub(crate) retired_bootstrap_viewports: BTreeSet<ViewportId>,
    pub(crate) pointer_edges: BTreeMap<u64, RetainedPointerEdge>,
    pub(crate) presentation_host: PresentationHostLease,
    pub(crate) transaction: NativeIngressTransaction,
}

struct PreparedNativeIngressPayload {
    batch: BackendIngressBatch,
    bindings: NativeBindingRoster,
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    effect_routes: BTreeMap<ViewportId, BoundNativeRoute>,
    route_less_keepalives: BTreeMap<ViewportId, RouteLessNativeKeepalive>,
    retired_bootstrap_viewports: BTreeSet<ViewportId>,
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
    retired_routes: BTreeMap<ExactNativeViewport, ViewportBinding>,
    external_retirements: BTreeSet<ExactNativeViewport>,
    deferred_replacement_retirements: BTreeSet<ExactNativeViewport>,
    retiring_restored_bootstraps: BTreeSet<ExactNativeViewport>,
    quarantined_native_lifetimes: BTreeSet<ExactNativeViewport>,
    pending_prefix_retirement: Option<BackendIngressPrefixRetirementReceipt>,
    pending_registrations: BTreeMap<ExactNativeViewport, PendingNativeRegistration>,
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
    deferred_replacements: BTreeMap<SurfaceId, ExactNativeViewport>,
    adopted_replacements: BTreeMap<ExactNativeViewport, ViewportBinding>,
    post_commit_records: Vec<PostCommitRecord>,
}

#[derive(Clone)]
struct NativeIngressStateSnapshot {
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    retired_routes: BTreeMap<ExactNativeViewport, ViewportBinding>,
    external_retirements: BTreeSet<ExactNativeViewport>,
    deferred_replacement_retirements: BTreeSet<ExactNativeViewport>,
    retiring_restored_bootstraps: BTreeSet<ExactNativeViewport>,
    quarantined_native_lifetimes: BTreeSet<ExactNativeViewport>,
    pending_registrations: BTreeMap<ExactNativeViewport, PendingNativeRegistration>,
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
            retired_routes: BTreeMap::new(),
            external_retirements: BTreeSet::new(),
            deferred_replacement_retirements: BTreeSet::new(),
            retiring_restored_bootstraps: BTreeSet::new(),
            quarantined_native_lifetimes: BTreeSet::new(),
            pending_prefix_retirement: None,
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
                effect_routes: prepared.effect_routes,
                route_less_keepalives: prepared.route_less_keepalives,
                retired_bootstrap_viewports: prepared.retired_bootstrap_viewports,
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
        let mut replacement_lineages =
            self.plan_deferred_replacement_lineages(ingress, configured)?;
        let mut routes = self.routes.clone();
        let mut provisional_routes = BTreeMap::new();
        let mut retirements = Vec::new();
        let mut terminal_bootstrap_viewports = BTreeSet::new();
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
                    let Some(route) =
                        self.route_for_semantic_input(&routes, native, &replacement_lineages)?
                    else {
                        continue;
                    };
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
                    let Some(route) =
                        self.route_for_semantic_input(&routes, native, &replacement_lineages)?
                    else {
                        continue;
                    };
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
                    if self.route_less_replacement(exact_native(native))
                        || self.effects.has_provisional_native(exact_native(native))
                        || replacement_lineages.is_route_less(exact_native(native))
                    {
                        continue;
                    }
                    let route = routes
                        .values()
                        .find(|route| route.native_binding == native)
                        .copied()
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
                        &mut provisional_routes,
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
                    self.quarantine_failed_native_lifetime(
                        exact_native(result.binding()),
                        &mut provisional_routes,
                    )?;
                }
                NativeIngressEvent::ViewportCreateResult(result) => {
                    let recorder = self.recorder.as_mut().expect("provider enrolled above");
                    self.effects.consume_create_result(result, recorder)?;
                }
                NativeIngressEvent::Retirement(tombstone) => {
                    let native = tombstone.binding();
                    let exact = exact_native(native);
                    let close_acknowledgement = translate_lifecycle_acknowledgement(
                        &self.effects,
                        tombstone.acknowledgement(),
                        native,
                    );
                    if replacement_lineages.take_intermediate(exact) {
                        self.retire_intermediate_replacement(exact)?;
                    } else if self.retire_quarantined_native_lifetime(exact)? {
                    } else if self.retire_materialized_restored_bootstrap(configured, exact)? {
                    } else if self.retire_deferred_replacement(exact)? {
                    } else if let Some(retirement) = self.retire_native_binding(
                        dockspace,
                        presentations,
                        &mut routes,
                        &mut provisional_routes,
                        exact,
                        close_acknowledgement,
                    )? {
                        retirements.push(retirement);
                    } else if exact.viewport() == ViewportId::ROOT
                        || configured.get(exact.viewport()).is_some()
                    {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "dockspace-owned native retirement omitted its exact route state",
                        ));
                    } else {
                        self.record_external_retirement(exact)?;
                    }
                    if let Some(successor) = replacement_lineages.take_successor(exact) {
                        self.insert_deferred_replacement_registration(successor, exact)?;
                    }
                }
                NativeIngressEvent::BindingIngressQuiesced(quiesced) => {
                    let exact = exact_native(quiesced.binding());
                    self.record_retirement_quiescence(presentations, exact)?;
                }
            }
        }

        if !observed_snapshot {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress batch omitted its atomic platform snapshot",
            ));
        }
        let (mut route_less_keepalives, terminal_viewports) = replacement_lineages.finish()?;
        terminal_bootstrap_viewports.extend(terminal_viewports);
        terminal_bootstrap_viewports.extend(
            self.quarantined_native_lifetimes
                .iter()
                .map(|exact| exact.viewport()),
        );
        route_less_keepalives.retain(|_, keepalive| {
            !self
                .quarantined_native_lifetimes
                .contains(&keepalive.exact())
        });
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
        let mut effect_routes = live_routes.clone();
        for (viewport, route) in provisional_routes {
            if effect_routes.insert(viewport, route).is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "one native viewport was both provisional and published",
                ));
            }
        }
        if route_less_keepalives
            .keys()
            .any(|viewport| effect_routes.contains_key(viewport))
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "one native viewport was both route-less keepalive and core-routed",
            ));
        }
        self.routes = routes;

        let bindings = NativeBindingRoster::new(
            live_routes
                .values()
                .copied()
                .map(BoundNativeRoute::adapter_route),
            retirements
                .iter()
                .copied()
                .filter_map(RetiredNativeRoute::adapter_retirement),
        );
        let retired_bootstrap_viewports = terminal_bootstrap_viewports;
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
            effect_routes,
            route_less_keepalives,
            retired_bootstrap_viewports,
            pointer_edges,
        })
    }

    fn retire_native_binding(
        &mut self,
        dockspace: &mut Dockspace,
        presentations: &mut NativePresentationLedger,
        routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        exact: ExactNativeViewport,
        close_acknowledgement: CloseEffectAcknowledgement,
    ) -> Result<Option<RetiredNativeRoute>, NativeRuntimeError> {
        if self.retired_routes.contains_key(&exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress repeated one retired viewport lifetime",
            ));
        }

        let published = routes
            .get(&exact.viewport())
            .copied()
            .filter(|route| route.exact() == exact);
        let pending = self.pending_registrations.get(&exact).copied();
        let provisional = self.effects.route_for_provisional_retirement(exact);
        if published.is_some() && (pending.is_some() || provisional.is_some()) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "one native lifetime has multiple retirement authorities",
            ));
        }
        if let (Some(pending), Some(provisional)) = (pending, provisional)
            && (pending.surface != provisional.surface()
                || pending.adopted_replacement_binding() != Some(provisional.core()))
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "adopted provisional retirement authorities disagree",
            ));
        }

        let authority = published
            .map(NativeRetirementAuthority::Published)
            .or_else(|| {
                Some(NativeRetirementAuthority::AdoptedProvisional(
                    pending?,
                    provisional?,
                ))
            })
            .or_else(|| {
                let pending = pending?;
                pending
                    .committed_binding()
                    .map(|binding| NativeRetirementAuthority::PendingRegistration(pending, binding))
            })
            .or_else(|| provisional.map(NativeRetirementAuthority::Provisional));
        let Some(authority) = authority else {
            if pending.is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "native viewport retired before its core registration committed",
                ));
            }
            return Ok(None);
        };
        let surface = authority.surface();
        let core = authority.core();
        let adapter_route_was_published = authority.adapter_route_was_published();

        if matches!(
            authority,
            NativeRetirementAuthority::Provisional(_)
                | NativeRetirementAuthority::AdoptedProvisional(..)
        ) {
            self.effects.retain_initialization_receipts(exact)?;
        } else {
            self.effects.forget_pending_native(exact);
        }
        if matches!(
            authority,
            NativeRetirementAuthority::PendingRegistration(..)
        ) {
            self.effects
                .retire_restored_viewport(exact.viewport(), surface)?;
        }
        let recorder = self.recorder.as_mut().expect("provider enrolled above");
        presentations.retire(dockspace, recorder, exact)?;
        match authority {
            NativeRetirementAuthority::Published(_) => {
                routes.remove(&exact.viewport());
            }
            NativeRetirementAuthority::PendingRegistration(..) => {
                self.pending_registrations.remove(&exact);
            }
            NativeRetirementAuthority::Provisional(_) => {}
            NativeRetirementAuthority::AdoptedProvisional(..) => {
                self.pending_registrations.remove(&exact);
            }
        }
        if matches!(
            authority,
            NativeRetirementAuthority::Provisional(_)
                | NativeRetirementAuthority::AdoptedProvisional(..)
        ) && provisional_routes
            .remove(&exact.viewport())
            .is_some_and(|route| {
                route.exact() != exact || route.surface() != surface || route.core() != core
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "provisional retirement changed its cycle-local route",
            ));
        }
        if self.retired_routes.insert(exact, core).is_some() {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress repeated one retired viewport route",
            ));
        }
        Ok(Some(RetiredNativeRoute {
            exact,
            surface,
            core,
            adapter_route_was_published,
            close_acknowledgement,
        }))
    }

    fn retire_deferred_replacement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(pending) = self.pending_registrations.get(&exact).copied() else {
            return Ok(false);
        };
        if pending.awaiting_predecessor_binding().is_none() {
            return Ok(false);
        }
        self.pending_registrations.remove(&exact);
        self.effects.forget_pending_native(exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "deferred native replacement repeated one retirement lifetime",
            ));
        }
        Ok(true)
    }

    fn retire_quarantined_native_lifetime(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        if !self.quarantined_native_lifetimes.remove(&exact) {
            return Ok(false);
        }
        self.pending_registrations.remove(&exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "quarantined native lifetime repeated one retirement",
            ));
        }
        Ok(true)
    }

    fn retire_materialized_restored_bootstrap(
        &mut self,
        configured: &NativeViewportRoster,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(spec) = configured
            .get(exact.viewport())
            .filter(|spec| spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some())
        else {
            return Ok(false);
        };
        if !self
            .effects
            .retire_materialized_restored_viewport(exact.viewport(), spec.surface())?
        {
            return Ok(false);
        }
        if !self.retiring_restored_bootstraps.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "materialized restored bootstrap repeated one retirement lifetime",
            ));
        }
        Ok(true)
    }

    fn retire_intermediate_replacement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        self.effects.forget_pending_native(exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native replacement lineage repeated one intermediate lifetime",
            ));
        }
        Ok(())
    }

    fn quarantine_failed_native_lifetime(
        &mut self,
        exact: ExactNativeViewport,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(effect_route) = self.effects.route_for_provisional_retirement(exact) else {
            return Ok(false);
        };
        if !effect_route.is_failed() {
            return Ok(false);
        }

        if let Some(pending) = self.pending_registrations.get(&exact).copied() {
            if pending.surface != effect_route.surface()
                || pending.adopted_replacement_binding() != Some(effect_route.core())
            {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "failed native initialization changed its pending core authority",
                ));
            }
            self.pending_registrations.remove(&exact);
        }
        if let Some(route) = provisional_routes.remove(&exact.viewport())
            && (route.exact() != exact
                || route.surface() != effect_route.surface()
                || route.core() != effect_route.core())
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "failed native initialization changed its provisional route",
            ));
        }
        self.effects.retain_initialization_receipts(exact)?;
        if !self.quarantined_native_lifetimes.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native initialization repeated one quarantine lifetime",
            ));
        }
        Ok(true)
    }

    fn route_for_semantic_input(
        &self,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        native: NativeViewportBinding,
        replacement_lineages: &DeferredReplacementLineagePlan,
    ) -> Result<Option<BoundNativeRoute>, NativeRuntimeError> {
        let route = routes
            .get(&native.viewport_id())
            .filter(|route| route.native_binding() == native)
            .copied();
        if route.is_some()
            || self.route_less_replacement(exact_native(native))
            || self.effects.has_provisional_native(exact_native(native))
            || replacement_lineages.is_route_less(exact_native(native))
        {
            return Ok(route);
        }
        Err(NativeRuntimeError::IngressUnavailable(
            "native semantic edge has no exact current core binding route",
        ))
    }

    fn route_less_replacement(&self, exact: ExactNativeViewport) -> bool {
        self.pending_registrations.contains_key(&exact)
            || self.deferred_replacement_retirements.contains(&exact)
            || self.quarantined_native_lifetimes.contains(&exact)
            || self.retiring_restored_bootstraps.contains(&exact)
            || self
                .effects
                .restored_create_is_materialized(exact.viewport())
    }

    fn record_retirement_quiescence(
        &mut self,
        presentations: &mut NativePresentationLedger,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        let Some(binding) = self.retired_routes.get(&exact).copied() else {
            if self.retiring_restored_bootstraps.remove(&exact) {
                return Ok(());
            }
            if self.deferred_replacement_retirements.remove(&exact) {
                self.effects.retire_initialization_receipts(exact);
                return Ok(());
            }
            if self.external_retirements.remove(&exact) {
                return Ok(());
            }
            return Err(NativeRuntimeError::IngressUnavailable(
                "native retirement quiescence omitted its retired route classification",
            ));
        };
        self.recorder
            .as_mut()
            .expect("provider enrolled above")
            .record_platform_binding_quiescence(binding)?;
        presentations.retirement_quiesced(exact);
        self.effects.retire_initialization_receipts(exact);
        Ok(())
    }

    fn record_external_retirement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        if self.external_retirements.insert(exact) {
            Ok(())
        } else {
            Err(NativeRuntimeError::IngressUnavailable(
                "external native viewport repeated one retirement lifetime",
            ))
        }
    }

    fn insert_pending_registration(
        &mut self,
        registration: PendingNativeRegistration,
    ) -> Result<(), NativeRuntimeError> {
        if self.pending_registrations.contains_key(&registration.exact)
            || self.pending_registrations.values().any(|pending| {
                pending.surface == registration.surface
                    || pending.exact.viewport() == registration.exact.viewport()
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native viewport registration repeated a pending lifetime or surface",
            ));
        }
        self.pending_registrations
            .insert(registration.exact, registration);
        Ok(())
    }

    fn insert_deferred_replacement_registration(
        &mut self,
        registration: PendingNativeRegistration,
        predecessor_exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        let predecessor =
            registration
                .predecessor_binding()
                .ok_or(NativeRuntimeError::IngressUnavailable(
                    "deferred replacement registration omitted its predecessor binding",
                ))?;
        if self.pending_registrations.contains_key(&registration.exact)
            || self.pending_registrations.values().any(|pending| {
                let conflicts = pending.surface == registration.surface
                    || pending.exact.viewport() == registration.exact.viewport();
                let is_exact_predecessor = pending.exact == predecessor_exact
                    && pending.committed_binding() == Some(predecessor);
                conflicts && !is_exact_predecessor
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "deferred native replacement conflicts with another pending lifetime",
            ));
        }
        self.pending_registrations
            .insert(registration.exact, registration);
        Ok(())
    }

    fn pending_registration_for_surface(
        &self,
        surface: SurfaceId,
    ) -> Option<PendingNativeRegistration> {
        self.pending_registrations
            .values()
            .copied()
            .find(|pending| pending.surface == surface)
    }

    fn advance_pending_registrations(
        &mut self,
        dockspace: &Dockspace,
        committed_through: Option<BackendIngressOrdinal>,
    ) -> Result<(), NativeRuntimeError> {
        for pending in self.pending_registrations.values_mut() {
            match pending.phase {
                PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(predecessor) => {
                    let current = dockspace.native_viewport_binding(pending.surface);
                    let retained_replacement = dockspace
                        .engine()
                        .viewport()
                        .recovery_pending(pending.surface)
                        .and_then(dockspace::frame::RecoveryPending::replacement_binding);
                    if current.is_some_and(|binding| {
                        binding != predecessor && Some(binding) != retained_replacement
                    }) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::AdoptedReplacement {
                    predecessor,
                    replacement,
                } => {
                    let current = dockspace.native_viewport_binding(pending.surface);
                    let retained = dockspace
                        .engine()
                        .viewport()
                        .recovery_pending(pending.surface)
                        .and_then(|recovery| recovery.replacement_binding());
                    if current
                        .is_some_and(|binding| binding != predecessor && binding != replacement)
                        || (current.is_none() && retained != Some(replacement))
                    {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "adopted native replacement lost its exact core authority",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::Staged(registration_ordinal)
                    if committed_through
                        .is_some_and(|committed| registration_ordinal <= committed) =>
                {
                    let binding = dockspace.native_viewport_binding(pending.surface).ok_or(
                        NativeRuntimeError::IngressUnavailable(
                            "committed native registration did not mint its core binding",
                        ),
                    )?;
                    pending.phase = PendingNativeRegistrationPhase::Committed(binding);
                }
                PendingNativeRegistrationPhase::Staged(_) => {
                    if dockspace.native_viewport_binding(pending.surface).is_some() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "native registration became visible before its committed ordinal",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::Committed(binding) => {
                    if dockspace.native_viewport_binding(pending.surface) != Some(binding) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "pending native registration lost or changed its exact core binding",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn state_snapshot(&self) -> NativeIngressStateSnapshot {
        NativeIngressStateSnapshot {
            routes: self.routes.clone(),
            retired_routes: self.retired_routes.clone(),
            external_retirements: self.external_retirements.clone(),
            deferred_replacement_retirements: self.deferred_replacement_retirements.clone(),
            retiring_restored_bootstraps: self.retiring_restored_bootstraps.clone(),
            quarantined_native_lifetimes: self.quarantined_native_lifetimes.clone(),
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
        self.retired_routes = snapshot.retired_routes;
        self.external_retirements = snapshot.external_retirements;
        self.deferred_replacement_retirements = snapshot.deferred_replacement_retirements;
        self.retiring_restored_bootstraps = snapshot.retiring_restored_bootstraps;
        self.quarantined_native_lifetimes = snapshot.quarantined_native_lifetimes;
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
            deferred_replacements: self
                .pending_registrations
                .values()
                .filter(|pending| pending.awaiting_predecessor_binding().is_some())
                .map(|pending| (pending.surface, pending.exact))
                .collect(),
            adopted_replacements: BTreeMap::new(),
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
    ) -> Result<(), NativeRuntimeError> {
        let terminal = cycle.effects.dispatch(
            emissions,
            routes,
            &cycle.deferred_replacements,
            &mut cycle.adopted_replacements,
            catalog,
            effect_sink,
            create_sink,
        )?;
        cycle
            .post_commit_records
            .extend(terminal.into_iter().map(PostCommitRecord::Effect));
        Ok(())
    }

    pub(crate) fn prepare_effect_cycle_adoptions(
        &mut self,
        cycle: &NativeEffectCycle,
    ) -> Result<(), NativeRuntimeError> {
        for (exact, replacement) in &cycle.adopted_replacements {
            let pending = self.pending_registrations.get_mut(exact).ok_or(
                NativeRuntimeError::IngressUnavailable(
                    "existing-window adoption lost its deferred native lifetime",
                ),
            )?;
            let predecessor = pending.awaiting_predecessor_binding().ok_or(
                NativeRuntimeError::IngressUnavailable(
                    "existing-window adoption repeated or changed its pending phase",
                ),
            )?;
            if pending.surface != replacement.surface() || *replacement == predecessor {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "existing-window adoption changed its core replacement authority",
                ));
            }
            pending.phase = PendingNativeRegistrationPhase::AdoptedReplacement {
                predecessor,
                replacement: *replacement,
            };
        }
        Ok(())
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
        dockspace: &mut Dockspace,
    ) -> Result<(), NativeRuntimeError> {
        if self.recorder.is_none() {
            return Ok(());
        }
        self.settle_pending_prefix_retirement(dockspace)?;
        let committed_through = dockspace
            .engine()
            .backend_ingress_commit_watermark()
            .map(|watermark| watermark.through());
        if let Some(watermark) = dockspace.engine().backend_ingress_commit_watermark() {
            let through = watermark.through();
            let work_area_commit = self
                .work_area_sidecars
                .latest_through(through.get())
                .cloned();
            let receipt = self
                .recorder
                .as_mut()
                .expect("the provider was checked above")
                .retire_committed_prefix(watermark)?;
            self.pointer_sidecars.retire_through(through.get());
            if let Some(work_area_commit) = work_area_commit {
                self.accepted_work_areas =
                    accepted_work_area_authority(dockspace, &work_area_commit);
            }
            self.work_area_sidecars.retire_through(through.get());
            self.pending_prefix_retirement = receipt;
            self.settle_pending_prefix_retirement(dockspace)?;
        }
        self.advance_pending_registrations(dockspace, committed_through)?;
        Ok(())
    }

    fn settle_pending_prefix_retirement(
        &mut self,
        dockspace: &mut Dockspace,
    ) -> Result<(), NativeRuntimeError> {
        if let Some(receipt) = self.pending_prefix_retirement.as_mut() {
            let quiesced_bindings = dockspace
                .adapter_settle_backend_ingress_prefix_retirement(receipt)?
                .into_iter()
                .collect::<BTreeSet<_>>();
            self.pending_prefix_retirement = None;
            self.retired_routes
                .retain(|_, binding| !quiesced_bindings.contains(binding));
        }
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

    fn plan_deferred_replacement_lineages(
        &self,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
    ) -> Result<DeferredReplacementLineagePlan, NativeRuntimeError> {
        let live = ingress
            .platform()
            .inventory()
            .iter()
            .copied()
            .map(exact_native)
            .collect::<BTreeSet<_>>();
        let mut retired_by_viewport = BTreeMap::<ViewportId, Vec<ExactNativeViewport>>::new();
        for retirement in
            ingress
                .ordered()
                .records()
                .iter()
                .filter_map(|record| match record.event() {
                    NativeIngressEvent::Retirement(retirement) => {
                        Some(exact_native(retirement.binding()))
                    }
                    _ => None,
                })
        {
            retired_by_viewport
                .entry(retirement.viewport())
                .or_default()
                .push(retirement);
        }

        self.plan_deferred_replacement_lineages_from_exacts(retired_by_viewport, &live, configured)
    }

    fn plan_deferred_replacement_lineages_from_exacts(
        &self,
        retired_by_viewport: BTreeMap<ViewportId, Vec<ExactNativeViewport>>,
        live: &BTreeSet<ExactNativeViewport>,
        configured: &NativeViewportRoster,
    ) -> Result<DeferredReplacementLineagePlan, NativeRuntimeError> {
        let mut plan = DeferredReplacementLineagePlan::default();
        for (viewport, retired) in retired_by_viewport {
            let authorities = retired
                .iter()
                .enumerate()
                .filter_map(|(index, exact)| {
                    self.replacement_lineage_authority(*exact)
                        .map(|authority| (index, *exact, authority))
                })
                .collect::<Vec<_>>();
            if authorities.len() > 1 {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "native replacement lineage contained multiple core authorities",
                ));
            }
            let Some((authority_index, authority_exact, (surface, predecessor))) =
                authorities.first().copied()
            else {
                continue;
            };
            let Some(spec) = configured.get(viewport) else {
                continue;
            };
            if !configured_child_accepts_predecessor(spec, surface, predecessor) {
                continue;
            }

            let retired_exact = retired.iter().copied().collect::<BTreeSet<_>>();
            let mut live_successors = live.iter().copied().filter(|candidate| {
                candidate.viewport() == viewport && !retired_exact.contains(candidate)
            });
            let live_successor = live_successors.next();
            if live_successors.next().is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "atomic native inventory repeated one replacement viewport lifetime",
                ));
            }

            plan.intermediate_retirements
                .extend(retired.iter().skip(authority_index + 1).copied());
            plan.route_less_lifetimes
                .extend(retired.iter().skip(authority_index + 1).copied());
            if let Some(successor) = live_successor {
                let registration = PendingNativeRegistration::awaiting_predecessor(
                    successor,
                    surface,
                    predecessor,
                );
                if plan
                    .successor_after_retirement
                    .insert(authority_exact, registration)
                    .is_some()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native replacement lineage repeated its authority retirement",
                    ));
                }
                plan.route_less_lifetimes.insert(successor);
                if plan
                    .live_keepalives
                    .insert(viewport, RouteLessNativeKeepalive::new(successor, surface))
                    .is_some()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native replacement lineage repeated one live keepalive viewport",
                    ));
                }
            } else {
                plan.terminal_viewports.insert(viewport);
            }
        }
        Ok(plan)
    }

    fn replacement_lineage_authority(
        &self,
        exact: ExactNativeViewport,
    ) -> Option<(SurfaceId, ViewportBinding)> {
        self.routes
            .get(&exact.viewport())
            .copied()
            .filter(|route| route.exact() == exact)
            .map(|route| (route.surface(), route.core()))
            .or_else(|| {
                let pending = self.pending_registrations.get(&exact).copied()?;
                pending
                    .committed_binding()
                    .or_else(|| pending.predecessor_binding())
                    .map(|binding| (pending.surface, binding))
            })
    }

    fn reconcile_snapshot_routes(
        &mut self,
        dockspace: &Dockspace,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
        retirements: &[RetiredNativeRoute],
        routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
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
            let exact = exact_native(native_binding);
            if !retirements
                .iter()
                .any(|retired| retired.surface() == root.surface())
            {
                let pending = self.pending_registrations.get(&exact).copied();
                if self
                    .pending_registration_for_surface(root.surface())
                    .is_some_and(|registration| registration.exact != exact)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "root registration native lifetime changed before routing",
                    ));
                }
                if let Some(registration) = pending
                    && registration.surface != root.surface()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "root registration changed its declared surface",
                    ));
                }
                let core = pending
                    .and_then(PendingNativeRegistration::committed_binding)
                    .or_else(|| {
                        pending
                            .is_none()
                            .then(|| dockspace.native_viewport_binding(root.surface()))
                            .flatten()
                    });
                if let Some(core) = core {
                    if Some(core.token()) != root.bootstrap_token() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "core root binding token differs from the bootstrap token",
                        ));
                    }
                    routes.insert(
                        ViewportId::ROOT,
                        BoundNativeRoute {
                            native_binding,
                            exact,
                            surface: root.surface(),
                            core,
                        },
                    );
                    self.pending_registrations.remove(&exact);
                } else if pending.is_none() {
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
                    self.insert_pending_registration(PendingNativeRegistration::staged(
                        exact,
                        root.surface(),
                        ordinal,
                    ))?;
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

            let exact = exact_native(native_binding);
            if self.quarantined_native_lifetimes.contains(&exact) {
                continue;
            }
            let restored = configured.get(viewport).filter(|spec| {
                spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some()
            });
            let pending = self.pending_registrations.get(&exact).copied();
            if let Some(spec) = restored {
                if self
                    .pending_registration_for_surface(spec.surface())
                    .is_some_and(|registration| registration.exact != exact)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "restored registration native lifetime changed before routing",
                    ));
                }
                if let Some(registration) = pending
                    && registration.surface != spec.surface()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "restored registration changed its declared surface",
                    ));
                }
            }
            if let Some(predecessor) = pending.and_then(|registration| {
                registration
                    .predecessor_binding()
                    .map(|predecessor| (registration.surface, predecessor))
            }) {
                let (surface, predecessor) = predecessor;
                let current = dockspace.native_viewport_binding(surface);
                if current == Some(predecessor) {
                    continue;
                }
                let Some(expected_replacement) =
                    pending.and_then(PendingNativeRegistration::adopted_replacement_binding)
                else {
                    let retained_replacement = dockspace
                        .engine()
                        .viewport()
                        .recovery_pending(surface)
                        .and_then(dockspace::frame::RecoveryPending::replacement_binding);
                    if current.is_some_and(|binding| Some(binding) != retained_replacement) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                    continue;
                };
                let Some(effect_route) = self.effects.route_for_new_binding(native_binding) else {
                    if current.is_some() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                    continue;
                };
                let core = effect_route.core();
                if effect_route.surface() != surface
                    || core != expected_replacement
                    || current.is_some_and(|binding| binding != core)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "deferred native replacement changed its core replacement authority",
                    ));
                }
                let route = BoundNativeRoute {
                    native_binding,
                    exact,
                    surface,
                    core,
                };
                if effect_route.is_publishable() {
                    if !self.effects.publish_route(exact) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "publishable native replacement lost its initialization proof",
                        ));
                    }
                    routes.insert(viewport, route);
                    self.pending_registrations.remove(&exact);
                } else {
                    provisional_routes.insert(viewport, route);
                }
                continue;
            }
            let effect_route = self.effects.route_for_new_binding(native_binding);
            if effect_route.is_some() && pending.is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "one native lifetime has both create-effect and bootstrap route authority",
                ));
            }
            let pending_route = pending.and_then(|registration| {
                registration
                    .committed_binding()
                    .map(|binding| (registration.surface, binding))
            });
            let effect_binding = effect_route.map(|route| (route.surface(), route.core()));
            let Some((surface, core)) = effect_binding.or_else(|| {
                let spec = restored?;
                pending_route.or_else(|| {
                    pending
                        .is_none()
                        .then(|| {
                            existing_restored_core_route(dockspace, spec.surface(), retirements)
                        })
                        .flatten()
                })
            }) else {
                if let Some(spec) = restored
                    && !retirements
                        .iter()
                        .any(|retired| retired.surface() == spec.surface())
                    && pending.is_none()
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
                    self.insert_pending_registration(PendingNativeRegistration::staged(
                        exact,
                        spec.surface(),
                        ordinal,
                    ))?;
                }
                // Unknown physical children remain outside dockspace authority. A restored
                // child joins only after its core registration commits on a prior cycle.
                continue;
            };
            if let Some(spec) = restored
                && effect_route.is_none()
                && Some(core.token()) != spec.bootstrap_token()
            {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "core child binding token differs from the restored token",
                ));
            }
            let route = BoundNativeRoute {
                native_binding,
                exact,
                surface,
                core,
            };
            if effect_route.is_some_and(|route| !route.is_publishable()) {
                provisional_routes.insert(viewport, route);
            } else {
                if effect_route.is_some() && !self.effects.publish_route(exact) {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "publishable native create lost its initialization proof",
                    ));
                }
                routes.insert(viewport, route);
                self.pending_registrations.remove(&exact);
            }
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
                retired.core,
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
                NativeScrollCancelReason::BindingRetired => ScrollCancelReason::BindingRetired,
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
    use dockspace::engine::BackendIngressProgress;
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch};
    use dockspace::pointer_receiver::PointerReceiverReceiptBatch;
    use dockspace::policy::DockPolicy;
    use dockspace::scene_manifest::MeasurementUnavailableReason;
    use dockspace::surface_recovery::SurfaceRecoveryBootstrap;
    use dockspace::viewport::{ViewportRole, WindowToken};
    use egui_dockspace::{EguiFrameScheduleKey, EguiPresentationResult, NativeCoreRoute};

    const HOST_SURFACE: SurfaceId = SurfaceId::new(1);
    const CHILD_SURFACE: SurfaceId = SurfaceId::new(2);

    fn test_dockspace() -> Dockspace {
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(HOST_SURFACE, SurfacePresentation::with_main(root));
        Dockspace::builder(
            "native-ingress-transaction",
            builder.build().expect("the test workspace is valid"),
        )
        .policy(DockPolicy::default())
        .build()
        .expect("the test dockspace is valid")
    }

    fn restored_test_dockspace() -> Dockspace {
        let host_root = RootId::new(1);
        let child_root = RootId::new(2);
        let mut builder = Workspace::builder();
        let host_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let child_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(host_root, RootRecord::new(host_tabs));
        builder.set_root(child_root, RootRecord::new(child_tabs));
        builder.set_surface(HOST_SURFACE, SurfacePresentation::with_main(host_root));
        builder.set_surface(CHILD_SURFACE, SurfacePresentation::with_main(child_root));
        Dockspace::builder(
            "native-pending-registration-retirement",
            builder.build().expect("the test workspace is valid"),
        )
        .policy(DockPolicy::default())
        .build()
        .expect("the test dockspace is valid")
    }

    fn record_idle_pointer(recorder: &mut BackendIngressRecorder) {
        let watermark = PointerEdgeSequence::new(0);
        recorder
            .record_pointer_segment(
                PointerEdgeJournal::new(watermark, watermark, Vec::new())
                    .expect("an idle pointer interval is canonical"),
            )
            .expect("the idle pointer interval enters the backend order");
    }

    fn commit_backend_batch(
        dockspace: &mut Dockspace,
        recorder: &BackendIngressRecorder,
        routes: impl IntoIterator<Item = NativeCoreRoute>,
        frame: u64,
    ) {
        commit_backend_batch_with_roster(
            dockspace,
            recorder,
            NativeBindingRoster::new(routes, []),
            frame,
        );
    }

    fn commit_backend_batch_with_roster(
        dockspace: &mut Dockspace,
        recorder: &BackendIngressRecorder,
        roster: NativeBindingRoster,
        frame: u64,
    ) {
        let mut input = dockspace
            .begin_native_cycle(EguiFrameScheduleKey::new(frame, 0), roster)
            .expect("the native frame begins");
        let progress = input
            .submit_ingress(
                recorder
                    .pending_batch()
                    .expect("the backend suffix freezes"),
            )
            .expect("the backend suffix reduces");
        let progress = if progress == BackendIngressProgress::ReceiverReceiptsRequired {
            input
                .submit_pointer_receiver_receipts(
                    PointerReceiverReceiptBatch::new([])
                        .expect("an empty receiver answer is canonical"),
                )
                .expect("the empty pointer interval resumes")
        } else {
            progress
        };
        assert_eq!(progress, BackendIngressProgress::Complete);
        let mut presentation = input
            .into_presentation()
            .expect("complete backend input enters presentation");
        let expected = presentation.expected_surfaces().collect::<Vec<_>>();
        for surface in expected {
            presentation
                .mark_surface_unavailable(
                    dockspace,
                    surface,
                    MeasurementUnavailableReason::Deferred,
                )
                .expect("the headless test explicitly completes every surface");
        }
        let commit = presentation
            .finish(dockspace)
            .expect("the backend frame commits atomically");
        for output in commit.into_parts().1 {
            output.settle_with(|_, _| EguiPresentationResult::Dropped);
        }
    }

    fn test_capabilities() -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_global_window_placement(PlatformCapability::Supported);
        capabilities.set_work_area(PlatformCapability::Supported);
        capabilities.set_global_focus_observation(PlatformCapability::Supported);
        capabilities
    }

    fn observed_host_window(binding: ViewportBinding) -> ObservedWindow {
        ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(1),
                Authority::Known(
                    PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("the client bounds are valid"),
                ),
                Authority::Known(
                    PhysicalRect::new(-8.0, -30.0, 656.0, 518.0)
                        .expect("the outer bounds are valid"),
                ),
                Authority::Known(ScaleFactor::new(1.0).expect("the scale is valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("the scale is valid")),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(1),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ))
            .with_close_requested(Authority::Known(false))
    }

    fn destroyed_child_snapshot(
        generation: u64,
        host: ViewportBinding,
        child: ViewportBinding,
    ) -> PlatformSnapshot {
        let host_window = observed_host_window(host);
        PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(generation),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(generation),
                Authority::Known(test_capabilities()),
            ),
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(generation),
                Authority::Known(GlobalFocusedWindow::Dock(host)),
                Authority::Known(None),
            ),
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(generation),
                Authority::Known(vec![host]),
            )
            .expect("the live host inventory is canonical"),
            vec![host_window],
            vec![WindowCloseObservation::new(
                child,
                CloseObservationGeneration::new(generation),
                Authority::Known(WindowCloseState::Destroyed),
                CloseEffectAcknowledgement::known(None),
            )],
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(generation),
                Authority::Known(vec![ObservedWorkArea::new(
                    WorkAreaToken::new(1),
                    PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
                    ScaleFactor::new(1.0).expect("the work-area scale is valid"),
                )]),
            )
            .expect("the work-area roster is canonical"),
        )
        .expect("the destroyed-child snapshot is canonical")
    }

    fn live_child_snapshot(
        generation: u64,
        host: ViewportBinding,
        child: ViewportBinding,
    ) -> PlatformSnapshot {
        let mut inventory = vec![host, child];
        inventory.sort_unstable();
        PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(generation),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(generation),
                Authority::Known(test_capabilities()),
            ),
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(generation),
                Authority::Known(GlobalFocusedWindow::Dock(host)),
                Authority::Known(None),
            ),
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(generation),
                Authority::Known(inventory),
            )
            .expect("the live child inventory is canonical"),
            vec![observed_host_window(host), observed_host_window(child)],
            Vec::new(),
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(generation),
                Authority::Known(vec![ObservedWorkArea::new(
                    WorkAreaToken::new(1),
                    PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
                    ScaleFactor::new(1.0).expect("the work-area scale is valid"),
                )]),
            )
            .expect("the work-area roster is canonical"),
        )
        .expect("the live-child snapshot is canonical")
    }

    struct PendingRestoredFixture {
        dockspace: Dockspace,
        bridge: NativeIngressBridge,
        presentations: NativePresentationLedger,
        host_route: NativeCoreRoute,
        child_exact: ExactNativeViewport,
        child_binding: ViewportBinding,
    }

    fn committed_pending_restored_registration() -> PendingRestoredFixture {
        let mut dockspace = restored_test_dockspace();
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        bridge.recorder = Some(
            dockspace
                .create_backend_ingress_provider(PointerEdgeSequence::new(0))
                .expect("the test backend provider is available"),
        );

        dockspace
            .record_backend_viewport_registration(
                bridge.recorder.as_mut().expect("the recorder is enrolled"),
                HOST_SURFACE,
                WindowToken::new(1),
                ViewportRole::Root,
                None,
            )
            .expect("the host registration is staged");
        record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
        commit_backend_batch(
            &mut dockspace,
            bridge.recorder.as_ref().expect("the recorder is enrolled"),
            [],
            1,
        );
        bridge
            .reclaim_committed_prefix(&mut dockspace)
            .expect("the committed host prefix is reclaimed");
        let host_binding = dockspace
            .native_viewport_binding(HOST_SURFACE)
            .expect("the host registration minted a binding");
        let host_route = NativeCoreRoute::new(
            ExactNativeViewport::new(ViewportId::ROOT, NativeViewportIncarnation::new(1)),
            HOST_SURFACE,
            host_binding,
        );

        let child_viewport = ViewportId::from_hash_of("pending-restored-child");
        let child_exact =
            ExactNativeViewport::new(child_viewport, NativeViewportIncarnation::new(1));
        let ordinal = dockspace
            .record_backend_child_viewport_bootstrap(
                bridge.recorder.as_mut().expect("the recorder is enrolled"),
                CHILD_SURFACE,
                WindowToken::new(2),
                SurfaceRecoveryBootstrap::new(HOST_SURFACE),
            )
            .expect("the restored child registration is staged");
        bridge
            .insert_pending_registration(PendingNativeRegistration::staged(
                child_exact,
                CHILD_SURFACE,
                ordinal,
            ))
            .expect("the restored child sidecar is staged");
        record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
        commit_backend_batch(
            &mut dockspace,
            bridge.recorder.as_ref().expect("the recorder is enrolled"),
            [host_route],
            2,
        );
        bridge
            .reclaim_committed_prefix(&mut dockspace)
            .expect("the committed child prefix is reclaimed");
        let child_binding = dockspace
            .native_viewport_binding(CHILD_SURFACE)
            .expect("the restored child registration minted a binding");
        assert_eq!(
            bridge
                .pending_registrations
                .get(&child_exact)
                .and_then(|pending| pending.committed_binding()),
            Some(child_binding),
        );

        PendingRestoredFixture {
            dockspace,
            bridge,
            presentations: NativePresentationLedger::new()
                .expect("presentation identity is available"),
            host_route,
            child_exact,
            child_binding,
        }
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
    fn committed_bootstrap_can_retire_before_its_adapter_route_and_fully_quiesce() {
        let PendingRestoredFixture {
            mut dockspace,
            mut bridge,
            mut presentations,
            host_route,
            child_exact,
            child_binding,
        } = committed_pending_restored_registration();
        let transaction = NativeIngressTransaction {
            recorder: bridge
                .recorder
                .as_ref()
                .expect("the recorder is enrolled")
                .savepoint(),
            state: bridge.state_snapshot(),
            presentation: presentations.prepare_savepoint(),
        };
        let mut unpublished_routes = BTreeMap::new();

        let retirement = bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut unpublished_routes,
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the committed bootstrap has exact retirement authority")
            .expect("the committed bootstrap is docking-owned");
        assert_eq!(retirement.surface(), CHILD_SURFACE);
        assert_eq!(retirement.core, child_binding);
        assert_eq!(retirement.adapter_retirement(), None);
        assert!(!bridge.pending_registrations.contains_key(&child_exact));
        assert_eq!(
            bridge.retired_routes.get(&child_exact),
            Some(&child_binding)
        );

        bridge
            .record_retirement_quiescence(&mut presentations, child_exact)
            .expect("full native quiescence follows the exact retirement");
        bridge
            .recorder
            .as_mut()
            .expect("the recorder is enrolled")
            .record_platform_snapshot(
                dockspace.engine().version().epoch(),
                destroyed_child_snapshot(1, host_route.core(), child_binding),
            )
            .expect("the ordered destroyed observation is recorded");
        record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
        commit_backend_batch(
            &mut dockspace,
            bridge.recorder.as_ref().expect("the recorder is enrolled"),
            [host_route],
            3,
        );
        bridge.commit_transaction(&presentations, transaction);
        presentations.accept_commit();
        assert!(dockspace.native_viewport_binding(CHILD_SURFACE).is_none());
        assert_eq!(
            dockspace
                .engine()
                .runtime_retention_manifest()
                .bindings()
                .destroyed_binding_guards(),
            1,
            "the committed destruction guard remains until exact backend quiescence settles",
        );

        bridge
            .reclaim_committed_prefix(&mut dockspace)
            .expect("the committed full-quiescence prefix reclaims its core guard");
        presentations
            .reclaim_committed_quiescence(&mut dockspace)
            .expect("the adapter presentation tombstone also reaches quiescence");
        assert!(bridge.retired_routes.is_empty());
        assert_eq!(
            dockspace
                .engine()
                .runtime_retention_manifest()
                .bindings()
                .destroyed_binding_guards(),
            0,
        );
    }

    #[test]
    fn bootstrap_before_route_retirement_rolls_back_to_the_committed_registration() {
        let PendingRestoredFixture {
            mut dockspace,
            mut bridge,
            mut presentations,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        let transaction = NativeIngressTransaction {
            recorder: bridge
                .recorder
                .as_ref()
                .expect("the recorder is enrolled")
                .savepoint(),
            state: bridge.state_snapshot(),
            presentation: presentations.prepare_savepoint(),
        };
        let mut unpublished_routes = BTreeMap::new();

        bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut unpublished_routes,
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the committed bootstrap may stage retirement")
            .expect("the child is docking-owned");
        bridge
            .record_retirement_quiescence(&mut presentations, child_exact)
            .expect("the staged retirement may stage exact quiescence");
        assert!(!bridge.pending_registrations.contains_key(&child_exact));
        assert!(bridge.retired_routes.contains_key(&child_exact));

        bridge.rollback_transaction(&mut dockspace, &mut presentations, transaction);

        assert_eq!(
            bridge
                .pending_registrations
                .get(&child_exact)
                .and_then(|pending| pending.committed_binding()),
            Some(child_binding),
        );
        assert!(!bridge.retired_routes.contains_key(&child_exact));
        assert!(
            bridge
                .recorder
                .as_ref()
                .expect("the recorder remains enrolled")
                .pending_batch()
                .expect("the rolled-back recorder remains valid")
                .is_empty(),
        );
        assert!(!presentations.has_committed_quiescence_work());

        assert!(
            bridge
                .retire_native_binding(
                    &mut dockspace,
                    &mut presentations,
                    &mut unpublished_routes,
                    &mut BTreeMap::new(),
                    child_exact,
                    CloseEffectAcknowledgement::known(None),
                )
                .expect("the exact retirement remains retryable after rollback")
                .is_some(),
        );
    }

    #[test]
    fn uncommitted_bootstrap_retirement_fails_closed_without_losing_registration() {
        let mut dockspace = test_dockspace();
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        bridge.recorder = Some(
            dockspace
                .create_backend_ingress_provider(PointerEdgeSequence::new(0))
                .expect("the test backend provider is available"),
        );
        let exact = ExactNativeViewport::new(
            ViewportId::from_hash_of("uncommitted-bootstrap"),
            NativeViewportIncarnation::new(1),
        );
        let ordinal = dockspace
            .record_backend_viewport_registration(
                bridge.recorder.as_mut().expect("the recorder is enrolled"),
                HOST_SURFACE,
                WindowToken::new(7),
                ViewportRole::Root,
                None,
            )
            .expect("the registration is staged");
        bridge
            .insert_pending_registration(PendingNativeRegistration::staged(
                exact,
                HOST_SURFACE,
                ordinal,
            ))
            .expect("the registration sidecar is staged");
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");

        let error = bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect_err("an uncommitted registration cannot authorize core retirement");
        assert!(matches!(error, NativeRuntimeError::IngressUnavailable(_)));
        assert!(bridge.pending_registrations.contains_key(&exact));
        assert!(!bridge.retired_routes.contains_key(&exact));
    }

    #[test]
    fn replacement_incarnation_waits_until_same_surface_retirement_commits() {
        let PendingRestoredFixture {
            dockspace,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        assert_eq!(
            existing_restored_core_route(&dockspace, CHILD_SURFACE, &[]),
            Some((CHILD_SURFACE, child_binding)),
        );
        let retirement = RetiredNativeRoute {
            exact: child_exact,
            surface: CHILD_SURFACE,
            core: child_binding,
            adapter_route_was_published: true,
            close_acknowledgement: CloseEffectAcknowledgement::known(None),
        };

        assert_eq!(
            existing_restored_core_route(&dockspace, CHILD_SURFACE, &[retirement]),
            None,
            "A2 must not route to C1 while the same batch retires A1/C1",
        );
    }

    #[test]
    fn runtime_and_restored_children_authorize_only_their_exact_replacement_predecessor() {
        let PendingRestoredFixture {
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        let runtime_child = crate::NativeSurfaceSpec::child(
            child_exact.viewport(),
            CHILD_SURFACE,
            egui::ViewportBuilder::default(),
        );
        assert!(configured_child_accepts_predecessor(
            &runtime_child,
            CHILD_SURFACE,
            child_binding,
        ));

        let restored_child = crate::NativeSurfaceSpec::restored_child(
            child_exact.viewport(),
            CHILD_SURFACE,
            child_binding.token(),
            SurfaceRecoveryBootstrap::new(HOST_SURFACE),
            egui::ViewportBuilder::default(),
        );
        assert!(configured_child_accepts_predecessor(
            &restored_child,
            CHILD_SURFACE,
            child_binding,
        ));

        let mismatched_restored_child = crate::NativeSurfaceSpec::restored_child(
            child_exact.viewport(),
            CHILD_SURFACE,
            WindowToken::new(child_binding.token().get() + 1),
            SurfaceRecoveryBootstrap::new(HOST_SURFACE),
            egui::ViewportBuilder::default(),
        );
        assert!(!configured_child_accepts_predecessor(
            &mismatched_restored_child,
            CHILD_SURFACE,
            child_binding,
        ));
    }

    fn runtime_child_roster(viewport: ViewportId) -> NativeViewportRoster {
        let mut roster = NativeViewportRoster::new(crate::NativeSurfaceSpec::root(
            HOST_SURFACE,
            WindowToken::new(900),
        ))
        .expect("the root native viewport is valid");
        roster
            .insert(crate::NativeSurfaceSpec::child(
                viewport,
                CHILD_SURFACE,
                egui::ViewportBuilder::default(),
            ))
            .expect("the runtime child native viewport is valid");
        roster
    }

    fn restored_child_roster(viewport: ViewportId) -> NativeViewportRoster {
        let mut roster = NativeViewportRoster::new(crate::NativeSurfaceSpec::root(
            HOST_SURFACE,
            WindowToken::new(900),
        ))
        .expect("the root native viewport is valid");
        roster
            .insert(crate::NativeSurfaceSpec::restored_child(
                viewport,
                CHILD_SURFACE,
                WindowToken::new(901),
                SurfaceRecoveryBootstrap::new(HOST_SURFACE),
                egui::ViewportBuilder::default(),
            ))
            .expect("the restored child native viewport is valid");
        roster
    }

    #[test]
    fn replacement_lineage_routes_only_the_final_live_incarnation() {
        let PendingRestoredFixture {
            bridge,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        let intermediate = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
        );
        let final_live = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 2),
        );
        let mut plan = bridge
            .plan_deferred_replacement_lineages_from_exacts(
                BTreeMap::from([(child_exact.viewport(), vec![child_exact, intermediate])]),
                &BTreeSet::from([final_live]),
                &runtime_child_roster(child_exact.viewport()),
            )
            .expect("the ordered replacement lineage is valid");

        assert_eq!(
            plan.successor_after_retirement.get(&child_exact),
            Some(&PendingNativeRegistration::awaiting_predecessor(
                final_live,
                CHILD_SURFACE,
                child_binding,
            )),
        );
        assert_eq!(
            plan.intermediate_retirements,
            BTreeSet::from([intermediate]),
        );
        assert_eq!(
            plan.route_less_lifetimes,
            BTreeSet::from([intermediate, final_live]),
        );
        assert_eq!(
            plan.live_keepalives,
            BTreeMap::from([(
                final_live.viewport(),
                RouteLessNativeKeepalive::new(final_live, CHILD_SURFACE),
            )]),
        );
        assert!(
            !bridge.routes.contains_key(&final_live.viewport()),
            "a physical keepalive must not become an authoritative route",
        );
        assert!(plan.terminal_viewports.is_empty());
        assert!(plan.take_intermediate(intermediate));
        assert!(plan.take_successor(child_exact).is_some());
        let (keepalives, terminal_viewports) = plan
            .finish()
            .expect("the ordered lineage is completely consumed");
        assert_eq!(
            keepalives,
            BTreeMap::from([(
                final_live.viewport(),
                RouteLessNativeKeepalive::new(final_live, CHILD_SURFACE),
            )]),
        );
        assert!(terminal_viewports.is_empty());
    }

    #[test]
    fn replacement_lineage_without_a_live_successor_keeps_only_tombstones() {
        let PendingRestoredFixture {
            bridge,
            child_exact,
            ..
        } = committed_pending_restored_registration();
        let intermediate = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
        );
        let mut plan = bridge
            .plan_deferred_replacement_lineages_from_exacts(
                BTreeMap::from([(child_exact.viewport(), vec![child_exact, intermediate])]),
                &BTreeSet::new(),
                &runtime_child_roster(child_exact.viewport()),
            )
            .expect("a fully retired replacement lineage is valid");

        assert!(plan.successor_after_retirement.is_empty());
        assert_eq!(
            plan.intermediate_retirements,
            BTreeSet::from([intermediate]),
        );
        assert_eq!(plan.route_less_lifetimes, BTreeSet::from([intermediate]),);
        assert!(plan.live_keepalives.is_empty());
        assert_eq!(
            plan.terminal_viewports,
            BTreeSet::from([child_exact.viewport()]),
        );
        assert!(plan.take_intermediate(intermediate));
        let (keepalives, terminal_viewports) = plan
            .finish()
            .expect("the terminal lineage is completely consumed");
        assert!(keepalives.is_empty());
        assert_eq!(terminal_viewports, BTreeSet::from([child_exact.viewport()]),);
    }

    #[test]
    fn deferred_replacement_adoption_is_transactional_and_fast_retirement_keeps_core_authority() {
        let PendingRestoredFixture {
            mut dockspace,
            mut bridge,
            mut presentations,
            host_route,
            child_exact,
            child_binding,
        } = committed_pending_restored_registration();
        let replacement = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
        );
        bridge.pending_registrations.insert(
            replacement,
            PendingNativeRegistration::awaiting_predecessor(
                replacement,
                CHILD_SURFACE,
                child_binding,
            ),
        );
        assert!(matches!(
            bridge.pending_registrations[&replacement].phase,
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding)
                if binding == child_binding
        ));

        let child_route = NativeCoreRoute::new(child_exact, CHILD_SURFACE, child_binding);
        bridge
            .recorder
            .as_mut()
            .expect("the recorder is enrolled")
            .record_platform_snapshot(
                dockspace.engine().version().epoch(),
                live_child_snapshot(1, host_route.core(), child_binding),
            )
            .expect("the live child coordinates enter the ordered prefix");
        record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
        commit_backend_batch(
            &mut dockspace,
            bridge.recorder.as_ref().expect("the recorder is enrolled"),
            [host_route, child_route],
            3,
        );
        bridge
            .reclaim_committed_prefix(&mut dockspace)
            .expect("the live child facts are reclaimed after commit");

        let retirement_transaction = NativeIngressTransaction {
            recorder: bridge
                .recorder
                .as_ref()
                .expect("the recorder is enrolled")
                .savepoint(),
            state: bridge.state_snapshot(),
            presentation: presentations.prepare_savepoint(),
        };
        bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the predecessor retirement is valid")
            .expect("the committed predecessor has core authority");
        bridge
            .record_retirement_quiescence(&mut presentations, child_exact)
            .expect("the predecessor ingress is fully quiescent");
        bridge
            .recorder
            .as_mut()
            .expect("the recorder is enrolled")
            .record_platform_snapshot(
                dockspace.engine().version().epoch(),
                destroyed_child_snapshot(2, host_route.core(), child_binding),
            )
            .expect("the predecessor destruction enters the ordered prefix");
        record_idle_pointer(bridge.recorder.as_mut().expect("the recorder is enrolled"));
        commit_backend_batch_with_roster(
            &mut dockspace,
            bridge.recorder.as_ref().expect("the recorder is enrolled"),
            NativeBindingRoster::new([host_route], [child_exact]),
            4,
        );
        bridge.commit_transaction(&presentations, retirement_transaction);
        presentations.accept_commit();
        bridge
            .reclaim_committed_prefix(&mut dockspace)
            .expect("the predecessor retirement boundary is reclaimed");
        let replacement_binding = dockspace
            .engine()
            .viewport()
            .recovery_pending(CHILD_SURFACE)
            .and_then(dockspace::frame::RecoveryPending::replacement_binding)
            .expect("core destruction mints one exact replacement binding");
        assert_ne!(replacement_binding, child_binding);

        let replay_transaction = NativeIngressTransaction {
            recorder: bridge
                .recorder
                .as_ref()
                .expect("the recorder is enrolled")
                .savepoint(),
            state: bridge.state_snapshot(),
            presentation: presentations.prepare_savepoint(),
        };
        let mut effect_cycle = bridge.begin_effect_cycle();
        assert_eq!(
            effect_cycle.deferred_replacements.get(&CHILD_SURFACE),
            Some(&replacement),
        );
        effect_cycle
            .adopted_replacements
            .insert(replacement, replacement_binding);
        bridge
            .prepare_effect_cycle_adoptions(&effect_cycle)
            .expect("the existing native lifetime adopts the core-minted replacement");
        assert!(matches!(
            bridge.pending_registrations[&replacement].phase,
            PendingNativeRegistrationPhase::AdoptedReplacement {
                predecessor,
                replacement,
            } if predecessor == child_binding && replacement == replacement_binding
        ));
        bridge.rollback_transaction(&mut dockspace, &mut presentations, replay_transaction);
        assert!(matches!(
            bridge.pending_registrations[&replacement].phase,
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding)
                if binding == child_binding
        ));

        bridge
            .prepare_effect_cycle_adoptions(&effect_cycle)
            .expect("the aborted adoption can be replayed exactly");
        assert!(matches!(
            bridge.pending_registrations[&replacement].phase,
            PendingNativeRegistrationPhase::AdoptedReplacement {
                predecessor,
                replacement,
            } if predecessor == child_binding && replacement == replacement_binding
        ));

        let retirement = bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                replacement,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("an adopted replacement has exact core retirement authority")
            .expect("the adopted replacement remains docking-owned before route publication");
        assert_eq!(retirement.core, replacement_binding);
        assert_eq!(retirement.adapter_retirement(), None);
        assert_eq!(
            bridge.retired_routes.get(&replacement),
            Some(&replacement_binding),
        );
        assert!(
            !bridge
                .deferred_replacement_retirements
                .contains(&replacement),
            "an adopted replacement must not fall back to the authority-free retirement lane",
        );
    }

    #[test]
    fn deferred_replacement_can_coexist_with_an_unpublished_committed_predecessor() {
        let PendingRestoredFixture {
            mut bridge,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        let replacement = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
        );
        let registration = PendingNativeRegistration::awaiting_predecessor(
            replacement,
            CHILD_SURFACE,
            child_binding,
        );

        bridge
            .insert_deferred_replacement_registration(registration, child_exact)
            .expect("the unpublished committed predecessor remains exact retirement authority");

        assert_eq!(
            bridge.pending_registrations[&child_exact].committed_binding(),
            Some(child_binding),
        );
        assert_eq!(
            bridge.pending_registrations[&replacement].predecessor_binding(),
            Some(child_binding),
        );
    }

    #[test]
    fn deferred_replacement_fast_retirement_is_exact_and_rollback_safe() {
        let PendingRestoredFixture {
            mut bridge,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        let replacement = ExactNativeViewport::new(
            child_exact.viewport(),
            NativeViewportIncarnation::new(child_exact.incarnation().get() + 1),
        );
        bridge.pending_registrations.insert(
            replacement,
            PendingNativeRegistration::awaiting_predecessor(
                replacement,
                CHILD_SURFACE,
                child_binding,
            ),
        );
        let snapshot = bridge.state_snapshot();

        assert!(
            bridge
                .retire_deferred_replacement(replacement)
                .expect("the route-less replacement has a typed retirement lane"),
        );
        assert!(!bridge.pending_registrations.contains_key(&replacement));
        assert!(bridge.route_less_replacement(replacement));

        bridge.restore_state(snapshot);
        assert!(bridge.pending_registrations.contains_key(&replacement));
        assert!(
            !bridge
                .deferred_replacement_retirements
                .contains(&replacement)
        );

        bridge
            .retire_deferred_replacement(replacement)
            .expect("the replayed retirement remains exact");
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");
        bridge
            .record_retirement_quiescence(&mut presentations, replacement)
            .expect("route-less replacement quiescence needs no forged core receipt");
        assert!(!bridge.route_less_replacement(replacement));
        assert!(matches!(
            bridge.record_retirement_quiescence(&mut presentations, replacement),
            Err(NativeRuntimeError::IngressUnavailable(_))
        ));
    }

    #[test]
    fn adopted_provisional_retirement_requires_matching_core_authorities() {
        let PendingRestoredFixture {
            mut dockspace,
            mut bridge,
            mut presentations,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        bridge
            .pending_registrations
            .get_mut(&child_exact)
            .expect("the replacement retains its pending registration")
            .phase = PendingNativeRegistrationPhase::AdoptedReplacement {
            predecessor: child_binding,
            replacement: child_binding,
        };
        bridge
            .effects
            .install_provisional_native_for_test(child_exact, child_binding, false);

        let retirement = bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the provisional lifetime has exact retirement authority")
            .expect("the provisional core binding produces one retirement");

        assert_eq!(retirement.surface, CHILD_SURFACE);
        assert_eq!(retirement.core, child_binding);
        assert_eq!(retirement.adapter_retirement(), None);
        assert_eq!(
            bridge.retired_routes.get(&child_exact),
            Some(&child_binding)
        );
        assert!(!bridge.pending_registrations.contains_key(&child_exact));
        assert!(!bridge.effects.has_provisional_native(child_exact));
        assert!(
            bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );
    }

    #[test]
    fn fresh_provisional_retirement_uses_effect_core_authority() {
        let PendingRestoredFixture {
            mut dockspace,
            mut bridge,
            mut presentations,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        bridge.pending_registrations.remove(&child_exact);
        bridge
            .effects
            .install_provisional_native_for_test(child_exact, child_binding, false);

        let retirement = bridge
            .retire_native_binding(
                &mut dockspace,
                &mut presentations,
                &mut BTreeMap::new(),
                &mut BTreeMap::new(),
                child_exact,
                CloseEffectAcknowledgement::known(None),
            )
            .expect("the fresh provisional lifetime has exact retirement authority")
            .expect("the fresh create core binding produces one retirement");

        assert_eq!(retirement.surface, CHILD_SURFACE);
        assert_eq!(retirement.core, child_binding);
        assert_eq!(retirement.adapter_retirement(), None);
        assert!(!bridge.effects.has_provisional_native(child_exact));
        assert!(
            bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );
    }

    #[test]
    fn failed_native_initialization_is_quarantined_and_rollback_safe() {
        let PendingRestoredFixture {
            mut bridge,
            child_exact,
            child_binding,
            ..
        } = committed_pending_restored_registration();
        bridge
            .pending_registrations
            .get_mut(&child_exact)
            .expect("the replacement retains its pending registration")
            .phase = PendingNativeRegistrationPhase::AdoptedReplacement {
            predecessor: child_binding,
            replacement: child_binding,
        };
        bridge
            .effects
            .install_provisional_native_for_test(child_exact, child_binding, true);
        let snapshot = bridge.state_snapshot();

        assert!(
            bridge
                .quarantine_failed_native_lifetime(child_exact, &mut BTreeMap::new())
                .expect("the failed initialization enters quarantine")
        );
        assert!(bridge.quarantined_native_lifetimes.contains(&child_exact));
        assert!(!bridge.pending_registrations.contains_key(&child_exact));
        assert!(!bridge.effects.has_provisional_native(child_exact));
        assert!(
            bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );
        assert!(bridge.route_less_replacement(child_exact));

        bridge.restore_state(snapshot);
        assert!(!bridge.quarantined_native_lifetimes.contains(&child_exact));
        assert!(bridge.pending_registrations.contains_key(&child_exact));
        assert!(bridge.effects.has_provisional_native(child_exact));
        assert!(
            !bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );

        bridge
            .quarantine_failed_native_lifetime(child_exact, &mut BTreeMap::new())
            .expect("the replayed failure enters the same quarantine");
        assert!(
            bridge
                .retire_quarantined_native_lifetime(child_exact)
                .expect("the quarantined lifetime has a route-less retirement lane")
        );
        assert!(!bridge.quarantined_native_lifetimes.contains(&child_exact));
        assert!(
            bridge
                .deferred_replacement_retirements
                .contains(&child_exact)
        );
        assert!(
            bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");
        bridge
            .record_retirement_quiescence(&mut presentations, child_exact)
            .expect("quiescence retires the initialization receipt tombstone");
        assert!(
            !bridge
                .effects
                .has_initialization_receipt_tombstone(child_exact)
        );
    }

    #[test]
    fn materialized_restored_bootstrap_can_retire_before_its_first_snapshot() {
        let viewport = ViewportId::from_hash_of("materialized-restored-retirement");
        let exact = ExactNativeViewport::new(viewport, NativeViewportIncarnation::new(1));
        let roster = restored_child_roster(viewport);
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        bridge
            .effects
            .install_materialized_restored_viewport_for_test(viewport, CHILD_SURFACE);
        let snapshot = bridge.state_snapshot();

        assert!(bridge.route_less_replacement(exact));
        assert!(
            bridge
                .retire_materialized_restored_bootstrap(&roster, exact)
                .expect("the materialized restored lifetime has a route-less retirement lane")
        );
        assert!(bridge.retiring_restored_bootstraps.contains(&exact));
        assert!(!bridge.effects.restored_create_is_materialized(viewport));

        bridge.restore_state(snapshot);
        assert!(!bridge.retiring_restored_bootstraps.contains(&exact));
        assert!(bridge.effects.restored_create_is_materialized(viewport));

        bridge
            .retire_materialized_restored_bootstrap(&roster, exact)
            .expect("the replayed retirement remains exact");
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");
        bridge
            .record_retirement_quiescence(&mut presentations, exact)
            .expect("exact quiescence releases the route-less bootstrap retirement");
        assert!(!bridge.route_less_replacement(exact));
        assert!(
            !bridge.effects.restored_create_is_materialized(viewport),
            "the viewport is eligible for restored create rescheduling",
        );
    }

    #[test]
    fn route_less_external_retirement_consumes_quiescence_without_core_authority() {
        let mut bridge = NativeIngressBridge::new().expect("runtime identity is available");
        let mut presentations =
            NativePresentationLedger::new().expect("presentation identity is available");
        let exact = ExactNativeViewport::new(
            ViewportId::from_hash_of("external-retirement"),
            NativeViewportIncarnation::new(1),
        );

        bridge
            .record_external_retirement(exact)
            .expect("an unknown physical viewport has an explicit external lane");
        assert!(bridge.external_retirements.contains(&exact));
        bridge
            .record_retirement_quiescence(&mut presentations, exact)
            .expect("fork quiescence terminates the external lane without a core receipt");
        assert!(bridge.external_retirements.is_empty());
        assert!(matches!(
            bridge.record_retirement_quiescence(&mut presentations, exact),
            Err(NativeRuntimeError::IngressUnavailable(_))
        ));
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
        let exact = ExactNativeViewport::new(ViewportId::ROOT, NativeViewportIncarnation::new(1));
        bridge
            .insert_pending_registration(PendingNativeRegistration::staged(exact, surface, ordinal))
            .expect("the registration sidecar is staged");
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
