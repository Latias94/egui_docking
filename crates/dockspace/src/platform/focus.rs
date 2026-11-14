//! Adapter-neutral native viewport activation and pane-focus coordination.
//!
//! Platform focus and pane focus are separate facts. A selected tab never proves
//! that its pane owns UI focus, and requesting native-window activation never
//! proves that the request was applied. The coordinator therefore keeps exact
//! binding identities, provider generations, effect identities, and pane-focus
//! acknowledgements until their corresponding facts arrive.

mod activation;
mod lifecycle;
mod observation;
mod pane;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::close::plan::CloseRequestId;
use crate::engine::event::ReductionCause;
use crate::ids::{ItemId, NativeCreateSagaId, SurfaceId};
use crate::input::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::effect::{
    EffectDispatchResult, EffectId, EffectLedger, EffectPhase, EffectRequest, PlatformEffect,
};
use crate::platform::frame::PanelFocus;
use crate::platform::viewport::{ViewportBinding, WindowBinding};
use crate::presentation::observation::{HostPresentationStreamId, PresentationHostLease};

macro_rules! focus_generation {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a generation from its protocol representation.
            #[must_use]
            #[allow(dead_code, reason = "Opaque identity domains share this protocol helper; each domain uses only the helpers it needs.")]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Advances without permitting ABA through integer wrapping.
            #[must_use]
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

focus_generation!(
    FocusObservationGeneration,
    "Provider-generated generation of one globally consistent focused-window observation."
);

#[cfg(test)]
impl FocusObservationGeneration {
    /// Returns the protocol representation used by platform snapshot fixtures.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Core-minted causal position of one focus-producing reducer fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusCausalStamp {
    generation: PaneFocusIntentGeneration,
    cause: ReductionCause,
}

impl FocusCausalStamp {
    /// Creates a causal stamp from a reducer-issued cause.
    ///
    /// The engine never accepts this value as input; it only publishes stamps
    /// that it minted while reducing an authoritative host frame. This
    /// constructor remains available for direct coordinator conformance tests.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(generation: PaneFocusIntentGeneration, cause: ReductionCause) -> Self {
        Self { generation, cause }
    }

    #[must_use]
    pub const fn generation(self) -> PaneFocusIntentGeneration {
        self.generation
    }

    #[must_use]
    pub const fn cause(self) -> ReductionCause {
        self.cause
    }
}
focus_generation!(
    ActivationGeneration,
    "Core-generated identity and ordering generation of one viewport activation."
);
focus_generation!(
    PaneFocusIntentGeneration,
    "Core reducer generation which orders competing pane-focus intents."
);
focus_generation!(
    PaneFocusIntentId,
    "Core-generated identity of one pane-focus command awaiting adapter acknowledgement."
);
focus_generation!(
    PaneFocusObservationGeneration,
    "Provider-generated generation of one surface's pane-focus observation."
);

/// Globally consistent native focus reported by one provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlobalFocusedWindow {
    /// One exact live docking window is focused, without selecting a placement or pane.
    Dock(WindowBinding),
    /// A non-docking application or system window is focused.
    Foreign,
    /// The provider authoritatively reports that no native window is focused.
    None,
}

/// One complete global focus observation and optional exact effect acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusObservationEnvelope {
    generation: FocusObservationGeneration,
    focused: Authority<GlobalFocusedWindow>,
    acknowledged_effect: Authority<Option<EffectId>>,
}

impl FocusObservationEnvelope {
    #[must_use]
    pub const fn new(
        generation: FocusObservationGeneration,
        focused: Authority<GlobalFocusedWindow>,
        acknowledged_effect: Authority<Option<EffectId>>,
    ) -> Self {
        Self {
            generation,
            focused,
            acknowledged_effect,
        }
    }

    #[must_use]
    pub const fn generation(self) -> FocusObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn focused(&self) -> &Authority<GlobalFocusedWindow> {
        &self.focused
    }

    #[must_use]
    pub const fn acknowledged_effect(&self) -> &Authority<Option<EffectId>> {
        &self.acknowledged_effect
    }
}

/// Last authoritative member of one provider-owned global focus stream.
///
/// Missing, skipped-generation, or same-generation conflicting envelopes revoke
/// current authority and require an explicit versioned tombstone before a later
/// known focus may become authoritative again. Older generations and exact
/// duplicates are inert. Provider replacement starts a new generation namespace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FocusObservationStream {
    current: Option<FocusObservationEnvelope>,
    last_generation: Option<FocusObservationGeneration>,
    last_envelope: Option<FocusObservationEnvelope>,
    requires_tombstone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusObservationStreamTransition {
    Current,
    Duplicate,
    Stale {
        current: FocusObservationGeneration,
    },
    AuthorityRevoked {
        generation: Option<FocusObservationGeneration>,
        reason: FocusAuthorityRevocation,
    },
    TombstoneRequired {
        generation: FocusObservationGeneration,
    },
}

/// The last observed pane-focus history for one logical surface.
///
/// History remains available after focus leaves this surface. It does not prove
/// that the recorded pane currently owns framework or native-window focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PanelFocusRecord {
    /// No adapter fact has ever established pane focus for this surface.
    #[default]
    NoHistory,
    /// This exact item most recently owned pane focus.
    Item(ItemId),
    /// The adapter explicitly observed that no dock pane owned focus.
    None,
}

impl PanelFocusRecord {
    #[must_use]
    pub const fn from_focus(focus: PanelFocus) -> Self {
        match focus {
            PanelFocus::Item(item) => Self::Item(item),
            PanelFocus::None => Self::None,
        }
    }
}

/// An adapter's actual UI focus within one owned surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostFocusState {
    /// This exact pane owns framework focus.
    Pane(ItemId),
    /// The dock workspace itself owns framework focus.
    Workspace,
    /// Focus belongs to application UI outside this dock workspace.
    External,
    /// The adapter positively observed that no UI target owns focus.
    None,
    /// The adapter cannot establish the current UI focus target.
    Unknown,
}

/// Actual UI focus, independent from restoration history and pending requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentUiFocus {
    /// An exact pane currently owns framework focus.
    Pane { surface: SurfaceId, item: ItemId },
    /// The surface workspace currently owns framework focus.
    Workspace { surface: SurfaceId },
    /// Application UI outside the docking workspace owns focus.
    External,
    /// The responsible provider positively observed no focused target.
    None,
    /// Current focus cannot be established from available authority.
    Unknown,
}

