//! Adapter-neutral native viewport activation and pane-focus coordination.
//!
//! Platform focus and pane focus are separate facts. A selected tab never proves
//! that its pane owns UI focus, and requesting native-window activation never
//! proves that the request was applied. The coordinator therefore keeps exact
//! binding identities, provider generations, effect identities, and pane-focus
//! acknowledgements until their corresponding facts arrive.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::close_plan::CloseRequestId;
use crate::effect::{
    EffectDispatchResult, EffectId, EffectLedger, EffectPhase, EffectRequest, PlatformEffect,
};
use crate::event::ReductionCause;
use crate::frame::PanelFocus;
use crate::ids::{ItemId, NativeCreateSagaId, SurfaceId};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::viewport::ViewportBinding;

macro_rules! focus_generation {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a generation from its protocol representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the protocol representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
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
    /// One exact live docking window binding is focused.
    Dock(ViewportBinding),
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

impl FocusObservationStream {
    fn observe(
        &mut self,
        observation: Option<FocusObservationEnvelope>,
    ) -> FocusObservationStreamTransition {
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return FocusObservationStreamTransition::AuthorityRevoked {
                generation: None,
                reason: FocusAuthorityRevocation::ObservationMissing,
            };
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => {
                return FocusObservationStreamTransition::Stale {
                    current: generation,
                };
            }
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope != Some(observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                    return FocusObservationStreamTransition::AuthorityRevoked {
                        generation: Some(observation.generation()),
                        reason: FocusAuthorityRevocation::EqualGenerationConflict,
                    };
                }
                return FocusObservationStreamTransition::Duplicate;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return FocusObservationStreamTransition::AuthorityRevoked {
                    generation: Some(observation.generation()),
                    reason: FocusAuthorityRevocation::GenerationGap {
                        previous: generation,
                    },
                };
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        if self.requires_tombstone && matches!(observation.focused(), Authority::Known(_)) {
            self.current = None;
            return FocusObservationStreamTransition::TombstoneRequired {
                generation: observation.generation(),
            };
        }
        if let Authority::Unknown(reason) = *observation.focused() {
            self.current = None;
            self.requires_tombstone = false;
            return FocusObservationStreamTransition::AuthorityRevoked {
                generation: Some(observation.generation()),
                reason: FocusAuthorityRevocation::Unknown(reason),
            };
        }
        self.current = Some(observation);
        self.requires_tombstone = false;
        FocusObservationStreamTransition::Current
    }

    #[cfg(test)]
    fn force_current_for_test(&mut self, observation: FocusObservationEnvelope) {
        let transition = self.observe(Some(observation));
        assert!(
            matches!(transition, FocusObservationStreamTransition::Current),
            "a test baseline must become current"
        );
    }

    pub(crate) const fn current(self) -> Option<FocusObservationEnvelope> {
        self.current
    }

    #[cfg(test)]
    pub(crate) const fn generation_watermark(self) -> Option<FocusObservationGeneration> {
        self.last_generation
    }

    /// Starts a fresh observation namespace for a replacement provider.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }
}

/// The last actually observed pane-focus state for one logical surface.
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

    #[must_use]
    pub const fn focus(self) -> Option<PanelFocus> {
        match self {
            Self::NoHistory => None,
            Self::Item(item) => Some(PanelFocus::Item(item)),
            Self::None => Some(PanelFocus::None),
        }
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
    const fn close_request(self) -> Option<CloseRequestId> {
        match self {
            Self::CloseRecovery { request } => Some(request),
            Self::Explicit
            | Self::DropCommitted
            | Self::TearOffCommitted
            | Self::PointerTabGesture
            | Self::RecoveryReplacement
            | Self::PlatformObservation => None,
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
}

impl PaneFocusIntentSource {
    #[must_use]
    const fn priority(self) -> u8 {
        match self {
            Self::PlatformActivation => 0,
            Self::ExplicitViewportActivation => 1,
            Self::PointerTabGesture => 2,
            Self::CloseRecovery => 3,
        }
    }
}

/// Core-derived gate for ordinary pane restoration after native focus changes.
///
/// Explicit viewport activations bypass this gate because they carry their own
/// exact pane-focus request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformFocusRestoreGate {
    /// A complete global provider roster proves no button remains pressed.
    KnownAllReleased,
    /// At least one exact provider pointer/button pair remains pressed.
    KnownDown,
    /// Global button authority is incomplete or unavailable.
    Unknown,
}

impl PlatformFocusRestoreGate {
    const fn allows_ordinary_restore(self) -> bool {
        matches!(self, Self::KnownAllReleased)
    }
}

/// One exact pane-focus command retained until an adapter observation confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFocusIntent {
    id: PaneFocusIntentId,
    causal: FocusCausalStamp,
    activation: Option<ActivationGeneration>,
    target: ViewportBinding,
    focus: PanelFocus,
    source: PaneFocusIntentSource,
    cause: Option<ViewportActivationCause>,
    focus_observation_baseline: FocusObservationGeneration,
    pane_observation_baseline: Option<PaneFocusObservationGeneration>,
}

impl PaneFocusIntent {
    #[must_use]
    pub const fn id(self) -> PaneFocusIntentId {
        self.id
    }

    #[must_use]
    pub const fn generation(self) -> PaneFocusIntentGeneration {
        self.causal.generation()
    }

    #[must_use]
    pub const fn causal(self) -> FocusCausalStamp {
        self.causal
    }

    #[must_use]
    pub const fn activation(self) -> Option<ActivationGeneration> {
        self.activation
    }

    #[must_use]
    pub const fn target(self) -> ViewportBinding {
        self.target
    }

    #[must_use]
    pub const fn focus(self) -> PanelFocus {
        self.focus
    }

    #[must_use]
    pub const fn source(self) -> PaneFocusIntentSource {
        self.source
    }

    #[must_use]
    pub const fn cause(self) -> Option<ViewportActivationCause> {
        self.cause
    }

    #[must_use]
    pub const fn focus_observation_baseline(self) -> FocusObservationGeneration {
        self.focus_observation_baseline
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
        reason: crate::effect::EffectIndeterminateReason,
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
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn causal(self) -> FocusCausalStamp {
        self.causal
    }

    #[must_use]
    pub const fn request(self) -> ViewportActivationRequest {
        self.request
    }

    #[must_use]
    pub const fn observation_baseline(self) -> Option<FocusObservationGeneration> {
        self.observation_baseline
    }
}

impl PendingViewportActivation {
    #[must_use]
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn causal(self) -> FocusCausalStamp {
        self.causal
    }

    #[must_use]
    pub const fn request(self) -> ViewportActivationRequest {
        self.request
    }

    #[must_use]
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
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn outcome(self) -> ActivationStartOutcome {
        self.outcome
    }

    /// Returns the older explicit activation whose effect obligation remains in the causal lane.
    #[must_use]
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
    pub const fn generation(self) -> FocusObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn evidence(self) -> PlatformFocusEvidence {
        self.evidence
    }
}

/// Why a pending activation was discarded while applying focus facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivationCancellation {
    SupersededByActivation {
        replacement: ActivationGeneration,
    },
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
    CloseRequestCleared {
        request: CloseRequestId,
    },
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

    #[must_use]
    pub const fn pane_reveal_rejection(
        &self,
    ) -> Option<(PaneFocusIntent, PaneFocusRevealRejection)> {
        match self.pane {
            AppliedPaneFocus::RevealRejected { intent, reason } => Some((intent, reason)),
            AppliedPaneFocus::None | AppliedPaneFocus::Intent(_) => None,
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

impl PaneFocusObservation {
    #[must_use]
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
    pub const fn acknowledging(mut self, intent: PaneFocusIntentId) -> Self {
        self.acknowledges = Some(intent);
        self
    }

    #[must_use]
    pub const fn generation(self) -> PaneFocusObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn focus(self) -> PanelFocus {
        self.focus
    }

    #[must_use]
    pub const fn acknowledges(self) -> Option<PaneFocusIntentId> {
        self.acknowledges
    }
}

/// Why one pane-focus observation was rejected without changing focus history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusObservationRejection {
    StaleBinding,
    ItemUnavailable { item: ItemId },
    UnknownIntent { intent: PaneFocusIntentId },
    IntentNotPublished { intent: PaneFocusIntentId },
    IntentBindingMismatch,
    IntentFocusMismatch,
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
    Rejected(PaneFocusObservationRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PanelFocusObservationRecord {
    observation: PaneFocusObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneIntentWatermark {
    causal: FocusCausalStamp,
    source: PaneFocusIntentSource,
    target: ViewportBinding,
    focus: PanelFocus,
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
    Foreign,
    None,
}

impl FocusClaimTarget {
    const fn from_global(focused: GlobalFocusedWindow) -> Self {
        match focused {
            GlobalFocusedWindow::Dock(binding) => Self::Dock(binding),
            GlobalFocusedWindow::Foreign => Self::Foreign,
            GlobalFocusedWindow::None => Self::None,
        }
    }

    const fn dock(self) -> Option<ViewportBinding> {
        match self {
            Self::Dock(binding) => Some(binding),
            Self::Foreign | Self::None => None,
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
            target: FocusClaimTarget::Dock(install.target),
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
    target: ViewportBinding,
    focus: PanelFocus,
    source: PaneFocusIntentSource,
    cause: Option<ViewportActivationCause>,
    focus_observation_baseline: FocusObservationGeneration,
}

/// Cleanup summary used when topology, binding, or item authority changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportFocusCleanup {
    global_authority_removed: bool,
    panel_record_removed: bool,
    pane_observation_removed: bool,
    pending_activation_cancelled: Option<(ActivationGeneration, ActivationCancellation)>,
    pending_intent_removed: Option<PaneFocusIntentId>,
    observe_only_activation_removed: Option<ActivationGeneration>,
}

impl ViewportFocusCleanup {
    #[must_use]
    pub const fn changed(self) -> bool {
        self.global_authority_removed
            || self.panel_record_removed
            || self.pane_observation_removed
            || self.pending_activation_cancelled.is_some()
            || self.pending_intent_removed.is_some()
            || self.observe_only_activation_removed.is_some()
    }

    #[must_use]
    pub const fn global_authority_removed(self) -> bool {
        self.global_authority_removed
    }

    #[must_use]
    pub const fn panel_record_removed(self) -> bool {
        self.panel_record_removed
    }

    #[must_use]
    pub const fn pending_activation_cancelled(
        self,
    ) -> Option<(ActivationGeneration, ActivationCancellation)> {
        self.pending_activation_cancelled
    }

    #[must_use]
    pub const fn pending_intent_removed(self) -> Option<PaneFocusIntentId> {
        self.pending_intent_removed
    }

    #[must_use]
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
    pub const fn before(&self) -> &Option<T> {
        &self.before
    }

    #[must_use]
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
    pub const fn panel(self) -> PanelFocusRecord {
        self.panel
    }

    #[must_use]
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
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    #[must_use]
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
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    #[must_use]
    pub const fn phase(&self) -> &FocusValueChange<EffectPhase> {
        &self.phase
    }

    #[must_use]
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
            && self.pending_activation.is_none()
            && self.observe_only_activation.is_none()
            && self.pane_intent.is_none()
            && self.surface_focus.is_empty()
            && self.effects.is_empty()
    }

    #[must_use]
    pub const fn global_observation(&self) -> Option<&FocusValueChange<FocusObservationEnvelope>> {
        self.global_observation.as_ref()
    }

    #[must_use]
    pub const fn pending_activation(&self) -> Option<&FocusValueChange<PendingViewportActivation>> {
        self.pending_activation.as_ref()
    }

    #[must_use]
    pub const fn observe_only_activation(
        &self,
    ) -> Option<&FocusValueChange<RecordedObserveOnlyActivation>> {
        self.observe_only_activation.as_ref()
    }

    #[must_use]
    pub const fn pane_intent(&self) -> Option<&FocusValueChange<PaneFocusIntent>> {
        self.pane_intent.as_ref()
    }

    #[must_use]
    pub fn surface_focus(&self) -> &[SurfaceFocusChange] {
        &self.surface_focus
    }

    #[must_use]
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
            pending_activation: FocusValueChange::between(
                before.focus_lane.driving,
                after.focus_lane.driving,
            ),
            observe_only_activation: FocusValueChange::between(
                before.recorded_observe_only_activation,
                after.recorded_observe_only_activation,
            ),
            pane_intent: FocusValueChange::between(
                before.pending_pane_intent,
                after.pending_pane_intent,
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
            let before_phase = before_record.map(crate::effect::EffectRecord::phase);
            let after_phase = after_record.map(crate::effect::EffectRecord::phase);
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
    destroyed_previous_focus: Option<ViewportBinding>,
    last_activation: ActivationGeneration,
    last_pane_intent: PaneFocusIntentId,
    focus_lane: FocusLane,
    pane_intent_watermark: Option<PaneIntentWatermark>,
    recorded_observe_only_activation: Option<RecordedObserveOnlyActivation>,
    pending_pane_intent: Option<PaneFocusIntent>,
    admitted_observe_only_activation: Option<ActivationGeneration>,
    admitted_pane_intent: Option<PaneFocusIntentId>,
    panel_focus_by_surface: BTreeMap<SurfaceId, PanelFocusRecord>,
    pane_observation_by_surface: BTreeMap<SurfaceId, PanelFocusObservationRecord>,
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

    fn focus_matches_reserved_claim(&self, binding: ViewportBinding) -> bool {
        self.focus_lane.winner.is_some_and(|claim| {
            claim.target == FocusClaimTarget::Dock(binding)
                && claim.cause == ViewportActivationCause::TearOffCommitted
        })
    }

    #[cfg(test)]
    pub(crate) fn suppressed_tear_off_bindings(&self) -> BTreeSet<ViewportBinding> {
        self.focus_lane.quarantined.clone()
    }

    fn complete_lifecycle_focus_barrier(&mut self, generation: ActivationGeneration) {
        if matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == generation
        ) {
            self.focus_lane.barrier = FocusBarrier::None;
            self.focus_lane.quarantined.clear();
            self.focus_lane.hazards.clear();
        }
    }

    fn cancel_lifecycle_focus_barrier(&mut self, generation: ActivationGeneration) {
        if matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == generation
        ) {
            self.focus_lane.barrier = FocusBarrier::ReconcileCurrent;
            self.focus_lane.quarantined.clear();
            self.focus_lane.hazards.clear();
        }
    }

    fn pane_claim_order(claim: PaneCausalClaim, floor: PaneCausalClaim) -> Ordering {
        claim
            .causal
            .generation()
            .cmp(&floor.causal.generation())
            .then_with(|| claim.source.priority().cmp(&floor.source.priority()))
    }

    fn pending_intent_claim(intent: PaneFocusIntent) -> PaneCausalClaim {
        PaneCausalClaim {
            causal: intent.causal,
            source: intent.source,
            target: FocusClaimTarget::Dock(intent.target),
            pane: PaneFocusDisposition::from_focus(intent.focus),
            cause: intent
                .cause
                .unwrap_or(ViewportActivationCause::PlatformObservation),
        }
    }

    fn publish_focus_winner(&mut self, winner: Option<PaneCausalClaim>) {
        if self.focus_lane.winner == winner {
            return;
        }
        self.focus_lane.winner = winner;
        if let (Some(winner), Some(intent)) = (winner, self.pending_pane_intent)
            && Self::pane_claim_order(winner, Self::pending_intent_claim(intent))
                == Ordering::Greater
        {
            self.pending_pane_intent = None;
            self.clear_admitted_pane_intent(intent.id);
        }
    }

    fn recompute_focus_winner(&mut self) {
        let winner = self
            .focus_lane
            .native_reservations
            .values()
            .map(|reservation| reservation.claim())
            .fold(self.focus_lane.settled_winner, |winner, claim| {
                winner.map_or(Some(claim), |current| {
                    if Self::pane_claim_order(claim, current) == Ordering::Greater {
                        Some(claim)
                    } else {
                        Some(current)
                    }
                })
            });
        self.publish_focus_winner(winner);
    }

    fn admit_pane_causal_claim(&mut self, claim: PaneCausalClaim) -> bool {
        if let Some(floor) = self.focus_lane.settled_winner {
            match Self::pane_claim_order(claim, floor) {
                Ordering::Less => return false,
                Ordering::Equal if claim != floor => return false,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        self.focus_lane.settled_winner = Some(claim);
        self.recompute_focus_winner();
        self.focus_lane.winner == Some(claim)
    }

    fn reserve_native_activation(
        &mut self,
        owner: NativeCreateSagaId,
        reservation: NativeActivationReservation,
    ) -> bool {
        if let Some(existing) = self.focus_lane.native_reservations.get(&owner) {
            return *existing == reservation;
        }
        let claim = reservation.claim();
        if self
            .focus_lane
            .native_reservations
            .values()
            .any(|existing| existing.claim() == claim)
        {
            return false;
        }
        if let Some(floor) = self.focus_lane.winner {
            match Self::pane_claim_order(claim, floor) {
                Ordering::Less => return false,
                Ordering::Equal if claim != floor => return false,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        self.focus_lane
            .native_reservations
            .insert(owner, reservation);
        self.recompute_focus_winner();
        true
    }

    fn retarget_lifecycle_focus_barrier(
        &mut self,
        superseded: Option<PendingViewportActivation>,
        replacement: ActivationGeneration,
    ) {
        let Some(superseded) = superseded else {
            return;
        };
        let inherited_barrier = matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == superseded.generation
        );
        let hazardous = if let Some(effect) = superseded.platform_focus.effect() {
            self.focus_lane.hazards.insert(effect, superseded);
            self.focus_lane
                .quarantined
                .insert(superseded.request.target);
            true
        } else {
            false
        };
        if !inherited_barrier && !hazardous {
            return;
        }
        self.focus_lane.barrier = FocusBarrier::Dock {
            activation: replacement,
        };
    }

    /// Reserves a semantic pane-focus claim before an asynchronous viewport can exist.
    ///
    /// A later completion may replay the exact claim. An older completion can
    /// still finish its topology transaction, but cannot regain focus authority.
    pub(crate) fn reserve_activation_causal(
        &mut self,
        owner: NativeCreateSagaId,
        causal: FocusCausalStamp,
        request: ViewportActivationRequest,
    ) -> bool {
        self.reserve_native_activation(owner, NativeActivationReservation { causal, request })
    }

    pub(crate) fn cancel_activation_reservation(&mut self, owner: NativeCreateSagaId) -> bool {
        let removed = self.focus_lane.native_reservations.remove(&owner).is_some();
        if removed {
            self.recompute_focus_winner();
        }
        removed
    }

    pub(crate) fn winning_activation_reservation(
        &self,
    ) -> Option<(
        NativeCreateSagaId,
        FocusCausalStamp,
        ViewportActivationRequest,
    )> {
        let winner = self.focus_lane.winner?;
        self.focus_lane
            .native_reservations
            .iter()
            .find_map(|(owner, reservation)| {
                (reservation.claim() == winner).then_some((
                    *owner,
                    reservation.causal,
                    reservation.request,
                ))
            })
    }

    pub(crate) fn activation_reservations(
        &self,
    ) -> Vec<(
        NativeCreateSagaId,
        FocusCausalStamp,
        ViewportActivationRequest,
    )> {
        let mut reservations = self
            .focus_lane
            .native_reservations
            .iter()
            .map(|(owner, reservation)| (*owner, reservation.causal, reservation.request))
            .collect::<Vec<_>>();
        reservations.sort_unstable_by(|left, right| {
            Self::pane_claim_order(
                PaneCausalClaim::from_request(right.1, right.2),
                PaneCausalClaim::from_request(left.1, left.2),
            )
            .then_with(|| right.0.cmp(&left.0))
        });
        reservations
    }

    pub(crate) fn lifecycle_barrier_protects(
        &self,
        stale_target: ViewportBinding,
        winner: ViewportBinding,
    ) -> bool {
        self.focus_lane.quarantined.contains(&stale_target)
            && self.focus_lane.driving.is_some_and(|pending| {
                pending.request.target == winner
                    && matches!(
                        self.focus_lane.barrier,
                        FocusBarrier::Dock { activation }
                            if activation == pending.generation
                    )
            })
    }

    pub(crate) fn request_winning_lifecycle_compensation(
        &mut self,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<Option<ActivationStart>, ViewportFocusError> {
        let Some(claim) = self.focus_lane.winner else {
            return Ok(None);
        };
        let Some(target) = claim.target.dock() else {
            return Ok(None);
        };
        let request = ViewportActivationRequest::with_disposition(target, claim.pane, claim.cause);
        let mut candidate = self.clone();
        let inherits_lifecycle_barrier =
            !matches!(candidate.focus_lane.barrier, FocusBarrier::None);
        let generation = candidate
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        candidate.last_activation = generation;
        if let Some(suppression) =
            Self::validate_focus_target(target, claim.pane, is_current_binding, item_is_on_surface)
        {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            let activation = ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(suppression),
            };
            *self = candidate;
            return Ok(Some(activation));
        }
        let Some(observation_baseline) = candidate
            .focus_observations
            .current()
            .map(|observation| observation.generation())
        else {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            *self = candidate;
            return Ok(Some(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::FocusObservationBaselineUnavailable,
                ),
            }));
        };
        if !can_request_platform_focus {
            if !inherits_lifecycle_barrier {
                candidate.focus_lane.quarantined.clear();
            }
            *self = candidate;
            return Ok(Some(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            }));
        }
        // This is a post-lifecycle ordering barrier, not a replay of the
        // winner's original gesture. It therefore emits an exact platform
        // focus effect even when the latest observation already names the
        // winner: the effect must be ordered after the superseded window's
        // show/visible proof.
        let superseded = candidate.focus_lane.driving.take();
        if let Some(record) = candidate.recorded_observe_only_activation.take() {
            candidate.clear_admitted_observe_only(record.generation);
        }
        candidate.focus_lane.driving = Some(PendingViewportActivation {
            generation,
            causal: claim.causal,
            request,
            observation_baseline,
            platform_focus: PendingPlatformFocus::EffectRequired,
        });
        candidate.retarget_lifecycle_focus_barrier(superseded, generation);
        if matches!(candidate.focus_lane.barrier, FocusBarrier::None) {
            candidate.focus_lane.barrier = FocusBarrier::Dock {
                activation: generation,
            };
        }
        let activation = ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::RequestPlatformFocus { target },
        };
        *self = candidate;
        Ok(Some(activation))
    }

    pub(crate) fn winning_focus_target(&self) -> Option<ViewportBinding> {
        self.focus_lane.winner.and_then(|claim| claim.target.dock())
    }

    pub(crate) fn superseded_lifecycle_focus_target(&self) -> Option<ViewportBinding> {
        if self.focus_lane.driving.is_some_and(|pending| {
            matches!(
                self.focus_lane.barrier,
                FocusBarrier::Dock { activation } if activation == pending.generation
            )
        }) {
            return None;
        }
        if let Some(observation) = self.focus_observations.current()
            && let Authority::Known(GlobalFocusedWindow::Dock(binding)) = observation.focused
            && self.focus_lane.quarantined.contains(&binding)
        {
            return Some(binding);
        }
        let pending = self.focus_lane.driving?;
        let is_barrier = matches!(
            self.focus_lane.barrier,
            FocusBarrier::Dock { activation } if activation == pending.generation
        );
        if !is_barrier && pending.platform_focus.effect().is_none() {
            return None;
        }
        let winner = self.winning_focus_target()?;
        (pending.request.target != winner).then_some(pending.request.target)
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
        self.admitted_pane_intent = self.pending_pane_intent.map(|intent| intent.id);
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

    #[must_use]
    pub const fn pending_activation(&self) -> Option<PendingViewportActivation> {
        self.focus_lane.driving
    }

    #[must_use]
    pub const fn recorded_observe_only_activation(&self) -> Option<RecordedObserveOnlyActivation> {
        self.recorded_observe_only_activation
    }

    #[must_use]
    pub const fn pending_pane_intent(&self) -> Option<PaneFocusIntent> {
        self.pending_pane_intent
    }

    #[must_use]
    pub fn panel_focus(&self, surface: SurfaceId) -> PanelFocusRecord {
        self.panel_focus_by_surface
            .get(&surface)
            .copied()
            .unwrap_or_default()
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

    /// Records an activation after validating its exact binding and explicit pane focus.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportFocusError::ActivationGenerationExhausted`] when the activation
    /// identity domain can no longer advance, or a pane-intent allocation error when an
    /// already-focused activation becomes immediately actionable.
    pub fn request_activation(
        &mut self,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let mut candidate = self.clone();
        let start = candidate.request_activation_in_place(
            request,
            causal,
            can_request_platform_focus,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(start)
    }

    pub(crate) fn request_reserved_activation(
        &mut self,
        owner: NativeCreateSagaId,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let expected = NativeActivationReservation { causal, request };
        if self.focus_lane.native_reservations.get(&owner) != Some(&expected) {
            return Err(ViewportFocusError::ActivationReservationMismatch { owner });
        }

        let mut candidate = self.clone();
        let removed = candidate.cancel_activation_reservation(owner);
        debug_assert!(removed, "the reservation was validated before cloning");
        let start = candidate.request_activation_in_place(
            request,
            causal,
            can_request_platform_focus,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(start)
    }

    fn request_activation_in_place(
        &mut self,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let generation = self
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        self.last_activation = generation;

        let request_validity = Self::validate_focus_target(
            request.target,
            request.pane,
            is_current_binding,
            item_is_on_surface,
        );
        if let Some(suppression) = request_validity {
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(suppression),
            });
        }
        let observation = self.focus_observations.current();
        if request.cause.requests_platform_focus() {
            let Some(observation) = observation else {
                return Ok(ActivationStart {
                    generation,
                    superseded: None,
                    outcome: ActivationStartOutcome::Suppressed(
                        ActivationSuppression::FocusObservationBaselineUnavailable,
                    ),
                });
            };
            let requires_effect = !Self::observation_focuses(observation, request.target)
                || self.focus_lane.driving.is_some();
            if requires_effect && !can_request_platform_focus {
                return Ok(ActivationStart {
                    generation,
                    superseded: None,
                    outcome: ActivationStartOutcome::Suppressed(
                        ActivationSuppression::PlatformFocusControlUnavailable,
                    ),
                });
            }
        }
        if !self.admit_pane_causal_claim(PaneCausalClaim::from_request(causal, request)) {
            if request.cause == ViewportActivationCause::TearOffCommitted
                && self
                    .focus_lane
                    .winner
                    .and_then(|claim| claim.target.dock())
                    .is_some_and(|winner| winner != request.target)
            {
                self.focus_lane.quarantined.insert(request.target);
            }
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::CausallySuperseded {
                        current: self
                            .focus_lane
                            .winner
                            .map_or_else(PaneFocusIntentGeneration::default, |floor| {
                                floor.causal.generation()
                            }),
                    },
                ),
            });
        }
        let requires_ordered_compensation = self.focus_lane.driving.is_some_and(|pending| {
            matches!(
                self.focus_lane.barrier,
                FocusBarrier::Dock { activation } if activation == pending.generation
            ) || pending.platform_focus.effect().is_some()
        });
        if !request.cause.requests_platform_focus() && !requires_ordered_compensation {
            let superseded = self.focus_lane.driving.take();
            return self.request_observe_only_activation(
                generation,
                request,
                causal,
                observation,
                superseded,
            );
        }
        if requires_ordered_compensation && !can_request_platform_focus {
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            });
        }

        let superseded = self.focus_lane.driving.take();
        self.recorded_observe_only_activation = None;
        let activation = self.request_platform_activation(
            generation,
            request,
            causal,
            observation,
            superseded,
            can_request_platform_focus,
        )?;
        if matches!(
            activation.outcome,
            ActivationStartOutcome::RequestPlatformFocus { .. }
        ) {
            self.retarget_lifecycle_focus_barrier(superseded, generation);
        }
        Ok(activation)
    }

    fn request_observe_only_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        observation: Option<FocusObservationEnvelope>,
        superseded: Option<PendingViewportActivation>,
    ) -> Result<ActivationStart, ViewportFocusError> {
        if request.pane == PaneFocusDisposition::Preserve {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::PaneFocusPreserved {
                    target: request.target,
                },
            });
        }
        if let Some(observation) = observation
            && Self::observation_focuses(observation, request.target)
        {
            self.recorded_observe_only_activation = None;
            let Some(focus) = request.focus() else {
                return Ok(ActivationStart {
                    generation,
                    superseded,
                    outcome: ActivationStartOutcome::PaneFocusPreserved {
                        target: request.target,
                    },
                });
            };
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal,
                activation: Some(generation),
                target: request.target,
                focus,
                source: request.cause.pane_source(),
                cause: Some(request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: Self::pane_intent_outcome(intent),
            });
        }

        let record = RecordedObserveOnlyActivation {
            generation,
            causal,
            request,
            observation_baseline: observation.map(|observation| observation.generation),
        };
        self.recorded_observe_only_activation = Some(record);
        Ok(ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::ObserveOnlyRecorded { record },
        })
    }

    fn request_platform_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        causal: FocusCausalStamp,
        observation: Option<FocusObservationEnvelope>,
        superseded: Option<PendingViewportActivation>,
        can_request_platform_focus: bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let Some(observation) = observation else {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::FocusObservationBaselineUnavailable,
                ),
            });
        };
        if Self::observation_focuses(observation, request.target) && superseded.is_none() {
            let Some(focus) = request.focus() else {
                return Ok(ActivationStart {
                    generation,
                    superseded,
                    outcome: ActivationStartOutcome::PaneFocusPreserved {
                        target: request.target,
                    },
                });
            };
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal,
                activation: Some(generation),
                target: request.target,
                focus,
                source: request.cause.pane_source(),
                cause: Some(request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: Self::pane_intent_outcome(intent),
            });
        }

        if !can_request_platform_focus {
            return Ok(ActivationStart {
                generation,
                superseded,
                outcome: ActivationStartOutcome::Suppressed(
                    ActivationSuppression::PlatformFocusControlUnavailable,
                ),
            });
        }

        self.focus_lane.driving = Some(PendingViewportActivation {
            generation,
            causal,
            request,
            observation_baseline: observation.generation,
            platform_focus: PendingPlatformFocus::EffectRequired,
        });
        Ok(ActivationStart {
            generation,
            superseded,
            outcome: ActivationStartOutcome::RequestPlatformFocus {
                target: request.target,
            },
        })
    }

    fn pane_intent_outcome(intent: Option<PaneFocusIntent>) -> ActivationStartOutcome {
        intent.map_or(ActivationStartOutcome::PaneIntentSuperseded, |intent| {
            ActivationStartOutcome::PaneFocusReady { intent }
        })
    }

    /// Correlates the effect-ledger identity allocated for a pending activation.
    pub fn attach_platform_focus_effect(
        &mut self,
        activation: ActivationGeneration,
        effect: EffectId,
    ) -> FocusEffectAttachment {
        let Some(pending) = self.focus_lane.driving.as_mut() else {
            return FocusEffectAttachment::StaleActivation;
        };
        if pending.generation != activation {
            return FocusEffectAttachment::StaleActivation;
        }
        match pending.platform_focus {
            PendingPlatformFocus::EffectRequired => {
                pending.platform_focus = PendingPlatformFocus::Requested { effect };
                FocusEffectAttachment::Applied
            }
            PendingPlatformFocus::Requested { effect: existing }
            | PendingPlatformFocus::Indeterminate {
                effect: existing, ..
            }
            | PendingPlatformFocus::ObservedAwaitingTarget {
                effect: existing, ..
            } if existing == effect => FocusEffectAttachment::Duplicate,
            PendingPlatformFocus::Requested { effect: existing }
            | PendingPlatformFocus::Indeterminate {
                effect: existing, ..
            }
            | PendingPlatformFocus::ObservedAwaitingTarget {
                effect: existing, ..
            } => FocusEffectAttachment::EffectAlreadyAttached { existing },
        }
    }

    /// Reduces a platform result which cannot itself claim that focus changed.
    pub fn report_platform_focus_effect(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> FocusEffectReportTransition {
        let Some(pending) = self.focus_lane.driving else {
            return self.report_superseded_focus_effect(effect, result);
        };
        let Some(expected) = pending.platform_focus.effect() else {
            return self.report_superseded_focus_effect(effect, result);
        };
        if expected != effect {
            return self.report_superseded_focus_effect(effect, result);
        }
        if matches!(
            pending.platform_focus,
            PendingPlatformFocus::ObservedAwaitingTarget { .. }
        ) {
            return FocusEffectReportTransition::Duplicate;
        }
        match result {
            EffectDispatchResult::Indeterminate(reason) => {
                if matches!(
                    pending.platform_focus,
                    PendingPlatformFocus::Indeterminate {
                        effect: current,
                        reason: current_reason,
                    } if current == effect && current_reason == reason
                ) {
                    return FocusEffectReportTransition::Duplicate;
                }
                self.focus_lane.driving = Some(PendingViewportActivation {
                    platform_focus: PendingPlatformFocus::Indeterminate { effect, reason },
                    ..pending
                });
                FocusEffectReportTransition::Indeterminate {
                    activation: pending.generation,
                }
            }
            EffectDispatchResult::DispatchFailed(_) => {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectFailed,
                }
            }
            EffectDispatchResult::Unsupported(_) => {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectUnsupported,
                }
            }
        }
    }

    fn report_superseded_focus_effect(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
    ) -> FocusEffectReportTransition {
        let Some(attempt) = self.focus_lane.hazards.get_mut(&effect) else {
            return FocusEffectReportTransition::StaleEffect;
        };
        match result {
            EffectDispatchResult::Indeterminate(reason) => {
                if matches!(
                    attempt.platform_focus,
                    PendingPlatformFocus::Indeterminate {
                        effect: current,
                        reason: current_reason,
                    } if current == effect && current_reason == reason
                ) {
                    return FocusEffectReportTransition::Duplicate;
                }
                attempt.platform_focus = PendingPlatformFocus::Indeterminate { effect, reason };
                FocusEffectReportTransition::Indeterminate {
                    activation: attempt.generation,
                }
            }
            EffectDispatchResult::DispatchFailed(_) => {
                let activation = attempt.generation;
                self.focus_lane.hazards.remove(&effect);
                self.reconcile_hazard_barrier_after_terminal_effect();
                FocusEffectReportTransition::Cancelled {
                    activation,
                    reason: ActivationCancellation::EffectFailed,
                }
            }
            EffectDispatchResult::Unsupported(_) => {
                let activation = attempt.generation;
                self.focus_lane.hazards.remove(&effect);
                self.reconcile_hazard_barrier_after_terminal_effect();
                FocusEffectReportTransition::Cancelled {
                    activation,
                    reason: ActivationCancellation::EffectUnsupported,
                }
            }
        }
    }

    fn reconcile_hazard_barrier_after_terminal_effect(&mut self) {
        let FocusBarrier::Hazard(mut barrier) = self.focus_lane.barrier else {
            return;
        };
        if !self.focus_lane.hazards.is_empty() {
            return;
        }

        let retained_authority = self
            .focus_observations
            .current()
            .and_then(Self::known_focus_target)
            .is_some_and(|target| {
                target
                    .dock()
                    .is_none_or(|binding| !self.focus_lane.quarantined.contains(&binding))
            });
        if retained_authority {
            self.focus_lane.barrier = FocusBarrier::None;
            self.focus_lane.quarantined.clear();
        } else {
            barrier.phase = FocusHazardBarrierPhase::AwaitingPostHazardAuthority;
            self.focus_lane.barrier = FocusBarrier::Hazard(barrier);
        }
    }

    /// Publishes one provider-generated globally consistent native-focus envelope.
    ///
    /// The exact provider generation is compared before any state transition. A matching
    /// activation completes only after its `RequestFocus` effect identity is attached and
    /// the observation is newer than the frozen baseline.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportFocusError::PaneFocusIntentIdExhausted`] if accepting the
    /// observation would require a pane intent after its identity domain is exhausted.
    pub fn publish_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            causal,
            PlatformFocusRestoreGate::KnownAllReleased,
            is_current_binding,
            is_current_binding,
            |_| false,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    pub(crate) fn publish_platform_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        restore_gate: PlatformFocusRestoreGate,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        is_isolated_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            causal,
            restore_gate,
            can_observe_binding,
            can_accept_activation,
            is_isolated_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    /// Revokes all authority derived from the current platform-focus provider.
    ///
    /// Pane-focus history remains available for restoration after a replacement
    /// provider establishes a fresh global-focus baseline.
    pub(crate) fn revoke_platform_provider_authority(&mut self) -> ViewportFocusCleanup {
        let had_global_authority = self.focus_observations.current().is_some();
        self.focus_observations.reset_for_provider_replacement();
        self.clear_provider_bound_state(had_global_authority)
    }

    /// Arms one-shot ordinary focus-restoration suppression when the provider
    /// destroys the exact window which most recently owned global focus.
    pub(crate) fn observe_destroyed_binding(&mut self, binding: ViewportBinding) -> bool {
        let was_last_focused = self
            .focus_observations
            .current()
            .is_some_and(|observation| {
                matches!(
                    observation.focused,
                    Authority::Known(GlobalFocusedWindow::Dock(focused)) if focused == binding
                )
            });
        if !was_last_focused {
            return false;
        }
        let changed = self.destroyed_previous_focus != Some(binding);
        self.destroyed_previous_focus = Some(binding);
        changed
    }

    fn known_focus_target(observation: FocusObservationEnvelope) -> Option<FocusClaimTarget> {
        match observation.focused {
            Authority::Known(focused) => Some(FocusClaimTarget::from_global(focused)),
            Authority::Unknown(_) => None,
        }
    }

    fn target_is_pending_hazard(&self, target: FocusClaimTarget) -> bool {
        target.dock().is_some_and(|binding| {
            self.focus_lane
                .hazards
                .values()
                .any(|attempt| attempt.request.target == binding)
        })
    }

    fn supersede_driving_with_platform_focus(
        &mut self,
        observation: FocusObservationEnvelope,
        focus_changed: bool,
        applied: &mut FocusObservationApplied,
    ) {
        if !focus_changed {
            return;
        }
        let Some(winner) = Self::known_focus_target(observation) else {
            return;
        };
        if winner
            .dock()
            .is_some_and(|binding| self.focus_lane.quarantined.contains(&binding))
        {
            return;
        }
        let Some(driving) = self.focus_lane.driving else {
            if !self.focus_lane.hazards.is_empty() && !self.target_is_pending_hazard(winner) {
                self.focus_lane.barrier = FocusBarrier::Hazard(FocusHazardBarrier {
                    winner,
                    observed_at: observation.generation,
                    phase: FocusHazardBarrierPhase::AwaitingHazards,
                });
            }
            return;
        };
        if winner == FocusClaimTarget::Dock(driving.request.target)
            || observation.generation <= driving.observation_baseline
        {
            return;
        }
        let acknowledged = driving.platform_focus.effect().is_some_and(|effect| {
            matches!(
                observation.acknowledged_effect,
                Authority::Known(Some(acknowledged)) if acknowledged == effect
            )
        });
        if acknowledged
            || matches!(
                driving.platform_focus,
                PendingPlatformFocus::ObservedAwaitingTarget { .. }
            )
        {
            return;
        }

        self.focus_lane.driving = None;
        if let Some(effect) = driving.platform_focus.effect() {
            self.focus_lane.hazards.insert(effect, driving);
            self.focus_lane.quarantined.insert(driving.request.target);
            self.focus_lane.barrier = FocusBarrier::Hazard(FocusHazardBarrier {
                winner,
                observed_at: observation.generation,
                phase: FocusHazardBarrierPhase::AwaitingHazards,
            });
        }
        applied.cancelled_activation = Some((
            driving.generation,
            ActivationCancellation::SupersededByPlatformFocus {
                observation: observation.generation,
            },
        ));
    }

    fn reduce_focus_hazards(
        &mut self,
        observation: FocusObservationEnvelope,
        can_accept_activation: impl Fn(ViewportBinding) -> bool,
        is_isolated_binding: impl Fn(ViewportBinding) -> bool,
        applied: &mut FocusObservationApplied,
    ) {
        let focused_binding = match observation.focused {
            Authority::Known(GlobalFocusedWindow::Dock(binding)) => Some(binding),
            Authority::Known(GlobalFocusedWindow::Foreign | GlobalFocusedWindow::None)
            | Authority::Unknown(_) => None,
        };
        let acknowledged_effect = match observation.acknowledged_effect {
            Authority::Known(effect) => effect,
            Authority::Unknown(_) => None,
        };
        let settled = self
            .focus_lane
            .hazards
            .iter()
            .filter_map(|(effect, attempt)| {
                if observation.generation <= attempt.observation_baseline {
                    return None;
                }
                let evidence = if acknowledged_effect == Some(*effect) {
                    Some(PlatformFocusEvidence::ExactEffectAcknowledgement)
                } else if focused_binding == Some(attempt.request.target) {
                    Some(PlatformFocusEvidence::NewerMatchingObservation)
                } else {
                    None
                }?;
                Some((*effect, attempt.request.target, evidence))
            })
            .collect::<Vec<_>>();

        let mut hazard_took_focus = false;
        let mut settled_any = false;
        for (effect, binding, evidence) in settled {
            settled_any = true;
            hazard_took_focus |= focused_binding == Some(binding);
            self.focus_lane.hazards.remove(&effect);
            applied.record_observed_effect(ObservedPlatformFocusEffect {
                effect,
                binding,
                generation: observation.generation,
                evidence,
            });
        }

        if let FocusBarrier::Hazard(mut barrier) = self.focus_lane.barrier {
            if self.focus_lane.hazards.is_empty() {
                let post_hazard_authority =
                    Self::known_focus_target(observation).is_some_and(|target| {
                        target.dock().is_none_or(|binding| {
                            !self.focus_lane.quarantined.contains(&binding)
                                && can_accept_activation(binding)
                                && !is_isolated_binding(binding)
                        })
                    });
                if !hazard_took_focus
                    && post_hazard_authority
                    && (settled_any || observation.generation > barrier.observed_at)
                {
                    self.focus_lane.barrier = FocusBarrier::None;
                    self.focus_lane.quarantined.clear();
                } else {
                    barrier.phase = FocusHazardBarrierPhase::AwaitingPostHazardAuthority;
                    self.focus_lane.barrier = FocusBarrier::Hazard(barrier);
                }
            }
        }
    }

    fn publish_focus_observation_in_place(
        &mut self,
        observation: FocusObservationEnvelope,
        causal: FocusCausalStamp,
        restore_gate: PlatformFocusRestoreGate,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        is_isolated_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let previous = self.focus_observations.current();
        match self.focus_observations.observe(Some(observation)) {
            FocusObservationStreamTransition::Current => {}
            FocusObservationStreamTransition::Duplicate => {
                return Ok(FocusObservationTransition::Duplicate);
            }
            FocusObservationStreamTransition::Stale { current } => {
                return Ok(FocusObservationTransition::Stale { current });
            }
            FocusObservationStreamTransition::AuthorityRevoked { generation, reason } => {
                let mut effect_settlement = FocusObservationApplied::default();
                if matches!(reason, FocusAuthorityRevocation::Unknown(_)) {
                    self.reduce_focus_hazards(
                        observation,
                        can_accept_activation,
                        is_isolated_binding,
                        &mut effect_settlement,
                    );
                    self.reduce_pending_activation_observation(
                        observation,
                        can_observe_binding,
                        can_accept_activation,
                        item_is_on_surface,
                        &mut effect_settlement,
                    )?;
                    return Ok(FocusObservationTransition::AuthorityRevoked {
                        generation,
                        reason,
                        cleanup: ViewportFocusCleanup {
                            global_authority_removed: previous.is_some(),
                            ..ViewportFocusCleanup::default()
                        },
                        effect_settlement: Box::new(effect_settlement),
                    });
                }
                let cleanup = self.clear_focus_stream_authority(previous.is_some());
                return Ok(FocusObservationTransition::AuthorityRevoked {
                    generation,
                    reason,
                    cleanup,
                    effect_settlement: Box::new(effect_settlement),
                });
            }
            FocusObservationStreamTransition::TombstoneRequired { generation } => {
                let cleanup = self.clear_focus_stream_authority(previous.is_some());
                return Ok(FocusObservationTransition::TombstoneRequired {
                    generation,
                    cleanup,
                });
            }
        }

        let mut applied = FocusObservationApplied::default();
        let focus_changed = previous.map(|previous| previous.focused) != Some(observation.focused);
        self.reduce_recorded_observe_only_activation(
            observation,
            can_observe_binding,
            can_accept_activation,
            item_is_on_surface,
            &mut applied,
        )?;
        self.supersede_driving_with_platform_focus(observation, focus_changed, &mut applied);
        self.reduce_focus_hazards(
            observation,
            can_accept_activation,
            is_isolated_binding,
            &mut applied,
        );
        self.reduce_pending_activation_observation(
            observation,
            can_observe_binding,
            can_accept_activation,
            item_is_on_surface,
            &mut applied,
        )?;
        let focus_requires_reconciliation =
            matches!(self.focus_lane.barrier, FocusBarrier::ReconcileCurrent);
        let focused_is_isolated = matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding))
                if is_isolated_binding(binding)
                    || self.focus_lane.quarantined.contains(&binding)
                    || self
                        .focus_lane
                        .hazards
                        .values()
                        .any(|attempt| attempt.request.target == binding)
        );
        if !focused_is_isolated
            && self.pending_pane_intent.is_some_and(|intent| {
                observation.generation > intent.focus_observation_baseline
                    && matches!(
                        observation.focused,
                        Authority::Known(focused)
                            if focused != GlobalFocusedWindow::Dock(intent.target)
                    )
            })
        {
            if let Some(intent) = self.pending_pane_intent.take() {
                self.clear_admitted_pane_intent(intent.id);
            }
        }

        let focused_current_binding = if (focus_changed || focus_requires_reconciliation)
            && let Authority::Known(GlobalFocusedWindow::Dock(binding)) = observation.focused
            && !focused_is_isolated
            && can_accept_activation(binding)
        {
            Some(binding)
        } else {
            None
        };
        let suppress_destroyed_previous_restore =
            focused_current_binding.is_some() && self.destroyed_previous_focus.take().is_some();

        if applied.pane_intent().is_none()
            && (focus_changed || focus_requires_reconciliation)
            && restore_gate.allows_ordinary_restore()
            && !suppress_destroyed_previous_restore
            && let Some(binding) = focused_current_binding
            && let Some(focus) = self.panel_focus(binding.surface()).focus()
        {
            if let PanelFocus::Item(item) = focus
                && !item_is_on_surface(binding.surface(), item)
            {
                self.panel_focus_by_surface.remove(&binding.surface());
            } else {
                applied.set_pane_intent(self.install_pane_intent(PaneIntentInstall {
                    causal,
                    activation: None,
                    target: binding,
                    focus,
                    source: PaneFocusIntentSource::PlatformActivation,
                    cause: None,
                    focus_observation_baseline: observation.generation,
                })?);
            }
        }

        if ((focus_changed && (previous.is_some() || self.focus_lane.winner.is_none()))
            || focus_requires_reconciliation)
            && applied.completed_activation().is_none()
            && applied.pane_intent().is_none()
            && matches!(observation.focused, Authority::Known(_))
        {
            let Authority::Known(focused) = observation.focused else {
                unreachable!("known focus was checked above")
            };
            let target = FocusClaimTarget::from_global(focused);
            if target.dock().is_none_or(|binding| {
                !is_isolated_binding(binding) && !self.focus_lane.quarantined.contains(&binding)
            }) {
                if target
                    .dock()
                    .is_some_and(|binding| self.focus_matches_reserved_claim(binding))
                {
                    if focus_requires_reconciliation {
                        self.focus_lane.barrier = FocusBarrier::None;
                    }
                } else {
                    let _ = self.admit_pane_causal_claim(PaneCausalClaim::platform_observation(
                        causal, focused,
                    ));
                    if focus_requires_reconciliation {
                        self.focus_lane.barrier = FocusBarrier::None;
                    }
                }
            }
        }

        Ok(FocusObservationTransition::Applied(Box::new(applied)))
    }

    fn reduce_recorded_observe_only_activation(
        &mut self,
        observation: FocusObservationEnvelope,
        can_observe_binding: impl Fn(ViewportBinding) -> bool,
        can_accept_activation: impl Fn(ViewportBinding) -> bool,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let Some(record) = self.recorded_observe_only_activation else {
            return Ok(());
        };
        if record
            .observation_baseline
            .is_some_and(|baseline| observation.generation <= baseline)
        {
            return Ok(());
        }

        let request = record.request;
        let target_is_current = if self.admitted_observe_only_activation == Some(record.generation)
        {
            can_observe_binding(request.target)
        } else {
            can_accept_activation(request.target)
        };
        let item_is_current = match request.pane {
            PaneFocusDisposition::Set(item) => item_is_on_surface(request.target.surface(), item),
            PaneFocusDisposition::Preserve | PaneFocusDisposition::Clear => true,
        };
        let target_focus = matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding)) if binding == request.target
        );
        let authoritative_non_target = matches!(
            observation.focused,
            Authority::Known(focused) if focused != GlobalFocusedWindow::Dock(request.target)
        );

        if request.cause.awaits_observed_focus()
            && target_is_current
            && item_is_current
            && target_focus
        {
            let was_admitted = self.admitted_observe_only_activation == Some(record.generation);
            self.recorded_observe_only_activation = None;
            self.clear_admitted_observe_only(record.generation);
            applied.cleared_observe_only_activation = Some(record.generation);
            if let Some(focus) = request.focus() {
                let intent = self.install_pane_intent(PaneIntentInstall {
                    causal: record.causal,
                    activation: Some(record.generation),
                    target: request.target,
                    focus,
                    source: request.cause.pane_source(),
                    cause: Some(request.cause),
                    focus_observation_baseline: observation.generation,
                })?;
                if was_admitted {
                    self.admitted_pane_intent = intent.map(|intent| intent.id);
                }
                applied.set_pane_intent(intent);
            }
        } else if !request.cause.awaits_observed_focus()
            || !target_is_current
            || !item_is_current
            || authoritative_non_target
        {
            self.recorded_observe_only_activation = None;
            self.clear_admitted_observe_only(record.generation);
            applied.cleared_observe_only_activation = Some(record.generation);
        }
        Ok(())
    }

    fn reduce_pending_activation_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        can_observe_binding: impl Fn(ViewportBinding) -> bool,
        can_accept_activation: impl Fn(ViewportBinding) -> bool,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let Some(pending) = self.focus_lane.driving else {
            return Ok(());
        };
        let target_is_current = match pending.platform_focus {
            PendingPlatformFocus::EffectRequired => can_accept_activation(pending.request.target),
            PendingPlatformFocus::Requested { .. }
            | PendingPlatformFocus::Indeterminate { .. }
            | PendingPlatformFocus::ObservedAwaitingTarget { .. } => {
                can_observe_binding(pending.request.target)
            }
        };
        if !target_is_current {
            self.cancel_pending_activation(pending, ActivationCancellation::StaleBinding, applied);
            return Ok(());
        }
        if let PaneFocusDisposition::Set(item) = pending.request.pane
            && !item_is_on_surface(pending.request.target.surface(), item)
        {
            self.cancel_pending_activation(
                pending,
                ActivationCancellation::ItemUnavailable { item },
                applied,
            );
            return Ok(());
        }
        if observation.generation <= pending.observation_baseline {
            return Ok(());
        }
        let Some(effect) = pending.platform_focus.effect() else {
            return Ok(());
        };

        let acknowledged = matches!(
            observation.acknowledged_effect,
            Authority::Known(Some(acknowledged)) if acknowledged == effect
        );
        let target_focused = Self::observation_focuses(observation, pending.request.target);
        if acknowledged || target_focused {
            applied.record_observed_effect(ObservedPlatformFocusEffect {
                effect,
                binding: pending.request.target,
                generation: observation.generation,
                evidence: if acknowledged {
                    PlatformFocusEvidence::ExactEffectAcknowledgement
                } else {
                    PlatformFocusEvidence::NewerMatchingObservation
                },
            });
        }
        if target_focused {
            self.complete_pending_activation(pending, observation, applied)?;
        } else if acknowledged {
            match observation.focused {
                Authority::Known(GlobalFocusedWindow::Dock(binding))
                    if self.focus_lane.quarantined.contains(&binding) =>
                {
                    self.reassert_after_quarantined_focus(pending, observation, applied)?;
                }
                Authority::Known(_) => self.cancel_pending_activation(
                    pending,
                    ActivationCancellation::EffectObservedWithoutTargetFocus,
                    applied,
                ),
                Authority::Unknown(_) => {
                    self.focus_lane.driving = Some(PendingViewportActivation {
                        platform_focus: PendingPlatformFocus::ObservedAwaitingTarget {
                            effect,
                            acknowledged_at: observation.generation,
                        },
                        ..pending
                    });
                }
            }
        } else if matches!(
            pending.platform_focus,
            PendingPlatformFocus::ObservedAwaitingTarget { .. }
        ) && matches!(observation.focused, Authority::Known(_))
        {
            if matches!(
                observation.focused,
                Authority::Known(GlobalFocusedWindow::Dock(binding))
                    if self.focus_lane.quarantined.contains(&binding)
            ) {
                self.reassert_after_quarantined_focus(pending, observation, applied)?;
            } else {
                self.cancel_pending_activation(
                    pending,
                    ActivationCancellation::EffectObservedWithoutTargetFocus,
                    applied,
                );
            }
        }
        Ok(())
    }

    fn reassert_after_quarantined_focus(
        &mut self,
        pending: PendingViewportActivation,
        observation: FocusObservationEnvelope,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let replacement = self
            .last_activation
            .checked_next()
            .ok_or(ViewportFocusError::ActivationGenerationExhausted)?;
        self.last_activation = replacement;
        self.focus_lane.driving = Some(PendingViewportActivation {
            generation: replacement,
            observation_baseline: observation.generation,
            platform_focus: PendingPlatformFocus::EffectRequired,
            ..pending
        });
        self.retarget_lifecycle_focus_barrier(Some(pending), replacement);
        applied.cancelled_activation = Some((
            pending.generation,
            ActivationCancellation::EffectObservedWithoutTargetFocus,
        ));
        Ok(())
    }

    fn complete_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        observation: FocusObservationEnvelope,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        self.focus_lane.driving = None;
        self.complete_lifecycle_focus_barrier(pending.generation);
        applied.completed_activation = Some(pending.generation);
        if let Some(focus) = pending.request.focus() {
            let intent = self.install_pane_intent(PaneIntentInstall {
                causal: pending.causal,
                activation: Some(pending.generation),
                target: pending.request.target,
                focus,
                source: pending.request.cause.pane_source(),
                cause: Some(pending.request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            self.admitted_pane_intent = intent.map(|intent| intent.id);
            applied.set_pane_intent(intent);
        }
        Ok(())
    }

    fn cancel_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        reason: ActivationCancellation,
        applied: &mut FocusObservationApplied,
    ) {
        self.focus_lane.driving = None;
        self.cancel_lifecycle_focus_barrier(pending.generation);
        applied.cancelled_activation = Some((pending.generation, reason));
    }

    /// Records an exact pane-focus fact and consumes only its matching pending intent.
    pub fn publish_pane_focus_observation(
        &mut self,
        observation: PaneFocusObservation,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> PaneFocusObservationTransition {
        if !is_current_binding(observation.binding) {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::StaleBinding,
            );
        }
        if let PanelFocus::Item(item) = observation.focus
            && !item_is_on_surface(observation.binding.surface(), item)
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::ItemUnavailable { item },
            );
        }

        let surface = observation.binding.surface();
        if let Some(current) = self.pane_observation_by_surface.get(&surface).copied() {
            let current_generation = current.observation.generation;
            if observation.generation < current_generation {
                return PaneFocusObservationTransition::Stale {
                    current: current_generation,
                };
            }
            if observation.generation == current_generation {
                return if observation == current.observation {
                    PaneFocusObservationTransition::Duplicate
                } else {
                    PaneFocusObservationTransition::EqualGenerationConflict {
                        generation: observation.generation,
                    }
                };
            }
        }

        let cleared_intent = if let Some(intent_id) = observation.acknowledges {
            let Some(intent) = self.pending_pane_intent else {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::UnknownIntent { intent: intent_id },
                );
            };
            if intent.id != intent_id {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::UnknownIntent { intent: intent_id },
                );
            }
            if self.admitted_pane_intent != Some(intent_id) {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentNotPublished { intent: intent_id },
                );
            }
            if intent.target != observation.binding {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentBindingMismatch,
                );
            }
            if intent.focus != observation.focus {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentFocusMismatch,
                );
            }
            if intent
                .pane_observation_baseline
                .is_some_and(|baseline| observation.generation <= baseline)
            {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::ObservationPrecedesIntent,
                );
            }
            self.pending_pane_intent = None;
            self.clear_admitted_pane_intent(intent_id);
            Some(intent_id)
        } else {
            if self.pending_pane_intent.is_some_and(|intent| {
                intent.target == observation.binding
                    && intent.focus != observation.focus
                    && intent
                        .pane_observation_baseline
                        .is_none_or(|baseline| observation.generation > baseline)
            }) {
                if let Some(intent) = self.pending_pane_intent.take() {
                    self.clear_admitted_pane_intent(intent.id);
                }
            }
            None
        };

        self.panel_focus_by_surface
            .insert(surface, PanelFocusRecord::from_focus(observation.focus));
        self.pane_observation_by_surface
            .insert(surface, PanelFocusObservationRecord { observation });
        PaneFocusObservationTransition::Applied { cleared_intent }
    }

    /// Removes binding-scoped pending state while preserving per-surface focus history.
    pub fn clear_binding(&mut self, binding: ViewportBinding) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.target != binding);
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        self.focus_lane.quarantined.remove(&binding);
        self.focus_lane
            .hazards
            .retain(|_, attempt| attempt.request.target != binding);
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.target == binding)
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::StaleBinding));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.target == binding)
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target == binding)
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        if self
            .pane_observation_by_surface
            .get(&binding.surface())
            .is_some_and(|record| record.observation.binding == binding)
        {
            self.pane_observation_by_surface.remove(&binding.surface());
            cleanup.pane_observation_removed = true;
        }
        cleanup
    }

    /// Removes every focus fact owned by a retired logical surface.
    pub fn clear_surface(&mut self, surface: SurfaceId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup {
            panel_record_removed: self.panel_focus_by_surface.remove(&surface).is_some(),
            pane_observation_removed: self.pane_observation_by_surface.remove(&surface).is_some(),
            ..ViewportFocusCleanup::default()
        };
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.target.surface() != surface);
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        self.focus_lane
            .quarantined
            .retain(|binding| binding.surface() != surface);
        self.focus_lane
            .hazards
            .retain(|_, attempt| attempt.request.target.surface() != surface);
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.target.surface() == surface)
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::SurfaceRemoved));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.target.surface() == surface)
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target.surface() == surface)
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        cleanup
    }

    /// Removes focus history and commands for an item which no longer exists on its surface.
    pub fn clear_item(&mut self, item: ItemId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.pane != PaneFocusDisposition::Set(item));
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        let recorded_surfaces = self
            .panel_focus_by_surface
            .iter()
            .filter_map(|(surface, focus)| {
                (*focus == PanelFocusRecord::Item(item)).then_some(*surface)
            })
            .collect::<Vec<_>>();
        for surface in recorded_surfaces {
            self.panel_focus_by_surface.remove(&surface);
            self.pane_observation_by_surface.remove(&surface);
            cleanup.panel_record_removed = true;
            cleanup.pane_observation_removed = true;
        }
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.pane == PaneFocusDisposition::Set(item))
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled = Some((
                pending.generation,
                ActivationCancellation::ItemUnavailable { item },
            ));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.focus == PanelFocus::Item(item))
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.pane == PaneFocusDisposition::Set(item))
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        cleanup
    }

    /// Clears only activation state derived from one close request identity.
    pub fn clear_close_request(&mut self, request: CloseRequestId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.cause.close_request() == Some(request))
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled = Some((
                pending.generation,
                ActivationCancellation::CloseRequestCleared { request },
            ));
        }
        if self.pending_pane_intent.is_some_and(|intent| {
            intent
                .cause
                .and_then(ViewportActivationCause::close_request)
                == Some(request)
        }) {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.cause.close_request() == Some(request))
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        cleanup
    }

    pub(crate) fn reject_pane_reveal(&mut self, intent: PaneFocusIntentId) -> bool {
        if self
            .pending_pane_intent
            .is_none_or(|pending| pending.id != intent)
        {
            return false;
        }
        self.pending_pane_intent = None;
        self.clear_admitted_pane_intent(intent);
        true
    }

    pub(crate) fn reconcile_authority(
        &mut self,
        surface_exists: impl Fn(SurfaceId) -> bool + Copy,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();

        self.focus_lane
            .quarantined
            .retain(|binding| surface_exists(binding.surface()));
        self.focus_lane.hazards.retain(|_, attempt| {
            surface_exists(attempt.request.target.surface())
                && can_observe_binding(attempt.request.target)
        });

        self.panel_focus_by_surface.retain(|surface, focus| {
            let keep = surface_exists(*surface)
                && match focus {
                    PanelFocusRecord::Item(item) => item_is_on_surface(*surface, *item),
                    PanelFocusRecord::NoHistory | PanelFocusRecord::None => true,
                };
            cleanup.panel_record_removed |= !keep;
            keep
        });
        self.pane_observation_by_surface.retain(|surface, record| {
            let observation = record.observation;
            let keep = surface_exists(*surface)
                && can_observe_binding(observation.binding)
                && match observation.focus {
                    PanelFocus::Item(item) => item_is_on_surface(*surface, item),
                    PanelFocus::None => true,
                };
            cleanup.pane_observation_removed |= !keep;
            keep
        });

        if let Some(pending) = self.focus_lane.driving {
            let binding_is_current = match pending.platform_focus {
                PendingPlatformFocus::EffectRequired => {
                    can_accept_activation(pending.request.target)
                }
                PendingPlatformFocus::Requested { .. }
                | PendingPlatformFocus::Indeterminate { .. }
                | PendingPlatformFocus::ObservedAwaitingTarget { .. } => {
                    can_observe_binding(pending.request.target)
                }
            };
            if let Some(reason) = Self::stale_activation_reason(
                pending.request,
                surface_exists,
                |binding| binding == pending.request.target && binding_is_current,
                item_is_on_surface,
            ) {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                cleanup.pending_activation_cancelled = Some((pending.generation, reason));
            }
        }
        if let Some(intent) = self.pending_pane_intent {
            let binding_is_current = if self.admitted_pane_intent == Some(intent.id) {
                can_observe_binding(intent.target)
            } else {
                can_accept_activation(intent.target)
            };
            if !surface_exists(intent.target.surface())
                || !binding_is_current
                || matches!(
                    intent.focus,
                    PanelFocus::Item(item)
                        if !item_is_on_surface(intent.target.surface(), item)
                )
            {
                self.pending_pane_intent = None;
                self.clear_admitted_pane_intent(intent.id);
                cleanup.pending_intent_removed = Some(intent.id);
            }
        }
        if let Some(record) = self.recorded_observe_only_activation {
            let binding_is_current =
                if self.admitted_observe_only_activation == Some(record.generation) {
                    can_observe_binding(record.request.target)
                } else {
                    can_accept_activation(record.request.target)
                };
            if Self::stale_activation_reason(
                record.request,
                surface_exists,
                |binding| binding == record.request.target && binding_is_current,
                item_is_on_surface,
            )
            .is_some()
            {
                self.recorded_observe_only_activation = None;
                self.clear_admitted_observe_only(record.generation);
                cleanup.observe_only_activation_removed = Some(record.generation);
            }
        }

        cleanup
    }

    fn clear_focus_stream_authority(
        &mut self,
        global_authority_removed: bool,
    ) -> ViewportFocusCleanup {
        let native_reservations = std::mem::take(&mut self.focus_lane.native_reservations);
        let settled_winner = self.focus_lane.settled_winner;
        let cleanup = self.clear_provider_bound_state(global_authority_removed);

        // Stream faults revoke provider-derived observations and in-flight effects,
        // but the native lifecycle still owns its release-time causal claims.
        self.focus_lane.settled_winner = settled_winner;
        self.focus_lane.native_reservations = native_reservations;
        self.recompute_focus_winner();
        cleanup
    }

    fn clear_provider_bound_state(
        &mut self,
        global_authority_removed: bool,
    ) -> ViewportFocusCleanup {
        let pending_activation_cancelled = self.focus_lane.driving.take().map(|pending| {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            (
                pending.generation,
                ActivationCancellation::PlatformAuthorityRevoked,
            )
        });
        let pending_intent_removed = self.pending_pane_intent.take().map(|intent| intent.id);
        let observe_only_activation_removed = self
            .recorded_observe_only_activation
            .take()
            .map(|record| record.generation);
        self.admitted_observe_only_activation = None;
        self.admitted_pane_intent = None;
        self.destroyed_previous_focus = None;
        self.focus_lane = FocusLane::default();
        ViewportFocusCleanup {
            global_authority_removed,
            pending_activation_cancelled,
            pending_intent_removed,
            observe_only_activation_removed,
            ..ViewportFocusCleanup::default()
        }
    }

    /// Clears workspace-bound focus authority while preserving monotonic identities.
    pub fn reconcile_workspace_replacement(&mut self) {
        self.focus_observations.reset_for_provider_replacement();
        self.destroyed_previous_focus = None;
        self.focus_lane = FocusLane::default();
        self.pane_intent_watermark = None;
        self.recorded_observe_only_activation = None;
        self.pending_pane_intent = None;
        self.admitted_observe_only_activation = None;
        self.admitted_pane_intent = None;
        self.panel_focus_by_surface.clear();
        self.pane_observation_by_surface.clear();
    }

    fn validate_focus_target(
        target: ViewportBinding,
        pane: PaneFocusDisposition,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Option<ActivationSuppression> {
        if !is_current_binding(target) {
            return Some(ActivationSuppression::StaleBinding);
        }
        if let PaneFocusDisposition::Set(item) = pane
            && !item_is_on_surface(target.surface(), item)
        {
            return Some(ActivationSuppression::ItemUnavailable { item });
        }
        None
    }

    fn stale_activation_reason(
        request: ViewportActivationRequest,
        surface_exists: impl FnOnce(SurfaceId) -> bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Option<ActivationCancellation> {
        if !surface_exists(request.target.surface()) {
            return Some(ActivationCancellation::SurfaceRemoved);
        }
        if !is_current_binding(request.target) {
            return Some(ActivationCancellation::StaleBinding);
        }
        if let PaneFocusDisposition::Set(item) = request.pane
            && !item_is_on_surface(request.target.surface(), item)
        {
            return Some(ActivationCancellation::ItemUnavailable { item });
        }
        None
    }

    fn observation_focuses(observation: FocusObservationEnvelope, target: ViewportBinding) -> bool {
        matches!(
            observation.focused,
            Authority::Known(GlobalFocusedWindow::Dock(binding)) if binding == target
        )
    }

    fn install_pane_intent(
        &mut self,
        install: PaneIntentInstall,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        let PaneIntentInstall {
            causal,
            activation,
            target,
            focus,
            source,
            cause,
            focus_observation_baseline,
        } = install;
        if !self.admit_pane_causal_claim(PaneCausalClaim::from_install(install)) {
            return Ok(None);
        }
        if let Some(watermark) = self.pane_intent_watermark {
            if causal.generation() < watermark.causal.generation()
                || (causal.generation() == watermark.causal.generation()
                    && source.priority() < watermark.source.priority())
            {
                return Ok(None);
            }
            if causal.generation() == watermark.causal.generation()
                && source == watermark.source
                && (target != watermark.target || focus != watermark.focus)
            {
                return Ok(None);
            }
            if causal.generation() == watermark.causal.generation()
                && source == watermark.source
                && target == watermark.target
                && focus == watermark.focus
                && let Some(existing) = self.pending_pane_intent
            {
                return Ok(Some(existing));
            }
        }

        let id = self
            .last_pane_intent
            .checked_next()
            .ok_or(ViewportFocusError::PaneFocusIntentIdExhausted)?;
        let pane_observation_baseline = self
            .pane_observation_by_surface
            .get(&target.surface())
            .map(|record| record.observation.generation);
        let intent = PaneFocusIntent {
            id,
            causal,
            activation,
            target,
            focus,
            source,
            cause,
            focus_observation_baseline,
            pane_observation_baseline,
        };
        self.last_pane_intent = id;
        self.pane_intent_watermark = Some(PaneIntentWatermark {
            causal,
            source,
            target,
            focus,
        });
        self.admitted_pane_intent = None;
        self.pending_pane_intent = Some(intent);
        Ok(Some(intent))
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
mod tests {
    use super::*;
    use crate::close_plan::{
        CloseAuthority, CloseCoordinator, CloseItemRequirement, ClosePlanTarget,
    };
    use crate::effect::DispatchFailureReason;
    use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch, WorkspaceRevision};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::policy::{CloseCapability, PolicyRevision};
    use crate::transition::WorkspaceVersion;
    use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

    fn binding() -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        )
    }

    fn second_binding() -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            SurfaceId::new(2),
            WindowToken::new(2),
            WindowIncarnation::new(2),
        )
    }

    fn third_binding() -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            SurfaceId::new(3),
            WindowToken::new(3),
            WindowIncarnation::new(3),
        )
    }

    fn close_request() -> CloseRequestId {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let authority = CloseAuthority::new(
            domain,
            WorkspaceVersion::new(WorkspaceEpoch::new(1), WorkspaceRevision::new(1)),
            PolicyRevision::new(1),
        );
        let item = ItemId::new(7);
        let mut coordinator = CloseCoordinator::new(domain);
        coordinator
            .open(
                authority,
                ClosePlanTarget::Item { item },
                [CloseItemRequirement::new(item, CloseCapability::Immediate)],
                (),
            )
            .expect("test close plan must open")
            .request()
    }

    fn focus_tombstone(generation: u64) -> FocusObservationEnvelope {
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        )
    }

    fn focus_stamp(generation: u64) -> FocusCausalStamp {
        FocusCausalStamp::new(
            PaneFocusIntentGeneration::new(generation),
            ReductionCause::SurfaceContributionBatch {
                tick: crate::ids::ReducerTickId::new(generation),
            },
        )
    }

    #[test]
    fn focus_observation_stream_requires_causal_tombstones() {
        let mut stream = FocusObservationStream::default();
        let initial = focus_observation(4, GlobalFocusedWindow::Dock(binding()));
        stream.observe(Some(initial));
        assert_eq!(stream.current(), Some(initial));

        stream.observe(Some(focus_observation(3, GlobalFocusedWindow::Foreign)));
        assert_eq!(stream.current(), Some(initial), "older facts are inert");
        stream.observe(Some(initial));
        assert_eq!(stream.current(), Some(initial), "duplicates are inert");

        stream.observe(Some(focus_observation(4, GlobalFocusedWindow::Foreign)));
        assert_eq!(
            stream.current(),
            None,
            "same-generation conflicts revoke focus authority"
        );
        stream.observe(Some(focus_observation(5, GlobalFocusedWindow::None)));
        assert_eq!(
            stream.current(),
            None,
            "known focus cannot bridge a same-generation conflict"
        );
        stream.observe(Some(focus_tombstone(6)));
        assert_eq!(stream.current(), None);

        let restored = focus_observation(7, GlobalFocusedWindow::Foreign);
        stream.observe(Some(restored));
        assert_eq!(stream.current(), Some(restored));
        assert_eq!(
            stream.generation_watermark(),
            Some(FocusObservationGeneration::new(7))
        );
    }

    #[test]
    fn focus_observation_stream_quarantines_missing_and_skipped_generations() {
        let mut stream = FocusObservationStream::default();
        stream.observe(Some(focus_observation(
            1,
            GlobalFocusedWindow::Dock(binding()),
        )));

        stream.observe(None);
        stream.observe(Some(focus_observation(2, GlobalFocusedWindow::Foreign)));
        assert_eq!(
            stream.current(),
            None,
            "known focus cannot bridge an unversioned gap"
        );
        stream.observe(Some(focus_tombstone(3)));
        let after_missing = focus_observation(4, GlobalFocusedWindow::Foreign);
        stream.observe(Some(after_missing));
        assert_eq!(stream.current(), Some(after_missing));

        stream.observe(Some(focus_observation(6, GlobalFocusedWindow::None)));
        assert_eq!(
            stream.current(),
            None,
            "a skipped generation revokes focus authority"
        );
        stream.observe(Some(focus_observation(7, GlobalFocusedWindow::Foreign)));
        assert_eq!(
            stream.current(),
            None,
            "known focus cannot bridge a numeric generation gap"
        );
        stream.observe(Some(focus_tombstone(8)));
        let restored = focus_observation(9, GlobalFocusedWindow::Dock(second_binding()));
        stream.observe(Some(restored));
        assert_eq!(stream.current(), Some(restored));

        stream.observe(Some(focus_tombstone(11)));
        stream.observe(Some(focus_observation(12, GlobalFocusedWindow::None)));
        assert_eq!(
            stream.current(),
            None,
            "a skipped unknown envelope is not the required next-generation tombstone"
        );
        stream.observe(Some(focus_tombstone(13)));
        let post_gap = focus_observation(14, GlobalFocusedWindow::None);
        stream.observe(Some(post_gap));
        assert_eq!(stream.current(), Some(post_gap));
    }

    #[test]
    fn focus_observation_stream_provider_replacement_resets_its_namespace() {
        let mut stream = FocusObservationStream::default();
        stream.observe(Some(focus_observation(
            40,
            GlobalFocusedWindow::Dock(binding()),
        )));
        stream.observe(Some(focus_observation(40, GlobalFocusedWindow::Foreign)));
        assert_eq!(stream.current(), None);

        stream.reset_for_provider_replacement();
        assert_eq!(stream.current(), None);
        assert_eq!(stream.generation_watermark(), None);

        let replacement_baseline =
            focus_observation(1, GlobalFocusedWindow::Dock(second_binding()));
        stream.observe(Some(replacement_baseline));
        assert_eq!(stream.current(), Some(replacement_baseline));

        stream.observe(Some(FocusObservationEnvelope::new(
            FocusObservationGeneration::new(2),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(None),
        )));
        assert_eq!(
            stream.current(),
            None,
            "unknown focus must not be synthesized into a known no-focus fact"
        );
        let explicit_none = focus_observation(3, GlobalFocusedWindow::None);
        stream.observe(Some(explicit_none));
        assert_eq!(stream.current(), Some(explicit_none));
    }

    #[test]
    fn coordinator_quarantines_known_focus_after_equal_generation_conflict() {
        let target = binding();
        let replacement = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                focus_observation(10, GlobalFocusedWindow::Dock(target)),
                1,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::Applied(_)
        ));

        let activation = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(replacement, PanelFocus::Item(ItemId::new(2))),
                focus_stamp(2),
                true,
                |candidate| candidate == target || candidate == replacement,
                |surface, item| surface == replacement.surface() && item == ItemId::new(2),
            )
            .expect("activation identity must remain available");
        assert!(matches!(
            activation.outcome(),
            ActivationStartOutcome::RequestPlatformFocus { .. }
        ));

        let conflict = publish_platform_focus(
            &mut coordinator,
            focus_observation(10, GlobalFocusedWindow::Dock(replacement)),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let FocusObservationTransition::AuthorityRevoked {
            generation,
            reason,
            cleanup,
            ..
        } = conflict
        else {
            panic!("an equal-generation conflict must revoke focus authority");
        };
        assert_eq!(generation, Some(FocusObservationGeneration::new(10)));
        assert_eq!(reason, FocusAuthorityRevocation::EqualGenerationConflict);
        assert!(cleanup.global_authority_removed());
        assert_eq!(
            cleanup.pending_activation_cancelled(),
            Some((
                activation.generation(),
                ActivationCancellation::PlatformAuthorityRevoked,
            ))
        );
        assert_eq!(coordinator.focus_observation(), None);

        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                focus_observation(11, GlobalFocusedWindow::Dock(replacement)),
                4,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::TombstoneRequired {
                generation,
                ..
            } if generation == FocusObservationGeneration::new(11)
        ));
        assert_eq!(coordinator.focus_observation(), None);

        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                focus_tombstone(12),
                5,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::AuthorityRevoked {
                reason: FocusAuthorityRevocation::Unknown(AuthorityUnavailableReason::NotReported),
                ..
            }
        ));
        let restored = focus_observation(13, GlobalFocusedWindow::Dock(replacement));
        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                restored,
                6,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.focus_observation(), Some(restored));
    }

    #[test]
    fn coordinator_revokes_focus_for_generation_gaps_and_unknown_tombstones() {
        let mut coordinator = ViewportFocusCoordinator::default();
        let initial = focus_observation(20, GlobalFocusedWindow::Dock(binding()));
        let _ = publish_platform_focus(
            &mut coordinator,
            initial,
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );

        let gap = publish_platform_focus(
            &mut coordinator,
            focus_observation(22, GlobalFocusedWindow::Foreign),
            2,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            gap,
            FocusObservationTransition::AuthorityRevoked {
                generation: Some(generation),
                reason: FocusAuthorityRevocation::GenerationGap { previous },
                cleanup,
                ..
            } if generation == FocusObservationGeneration::new(22)
                && previous == FocusObservationGeneration::new(20)
                && cleanup.global_authority_removed()
        ));
        assert_eq!(coordinator.focus_observation(), None);

        let _ = publish_platform_focus(
            &mut coordinator,
            focus_tombstone(23),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let restored = focus_observation(24, GlobalFocusedWindow::Foreign);
        let _ = publish_platform_focus(
            &mut coordinator,
            restored,
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert_eq!(coordinator.focus_observation(), Some(restored));

        let unknown = publish_platform_focus(
            &mut coordinator,
            focus_tombstone(25),
            5,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            unknown,
            FocusObservationTransition::AuthorityRevoked {
                generation: Some(generation),
                reason: FocusAuthorityRevocation::Unknown(
                    AuthorityUnavailableReason::NotReported
                ),
                cleanup,
                ..
            } if generation == FocusObservationGeneration::new(25)
                && cleanup.global_authority_removed()
        ));
        assert_eq!(
            coordinator.focus_observation(),
            None,
            "unknown focus must not be synthesized into GlobalFocusedWindow::None"
        );
    }

    #[test]
    fn focus_stream_faults_preserve_native_activation_reservations() {
        let owner = NativeCreateSagaId::new(71);
        let request = ViewportActivationRequest::tear_off_committed(
            second_binding(),
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let reservation = (owner, focus_stamp(2), request);
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(20, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

        let gap = publish_platform_focus(
            &mut coordinator,
            focus_observation(22, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            gap,
            FocusObservationTransition::AuthorityRevoked {
                reason: FocusAuthorityRevocation::GenerationGap { .. },
                ..
            }
        ));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some(reservation),
            "a provider stream fault must not revoke a core-owned native reservation"
        );

        let tombstone_required = publish_platform_focus(
            &mut coordinator,
            focus_observation(23, GlobalFocusedWindow::None),
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            tombstone_required,
            FocusObservationTransition::TombstoneRequired { .. }
        ));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some(reservation)
        );

        let tombstone = publish_platform_focus(
            &mut coordinator,
            focus_tombstone(24),
            5,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            tombstone,
            FocusObservationTransition::AuthorityRevoked {
                reason: FocusAuthorityRevocation::Unknown(_),
                ..
            }
        ));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some(reservation)
        );

        let baseline = publish_platform_focus(
            &mut coordinator,
            focus_observation(25, GlobalFocusedWindow::None),
            6,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(baseline, FocusObservationTransition::Applied(_)));

        let activation = coordinator
            .request_reserved_activation(
                owner,
                request,
                focus_stamp(2),
                true,
                |candidate| candidate == binding() || candidate == second_binding(),
                |surface, item| surface == second_binding().surface() && item == ItemId::new(2),
            )
            .expect("the exact native reservation must remain replayable");
        assert_eq!(
            activation.outcome(),
            ActivationStartOutcome::RequestPlatformFocus {
                target: second_binding(),
            }
        );
    }

    #[test]
    fn equal_generation_focus_conflicts_preserve_native_activation_reservations() {
        let owner = NativeCreateSagaId::new(72);
        let request = ViewportActivationRequest::tear_off_committed(
            second_binding(),
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(30, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

        let conflict = publish_platform_focus(
            &mut coordinator,
            focus_observation(30, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            conflict,
            FocusObservationTransition::AuthorityRevoked {
                reason: FocusAuthorityRevocation::EqualGenerationConflict,
                ..
            }
        ));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some((owner, focus_stamp(2), request))
        );
    }

    #[test]
    fn provider_replacement_revokes_native_activation_reservations() {
        let owner = NativeCreateSagaId::new(73);
        let request = ViewportActivationRequest::tear_off_committed(
            second_binding(),
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(40, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

        let cleanup = coordinator.revoke_platform_provider_authority();

        assert!(cleanup.global_authority_removed());
        assert!(coordinator.activation_reservations().is_empty());
        assert_eq!(coordinator.winning_activation_reservation(), None);
    }

    #[test]
    fn provider_revocation_preserves_pane_history_and_resets_focus_namespace() {
        let target = binding();
        let replacement = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(40, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            coordinator.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(1),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                ),
                |candidate| candidate == target || candidate == replacement,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            ),
            PaneFocusObservationTransition::Applied { .. }
        ));

        let pane_activation = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                |candidate| candidate == target || candidate == replacement,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("pane activation must remain available");
        let ActivationStartOutcome::PaneFocusReady { .. } = pane_activation.outcome() else {
            panic!("an already-focused target must install a pane intent");
        };
        let observe_only = coordinator
            .request_activation(
                ViewportActivationRequest::pointer_tab_gesture(
                    replacement,
                    PanelFocus::Item(ItemId::new(2)),
                ),
                focus_stamp(3),
                true,
                |candidate| candidate == target || candidate == replacement,
                |surface, item| surface == replacement.surface() && item == ItemId::new(2),
            )
            .expect("observe-only activation must remain available");
        let ActivationStartOutcome::ObserveOnlyRecorded { record } = observe_only.outcome() else {
            panic!("an unfocused pointer target must wait for provider focus");
        };
        assert!(
            coordinator.pending_pane_intent().is_none(),
            "a newer causal claim immediately supersedes an unacknowledged older pane intent"
        );

        let cleanup = coordinator.revoke_platform_provider_authority();
        assert!(cleanup.global_authority_removed());
        assert_eq!(cleanup.pending_intent_removed(), None);
        assert_eq!(
            cleanup.observe_only_activation_removed(),
            Some(record.generation())
        );
        assert_eq!(coordinator.focus_observation(), None);
        assert_eq!(
            coordinator.panel_focus(target.surface()),
            PanelFocusRecord::Item(ItemId::new(1))
        );
        assert!(
            coordinator
                .surface_focus_state(target.surface())
                .and_then(SurfaceFocusState::observation)
                .is_some(),
            "provider replacement must retain the pane observation history"
        );

        let replacement_baseline = focus_observation(1, GlobalFocusedWindow::Dock(replacement));
        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                replacement_baseline,
                4,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.focus_observation(), Some(replacement_baseline));

        let pending = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(5),
                true,
                |candidate| candidate == target || candidate == replacement,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("pending activation must remain available");
        let cleanup = coordinator.revoke_platform_provider_authority();
        assert_eq!(
            cleanup.pending_activation_cancelled(),
            Some((
                pending.generation(),
                ActivationCancellation::PlatformAuthorityRevoked,
            ))
        );
        assert_eq!(
            coordinator.panel_focus(target.surface()),
            PanelFocusRecord::Item(ItemId::new(1))
        );
    }

    #[test]
    fn workspace_replacement_resets_the_focus_provider_namespace() {
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(99, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );

        coordinator.reconcile_workspace_replacement();
        let replacement = focus_observation(1, GlobalFocusedWindow::Dock(second_binding()));
        assert!(matches!(
            publish_platform_focus(
                &mut coordinator,
                replacement,
                2,
                PlatformFocusRestoreGate::KnownAllReleased,
            ),
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.focus_observation(), Some(replacement));
    }

    fn focus_observation(
        generation: u64,
        focused: GlobalFocusedWindow,
    ) -> FocusObservationEnvelope {
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(focused),
            Authority::Known(None),
        )
    }

    fn publish_platform_focus(
        coordinator: &mut ViewportFocusCoordinator,
        observation: FocusObservationEnvelope,
        pane_generation: u64,
        gate: PlatformFocusRestoreGate,
    ) -> FocusObservationTransition {
        let bindings = [binding(), second_binding()];
        coordinator
            .publish_platform_focus_observation(
                observation,
                focus_stamp(pane_generation),
                gate,
                |candidate| bindings.contains(&candidate),
                |candidate| bindings.contains(&candidate),
                |_| false,
                |surface, item| {
                    (surface == SurfaceId::new(1) && item == ItemId::new(1))
                        || (surface == SurfaceId::new(2) && item == ItemId::new(2))
                },
            )
            .expect("focus identity domains must remain available")
    }

    fn pending_lifecycle_focus_barrier() -> (
        ViewportFocusCoordinator,
        ViewportBinding,
        ViewportBinding,
        ActivationStart,
        EffectId,
    ) {
        let older = binding();
        let winner = second_binding();
        let binding_is_live = |candidate| candidate == older || candidate == winner;
        let item_is_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == winner.surface() && item == ItemId::new(2))
        };
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Foreign),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let older_request = ViewportActivationRequest::tear_off_committed(
            older,
            PaneFocusDisposition::Set(ItemId::new(1)),
        );
        assert!(coordinator.reserve_activation_causal(
            NativeCreateSagaId::new(1),
            focus_stamp(2),
            older_request,
        ));
        assert!(
            coordinator.admit_pane_causal_claim(PaneCausalClaim::from_request(
                focus_stamp(3),
                ViewportActivationRequest::pointer_tab_gesture(
                    winner,
                    PanelFocus::Item(ItemId::new(2)),
                ),
            ))
        );
        let old_completion = coordinator
            .request_reserved_activation(
                NativeCreateSagaId::new(1),
                older_request,
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("the old completion must reduce to causal suppression");
        assert!(matches!(
            old_completion.outcome(),
            ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded { .. })
        ));
        let barrier = coordinator
            .request_winning_lifecycle_compensation(true, binding_is_live, item_is_live)
            .expect("the lifecycle barrier must allocate")
            .expect("the winner must remain addressable");
        let effect = EffectId::new(42);
        assert_eq!(
            coordinator.attach_platform_focus_effect(barrier.generation(), effect),
            FocusEffectAttachment::Applied
        );
        (coordinator, older, winner, barrier, effect)
    }

    fn assert_installed_focus_delta(delta: &FocusDelta, intent: PaneFocusIntent, effect: EffectId) {
        assert!(delta.global_observation().is_some());
        assert_eq!(
            delta
                .pane_intent()
                .and_then(|change| change.after().as_ref())
                .copied(),
            Some(intent)
        );
        assert_eq!(delta.surface_focus().len(), 1);
        assert_eq!(delta.effects().len(), 1);
        assert_eq!(delta.effects()[0].request().id(), effect);
        assert_eq!(delta.effects()[0].phase().before(), &None);
        assert_eq!(
            delta.effects()[0].phase().after(),
            &Some(EffectPhase::Requested)
        );
    }

    fn assert_settled_focus_delta(
        delta: &FocusDelta,
        intent: PaneFocusIntent,
        observed: ObservedPlatformFocusEffect,
        inventory_generation: InventoryGeneration,
    ) {
        assert_eq!(
            delta
                .pane_intent()
                .and_then(|change| change.before().as_ref())
                .copied(),
            Some(intent)
        );
        assert_eq!(
            delta
                .pane_intent()
                .and_then(|change| change.after().as_ref()),
            None
        );
        assert_eq!(delta.effects()[0].observed(), Some(observed));
        assert_eq!(
            delta.effects()[0].phase().after(),
            &Some(EffectPhase::ObservedApplied {
                inventory_generation,
            })
        );
    }

    #[test]
    fn activation_generation_exhaustion_is_atomic() {
        let mut coordinator = ViewportFocusCoordinator::default();
        coordinator.exhaust_activation_generations();
        let before = coordinator.clone();
        assert_eq!(
            coordinator.request_activation(
                ViewportActivationRequest::explicit(binding(), PanelFocus::None),
                focus_stamp(1),
                true,
                |_| true,
                |_, _| true,
            ),
            Err(ViewportFocusError::ActivationGenerationExhausted)
        );
        assert_eq!(coordinator, before);
    }

    #[test]
    fn pane_intent_identity_exhaustion_does_not_publish_partial_state() {
        let mut coordinator = ViewportFocusCoordinator::default();
        let target = binding();
        coordinator
            .focus_observations
            .force_current_for_test(FocusObservationEnvelope::new(
                FocusObservationGeneration::new(1),
                Authority::Known(GlobalFocusedWindow::Dock(target)),
                Authority::Known(None),
            ));
        coordinator.exhaust_pane_intent_ids();
        let before = coordinator.clone();
        assert_eq!(
            coordinator.request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::None),
                focus_stamp(1),
                true,
                |_| true,
                |_, _| true,
            ),
            Err(ViewportFocusError::PaneFocusIntentIdExhausted)
        );
        assert_eq!(coordinator, before);
    }

    #[test]
    fn authoritative_mouse_down_suppresses_only_ordinary_platform_restore() {
        let target = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        coordinator
            .panel_focus_by_surface
            .insert(target.surface(), PanelFocusRecord::Item(ItemId::new(2)));

        let suppressed = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(target)),
            2,
            PlatformFocusRestoreGate::KnownDown,
        );
        let FocusObservationTransition::Applied(suppressed) = suppressed else {
            panic!("new focus must be accepted");
        };
        assert!(suppressed.pane_intent().is_none());

        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let restored = publish_platform_focus(
            &mut coordinator,
            focus_observation(4, GlobalFocusedWindow::Dock(target)),
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let FocusObservationTransition::Applied(restored) = restored else {
            panic!("later focus must be accepted");
        };
        assert_eq!(
            restored.pane_intent().map(PaneFocusIntent::source),
            Some(PaneFocusIntentSource::PlatformActivation)
        );
    }

    #[test]
    fn unknown_button_authority_cannot_restore_ordinary_pane_focus() {
        assert!(PlatformFocusRestoreGate::KnownAllReleased.allows_ordinary_restore());
        assert!(!PlatformFocusRestoreGate::KnownDown.allows_ordinary_restore());
        assert!(!PlatformFocusRestoreGate::Unknown.allows_ordinary_restore());
    }

    #[test]
    fn destroyed_previous_focus_suppresses_one_ordinary_restore() {
        let target = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        coordinator
            .panel_focus_by_surface
            .insert(target.surface(), PanelFocusRecord::Item(ItemId::new(2)));
        assert!(coordinator.observe_destroyed_binding(binding()));

        let suppressed = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(target)),
            2,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let FocusObservationTransition::Applied(suppressed) = suppressed else {
            panic!("fallback focus must be accepted");
        };
        assert!(suppressed.pane_intent().is_none());

        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let restored = publish_platform_focus(
            &mut coordinator,
            focus_observation(4, GlobalFocusedWindow::Dock(target)),
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let FocusObservationTransition::Applied(restored) = restored else {
            panic!("later platform focus must be accepted");
        };
        assert_eq!(
            restored.pane_intent().map(PaneFocusIntent::source),
            Some(PaneFocusIntentSource::PlatformActivation)
        );
    }

    #[test]
    fn explicit_activation_bypasses_both_platform_restore_gates() {
        let target = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(coordinator.observe_destroyed_binding(binding()));
        let start = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
                focus_stamp(2),
                true,
                |candidate| [binding(), target].contains(&candidate),
                |surface, item| surface == target.surface() && item == ItemId::new(2),
            )
            .expect("explicit activation must allocate");
        let effect = EffectId::new(1);
        assert_eq!(
            coordinator.attach_platform_focus_effect(start.generation(), effect),
            FocusEffectAttachment::Applied
        );

        let completed = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(target)),
            3,
            PlatformFocusRestoreGate::KnownDown,
        );
        let FocusObservationTransition::Applied(completed) = completed else {
            panic!("explicit target focus must be accepted");
        };
        assert_eq!(completed.completed_activation(), Some(start.generation()));
        assert_eq!(
            completed.pane_intent().map(PaneFocusIntent::source),
            Some(PaneFocusIntentSource::ExplicitViewportActivation)
        );
    }

    #[test]
    fn closing_binding_cancels_unissued_activation_but_settles_emitted_obligation() {
        let target = second_binding();
        let current = binding();
        let binding_is_live = |candidate| candidate == current || candidate == target;
        let item_is_live = |surface, item| {
            (surface == current.surface() && item == ItemId::new(1))
                || (surface == target.surface() && item == ItemId::new(2))
        };

        let mut unissued = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut unissued,
            focus_observation(1, GlobalFocusedWindow::Dock(current)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let start = unissued
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("unissued activation must allocate");
        let cleanup = unissued.reconcile_authority(
            |_| true,
            binding_is_live,
            |candidate| candidate == current,
            item_is_live,
        );
        assert_eq!(
            cleanup.pending_activation_cancelled(),
            Some((start.generation(), ActivationCancellation::StaleBinding))
        );

        let mut emitted = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut emitted,
            focus_observation(1, GlobalFocusedWindow::Dock(current)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let start = emitted
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("emitted activation must allocate");
        let effect = EffectId::new(88);
        assert_eq!(
            emitted.attach_platform_focus_effect(start.generation(), effect),
            FocusEffectAttachment::Applied
        );
        assert!(
            !emitted
                .reconcile_authority(
                    |_| true,
                    binding_is_live,
                    |candidate| candidate == current,
                    item_is_live,
                )
                .changed(),
            "an emitted request remains observable while its target is closing"
        );

        let settled = emitted
            .publish_platform_focus_observation(
                focus_observation(2, GlobalFocusedWindow::Dock(target)),
                focus_stamp(3),
                PlatformFocusRestoreGate::KnownDown,
                binding_is_live,
                |candidate| candidate == current,
                |_| false,
                item_is_live,
            )
            .expect("late target focus must settle the emitted request");
        let FocusObservationTransition::Applied(settled) = settled else {
            panic!("late target focus must apply");
        };
        let intent = settled
            .pane_intent()
            .expect("the original pane obligation must survive settlement");
        assert_eq!(intent.causal(), focus_stamp(2));
        assert!(
            !emitted
                .reconcile_authority(
                    |_| true,
                    binding_is_live,
                    |candidate| candidate == current,
                    item_is_live,
                )
                .changed(),
            "the pane continuation of an emitted activation remains acknowledgeable"
        );
    }

    #[test]
    fn published_observe_only_obligation_accepts_a_late_closing_ack() {
        let target = second_binding();
        let current = binding();
        let binding_is_live = |candidate| candidate == current || candidate == target;
        let item_is_live = |surface, item| {
            (surface == current.surface() && item == ItemId::new(1))
                || (surface == target.surface() && item == ItemId::new(2))
        };
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(current)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let start = coordinator
            .request_activation(
                ViewportActivationRequest::pointer_tab_gesture(
                    target,
                    PanelFocus::Item(ItemId::new(2)),
                ),
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("observe-only activation must allocate");
        assert!(matches!(
            start.outcome(),
            ActivationStartOutcome::ObserveOnlyRecorded { .. }
        ));
        coordinator.mark_boundary_published();
        assert!(
            !coordinator
                .reconcile_authority(
                    |_| true,
                    binding_is_live,
                    |candidate| candidate == current,
                    item_is_live,
                )
                .changed()
        );

        let settled = coordinator
            .publish_platform_focus_observation(
                focus_observation(2, GlobalFocusedWindow::Dock(target)),
                focus_stamp(3),
                PlatformFocusRestoreGate::KnownDown,
                binding_is_live,
                |candidate| candidate == current,
                |_| false,
                item_is_live,
            )
            .expect("late pointer focus must settle");
        let FocusObservationTransition::Applied(settled) = settled else {
            panic!("late pointer focus must apply");
        };
        let intent = settled
            .pane_intent()
            .expect("late pointer focus must release the pane intent");
        assert_eq!(intent.causal(), focus_stamp(2));
        assert!(
            !coordinator
                .reconcile_authority(
                    |_| true,
                    binding_is_live,
                    |candidate| candidate == current,
                    item_is_live,
                )
                .changed()
        );
        assert_eq!(
            coordinator.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(1),
                    target,
                    PanelFocus::Item(ItemId::new(2)),
                )
                .acknowledging(intent.id()),
                binding_is_live,
                item_is_live,
            ),
            PaneFocusObservationTransition::Applied {
                cleared_intent: Some(intent.id())
            }
        );
    }

    #[test]
    fn same_generation_explicit_activation_cannot_clear_higher_priority_close_recovery() {
        let target = binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );

        let recovery = coordinator
            .request_activation(
                ViewportActivationRequest::close_recovery(
                    target,
                    PaneFocusDisposition::Set(ItemId::new(1)),
                    close_request(),
                ),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("focused close recovery must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: recovery } = recovery.outcome() else {
            panic!("focused close recovery must install a pane intent");
        };

        let explicit = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("explicit activation must allocate its diagnostic identity");
        assert_eq!(
            explicit.outcome(),
            ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
                current: PaneFocusIntentGeneration::new(2),
            }),
            "a lower-priority same-generation activation must lose arbitration"
        );
        assert_eq!(
            coordinator.pending_pane_intent(),
            Some(recovery),
            "a rejected activation must leave the existing recovery intent intact"
        );
    }

    #[test]
    fn same_generation_close_recovery_supersedes_lower_priority_explicit_activation() {
        let target = binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );

        let explicit = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("focused explicit activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: explicit } = explicit.outcome() else {
            panic!("focused explicit activation must install a pane intent");
        };

        let recovery = coordinator
            .request_activation(
                ViewportActivationRequest::close_recovery(
                    target,
                    PaneFocusDisposition::Set(ItemId::new(1)),
                    close_request(),
                ),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |surface, item| surface == target.surface() && item == ItemId::new(1),
            )
            .expect("close recovery must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: recovery } = recovery.outcome() else {
            panic!("higher-priority same-generation recovery must install its pane intent");
        };
        assert_ne!(recovery.id(), explicit.id());
        assert_eq!(recovery.source(), PaneFocusIntentSource::CloseRecovery);
        assert_eq!(coordinator.pending_pane_intent(), Some(recovery));
    }

    #[test]
    fn same_generation_high_priority_preserve_wins_in_both_orders() {
        let target = binding();
        let is_current = |candidate| candidate == target;
        let item_is_current = |surface, item| surface == target.surface() && item == ItemId::new(1);
        let preserve = || {
            ViewportActivationRequest::with_disposition(
                target,
                PaneFocusDisposition::Preserve,
                ViewportActivationCause::CloseRecovery {
                    request: close_request(),
                },
            )
        };

        let mut lower_first = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut lower_first,
            focus_observation(1, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let lower = lower_first
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                is_current,
                item_is_current,
            )
            .expect("lower-priority activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: lower } = lower.outcome() else {
            panic!("focused lower-priority activation must install an intent");
        };
        lower_first.mark_boundary_published();
        let higher = lower_first
            .request_activation(
                preserve(),
                focus_stamp(2),
                true,
                is_current,
                item_is_current,
            )
            .expect("higher-priority preserve must allocate");
        assert_eq!(
            higher.outcome(),
            ActivationStartOutcome::PaneFocusPreserved { target }
        );
        assert!(lower_first.pending_pane_intent().is_none());
        assert_eq!(
            lower_first.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(1),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                )
                .acknowledging(lower.id()),
                is_current,
                item_is_current,
            ),
            PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::UnknownIntent { intent: lower.id() }
            )
        );

        let mut higher_first = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut higher_first,
            focus_observation(1, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let _ = higher_first
            .request_activation(
                preserve(),
                focus_stamp(2),
                true,
                is_current,
                item_is_current,
            )
            .expect("higher-priority preserve must allocate first");
        let lower = higher_first
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                is_current,
                item_is_current,
            )
            .expect("lower-priority activation receives a diagnostic result");
        assert_eq!(
            lower.outcome(),
            ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
                current: PaneFocusIntentGeneration::new(2),
            })
        );
        assert!(higher_first.pending_pane_intent().is_none());
    }

    #[test]
    fn cancelling_a_newer_native_reservation_restores_the_older_owner() {
        let older = binding();
        let newer = second_binding();
        let older_owner = NativeCreateSagaId::new(41);
        let newer_owner = NativeCreateSagaId::new(42);
        let older_request = ViewportActivationRequest::tear_off_committed(
            older,
            PaneFocusDisposition::Set(ItemId::new(1)),
        );
        let newer_request = ViewportActivationRequest::tear_off_committed(
            newer,
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();

        assert!(coordinator.reserve_activation_causal(older_owner, focus_stamp(2), older_request,));
        assert!(coordinator.reserve_activation_causal(newer_owner, focus_stamp(3), newer_request,));
        assert_eq!(coordinator.winning_focus_target(), Some(newer));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some((newer_owner, focus_stamp(3), newer_request))
        );

        assert!(coordinator.cancel_activation_reservation(newer_owner));
        assert_eq!(coordinator.winning_focus_target(), Some(older));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some((older_owner, focus_stamp(2), older_request))
        );
        assert!(!coordinator.cancel_activation_reservation(newer_owner));
    }

    #[test]
    fn cancelling_an_older_native_reservation_cannot_roll_back_a_newer_owner() {
        let older = binding();
        let newer = second_binding();
        let older_owner = NativeCreateSagaId::new(43);
        let newer_owner = NativeCreateSagaId::new(44);
        let older_request = ViewportActivationRequest::tear_off_committed(
            older,
            PaneFocusDisposition::Set(ItemId::new(1)),
        );
        let newer_request = ViewportActivationRequest::tear_off_committed(
            newer,
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();

        assert!(coordinator.reserve_activation_causal(older_owner, focus_stamp(2), older_request,));
        assert!(coordinator.reserve_activation_causal(newer_owner, focus_stamp(3), newer_request,));
        assert!(coordinator.cancel_activation_reservation(older_owner));
        assert_eq!(coordinator.winning_focus_target(), Some(newer));
        assert_eq!(
            coordinator.winning_activation_reservation(),
            Some((newer_owner, focus_stamp(3), newer_request))
        );
    }

    #[test]
    fn native_activation_reservation_blocks_older_readiness_completion() {
        let older = binding();
        let newer = second_binding();
        let bindings_are_live = |candidate| candidate == older || candidate == newer;
        let items_are_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == newer.surface() && item == ItemId::new(2))
        };
        let older_request = ViewportActivationRequest::tear_off_committed(
            older,
            PaneFocusDisposition::Set(ItemId::new(1)),
        );
        let newer_request = ViewportActivationRequest::tear_off_committed(
            newer,
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Foreign),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );

        assert!(coordinator.reserve_activation_causal(
            NativeCreateSagaId::new(1),
            focus_stamp(2),
            older_request,
        ));
        assert!(coordinator.reserve_activation_causal(
            NativeCreateSagaId::new(2),
            focus_stamp(3),
            newer_request,
        ));

        let late_older = coordinator
            .request_reserved_activation(
                NativeCreateSagaId::new(1),
                older_request,
                focus_stamp(2),
                true,
                bindings_are_live,
                items_are_live,
            )
            .expect("an older completion must produce a diagnostic result");
        assert_eq!(
            late_older.outcome(),
            ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
                current: PaneFocusIntentGeneration::new(3),
            })
        );

        let current = coordinator
            .request_reserved_activation(
                NativeCreateSagaId::new(2),
                newer_request,
                focus_stamp(3),
                true,
                bindings_are_live,
                items_are_live,
            )
            .expect("the exact reserved completion must remain admissible");
        assert_eq!(
            current.outcome(),
            ActivationStartOutcome::RequestPlatformFocus { target: newer }
        );
    }

    #[test]
    fn isolated_staging_focus_completes_the_exact_reserved_claim_after_admission() {
        let current = binding();
        let staging = second_binding();
        let request = ViewportActivationRequest::tear_off_committed(
            staging,
            PaneFocusDisposition::Set(ItemId::new(2)),
        );
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(current)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(coordinator.reserve_activation_causal(
            NativeCreateSagaId::new(1),
            focus_stamp(2),
            request,
        ));

        let staging_focus = coordinator
            .publish_platform_focus_observation(
                focus_observation(2, GlobalFocusedWindow::Dock(staging)),
                focus_stamp(3),
                PlatformFocusRestoreGate::KnownAllReleased,
                |candidate| candidate == current,
                |candidate| candidate == current,
                |candidate| candidate == staging,
                |surface, item| surface == staging.surface() && item == ItemId::new(2),
            )
            .expect("a structurally exact staging focus fact must be retained");
        let FocusObservationTransition::Applied(staging_focus) = staging_focus else {
            panic!("staging focus must apply in its isolated lane");
        };
        assert!(staging_focus.pane_intent().is_none());

        let admitted = coordinator
            .request_reserved_activation(
                NativeCreateSagaId::new(1),
                request,
                focus_stamp(2),
                true,
                |candidate| candidate == current || candidate == staging,
                |surface, item| surface == staging.surface() && item == ItemId::new(2),
            )
            .expect("the admitted target must replay its release-time claim");
        let ActivationStartOutcome::PaneFocusReady { intent } = admitted.outcome() else {
            panic!("the already-focused admitted target must install its exact pane intent");
        };
        assert_eq!(intent.target(), staging);
        assert_eq!(intent.causal(), focus_stamp(2));
        assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(2)));
    }

    #[test]
    fn lifecycle_compensation_reasserts_a_pointer_winner_after_old_staging_focus() {
        let older = binding();
        let winner = second_binding();
        let binding_is_live = |candidate| candidate == older || candidate == winner;
        let item_is_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == winner.surface() && item == ItemId::new(2))
        };
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(winner)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let selected = coordinator
            .request_activation(
                ViewportActivationRequest::pointer_tab_gesture(
                    winner,
                    PanelFocus::Item(ItemId::new(2)),
                ),
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("the winning tab gesture must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent } = selected.outcome() else {
            panic!("the focused winning tab must receive a pane intent");
        };
        coordinator.mark_boundary_published();
        let _ = coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                winner,
                PanelFocus::Item(ItemId::new(2)),
            )
            .acknowledging(intent.id()),
            binding_is_live,
            item_is_live,
        );

        let old_completion = coordinator
            .request_activation(
                ViewportActivationRequest::tear_off_committed(
                    older,
                    PaneFocusDisposition::Set(ItemId::new(1)),
                ),
                focus_stamp(1),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("old completion must return a causal diagnostic");
        assert!(matches!(
            old_completion.outcome(),
            ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded { .. })
        ));
        let suppressed = coordinator.suppressed_tear_off_bindings();
        assert!(suppressed.contains(&older));

        let reasserted = coordinator
            .request_winning_lifecycle_compensation(true, binding_is_live, item_is_live)
            .expect("winner compensation must allocate")
            .expect("the latest focus claim names a docking target");
        assert_eq!(
            reasserted.outcome(),
            ActivationStartOutcome::RequestPlatformFocus { target: winner }
        );
        assert_eq!(
            coordinator
                .pending_activation()
                .map(PendingViewportActivation::request),
            Some(ViewportActivationRequest::pointer_tab_gesture(
                winner,
                PanelFocus::Item(ItemId::new(2)),
            ))
        );
        let effect = EffectId::new(41);
        assert_eq!(
            coordinator.attach_platform_focus_effect(reasserted.generation(), effect),
            FocusEffectAttachment::Applied
        );

        let late_auto_focus = coordinator
            .publish_platform_focus_observation(
                focus_observation(2, GlobalFocusedWindow::Dock(older)),
                focus_stamp(3),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |candidate| suppressed.contains(&candidate),
                item_is_live,
            )
            .expect("the delayed auto-focus fact must remain isolated from pane arbitration");
        let FocusObservationTransition::Applied(late_auto_focus) = late_auto_focus else {
            panic!("the delayed auto-focus fact must advance the provider stream");
        };
        assert!(late_auto_focus.pane_intent().is_none());
        assert!(coordinator.pending_activation().is_some());
        assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

        let winner_restored = coordinator
            .publish_platform_focus_observation(
                focus_observation(3, GlobalFocusedWindow::Dock(winner)),
                focus_stamp(4),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the post-create focus barrier must settle on its exact winner");
        let FocusObservationTransition::Applied(winner_restored) = winner_restored else {
            panic!("the winner focus fact must advance the provider stream");
        };
        assert_eq!(
            winner_restored.completed_activation(),
            Some(reasserted.generation())
        );
        assert!(coordinator.suppressed_tear_off_bindings().is_empty());

        let later_user_focus = coordinator
            .publish_platform_focus_observation(
                focus_observation(4, GlobalFocusedWindow::Dock(older)),
                focus_stamp(5),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("focus after the settled barrier must remain a normal platform fact");
        assert!(matches!(
            later_user_focus,
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.winning_focus_target(), Some(older));
    }

    #[test]
    fn newer_pointer_claim_retargets_an_unsettled_lifecycle_focus_barrier() {
        let (mut coordinator, older, old_winner, barrier, old_effect) =
            pending_lifecycle_focus_barrier();
        let new_winner = third_binding();
        let binding_is_live =
            |candidate| candidate == older || candidate == old_winner || candidate == new_winner;
        let item_is_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == old_winner.surface() && item == ItemId::new(2))
                || (surface == new_winner.surface() && item == ItemId::new(3))
        };

        let successor = coordinator
            .request_activation(
                ViewportActivationRequest::pointer_tab_gesture(
                    new_winner,
                    PanelFocus::Item(ItemId::new(3)),
                ),
                focus_stamp(4),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("the pointer claim must serialize after the lifecycle barrier");
        assert_eq!(
            successor
                .superseded()
                .map(PendingViewportActivation::generation),
            Some(barrier.generation())
        );
        assert_eq!(
            successor.outcome(),
            ActivationStartOutcome::RequestPlatformFocus { target: new_winner }
        );
        assert_eq!(coordinator.winning_focus_target(), Some(new_winner));
        assert!(coordinator.suppressed_tear_off_bindings().contains(&older));
        assert!(
            coordinator
                .suppressed_tear_off_bindings()
                .contains(&old_winner)
        );

        let successor_effect = EffectId::new(43);
        assert_eq!(
            coordinator.attach_platform_focus_effect(successor.generation(), successor_effect),
            FocusEffectAttachment::Applied
        );
        let late_old_focus = coordinator
            .publish_platform_focus_observation(
                FocusObservationEnvelope::new(
                    FocusObservationGeneration::new(2),
                    Authority::Known(GlobalFocusedWindow::Dock(old_winner)),
                    Authority::Known(Some(old_effect)),
                ),
                focus_stamp(5),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the predecessor effect may settle without regaining focus authority");
        assert!(matches!(
            late_old_focus,
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.winning_focus_target(), Some(new_winner));
        assert_eq!(
            coordinator
                .pending_activation()
                .map(PendingViewportActivation::generation),
            Some(successor.generation())
        );

        let completed = coordinator
            .publish_platform_focus_observation(
                FocusObservationEnvelope::new(
                    FocusObservationGeneration::new(3),
                    Authority::Known(GlobalFocusedWindow::Dock(new_winner)),
                    Authority::Known(Some(successor_effect)),
                ),
                focus_stamp(6),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the successor focus proof must settle the retargeted barrier");
        let FocusObservationTransition::Applied(completed) = completed else {
            panic!("the successor observation must advance the provider stream");
        };
        assert_eq!(
            completed.completed_activation(),
            Some(successor.generation())
        );
        assert_eq!(
            completed.pane_intent().map(PaneFocusIntent::target),
            Some(new_winner)
        );
        assert!(coordinator.suppressed_tear_off_bindings().is_empty());
    }

    #[test]
    fn provider_replacement_atomically_retires_an_unsettled_lifecycle_focus_barrier() {
        let (mut coordinator, older, _winner, barrier, _effect) = pending_lifecycle_focus_barrier();
        assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

        let cleanup = coordinator.revoke_platform_provider_authority();
        assert_eq!(
            cleanup.pending_activation_cancelled(),
            Some((
                barrier.generation(),
                ActivationCancellation::PlatformAuthorityRevoked,
            ))
        );
        assert!(coordinator.suppressed_tear_off_bindings().is_empty());

        let replacement_focus = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(older)),
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            replacement_focus,
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(
            coordinator
                .focus_observation()
                .map(|observation| *observation.focused()),
            Some(Authority::Known(GlobalFocusedWindow::Dock(older)))
        );
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Foreign),
            5,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Dock(older)),
            6,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert_eq!(coordinator.winning_focus_target(), Some(older));
    }

    #[test]
    fn failed_lifecycle_focus_barrier_reconciles_an_unchanged_current_focus_once() {
        let (mut coordinator, older, _winner, barrier, effect) = pending_lifecycle_focus_barrier();
        let isolated = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(older)),
            4,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(isolated, FocusObservationTransition::Applied(_)));
        assert_ne!(coordinator.winning_focus_target(), Some(older));

        assert!(matches!(
            coordinator.report_platform_focus_effect(
                effect,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
            FocusEffectReportTransition::Cancelled {
                activation,
                reason: ActivationCancellation::EffectFailed,
            } if activation == barrier.generation()
        ));
        assert!(coordinator.suppressed_tear_off_bindings().is_empty());

        let reconciled = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Dock(older)),
            5,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(reconciled, FocusObservationTransition::Applied(_)));
        assert_eq!(coordinator.winning_focus_target(), Some(older));

        let duplicate_target = publish_platform_focus(
            &mut coordinator,
            focus_observation(4, GlobalFocusedWindow::Dock(older)),
            6,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        assert!(matches!(
            duplicate_target,
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.winning_focus_target(), Some(older));
    }

    #[test]
    fn acknowledged_lifecycle_barrier_with_quarantined_target_reasserts_winner() {
        let (mut coordinator, older, winner, barrier, effect) = pending_lifecycle_focus_barrier();
        let binding_is_live = |candidate| candidate == older || candidate == winner;
        let item_is_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == winner.surface() && item == ItemId::new(2))
        };

        let transition = coordinator
            .publish_platform_focus_observation(
                FocusObservationEnvelope::new(
                    FocusObservationGeneration::new(2),
                    Authority::Known(GlobalFocusedWindow::Dock(older)),
                    Authority::Known(Some(effect)),
                ),
                focus_stamp(4),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the exact effect acknowledgement and focus fact must reduce atomically");
        let FocusObservationTransition::Applied(applied) = transition else {
            panic!("the acknowledged observation must advance the provider stream");
        };
        assert_eq!(
            applied.observed_effect().map(|observed| observed.effect()),
            Some(effect)
        );
        assert_eq!(
            applied.cancelled_activation(),
            Some((
                barrier.generation(),
                ActivationCancellation::EffectObservedWithoutTargetFocus,
            ))
        );
        assert_eq!(coordinator.winning_focus_target(), Some(winner));
        assert!(coordinator.suppressed_tear_off_bindings().contains(&older));
        assert!(matches!(
            coordinator
                .pending_activation()
                .map(PendingViewportActivation::platform_focus),
            Some(PendingPlatformFocus::EffectRequired)
        ));
    }

    #[test]
    fn unknown_exact_ack_preserves_the_complete_lifecycle_focus_lane() {
        let (mut coordinator, older, winner, barrier, effect) = pending_lifecycle_focus_barrier();
        let binding_is_live = |candidate| candidate == older || candidate == winner;
        let item_is_live = |surface, item| {
            (surface == older.surface() && item == ItemId::new(1))
                || (surface == winner.surface() && item == ItemId::new(2))
        };
        let reason = AuthorityUnavailableReason::NotReported;

        let unknown = coordinator
            .publish_platform_focus_observation(
                FocusObservationEnvelope::new(
                    FocusObservationGeneration::new(2),
                    Authority::Unknown(reason),
                    Authority::Known(Some(effect)),
                ),
                focus_stamp(4),
                PlatformFocusRestoreGate::Unknown,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the exact acknowledgement must survive unknown focus authority");
        let FocusObservationTransition::AuthorityRevoked {
            reason: revoked,
            cleanup,
            effect_settlement,
            ..
        } = unknown
        else {
            panic!("unknown focus must revoke only the provider focus namespace");
        };
        assert_eq!(revoked, FocusAuthorityRevocation::Unknown(reason));
        assert!(cleanup.pending_activation_cancelled().is_none());
        assert_eq!(
            effect_settlement
                .observed_effect()
                .map(ObservedPlatformFocusEffect::effect),
            Some(effect)
        );
        assert!(matches!(
            coordinator
                .pending_activation()
                .map(PendingViewportActivation::platform_focus),
            Some(PendingPlatformFocus::ObservedAwaitingTarget {
                effect: pending_effect,
                ..
            }) if pending_effect == effect
        ));
        assert_eq!(coordinator.winning_focus_target(), Some(winner));
        assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

        let late_old_target = coordinator
            .publish_platform_focus_observation(
                focus_observation(3, GlobalFocusedWindow::Dock(older)),
                focus_stamp(5),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the late isolated target must advance only the provider stream");
        assert!(matches!(
            late_old_target,
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.winning_focus_target(), Some(winner));
        let replacement = coordinator
            .pending_activation()
            .expect("the quarantined target must create a new winner reassertion");
        assert_ne!(replacement.generation(), barrier.generation());
        assert_eq!(
            replacement.platform_focus(),
            PendingPlatformFocus::EffectRequired
        );
        let replacement_effect = EffectId::new(44);
        assert_eq!(
            coordinator.attach_platform_focus_effect(replacement.generation(), replacement_effect,),
            FocusEffectAttachment::Applied
        );

        let completed = coordinator
            .publish_platform_focus_observation(
                FocusObservationEnvelope::new(
                    FocusObservationGeneration::new(4),
                    Authority::Known(GlobalFocusedWindow::Dock(winner)),
                    Authority::Known(Some(replacement_effect)),
                ),
                focus_stamp(6),
                PlatformFocusRestoreGate::KnownAllReleased,
                binding_is_live,
                binding_is_live,
                |_| false,
                item_is_live,
            )
            .expect("the later winner fact must complete the retained lane");
        let FocusObservationTransition::Applied(completed) = completed else {
            panic!("the winner fact must advance the replacement focus baseline");
        };
        assert_eq!(
            completed.completed_activation(),
            Some(replacement.generation())
        );
        assert!(coordinator.suppressed_tear_off_bindings().is_empty());
    }

    #[test]
    fn matching_unacknowledged_pane_observation_does_not_consume_intent() {
        let target = binding();
        let binding_is_live = |candidate| candidate == target;
        let item_is_live = |surface, item| surface == target.surface() && item == ItemId::new(1);
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(target)),
            1,
            PlatformFocusRestoreGate::KnownAllReleased,
        );
        let _ = coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                target,
                PanelFocus::Item(ItemId::new(1)),
            ),
            binding_is_live,
            item_is_live,
        );
        let activation = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                binding_is_live,
                item_is_live,
            )
            .expect("focused activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome() else {
            panic!("focused activation must install an intent");
        };
        coordinator.mark_boundary_published();

        assert_eq!(
            coordinator.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(2),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                ),
                binding_is_live,
                item_is_live,
            ),
            PaneFocusObservationTransition::Applied {
                cleared_intent: None,
            }
        );
        assert_eq!(coordinator.pending_pane_intent(), Some(intent));
        assert_eq!(
            coordinator.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(3),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                )
                .acknowledging(intent.id()),
                binding_is_live,
                item_is_live,
            ),
            PaneFocusObservationTransition::Applied {
                cleared_intent: Some(intent.id()),
            }
        );
    }

    #[test]
    fn focus_delta_reports_final_intent_surface_and_effect_state() {
        let target = binding();
        let before = ViewportFocusCoordinator::default();
        let before_effects = EffectLedger::default();
        let mut provider_authority =
            PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(1));
        let provider = provider_authority
            .create()
            .expect("test platform provider must be available");
        let mut after = before.clone();
        let _ = after
            .publish_focus_observation(
                focus_observation(1, GlobalFocusedWindow::Dock(target)),
                focus_stamp(1),
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1),
            )
            .expect("focus observation must apply");
        assert!(matches!(
            after.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(1),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                ),
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1),
            ),
            PaneFocusObservationTransition::Applied { .. }
        ));
        let activation = after
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1),
            )
            .expect("already focused activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome() else {
            panic!("already focused activation must publish an intent");
        };
        let mut after_effects = before_effects.clone();
        let effect = after_effects
            .request(PlatformEffect::RequestFocus {
                binding: target,
                after: None,
            })
            .expect("focus effect must allocate");
        let emitted = after_effects
            .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
            .expect("focus effect must have valid causal predecessors");
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0].id(), effect);

        let installed = FocusDelta::between(&before, &after, &before_effects, &after_effects, &[]);
        assert_installed_focus_delta(&installed, intent, effect);
        after.mark_boundary_published();

        let before_settlement = after.clone();
        let before_settlement_effects = after_effects.clone();
        assert!(matches!(
            after.publish_pane_focus_observation(
                PaneFocusObservation::new(
                    PaneFocusObservationGeneration::new(2),
                    target,
                    PanelFocus::Item(ItemId::new(1)),
                )
                .acknowledging(intent.id()),
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1),
            ),
            PaneFocusObservationTransition::Applied {
                cleared_intent: Some(cleared)
            } if cleared == intent.id()
        ));
        assert_eq!(
            after_effects.mark_observed_applied(
                provider,
                effect,
                target,
                InventoryGeneration::new(2),
            ),
            crate::effect::EffectTransition::Applied
        );
        let observed = ObservedPlatformFocusEffect {
            effect,
            binding: target,
            generation: FocusObservationGeneration::new(2),
            evidence: PlatformFocusEvidence::ExactEffectAcknowledgement,
        };
        let settled = FocusDelta::between(
            &before_settlement,
            &after,
            &before_settlement_effects,
            &after_effects,
            &[observed],
        );
        assert_settled_focus_delta(&settled, intent, observed, InventoryGeneration::new(2));

        let before_cleanup = after.clone();
        assert!(after.clear_surface(target.surface()).changed());
        let cleanup =
            FocusDelta::between(&before_cleanup, &after, &after_effects, &after_effects, &[]);
        assert_eq!(cleanup.surface_focus().len(), 1);
        assert!(cleanup.surface_focus()[0].state().after().is_none());
    }

    #[test]
    fn focus_delta_collapses_drop_then_close_to_the_final_pane_intent() {
        let target = binding();
        let before = ViewportFocusCoordinator::default();
        let effects = EffectLedger::default();
        let mut after = before.clone();
        let _ = after
            .publish_focus_observation(
                focus_observation(1, GlobalFocusedWindow::Dock(target)),
                focus_stamp(1),
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1) || item == ItemId::new(2),
            )
            .expect("focus observation must apply");

        let drop = after
            .request_activation(
                ViewportActivationRequest::drop_committed(
                    target,
                    PaneFocusDisposition::Set(ItemId::new(1)),
                ),
                focus_stamp(2),
                true,
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1) || item == ItemId::new(2),
            )
            .expect("drop activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: drop } = drop.outcome() else {
            panic!("focused drop target must publish a pane intent");
        };
        assert_eq!(drop.cause(), Some(ViewportActivationCause::DropCommitted));

        let close_request = close_request();
        let close = after
            .request_activation(
                ViewportActivationRequest::close_recovery(
                    target,
                    PaneFocusDisposition::Set(ItemId::new(2)),
                    close_request,
                ),
                focus_stamp(3),
                true,
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1) || item == ItemId::new(2),
            )
            .expect("close recovery must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: close } = close.outcome() else {
            panic!("focused close target must publish a pane intent");
        };
        assert_ne!(drop.id(), close.id());
        assert_eq!(close.source(), PaneFocusIntentSource::CloseRecovery);
        assert_eq!(
            close.cause(),
            Some(ViewportActivationCause::CloseRecovery {
                request: close_request
            })
        );

        let delta = FocusDelta::between(&before, &after, &effects, &effects, &[]);
        let pane = delta
            .pane_intent()
            .expect("the boundary must publish its final pane intent");
        assert_eq!(pane.before(), &None);
        assert_eq!(pane.after(), &Some(close));
        assert_ne!(pane.after(), &Some(drop));
    }
}
