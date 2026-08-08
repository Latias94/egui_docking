//! Exact translation from the fork event-loop protocol into dockspace ingress.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::PlatformObservationLease;
use dockspace::backend_ingress::{
    BackendIngressBatch, BackendIngressOrdinal, BackendIngressPayload,
    BackendIngressPrefixRetirementReceipt, BackendIngressRecorder, BackendIngressSavepoint,
};
#[cfg(test)]
use dockspace::effect::EffectResult;
use dockspace::effect::{EffectId, PlatformEffectEmission};
use dockspace::engine::EngineInput;
use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::ids::{SurfaceId, WorkspaceEpoch};
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
use dockspace::transition::EngineTransition;
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
    NativePointerIdentity, NativePointerInputState, NativePointerStreamCancelReason,
    NativePresentationState, NativeScrollCancelReason, NativeScrollDelta, NativeScrollEdge,
    NativeScrollMomentum, NativeScrollPhase, NativeUnavailableReason, NativeViewportBinding,
    NativeViewportCreateSink, NativeWindowSnapshot, NativeWorkAreaRosterObservation,
    NativeWorkAreaRoute,
};
use egui::ViewportId;
use egui_dockspace::{
    Dockspace, ExactNativeViewport, NativeBindingRoster, NativeCoreRoute,
    NativeViewportIncarnation, PaintReceiverFingerprint, PaintReceiverLookup,
};

use crate::effects::{DeferredEffectResult, NativeEffectDriver};
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
    Effect(DeferredEffectResult),
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

impl NativeEffectCycle {
    pub(crate) fn settle_effect_results(&mut self, transition: &EngineTransition) {
        self.effects.settle_effect_results(transition);
    }
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
        self.flush_post_commit_records(dockspace.engine().version().epoch())?;
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
                    self.effects.consume_effect_result(
                        result,
                        dockspace.engine().version().epoch(),
                        recorder,
                    )?;
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
        if dockspace.has_pending_document_restore() {
            match dockspace.pending_document_restore_matches_current_surface_roster() {
                Some(true) => {}
                Some(false) => return Err(NativeRuntimeError::WorkspaceRosterMismatch),
                None => {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "queued document restore lost its validated workspace",
                    ));
                }
            }
        }
        let batch = {
            let recorder = self
                .recorder
                .as_mut()
                .ok_or(NativeRuntimeError::IngressUnavailable(
                    "native backend provider was not enrolled",
                ))?;
            dockspace.record_pending_backend_document_restore(recorder)?;
            recorder.pending_batch()?
        };
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
                self.effects.retire_cleanup_native(exact);
                return Ok(());
            }
            if self.deferred_replacement_retirements.remove(&exact) {
                self.effects.retire_initialization_receipts(exact);
                self.effects.retire_cleanup_native(exact);
                return Ok(());
            }
            if self.external_retirements.remove(&exact) {
                self.effects.retire_cleanup_native(exact);
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
        self.effects.retire_cleanup_binding(binding, exact);
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

    fn flush_post_commit_records(
        &mut self,
        current_epoch: WorkspaceEpoch,
    ) -> Result<(), NativeRuntimeError> {
        let records = std::mem::take(&mut self.post_commit_records);
        let recorder = self
            .recorder
            .as_mut()
            .expect("the provider was enrolled before flushing the native outbox");
        for record in records {
            match record {
                PostCommitRecord::Effect(result) => {
                    self.effects
                        .record_deferred_effect_result(recorder, current_epoch, result)?;
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
            NativePointerEdgeKind::ContactEnded(button) => {
                PointerEdgeKind::ContactEnded(translate_button(button))
            }
            NativePointerEdgeKind::StreamEnded => PointerEdgeKind::StreamEnded,
            NativePointerEdgeKind::CaptureChanged => PointerEdgeKind::CaptureChanged,
            NativePointerEdgeKind::StreamCancelled(reason) => {
                PointerEdgeKind::StreamCancelled(translate_pointer_cancel_reason(reason))
            }
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
        let stream_terminal = matches!(
            kind,
            PointerEdgeKind::ContactEnded(_)
                | PointerEdgeKind::StreamEnded
                | PointerEdgeKind::StreamCancelled(_)
        );
        let translated = PointerEdge::new_with_delivery(
            sequence,
            pointer,
            kind,
            PointerEdgeLocation::Desktop { route },
            delivery_owner,
            capture_owner,
        );
        if stream_terminal {
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

mod translation;

use translation::*;

#[cfg(test)]
#[path = "ingress/tests.rs"]
mod tests;