impl CurrentUiFocus {
    const fn from_host(surface: SurfaceId, state: HostFocusState) -> Self {
        match state {
            HostFocusState::Pane(item) => Self::Pane { surface, item },
            HostFocusState::Workspace => Self::Workspace { surface },
            HostFocusState::External => Self::External,
            HostFocusState::None => Self::None,
            HostFocusState::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CurrentUiFocusRecord {
    focus: CurrentUiFocus,
    scope: Option<(SurfaceId, Option<ViewportBinding>)>,
    host_scope: Option<(PresentationHostLease, HostPresentationStreamId)>,
}

/// One independent UI focus fact captured within an exact presentation owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostFocusObservation {
    generation: PaneFocusObservationGeneration,
    host: PresentationHostLease,
    stream: HostPresentationStreamId,
    surface: SurfaceId,
    state: HostFocusState,
}

impl HostFocusObservation {
    /// Captures one generation in an exact host-owned presentation stream.
    #[must_use]
    pub const fn new(
        generation: PaneFocusObservationGeneration,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
        surface: SurfaceId,
        state: HostFocusState,
    ) -> Self {
        Self {
            generation,
            host,
            stream,
            surface,
            state,
        }
    }

    /// Returns the host which captured this UI fact.
    #[must_use]
    pub const fn host(self) -> PresentationHostLease {
        self.host
    }

    /// Returns the exact stream whose current ownership must still validate.
    #[must_use]
    pub const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    /// Returns the logical surface delimiting this observation's scope.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Pane-focus change frozen by one semantic activation.
///
/// This is deliberately distinct from [`PanelFocus`]. `PanelFocus` is an
/// exact adapter observation or command, while `Preserve` means that no pane
/// command may be minted at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusDisposition {
    /// Keep the target surface's pane-focus state unchanged.
    Preserve,
    /// Focus this exact item after the viewport activation is authoritative.
    Set(ItemId),
    /// Explicitly clear pane focus after the viewport activation is authoritative.
    Clear,
}

impl PaneFocusDisposition {
    #[must_use]
    pub const fn from_focus(focus: PanelFocus) -> Self {
        match focus {
            PanelFocus::Item(item) => Self::Set(item),
            PanelFocus::None => Self::Clear,
        }
    }

    #[must_use]
    pub const fn from_record(record: PanelFocusRecord) -> Self {
        match record {
            PanelFocusRecord::NoHistory => Self::Preserve,
            PanelFocusRecord::Item(item) => Self::Set(item),
            PanelFocusRecord::None => Self::Clear,
        }
    }

    #[must_use]
    pub const fn focus(self) -> Option<PanelFocus> {
        match self {
            Self::Preserve => None,
            Self::Set(item) => Some(PanelFocus::Item(item)),
            Self::Clear => Some(PanelFocus::None),
        }
    }
}

/// Semantic origin of one explicit viewport activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportActivationCause {
    /// An application or user command explicitly activated the viewport.
    Explicit,
    /// A completed docking delivery selected the target viewport.
    DropCommitted,
    /// A completed native tear-off selected the newly created viewport.
    TearOffCommitted,
    /// A pointer press selected a tab in an existing viewport.
    ///
    /// The gesture may observe native focus but never requests it: the platform
    /// pointer dispatch remains the sole authority for any window-focus change.
    PointerTabGesture,
    /// A close transaction moved content into an existing target viewport.
    CloseRecovery { request: CloseRequestId },
    /// A retained native surface replacement was admitted after destruction.
    RecoveryReplacement,
    /// An authoritative platform focus change with no explicit semantic request.
    PlatformObservation,
}

impl ViewportActivationCause {
    #[must_use]
    pub const fn requests_platform_focus(self) -> bool {
        !matches!(
            self,
            Self::PointerTabGesture
                | Self::CloseRecovery { .. }
                | Self::RecoveryReplacement
                | Self::PlatformObservation
        )
    }

    #[must_use]
    const fn pane_source(self) -> PaneFocusIntentSource {
        match self {
            Self::PointerTabGesture => PaneFocusIntentSource::PointerTabGesture,
            Self::CloseRecovery { .. } | Self::RecoveryReplacement => {
                PaneFocusIntentSource::CloseRecovery
            }
            Self::PlatformObservation => PaneFocusIntentSource::PlatformActivation,
            Self::Explicit | Self::DropCommitted | Self::TearOffCommitted => {
                PaneFocusIntentSource::ExplicitViewportActivation
            }
        }
    }

    #[must_use]
    const fn awaits_observed_focus(self) -> bool {
        matches!(self, Self::PointerTabGesture)
    }
}

/// Exact activation request created only after its semantic mutation succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportActivationRequest {
    target: ViewportBinding,
    pane: PaneFocusDisposition,
    cause: ViewportActivationCause,
}

impl ViewportActivationRequest {
    #[must_use]
    #[doc(hidden)]
    pub const fn new(
        target: ViewportBinding,
        focus: PanelFocus,
        cause: ViewportActivationCause,
    ) -> Self {
        Self::with_disposition(target, PaneFocusDisposition::from_focus(focus), cause)
    }

    #[must_use]
    pub const fn with_disposition(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
        cause: ViewportActivationCause,
    ) -> Self {
        Self {
            target,
            pane,
            cause,
        }
    }

    #[must_use]
    pub const fn explicit(target: ViewportBinding, focus: PanelFocus) -> Self {
        Self::new(target, focus, ViewportActivationCause::Explicit)
    }

    #[must_use]
    pub(crate) const fn drop_committed(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
    ) -> Self {
        Self::with_disposition(target, pane, ViewportActivationCause::DropCommitted)
    }

    #[must_use]
    pub(crate) const fn tear_off_committed(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
    ) -> Self {
        Self::with_disposition(target, pane, ViewportActivationCause::TearOffCommitted)
    }

    #[must_use]
    pub(crate) const fn pointer_tab_gesture(target: ViewportBinding, focus: PanelFocus) -> Self {
        Self::new(target, focus, ViewportActivationCause::PointerTabGesture)
    }

    #[must_use]
    #[doc(hidden)]
    pub const fn close_recovery(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
        request: CloseRequestId,
    ) -> Self {
        Self::with_disposition(
            target,
            pane,
            ViewportActivationCause::CloseRecovery { request },
        )
    }

    #[must_use]
    pub(crate) const fn recovery_replacement(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
    ) -> Self {
        Self::with_disposition(target, pane, ViewportActivationCause::RecoveryReplacement)
    }

    #[must_use]
    pub const fn target(self) -> ViewportBinding {
        self.target
    }

    #[must_use]
    pub const fn pane(self) -> PaneFocusDisposition {
        self.pane
    }

    #[must_use]
    pub const fn focus(self) -> Option<PanelFocus> {
        self.pane.focus()
    }

    #[must_use]
    pub const fn cause(self) -> ViewportActivationCause {
        self.cause
    }
}

/// Source precedence for pane intents in one reducer generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusIntentSource {
    PlatformActivation,
    ExplicitViewportActivation,
    PointerTabGesture,
    CloseRecovery,
    WorkspaceReplacement,
}

impl PaneFocusIntentSource {
    #[must_use]
    const fn priority(self) -> u8 {
        match self {
            Self::PlatformActivation => 0,
            Self::ExplicitViewportActivation => 1,
            Self::PointerTabGesture => 2,
            Self::CloseRecovery => 3,
            Self::WorkspaceReplacement => 2,
        }
    }
}

/// One exact pane-focus command retained until an adapter observation confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFocusIntent {
    id: PaneFocusIntentId,
    causal: FocusCausalStamp,
    activation: Option<ActivationGeneration>,
    surface: SurfaceId,
    item: Option<ItemId>,
    native: PaneFocusNativeTarget,
    source: PaneFocusIntentSource,
    cause: Option<ViewportActivationCause>,
    focus_observation_baseline: FocusObservationGeneration,
    pane_observation_baseline: Option<PaneFocusObservationGeneration>,
}

/// An unresolved native destination must never acquire local focus authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneFocusNativeTarget {
    Local,
    Waiting,
    Bound(ViewportBinding),
}

/// Replacement distinguishes deliberate outside focus from an absent history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceReplacementFocus {
    NoHistory,
    OutsideDock,
    Dock {
        surface: SurfaceId,
        item: Option<ItemId>,
    },
}

impl PaneFocusIntent {
    #[must_use]
    pub const fn id(self) -> PaneFocusIntentId {
        self.id
    }

    #[must_use]
    pub const fn causal(self) -> FocusCausalStamp {
        self.causal
    }

    #[must_use]
    #[cfg(test)]
    pub const fn activation(self) -> Option<ActivationGeneration> {
        self.activation
    }

    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    #[must_use]
    pub const fn item(self) -> Option<ItemId> {
        self.item
    }

    #[must_use]
    pub const fn native_guard(self) -> Option<ViewportBinding> {
        match self.native {
            PaneFocusNativeTarget::Bound(binding) => Some(binding),
            PaneFocusNativeTarget::Local | PaneFocusNativeTarget::Waiting => None,
        }
    }

    /// Returns whether this intent still needs a fresh, eligible native binding.
    ///
    /// Such an intent is retained internally and cannot be published to an adapter.
    #[must_use]
    pub const fn awaiting_native_binding(self) -> bool {
        matches!(self.native, PaneFocusNativeTarget::Waiting)
    }

    #[must_use]
    pub const fn focus(self) -> PanelFocus {
        match self.item {
            Some(item) => PanelFocus::Item(item),
            None => PanelFocus::None,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub const fn source(self) -> PaneFocusIntentSource {
        self.source
    }

    #[must_use]
    #[cfg(test)]
    pub const fn cause(self) -> Option<ViewportActivationCause> {
        self.cause
    }

    #[must_use]
    pub const fn pane_observation_baseline(self) -> Option<PaneFocusObservationGeneration> {
        self.pane_observation_baseline
    }
}

/// Platform-focus phase of one explicit activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingPlatformFocus {
    /// The core must atomically allocate and attach a `RequestFocus` effect.
    EffectRequired,
    /// The effect is emitted or awaiting an adapter result.
    Requested { effect: EffectId },
    /// Dispatch outcome is unknown; only authoritative focus facts can resolve it.
    Indeterminate {
        effect: EffectId,
        reason: crate::platform::effect::EffectIndeterminateReason,
    },
    /// The provider acknowledged the exact effect while global focus remained unknown.
    ObservedAwaitingTarget {
        effect: EffectId,
        acknowledged_at: FocusObservationGeneration,
    },
}

impl PendingPlatformFocus {
    #[must_use]
    pub const fn effect(self) -> Option<EffectId> {
        match self {
            Self::EffectRequired => None,
            Self::Requested { effect }
            | Self::Indeterminate { effect, .. }
            | Self::ObservedAwaitingTarget { effect, .. } => Some(effect),
        }
    }
}

/// Queryable exact-incarnation activation waiting for backend confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingViewportActivation {
    generation: ActivationGeneration,
    causal: FocusCausalStamp,
    request: ViewportActivationRequest,
    observation_baseline: FocusObservationGeneration,
    platform_focus: PendingPlatformFocus,
}

/// Observe-only activation retained for causal settlement and exact cleanup.
///
/// The cause determines whether a newer exact target-focus observation releases
/// a pane intent. Close recovery remains diagnostic-only; a pointer tab gesture
/// may complete only from a causally newer provider observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedObserveOnlyActivation {
    generation: ActivationGeneration,
    causal: FocusCausalStamp,
    request: ViewportActivationRequest,
    observation_baseline: Option<FocusObservationGeneration>,
}

impl RecordedObserveOnlyActivation {
    #[must_use]
    #[cfg(test)]
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    #[cfg(test)]
    pub const fn request(self) -> ViewportActivationRequest {
        self.request
    }
}

impl PendingViewportActivation {
    #[must_use]
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn request(self) -> ViewportActivationRequest {
        self.request
    }

    #[must_use]
    #[cfg(test)]
    pub const fn observation_baseline(self) -> FocusObservationGeneration {
        self.observation_baseline
    }

    #[must_use]
    pub const fn platform_focus(self) -> PendingPlatformFocus {
        self.platform_focus
    }
}

/// Why an activation could not enter a pending or pane-intent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivationSuppression {
    StaleBinding,
    ItemUnavailable { item: ItemId },
    CausallySuperseded { current: PaneFocusIntentGeneration },
    FocusObservationBaselineUnavailable,
    PlatformFocusControlUnavailable,
}

/// Why the core could not reveal the exact pane named by an installed focus intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusRevealRejection {
    /// The item no longer belongs to the target surface.
    ItemUnavailable { item: ItemId },
    /// Native lifecycle recovery froze the surface presentation snapshot.
    SurfaceLifecycleFrozen { surface: SurfaceId },
    /// Workspace policy rejected the reveal mutation.
    PolicyRejected,
    /// A checked workspace precondition rejected the reveal mutation.
    WorkspaceRejected,
}

/// Result of recording one activation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationStartOutcome {
    /// The target already had authoritative native focus and a pane intent was installed.
    PaneFocusReady { intent: PaneFocusIntent },
    /// The target activation is complete and pane focus was intentionally left unchanged.
    PaneFocusPreserved { target: ViewportBinding },
    /// An explicit activation must emit `PlatformEffect::RequestFocus` for this binding.
    RequestPlatformFocus { target: ViewportBinding },
    /// A higher-priority same-generation pane intent remains authoritative.
    PaneIntentSuperseded,
    /// A pane intent was installed but its exact reveal precondition was rejected.
    PaneRevealRejected {
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    },
    /// Observe-only activation was recorded without requesting platform focus.
    ObserveOnlyRecorded {
        record: RecordedObserveOnlyActivation,
    },
    /// The request failed closed without issuing a platform effect.
    Suppressed(ActivationSuppression),
}

/// Identity and immediate outcome of one activation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationStart {
    generation: ActivationGeneration,
    superseded: Option<PendingViewportActivation>,
    outcome: ActivationStartOutcome,
}

impl ActivationStart {
    #[must_use]
    #[cfg(test)]
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn outcome(self) -> ActivationStartOutcome {
        self.outcome
    }

    /// Returns the older explicit activation whose effect obligation remains in the causal lane.
    #[must_use]
    #[cfg(test)]
    pub const fn superseded(self) -> Option<PendingViewportActivation> {
        self.superseded
    }

    pub(crate) fn reject_pane_reveal(
        mut self,
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    ) -> Self {
        if matches!(
            self.outcome,
            ActivationStartOutcome::PaneFocusReady { intent: current }
                if current.id == intent.id
        ) {
            self.outcome = ActivationStartOutcome::PaneRevealRejected { intent, reason };
        }
        self
    }
}

/// Evidence used to settle a platform focus effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformFocusEvidence {
    /// The provider named the exact effect in its acknowledgement envelope.
    ExactEffectAcknowledgement,
    /// A causally newer observation authoritatively reported the requested binding focused.
    NewerMatchingObservation,
}

/// One effect proven applied by a global focus observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObservedPlatformFocusEffect {
    effect: EffectId,
    binding: ViewportBinding,
    generation: FocusObservationGeneration,
    evidence: PlatformFocusEvidence,
}

impl ObservedPlatformFocusEffect {
    pub(crate) const fn new(
        effect: EffectId,
        binding: ViewportBinding,
        generation: FocusObservationGeneration,
        evidence: PlatformFocusEvidence,
    ) -> Self {
        Self {
            effect,
            binding,
            generation,
            evidence,
        }
    }

    #[must_use]
    pub const fn effect(self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    #[cfg(test)]
    pub const fn evidence(self) -> PlatformFocusEvidence {
        self.evidence
    }
}

/// Why a pending activation was discarded while applying focus facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivationCancellation {
    SupersededByPlatformFocus {
        observation: FocusObservationGeneration,
    },
    EffectObservedWithoutTargetFocus,
    EffectFailed,
    EffectUnsupported,
    StaleBinding,
    ItemUnavailable {
        item: ItemId,
    },
    SurfaceRemoved,
    PlatformAuthorityRevoked,
}

/// State changes produced by one newly accepted global focus observation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusObservationApplied {
    observed_effects: Vec<ObservedPlatformFocusEffect>,
    completed_activation: Option<ActivationGeneration>,
    cancelled_activation: Option<(ActivationGeneration, ActivationCancellation)>,
    pane: AppliedPaneFocus,
    cleared_observe_only_activation: Option<ActivationGeneration>,
}

/// Why a provider focus envelope revoked the current global-focus authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusAuthorityRevocation {
    /// The provider omitted an envelope from an otherwise complete boundary.
    ObservationMissing,
    /// Two distinct facts reused the same provider generation.
    EqualGenerationConflict,
    /// The provider skipped one or more generations from the previous watermark.
    GenerationGap {
        previous: FocusObservationGeneration,
    },
    /// The provider explicitly reported that global focus is unavailable.
    Unknown(AuthorityUnavailableReason),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum AppliedPaneFocus {
    #[default]
    None,
    Intent(PaneFocusIntent),
    #[allow(
        dead_code,
        reason = "Pane reveal rejection preserves the exact intent and failure in the focus protocol."
    )]
    RevealRejected {
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    },
}

impl FocusObservationApplied {
    #[must_use]
    pub fn observed_effects(&self) -> &[ObservedPlatformFocusEffect] {
        &self.observed_effects
    }

    /// Returns the first effect proof for callers which expect a single active focus attempt.
    ///
    /// A focus envelope can settle more than one superseded hazard. New protocol consumers
    /// should use [`Self::observed_effects`] and preserve the complete ordered proof vector.
    #[must_use]
    #[cfg(test)]
    pub fn observed_effect(&self) -> Option<ObservedPlatformFocusEffect> {
        self.observed_effects.first().copied()
    }

    pub(crate) fn discard_observed_effect(&mut self, effect: EffectId) {
        self.observed_effects
            .retain(|observed| observed.effect != effect);
    }

    fn record_observed_effect(&mut self, observed: ObservedPlatformFocusEffect) {
        if !self
            .observed_effects
            .iter()
            .any(|current| current.effect == observed.effect)
        {
            self.observed_effects.push(observed);
        }
    }

    pub(crate) fn record_acknowledged_effect_settlement(
        &mut self,
        observed: ObservedPlatformFocusEffect,
    ) {
        self.record_observed_effect(observed);
    }

    #[must_use]
    pub const fn completed_activation(&self) -> Option<ActivationGeneration> {
        self.completed_activation
    }

    #[must_use]
    #[cfg(test)]
    pub const fn cancelled_activation(
        &self,
    ) -> Option<(ActivationGeneration, ActivationCancellation)> {
        self.cancelled_activation
    }

    #[must_use]
    pub const fn pane_intent(&self) -> Option<PaneFocusIntent> {
        match self.pane {
            AppliedPaneFocus::Intent(intent) => Some(intent),
            AppliedPaneFocus::None | AppliedPaneFocus::RevealRejected { .. } => None,
        }
    }

    fn set_pane_intent(&mut self, intent: Option<PaneFocusIntent>) {
        self.pane = intent.map_or(AppliedPaneFocus::None, AppliedPaneFocus::Intent);
    }

    pub(crate) fn reject_pane_reveal(
        &mut self,
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    ) {
        if matches!(self.pane, AppliedPaneFocus::Intent(current) if current.id == intent.id) {
            self.pane = AppliedPaneFocus::RevealRejected { intent, reason };
        }
    }

    #[must_use]
    #[cfg(test)]
    pub const fn cleared_observe_only_activation(&self) -> Option<ActivationGeneration> {
        self.cleared_observe_only_activation
    }
}

/// Deterministic acceptance result for one provider focus envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusObservationTransition {
    Applied(Box<FocusObservationApplied>),
    Duplicate,
    Stale {
        current: FocusObservationGeneration,
    },
    /// The envelope invalidated current global-focus authority.
    ///
    /// A causally valid unknown tombstone may still carry an exact effect
    /// acknowledgement. `effect_settlement` records only that independent fact;
    /// it never turns the unknown focused-window value into authority.
    AuthorityRevoked {
        generation: Option<FocusObservationGeneration>,
        reason: FocusAuthorityRevocation,
        cleanup: ViewportFocusCleanup,
        effect_settlement: Box<FocusObservationApplied>,
    },
    /// A known envelope was quarantined until the provider publishes an explicit tombstone.
    TombstoneRequired {
        generation: FocusObservationGeneration,
        cleanup: ViewportFocusCleanup,
    },
}

impl FocusObservationTransition {
    pub(crate) fn effect_settlement(&self) -> Option<&FocusObservationApplied> {
        match self {
            Self::Applied(applied)
            | Self::AuthorityRevoked {
                effect_settlement: applied,
                ..
            } => Some(applied),
            Self::Duplicate | Self::Stale { .. } | Self::TombstoneRequired { .. } => None,
        }
    }

    pub(crate) fn effect_settlement_mut(&mut self) -> Option<&mut FocusObservationApplied> {
        match self {
            Self::Applied(applied)
            | Self::AuthorityRevoked {
                effect_settlement: applied,
                ..
            } => Some(applied),
            Self::Duplicate | Self::Stale { .. } | Self::TombstoneRequired { .. } => None,
        }
    }
}

/// Result of attaching an exact effect identity to a pending activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEffectAttachment {
    Applied,
    Duplicate,
    StaleActivation,
    EffectAlreadyAttached { existing: EffectId },
}

/// Result of reducing a non-success platform effect report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEffectReportTransition {
    Indeterminate {
        activation: ActivationGeneration,
    },
    Cancelled {
        activation: ActivationGeneration,
        reason: ActivationCancellation,
    },
    Duplicate,
    StaleEffect,
}

/// One adapter-observed pane focus fact, optionally acknowledging a core intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFocusObservation {
    generation: PaneFocusObservationGeneration,
    binding: ViewportBinding,
    focus: PanelFocus,
    acknowledges: Option<PaneFocusIntentId>,
}

/// Exact result of resolving one published pane-focus request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusRequestObservationState {
    /// The requested pane target owns framework focus.
    Focused,
    /// The target is available but does not yet own framework focus.
    NotFocused,
    /// This request cannot bind to a framework focus target in its revision.
    Unavailable,
}

/// One exact request-bound pane-focus observation.
///
/// The target identity is copied from the opaque request so an adapter cannot
/// acknowledge a request while reporting a different surface, item, or native
/// incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFocusRequestObservation {
    generation: PaneFocusObservationGeneration,
    intent: PaneFocusIntentId,
    surface: SurfaceId,
    item: Option<ItemId>,
    native_guard: Option<ViewportBinding>,
    state: PaneFocusRequestObservationState,
}

impl PaneFocusRequestObservation {
    #[must_use]
    pub const fn new(
        generation: PaneFocusObservationGeneration,
        intent: PaneFocusIntent,
        state: PaneFocusRequestObservationState,
    ) -> Self {
        Self {
            generation,
            intent: intent.id,
            surface: intent.surface,
            item: intent.item,
            native_guard: intent.native_guard(),
            state,
        }
    }

    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    #[must_use]
    pub const fn native_guard(self) -> Option<ViewportBinding> {
        self.native_guard
    }
}

impl PaneFocusObservation {
    #[must_use]
    #[cfg(test)]
    pub const fn new(
        generation: PaneFocusObservationGeneration,
        binding: ViewportBinding,
        focus: PanelFocus,
    ) -> Self {
        Self {
            generation,
            binding,
            focus,
            acknowledges: None,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub const fn acknowledging(mut self, intent: PaneFocusIntentId) -> Self {
        self.acknowledges = Some(intent);
        self
    }
}

/// Why one pane-focus observation was rejected without changing focus history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusObservationRejection {
    StaleBinding,
    StaleHostScope,
    ItemUnavailable { item: ItemId },
    UnknownIntent { intent: PaneFocusIntentId },
    IntentNotPublished { intent: PaneFocusIntentId },
    IntentBindingMismatch,
    IntentFocusMismatch,
    IntentTargetMismatch,
    ObservationPrecedesIntent,
}

/// Deterministic acceptance result for one pane-focus observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocusObservationTransition {
    Applied {
        cleared_intent: Option<PaneFocusIntentId>,
    },
    Duplicate,
    Stale {
        current: PaneFocusObservationGeneration,
    },
    EqualGenerationConflict {
        generation: PaneFocusObservationGeneration,
    },
    Focused {
        cleared_intent: PaneFocusIntentId,
    },
    NotFocused {
        intent: PaneFocusIntentId,
    },
    Unavailable {
        intent: PaneFocusIntentId,
    },
    Rejected(PaneFocusObservationRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanelFocusObservationRecord {
    observation: PaneFocusObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneFocusRequestObservationRecord {
    observation: PaneFocusRequestObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneIntentWatermark {
    causal: FocusCausalStamp,
    source: PaneFocusIntentSource,
    surface: SurfaceId,
    item: Option<ItemId>,
    native_guard: Option<ViewportBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneCausalClaim {
    causal: FocusCausalStamp,
    source: PaneFocusIntentSource,
    target: FocusClaimTarget,
    pane: PaneFocusDisposition,
    cause: ViewportActivationCause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeActivationReservation {
    causal: FocusCausalStamp,
    request: ViewportActivationRequest,
}

impl NativeActivationReservation {
    const fn claim(self) -> PaneCausalClaim {
        PaneCausalClaim::from_request(self.causal, self.request)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusClaimTarget {
    Dock(ViewportBinding),
    Window(WindowBinding),
    Surface(SurfaceId),
    Foreign,
    None,
}

impl FocusClaimTarget {
    const fn from_global(focused: GlobalFocusedWindow) -> Self {
        match focused {
            GlobalFocusedWindow::Dock(binding) => Self::Window(binding),
            GlobalFocusedWindow::Foreign => Self::Foreign,
            GlobalFocusedWindow::None => Self::None,
        }
    }

    const fn dock(self) -> Option<ViewportBinding> {
        match self {
            Self::Dock(binding) => Some(binding),
            Self::Window(_) | Self::Surface(_) | Self::Foreign | Self::None => None,
        }
    }

    const fn window(self) -> Option<WindowBinding> {
        match self {
            Self::Dock(binding) => Some(binding.window()),
            Self::Window(window) => Some(window),
            Self::Surface(_) | Self::Foreign | Self::None => None,
        }
    }
}

impl PaneCausalClaim {
    const fn from_request(causal: FocusCausalStamp, request: ViewportActivationRequest) -> Self {
        Self {
            causal,
            source: request.cause.pane_source(),
            target: FocusClaimTarget::Dock(request.target),
            pane: request.pane,
            cause: request.cause,
        }
    }

    fn from_install(install: PaneIntentInstall) -> Self {
        Self {
            causal: install.causal,
            source: install.source,
            target: install.native_guard.map_or(
                FocusClaimTarget::Surface(install.surface),
                FocusClaimTarget::Dock,
            ),
            pane: PaneFocusDisposition::from_focus(install.focus),
            cause: install
                .cause
                .unwrap_or(ViewportActivationCause::PlatformObservation),
        }
    }

    const fn platform_observation(causal: FocusCausalStamp, focused: GlobalFocusedWindow) -> Self {
        Self {
            causal,
            source: PaneFocusIntentSource::PlatformActivation,
            target: FocusClaimTarget::from_global(focused),
            pane: PaneFocusDisposition::Preserve,
            cause: ViewportActivationCause::PlatformObservation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusHazardBarrierPhase {
    AwaitingHazards,
    AwaitingPostHazardAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusHazardBarrier {
    winner: FocusClaimTarget,
    observed_at: FocusObservationGeneration,
    phase: FocusHazardBarrierPhase,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum FocusBarrier {
    #[default]
    None,
    Dock {
        activation: ActivationGeneration,
    },
    Hazard(FocusHazardBarrier),
    ReconcileCurrent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct FocusLane {
    winner: Option<PaneCausalClaim>,
    settled_winner: Option<PaneCausalClaim>,
    native_reservations: BTreeMap<NativeCreateSagaId, NativeActivationReservation>,
    driving: Option<PendingViewportActivation>,
    hazards: BTreeMap<EffectId, PendingViewportActivation>,
    quarantined: BTreeSet<ViewportBinding>,
    barrier: FocusBarrier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneIntentInstall {
    causal: FocusCausalStamp,
    activation: Option<ActivationGeneration>,
    surface: SurfaceId,
    native_guard: Option<ViewportBinding>,
    focus: PanelFocus,
    source: PaneFocusIntentSource,
    cause: Option<ViewportActivationCause>,
    focus_observation_baseline: FocusObservationGeneration,
}

/// Cleanup summary used when topology, binding, or item authority changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportFocusCleanup {
    global_authority_removed: bool,
    current_focus_changed: bool,
    panel_record_removed: bool,
    pane_observation_removed: bool,
    pending_activation_cancelled: Option<(ActivationGeneration, ActivationCancellation)>,
    pending_intent_removed: Option<PaneFocusIntentId>,
    observe_only_activation_removed: Option<ActivationGeneration>,
}

impl ViewportFocusCleanup {
    #[must_use]
    #[cfg(test)]
    pub const fn changed(self) -> bool {
        self.global_authority_removed
            || self.current_focus_changed
            || self.panel_record_removed
            || self.pane_observation_removed
            || self.pending_activation_cancelled.is_some()
            || self.pending_intent_removed.is_some()
            || self.observe_only_activation_removed.is_some()
    }

    #[must_use]
    #[cfg(test)]
    pub const fn global_authority_removed(self) -> bool {
        self.global_authority_removed
    }

    #[must_use]
    #[cfg(test)]
    pub const fn pending_activation_cancelled(
        self,
    ) -> Option<(ActivationGeneration, ActivationCancellation)> {
        self.pending_activation_cancelled
    }

    #[must_use]
    #[cfg(test)]
    pub const fn pending_intent_removed(self) -> Option<PaneFocusIntentId> {
        self.pending_intent_removed
    }

    #[must_use]
    #[cfg(test)]
    pub const fn observe_only_activation_removed(self) -> Option<ActivationGeneration> {
        self.observe_only_activation_removed
    }
}

/// Before-and-after values for one adapter-observable focus slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusValueChange<T> {
    before: Option<T>,
    after: Option<T>,
}

impl<T> FocusValueChange<T> {
    #[must_use]
    #[cfg(test)]
    pub const fn before(&self) -> &Option<T> {
        &self.before
    }

    #[must_use]
    #[cfg(test)]
    pub const fn after(&self) -> &Option<T> {
        &self.after
    }
}

impl<T> FocusValueChange<T>
where
    T: PartialEq,
{
    fn between(before: Option<T>, after: Option<T>) -> Option<Self> {
        (before != after).then_some(Self { before, after })
    }
}

/// Published pane-focus history and exact adapter observation for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceFocusState {
    panel: PanelFocusRecord,
    observation: Option<PaneFocusObservation>,
}

impl SurfaceFocusState {
    #[must_use]
    #[cfg(test)]
    pub const fn observation(self) -> Option<PaneFocusObservation> {
        self.observation
    }
}

/// Net pane-focus state change for one stable logical surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceFocusChange {
    surface: SurfaceId,
    state: FocusValueChange<SurfaceFocusState>,
}

impl SurfaceFocusChange {
    #[must_use]
    #[cfg(test)]
    pub const fn state(&self) -> &FocusValueChange<SurfaceFocusState> {
        &self.state
    }
}

/// Net phase change for one exact global `RequestFocus` effect.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusEffectChange {
    request: EffectRequest,
    phase: FocusValueChange<EffectPhase>,
    observed: Option<ObservedPlatformFocusEffect>,
}

impl FocusEffectChange {
    #[must_use]
    #[cfg(test)]
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    #[must_use]
    #[cfg(test)]
    pub const fn phase(&self) -> &FocusValueChange<EffectPhase> {
        &self.phase
    }

    #[must_use]
    #[cfg(test)]
    pub const fn observed(&self) -> Option<ObservedPlatformFocusEffect> {
        self.observed
    }
}

/// Adapter-facing net focus publication for one atomic engine boundary.
///
/// Reducer-internal transient intents are deliberately omitted. An adapter sees
/// only the final state it may act on after the whole boundary commits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusDelta {
    global_observation: Option<FocusValueChange<FocusObservationEnvelope>>,
    current_focus: Option<FocusValueChange<CurrentUiFocus>>,
    pending_activation: Option<FocusValueChange<PendingViewportActivation>>,
    observe_only_activation: Option<FocusValueChange<RecordedObserveOnlyActivation>>,
    pane_intent: Option<FocusValueChange<PaneFocusIntent>>,
    surface_focus: Vec<SurfaceFocusChange>,
    effects: Vec<FocusEffectChange>,
}

impl FocusDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.global_observation.is_none()
            && self.current_focus.is_none()
            && self.pending_activation.is_none()
            && self.observe_only_activation.is_none()
            && self.pane_intent.is_none()
            && self.surface_focus.is_empty()
            && self.effects.is_empty()
    }

    #[must_use]
    #[cfg(test)]
    pub const fn global_observation(&self) -> Option<&FocusValueChange<FocusObservationEnvelope>> {
        self.global_observation.as_ref()
    }

    #[must_use]
    #[cfg(test)]
    pub const fn observe_only_activation(
        &self,
    ) -> Option<&FocusValueChange<RecordedObserveOnlyActivation>> {
        self.observe_only_activation.as_ref()
    }

    #[must_use]
    #[cfg(test)]
    pub const fn pane_intent(&self) -> Option<&FocusValueChange<PaneFocusIntent>> {
        self.pane_intent.as_ref()
    }

    #[must_use]
    #[cfg(test)]
    pub fn surface_focus(&self) -> &[SurfaceFocusChange] {
        &self.surface_focus
    }

    #[must_use]
    #[cfg(test)]
    pub fn effects(&self) -> &[FocusEffectChange] {
        &self.effects
    }

    pub(crate) fn between(
        before: &ViewportFocusCoordinator,
        after: &ViewportFocusCoordinator,
        before_effects: &EffectLedger,
        after_effects: &EffectLedger,
        observed_effects: &[ObservedPlatformFocusEffect],
    ) -> Self {
        let mut delta = Self {
            global_observation: FocusValueChange::between(
                before.focus_observations.current(),
                after.focus_observations.current(),
            ),
            current_focus: FocusValueChange::between(before.current_focus(), after.current_focus()),
            pending_activation: FocusValueChange::between(
                before.focus_lane.driving,
                after.focus_lane.driving,
            ),
            observe_only_activation: FocusValueChange::between(
                before.recorded_observe_only_activation,
                after.recorded_observe_only_activation,
            ),
            pane_intent: FocusValueChange::between(
                before
                    .pending_pane_intent
                    .filter(|intent| !intent.awaiting_native_binding()),
                after
                    .pending_pane_intent
                    .filter(|intent| !intent.awaiting_native_binding()),
            ),
            ..Self::default()
        };

        let surfaces = before
            .panel_focus_by_surface
            .keys()
            .chain(before.pane_observation_by_surface.keys())
            .chain(after.panel_focus_by_surface.keys())
            .chain(after.pane_observation_by_surface.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        for surface in surfaces {
            if let Some(state) = FocusValueChange::between(
                before.surface_focus_state(surface),
                after.surface_focus_state(surface),
            ) {
                delta
                    .surface_focus
                    .push(SurfaceFocusChange { surface, state });
            }
        }

        let observed = observed_effects
            .iter()
            .copied()
            .map(|effect| (effect.effect(), effect))
            .collect::<BTreeMap<_, _>>();
        let focus_effect_ids = before_effects
            .records()
            .chain(after_effects.records())
            .filter_map(|(id, record)| {
                matches!(
                    record.request().effect(),
                    PlatformEffect::RequestFocus { .. }
                )
                .then_some(id)
            })
            .collect::<BTreeSet<_>>();
        for effect in focus_effect_ids {
            let before_record = before_effects.record(effect);
            let after_record = after_effects.record(effect);
            let before_phase = before_record.map(crate::platform::effect::EffectRecord::phase);
            let after_phase = after_record.map(crate::platform::effect::EffectRecord::phase);
            let observed = observed.get(&effect).copied();
            let Some(phase) = FocusValueChange::between(before_phase, after_phase).or_else(|| {
                observed.map(|_| FocusValueChange {
                    before: before_phase,
                    after: after_phase,
                })
            }) else {
                continue;
            };
            let request = after_record
                .or(before_record)
                .expect("a focus effect identity came from at least one ledger")
                .request()
                .clone();
            delta.effects.push(FocusEffectChange {
                request,
                phase,
                observed,
            });
        }
        delta
    }
}

/// Core-owned activation and pane-focus state shared by all UI adapters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewportFocusCoordinator {
    focus_observations: FocusObservationStream,
    last_activation: ActivationGeneration,
    last_pane_intent: PaneFocusIntentId,
    focus_lane: FocusLane,
    pane_intent_watermark: Option<PaneIntentWatermark>,
    recorded_observe_only_activation: Option<RecordedObserveOnlyActivation>,
    pending_pane_intent: Option<PaneFocusIntent>,
    admitted_observe_only_activation: Option<ActivationGeneration>,
    admitted_pane_intent: Option<PaneFocusIntentId>,
    current_ui_focus: Option<CurrentUiFocusRecord>,
    panel_focus_by_surface: BTreeMap<SurfaceId, PanelFocusRecord>,
    host_focus_observation_by_surface: BTreeMap<SurfaceId, HostFocusObservation>,
    pane_observation_by_surface: BTreeMap<SurfaceId, PanelFocusObservationRecord>,
    pane_request_observation_by_surface: BTreeMap<SurfaceId, PaneFocusRequestObservationRecord>,
}

impl ViewportFocusCoordinator {
    pub(crate) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for observation in [
            self.focus_observations.current,
            self.focus_observations.last_envelope,
        ]
        .into_iter()
        .flatten()
        {
            if let Authority::Known(Some(effect)) = *observation.acknowledged_effect() {
                effects.insert(effect);
            }
        }
        if let Some(effect) = self
            .focus_lane
            .driving
            .and_then(|activation| activation.platform_focus().effect())
        {
            effects.insert(effect);
        }
        effects.extend(self.focus_lane.hazards.keys().copied());
        effects.extend(
            self.focus_lane
                .hazards
                .values()
                .filter_map(|activation| activation.platform_focus().effect()),
        );
    }

    /// Marks the focus obligations exposed by one committed engine boundary.
    ///
    /// A later lifecycle change may stop accepting new activation while the
    /// exact obligations already exposed to an adapter still need to settle.
    #[doc(hidden)]
    pub fn mark_boundary_published(&mut self) {
        self.admitted_observe_only_activation = self
            .recorded_observe_only_activation
            .map(|record| record.generation);
        self.admitted_pane_intent = self
            .pending_pane_intent
            .filter(|intent| !intent.awaiting_native_binding())
            .map(|intent| intent.id);
    }

    fn clear_admitted_observe_only(&mut self, generation: ActivationGeneration) {
        if self.admitted_observe_only_activation == Some(generation) {
            self.admitted_observe_only_activation = None;
        }
    }

    fn clear_admitted_pane_intent(&mut self, intent: PaneFocusIntentId) {
        if self.admitted_pane_intent == Some(intent) {
            self.admitted_pane_intent = None;
        }
    }

    #[must_use]
    pub const fn focus_observation(&self) -> Option<FocusObservationEnvelope> {
        self.focus_observations.current()
    }

    /// Returns ordering evidence without treating an unknown focus owner as authority.
    pub(crate) const fn activation_observation_baseline(
        &self,
    ) -> Option<FocusObservationGeneration> {
        if self.focus_observations.requires_tombstone {
            None
        } else {
            self.focus_observations.last_generation
        }
    }

    #[must_use]
    pub const fn pending_activation(&self) -> Option<PendingViewportActivation> {
        self.focus_lane.driving
    }

    #[must_use]
    #[cfg(test)]
    pub const fn recorded_observe_only_activation(&self) -> Option<RecordedObserveOnlyActivation> {
        self.recorded_observe_only_activation
    }

    #[must_use]
    pub const fn pending_pane_intent(&self) -> Option<PaneFocusIntent> {
        self.pending_pane_intent
    }

    /// Returns only the exact pane-focus request admitted at a committed boundary.
    #[must_use]
    pub fn published_pane_intent(&self) -> Option<PaneFocusIntent> {
        match self.pending_pane_intent {
            Some(intent) if self.admitted_pane_intent == Some(intent.id) => Some(intent),
            Some(_) | None => None,
        }
    }

    #[must_use]
    pub fn panel_focus(&self, surface: SurfaceId) -> PanelFocusRecord {
        self.panel_focus_by_surface
            .get(&surface)
            .copied()
            .unwrap_or_default()
    }

    /// Returns actual observed focus; `None` means that no fact was established.
    #[must_use]
    pub const fn current_focus(&self) -> Option<CurrentUiFocus> {
        match self.current_ui_focus {
            Some(record) => Some(record.focus),
            None => None,
        }
    }

    pub(crate) fn matches_current_host_focus(&self, observation: HostFocusObservation) -> bool {
        self.current_ui_focus.is_some_and(|record| {
            record.focus == CurrentUiFocus::from_host(observation.surface, observation.state)
                && record
                    .scope
                    .is_some_and(|(surface, _)| surface == observation.surface)
                && record.host_scope == Some((observation.host, observation.stream))
        })
    }

    /// Returns the current pane on this surface, without consulting its history.
    #[must_use]
    pub const fn current_pane_focus(&self, surface: SurfaceId) -> Option<ItemId> {
        match self.current_focus() {
            Some(CurrentUiFocus::Pane {
                surface: current,
                item,
            }) if current.get() == surface.get() => Some(item),
            _ => None,
        }
    }

    fn surface_focus_state(&self, surface: SurfaceId) -> Option<SurfaceFocusState> {
        let panel = self.panel_focus_by_surface.get(&surface).copied();
        let observation = self
            .pane_observation_by_surface
            .get(&surface)
            .map(|record| record.observation);
        (panel.is_some() || observation.is_some()).then_some(SurfaceFocusState {
            panel: panel.unwrap_or_default(),
            observation,
        })
    }

    #[cfg(test)]
    pub(crate) fn exhaust_activation_generations(&mut self) {
        self.last_activation = ActivationGeneration::new(u64::MAX);
    }

    #[cfg(test)]
    pub(crate) fn exhaust_pane_intent_ids(&mut self) {
        self.last_pane_intent = PaneFocusIntentId::new(u64::MAX);
    }
}

/// Non-wrapping identity allocation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ViewportFocusError {
    #[error("focus reducer generation is exhausted")]
    ReducerGenerationExhausted,
    #[error("viewport activation generation is exhausted")]
    ActivationGenerationExhausted,
    #[error("pane-focus intent identity is exhausted")]
    PaneFocusIntentIdExhausted,
    #[error("a viewport focus effect could not attach to its activation")]
    EffectAttachmentInvariant,
    #[error("a rejected pane reveal did not own the pending pane intent")]
    PaneRevealInvariant,
    #[error("native create saga {owner:?} does not own the requested focus reservation")]
    ActivationReservationMismatch { owner: NativeCreateSagaId },
}

/// Convenience constructor for a complete unknown focus observation.
#[must_use]
#[cfg(test)]
pub const fn unknown_focus_observation(
    generation: FocusObservationGeneration,
    reason: AuthorityUnavailableReason,
) -> FocusObservationEnvelope {
    FocusObservationEnvelope::new(
        generation,
        Authority::Unknown(reason),
        Authority::Unknown(reason),
    )
}

#[cfg(test)]
mod tests;
