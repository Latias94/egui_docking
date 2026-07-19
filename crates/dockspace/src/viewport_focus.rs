//! Adapter-neutral native viewport activation and pane-focus coordination.
//!
//! Platform focus and pane focus are separate facts. A selected tab never proves
//! that its pane owns UI focus, and requesting native-window activation never
//! proves that the request was applied. The coordinator therefore keeps exact
//! binding identities, provider generations, effect identities, and pane-focus
//! acknowledgements until their corresponding facts arrive.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::{
    EffectDispatchResult, EffectId, EffectLedger, EffectPhase, EffectRequest, PlatformEffect,
};
use crate::frame::{PanelFocus, ViewportCloseRequestId};
use crate::ids::{ItemId, SurfaceId};
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

/// Semantic origin of one explicit viewport activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportActivationCause {
    /// An application or user command explicitly activated the viewport.
    Explicit,
    /// A completed docking delivery selected the target viewport.
    DropCommitted,
    /// A completed native tear-off selected the newly created viewport.
    TearOffCommitted,
    /// A close transaction moved content into an existing target viewport.
    CloseRecovery { request: ViewportCloseRequestId },
}

impl ViewportActivationCause {
    #[must_use]
    pub const fn requests_platform_focus(self) -> bool {
        !matches!(self, Self::CloseRecovery { .. })
    }

    #[must_use]
    const fn pane_source(self) -> PaneFocusIntentSource {
        match self {
            Self::CloseRecovery { .. } => PaneFocusIntentSource::CloseRecovery,
            Self::Explicit | Self::DropCommitted | Self::TearOffCommitted => {
                PaneFocusIntentSource::ExplicitViewportActivation
            }
        }
    }

    #[must_use]
    const fn close_request(self) -> Option<ViewportCloseRequestId> {
        match self {
            Self::CloseRecovery { request } => Some(request),
            Self::Explicit | Self::DropCommitted | Self::TearOffCommitted => None,
        }
    }
}

/// Exact activation request created only after its semantic mutation succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewportActivationRequest {
    target: ViewportBinding,
    focus: PanelFocus,
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
        Self {
            target,
            focus,
            cause,
        }
    }

    #[must_use]
    pub const fn explicit(target: ViewportBinding, focus: PanelFocus) -> Self {
        Self::new(target, focus, ViewportActivationCause::Explicit)
    }

    #[must_use]
    pub(crate) const fn drop_committed(target: ViewportBinding, focus: PanelFocus) -> Self {
        Self::new(target, focus, ViewportActivationCause::DropCommitted)
    }

    #[must_use]
    pub(crate) const fn tear_off_committed(target: ViewportBinding, focus: PanelFocus) -> Self {
        Self::new(target, focus, ViewportActivationCause::TearOffCommitted)
    }

    #[must_use]
    #[doc(hidden)]
    pub const fn close_recovery(
        target: ViewportBinding,
        focus: PanelFocus,
        request: ViewportCloseRequestId,
    ) -> Self {
        Self::new(
            target,
            focus,
            ViewportActivationCause::CloseRecovery { request },
        )
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
    pub const fn cause(self) -> ViewportActivationCause {
        self.cause
    }
}

/// Source precedence for pane intents in one reducer generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneFocusIntentSource {
    PlatformActivation,
    ExplicitViewportActivation,
    CloseRecovery,
}

impl PaneFocusIntentSource {
    #[must_use]
    const fn priority(self) -> u8 {
        match self {
            Self::PlatformActivation => 0,
            Self::ExplicitViewportActivation => 1,
            Self::CloseRecovery => 2,
        }
    }
}

/// Core-derived gate for ordinary pane restoration after native focus changes.
///
/// Explicit viewport activations bypass this gate because they carry their own
/// exact pane-focus request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformFocusRestoreGate {
    NoAuthoritativeMouseDown,
    AuthoritativeMouseDown,
}

impl PlatformFocusRestoreGate {
    const fn allows_ordinary_restore(self) -> bool {
        matches!(self, Self::NoAuthoritativeMouseDown)
    }
}

/// One exact pane-focus command retained until an adapter observation confirms it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneFocusIntent {
    id: PaneFocusIntentId,
    generation: PaneFocusIntentGeneration,
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
        self.generation
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
    request: ViewportActivationRequest,
    observation_baseline: FocusObservationGeneration,
    platform_focus: PendingPlatformFocus,
}

/// Observe-only close recovery retained for diagnostics and exact cleanup.
///
/// This record is never promoted when its target later becomes focused. A newer
/// global focus observation or explicit activation removes it instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedObserveOnlyActivation {
    generation: ActivationGeneration,
    request: ViewportActivationRequest,
    observation_baseline: Option<FocusObservationGeneration>,
}

impl RecordedObserveOnlyActivation {
    #[must_use]
    pub const fn generation(self) -> ActivationGeneration {
        self.generation
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
    /// An explicit activation must emit `PlatformEffect::RequestFocus` for this binding.
    RequestPlatformFocus { target: ViewportBinding },
    /// A higher-priority same-generation pane intent remains authoritative.
    PaneIntentSuperseded,
    /// A pane intent was installed but its exact reveal precondition was rejected.
    PaneRevealRejected {
        intent: PaneFocusIntent,
        reason: PaneFocusRevealRejection,
    },
    /// Observe-only recovery was recorded without raising or later focusing its target.
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
    SupersededByActivation { replacement: ActivationGeneration },
    EffectObservedWithoutTargetFocus,
    EffectFailed,
    EffectUnsupported,
    StaleBinding,
    ItemUnavailable { item: ItemId },
    SurfaceRemoved,
    CloseRequestCleared { request: ViewportCloseRequestId },
}

/// State changes produced by one newly accepted global focus observation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusObservationApplied {
    observed_effect: Option<ObservedPlatformFocusEffect>,
    acknowledged_effect_settlement: Option<ObservedPlatformFocusEffect>,
    completed_activation: Option<ActivationGeneration>,
    cancelled_activation: Option<(ActivationGeneration, ActivationCancellation)>,
    pane: AppliedPaneFocus,
    cleared_observe_only_activation: Option<ActivationGeneration>,
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
    pub const fn observed_effect(self) -> Option<ObservedPlatformFocusEffect> {
        self.observed_effect
    }

    /// Returns an exact acknowledged focus effect settled independently of the active activation.
    #[must_use]
    pub const fn acknowledged_effect_settlement(self) -> Option<ObservedPlatformFocusEffect> {
        self.acknowledged_effect_settlement
    }

    pub(crate) fn discard_observed_effect(&mut self, effect: EffectId) {
        if self
            .observed_effect
            .is_some_and(|observed| observed.effect == effect)
        {
            self.observed_effect = None;
        }
    }

    pub(crate) fn record_acknowledged_effect_settlement(
        &mut self,
        observed: ObservedPlatformFocusEffect,
    ) {
        if self
            .observed_effect
            .is_some_and(|current| current.effect == observed.effect)
        {
            return;
        }
        self.acknowledged_effect_settlement = Some(observed);
    }

    #[must_use]
    pub const fn completed_activation(self) -> Option<ActivationGeneration> {
        self.completed_activation
    }

    #[must_use]
    pub const fn cancelled_activation(
        self,
    ) -> Option<(ActivationGeneration, ActivationCancellation)> {
        self.cancelled_activation
    }

    #[must_use]
    pub const fn pane_intent(self) -> Option<PaneFocusIntent> {
        match self.pane {
            AppliedPaneFocus::Intent(intent) => Some(intent),
            AppliedPaneFocus::None | AppliedPaneFocus::RevealRejected { .. } => None,
        }
    }

    #[must_use]
    pub const fn pane_reveal_rejection(
        self,
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
    pub const fn cleared_observe_only_activation(self) -> Option<ActivationGeneration> {
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
    EqualGenerationConflict {
        generation: FocusObservationGeneration,
    },
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
    generation: PaneFocusIntentGeneration,
    source: PaneFocusIntentSource,
    target: ViewportBinding,
    focus: PanelFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneIntentInstall {
    generation: PaneFocusIntentGeneration,
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
    panel_record_removed: bool,
    pane_observation_removed: bool,
    pending_activation_cancelled: Option<(ActivationGeneration, ActivationCancellation)>,
    pending_intent_removed: Option<PaneFocusIntentId>,
    observe_only_activation_removed: Option<ActivationGeneration>,
}

impl ViewportFocusCleanup {
    #[must_use]
    pub const fn changed(self) -> bool {
        self.panel_record_removed
            || self.pane_observation_removed
            || self.pending_activation_cancelled.is_some()
            || self.pending_intent_removed.is_some()
            || self.observe_only_activation_removed.is_some()
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
                before.last_focus_observation,
                after.last_focus_observation,
            ),
            pending_activation: FocusValueChange::between(
                before.pending_activation,
                after.pending_activation,
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
    last_focus_observation: Option<FocusObservationEnvelope>,
    destroyed_previous_focus: Option<ViewportBinding>,
    last_activation: ActivationGeneration,
    last_pane_intent: PaneFocusIntentId,
    pane_intent_watermark: Option<PaneIntentWatermark>,
    pending_activation: Option<PendingViewportActivation>,
    recorded_observe_only_activation: Option<RecordedObserveOnlyActivation>,
    pending_pane_intent: Option<PaneFocusIntent>,
    panel_focus_by_surface: BTreeMap<SurfaceId, PanelFocusRecord>,
    pane_observation_by_surface: BTreeMap<SurfaceId, PanelFocusObservationRecord>,
}

impl ViewportFocusCoordinator {
    #[must_use]
    pub const fn focus_observation(&self) -> Option<FocusObservationEnvelope> {
        self.last_focus_observation
    }

    #[must_use]
    pub const fn pending_activation(&self) -> Option<PendingViewportActivation> {
        self.pending_activation
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
        pane_generation: PaneFocusIntentGeneration,
        can_request_platform_focus: bool,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Result<ActivationStart, ViewportFocusError> {
        let mut candidate = self.clone();
        let start = candidate.request_activation_in_place(
            request,
            pane_generation,
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
        pane_generation: PaneFocusIntentGeneration,
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
            request.focus,
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

        let observation = self.last_focus_observation;
        if !request.cause.requests_platform_focus() {
            return self.request_observe_only_activation(
                generation,
                request,
                pane_generation,
                observation,
            );
        }

        let superseded = self.pending_activation.take();
        self.supersede_recovery_intent();
        self.request_platform_activation(
            generation,
            request,
            pane_generation,
            observation,
            superseded,
            can_request_platform_focus,
        )
    }

    fn request_observe_only_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        pane_generation: PaneFocusIntentGeneration,
        observation: Option<FocusObservationEnvelope>,
    ) -> Result<ActivationStart, ViewportFocusError> {
        if let Some(observation) = observation
            && Self::observation_focuses(observation, request.target)
        {
            self.recorded_observe_only_activation = None;
            let intent = self.install_pane_intent(PaneIntentInstall {
                generation: pane_generation,
                activation: Some(generation),
                target: request.target,
                focus: request.focus,
                source: request.cause.pane_source(),
                cause: Some(request.cause),
                focus_observation_baseline: observation.generation,
            })?;
            return Ok(ActivationStart {
                generation,
                superseded: None,
                outcome: Self::pane_intent_outcome(intent),
            });
        }

        let record = RecordedObserveOnlyActivation {
            generation,
            request,
            observation_baseline: observation.map(|observation| observation.generation),
        };
        self.recorded_observe_only_activation = Some(record);
        Ok(ActivationStart {
            generation,
            superseded: None,
            outcome: ActivationStartOutcome::ObserveOnlyRecorded { record },
        })
    }

    fn request_platform_activation(
        &mut self,
        generation: ActivationGeneration,
        request: ViewportActivationRequest,
        pane_generation: PaneFocusIntentGeneration,
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
            let intent = self.install_pane_intent(PaneIntentInstall {
                generation: pane_generation,
                activation: Some(generation),
                target: request.target,
                focus: request.focus,
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

        self.pending_activation = Some(PendingViewportActivation {
            generation,
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
        let Some(pending) = self.pending_activation.as_mut() else {
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
        let Some(pending) = self.pending_activation else {
            return FocusEffectReportTransition::StaleEffect;
        };
        let Some(expected) = pending.platform_focus.effect() else {
            return FocusEffectReportTransition::StaleEffect;
        };
        if expected != effect {
            return FocusEffectReportTransition::StaleEffect;
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
                self.pending_activation = Some(PendingViewportActivation {
                    platform_focus: PendingPlatformFocus::Indeterminate { effect, reason },
                    ..pending
                });
                FocusEffectReportTransition::Indeterminate {
                    activation: pending.generation,
                }
            }
            EffectDispatchResult::DispatchFailed(_) => {
                self.pending_activation = None;
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectFailed,
                }
            }
            EffectDispatchResult::Unsupported(_) => {
                self.pending_activation = None;
                FocusEffectReportTransition::Cancelled {
                    activation: pending.generation,
                    reason: ActivationCancellation::EffectUnsupported,
                }
            }
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
        pane_generation: PaneFocusIntentGeneration,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            pane_generation,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    pub(crate) fn publish_platform_focus_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        pane_generation: PaneFocusIntentGeneration,
        restore_gate: PlatformFocusRestoreGate,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        let mut candidate = self.clone();
        let transition = candidate.publish_focus_observation_in_place(
            observation,
            pane_generation,
            restore_gate,
            is_current_binding,
            item_is_on_surface,
        )?;
        *self = candidate;
        Ok(transition)
    }

    /// Arms one-shot ordinary focus-restoration suppression when the provider
    /// destroys the exact window which most recently owned global focus.
    pub(crate) fn observe_destroyed_binding(&mut self, binding: ViewportBinding) -> bool {
        let was_last_focused = self.last_focus_observation.is_some_and(|observation| {
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

    fn publish_focus_observation_in_place(
        &mut self,
        observation: FocusObservationEnvelope,
        pane_generation: PaneFocusIntentGeneration,
        restore_gate: PlatformFocusRestoreGate,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> Result<FocusObservationTransition, ViewportFocusError> {
        if let Some(current) = self.last_focus_observation {
            if observation.generation < current.generation {
                return Ok(FocusObservationTransition::Stale {
                    current: current.generation,
                });
            }
            if observation.generation == current.generation {
                return Ok(if observation == current {
                    FocusObservationTransition::Duplicate
                } else {
                    FocusObservationTransition::EqualGenerationConflict {
                        generation: observation.generation,
                    }
                });
            }
        }

        let previous = self.last_focus_observation;
        self.last_focus_observation = Some(observation);
        let mut applied = FocusObservationApplied {
            cleared_observe_only_activation: self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation),
            ..FocusObservationApplied::default()
        };

        if self.pending_pane_intent.is_some_and(|intent| {
            observation.generation > intent.focus_observation_baseline
                && matches!(
                    observation.focused,
                    Authority::Known(focused)
                        if focused != GlobalFocusedWindow::Dock(intent.target)
                )
        }) {
            self.pending_pane_intent = None;
        }

        let focus_changed = previous.map(|previous| previous.focused) != Some(observation.focused);
        let focused_current_binding = if focus_changed
            && let Authority::Known(GlobalFocusedWindow::Dock(binding)) = observation.focused
            && is_current_binding(binding)
        {
            Some(binding)
        } else {
            None
        };
        let suppress_destroyed_previous_restore =
            focused_current_binding.is_some() && self.destroyed_previous_focus.take().is_some();

        self.reduce_pending_activation_observation(
            observation,
            pane_generation,
            is_current_binding,
            item_is_on_surface,
            &mut applied,
        )?;

        if applied.pane_intent().is_none()
            && focus_changed
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
                    generation: pane_generation,
                    activation: None,
                    target: binding,
                    focus,
                    source: PaneFocusIntentSource::PlatformActivation,
                    cause: None,
                    focus_observation_baseline: observation.generation,
                })?);
            }
        }

        Ok(FocusObservationTransition::Applied(Box::new(applied)))
    }

    fn reduce_pending_activation_observation(
        &mut self,
        observation: FocusObservationEnvelope,
        pane_generation: PaneFocusIntentGeneration,
        is_current_binding: impl Fn(ViewportBinding) -> bool,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        let Some(pending) = self.pending_activation else {
            return Ok(());
        };
        if !is_current_binding(pending.request.target) {
            self.cancel_pending_activation(pending, ActivationCancellation::StaleBinding, applied);
            return Ok(());
        }
        if let PanelFocus::Item(item) = pending.request.focus
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
            applied.observed_effect = Some(ObservedPlatformFocusEffect {
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
            self.complete_pending_activation(pending, observation, pane_generation, applied)?;
        } else if acknowledged {
            match observation.focused {
                Authority::Known(_) => self.cancel_pending_activation(
                    pending,
                    ActivationCancellation::EffectObservedWithoutTargetFocus,
                    applied,
                ),
                Authority::Unknown(_) => {
                    self.pending_activation = Some(PendingViewportActivation {
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
            self.cancel_pending_activation(
                pending,
                ActivationCancellation::EffectObservedWithoutTargetFocus,
                applied,
            );
        }
        Ok(())
    }

    fn complete_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        observation: FocusObservationEnvelope,
        pane_generation: PaneFocusIntentGeneration,
        applied: &mut FocusObservationApplied,
    ) -> Result<(), ViewportFocusError> {
        self.pending_activation = None;
        applied.completed_activation = Some(pending.generation);
        applied.set_pane_intent(self.install_pane_intent(PaneIntentInstall {
            generation: pane_generation,
            activation: Some(pending.generation),
            target: pending.request.target,
            focus: pending.request.focus,
            source: pending.request.cause.pane_source(),
            cause: Some(pending.request.cause),
            focus_observation_baseline: observation.generation,
        })?);
        Ok(())
    }

    fn cancel_pending_activation(
        &mut self,
        pending: PendingViewportActivation,
        reason: ActivationCancellation,
        applied: &mut FocusObservationApplied,
    ) {
        self.pending_activation = None;
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
            Some(intent_id)
        } else {
            if self.pending_pane_intent.is_some_and(|intent| {
                intent.target == observation.binding
                    && intent
                        .pane_observation_baseline
                        .is_none_or(|baseline| observation.generation > baseline)
            }) {
                self.pending_pane_intent = None;
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
        if self
            .pending_activation
            .is_some_and(|pending| pending.request.target == binding)
            && let Some(pending) = self.pending_activation.take()
        {
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::StaleBinding));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.target == binding)
        {
            cleanup.pending_intent_removed =
                self.pending_pane_intent.take().map(|intent| intent.id);
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target == binding)
        {
            cleanup.observe_only_activation_removed = self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation);
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
        if self
            .pending_activation
            .is_some_and(|pending| pending.request.target.surface() == surface)
            && let Some(pending) = self.pending_activation.take()
        {
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::SurfaceRemoved));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.target.surface() == surface)
        {
            cleanup.pending_intent_removed =
                self.pending_pane_intent.take().map(|intent| intent.id);
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target.surface() == surface)
        {
            cleanup.observe_only_activation_removed = self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation);
        }
        cleanup
    }

    /// Removes focus history and commands for an item which no longer exists on its surface.
    pub fn clear_item(&mut self, item: ItemId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
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
            .pending_activation
            .is_some_and(|pending| pending.request.focus == PanelFocus::Item(item))
            && let Some(pending) = self.pending_activation.take()
        {
            cleanup.pending_activation_cancelled = Some((
                pending.generation,
                ActivationCancellation::ItemUnavailable { item },
            ));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.focus == PanelFocus::Item(item))
        {
            cleanup.pending_intent_removed =
                self.pending_pane_intent.take().map(|intent| intent.id);
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.focus == PanelFocus::Item(item))
        {
            cleanup.observe_only_activation_removed = self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation);
        }
        cleanup
    }

    /// Clears only activation state derived from one close request identity.
    pub fn clear_close_request(&mut self, request: ViewportCloseRequestId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        if self
            .pending_activation
            .is_some_and(|pending| pending.request.cause.close_request() == Some(request))
            && let Some(pending) = self.pending_activation.take()
        {
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
            cleanup.pending_intent_removed =
                self.pending_pane_intent.take().map(|intent| intent.id);
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.cause.close_request() == Some(request))
        {
            cleanup.observe_only_activation_removed = self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation);
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
        true
    }

    pub(crate) fn reconcile_authority(
        &mut self,
        surface_exists: impl Fn(SurfaceId) -> bool + Copy,
        is_current_binding: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();

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
                && is_current_binding(observation.binding)
                && match observation.focus {
                    PanelFocus::Item(item) => item_is_on_surface(*surface, item),
                    PanelFocus::None => true,
                };
            cleanup.pane_observation_removed |= !keep;
            keep
        });

        if let Some(pending) = self.pending_activation
            && let Some(reason) = Self::stale_activation_reason(
                pending.request,
                surface_exists,
                is_current_binding,
                item_is_on_surface,
            )
        {
            self.pending_activation = None;
            cleanup.pending_activation_cancelled = Some((pending.generation, reason));
        }
        if self.pending_pane_intent.is_some_and(|intent| {
            !surface_exists(intent.target.surface())
                || !is_current_binding(intent.target)
                || matches!(
                    intent.focus,
                    PanelFocus::Item(item)
                        if !item_is_on_surface(intent.target.surface(), item)
                )
        }) {
            cleanup.pending_intent_removed =
                self.pending_pane_intent.take().map(|intent| intent.id);
        }
        if self.recorded_observe_only_activation.is_some_and(|record| {
            Self::stale_activation_reason(
                record.request,
                surface_exists,
                is_current_binding,
                item_is_on_surface,
            )
            .is_some()
        }) {
            cleanup.observe_only_activation_removed = self
                .recorded_observe_only_activation
                .take()
                .map(|record| record.generation);
        }
        if self.pane_intent_watermark.is_some_and(|watermark| {
            !surface_exists(watermark.target.surface())
                || !is_current_binding(watermark.target)
                || matches!(
                    watermark.focus,
                    PanelFocus::Item(item)
                        if !item_is_on_surface(watermark.target.surface(), item)
                )
        }) {
            self.pane_intent_watermark = None;
        }

        cleanup
    }

    /// Clears workspace-bound focus authority while preserving monotonic identities.
    pub fn reconcile_workspace_replacement(&mut self) {
        self.last_focus_observation = None;
        self.destroyed_previous_focus = None;
        self.pane_intent_watermark = None;
        self.pending_activation = None;
        self.recorded_observe_only_activation = None;
        self.pending_pane_intent = None;
        self.panel_focus_by_surface.clear();
        self.pane_observation_by_surface.clear();
    }

    fn validate_focus_target(
        target: ViewportBinding,
        focus: PanelFocus,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> Option<ActivationSuppression> {
        if !is_current_binding(target) {
            return Some(ActivationSuppression::StaleBinding);
        }
        if let PanelFocus::Item(item) = focus
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
        if let PanelFocus::Item(item) = request.focus
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
            generation,
            activation,
            target,
            focus,
            source,
            cause,
            focus_observation_baseline,
        } = install;
        if let Some(watermark) = self.pane_intent_watermark {
            if generation < watermark.generation
                || (generation == watermark.generation
                    && source.priority() < watermark.source.priority())
            {
                return Ok(None);
            }
            if generation == watermark.generation
                && source == watermark.source
                && (target != watermark.target || focus != watermark.focus)
            {
                return Ok(None);
            }
            if generation == watermark.generation
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
            generation,
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
            generation,
            source,
            target,
            focus,
        });
        self.pending_pane_intent = Some(intent);
        Ok(Some(intent))
    }

    fn supersede_recovery_intent(&mut self) {
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.source == PaneFocusIntentSource::CloseRecovery)
        {
            self.pending_pane_intent = None;
        }
        self.recorded_observe_only_activation = None;
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
    use crate::ids::WorkspaceEpoch;
    use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

    fn binding() -> ViewportBinding {
        ViewportBinding::new(
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        )
    }

    fn second_binding() -> ViewportBinding {
        ViewportBinding::new(
            WorkspaceEpoch::new(1),
            SurfaceId::new(2),
            WindowToken::new(2),
            WindowIncarnation::new(2),
        )
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
                PaneFocusIntentGeneration::new(pane_generation),
                gate,
                |candidate| bindings.contains(&candidate),
                |surface, item| {
                    (surface == SurfaceId::new(1) && item == ItemId::new(1))
                        || (surface == SurfaceId::new(2) && item == ItemId::new(2))
                },
            )
            .expect("focus identity domains must remain available")
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
                inventory_generation: InventoryGeneration::new(1),
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
                PaneFocusIntentGeneration::new(1),
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
        coordinator.last_focus_observation = Some(FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Known(GlobalFocusedWindow::Dock(target)),
            Authority::Known(None),
        ));
        coordinator.exhaust_pane_intent_ids();
        let before = coordinator.clone();
        assert_eq!(
            coordinator.request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::None),
                PaneFocusIntentGeneration::new(1),
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
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        coordinator
            .panel_focus_by_surface
            .insert(target.surface(), PanelFocusRecord::Item(ItemId::new(2)));

        let suppressed = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(target)),
            2,
            PlatformFocusRestoreGate::AuthoritativeMouseDown,
        );
        let FocusObservationTransition::Applied(suppressed) = suppressed else {
            panic!("new focus must be accepted");
        };
        assert!(suppressed.pane_intent().is_none());

        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        let restored = publish_platform_focus(
            &mut coordinator,
            focus_observation(4, GlobalFocusedWindow::Dock(target)),
            4,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
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
    fn destroyed_previous_focus_suppresses_one_ordinary_restore() {
        let target = second_binding();
        let mut coordinator = ViewportFocusCoordinator::default();
        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(1, GlobalFocusedWindow::Dock(binding())),
            1,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        coordinator
            .panel_focus_by_surface
            .insert(target.surface(), PanelFocusRecord::Item(ItemId::new(2)));
        assert!(coordinator.observe_destroyed_binding(binding()));

        let suppressed = publish_platform_focus(
            &mut coordinator,
            focus_observation(2, GlobalFocusedWindow::Dock(target)),
            2,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        let FocusObservationTransition::Applied(suppressed) = suppressed else {
            panic!("fallback focus must be accepted");
        };
        assert!(suppressed.pane_intent().is_none());

        let _ = publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Foreign),
            3,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        let restored = publish_platform_focus(
            &mut coordinator,
            focus_observation(4, GlobalFocusedWindow::Dock(target)),
            4,
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
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
            PlatformFocusRestoreGate::NoAuthoritativeMouseDown,
        );
        assert!(coordinator.observe_destroyed_binding(binding()));
        let start = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
                PaneFocusIntentGeneration::new(2),
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
            PlatformFocusRestoreGate::AuthoritativeMouseDown,
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
    fn focus_delta_reports_final_intent_surface_and_effect_state() {
        let target = binding();
        let before = ViewportFocusCoordinator::default();
        let before_effects = EffectLedger::default();
        let mut after = before.clone();
        let _ = after
            .publish_focus_observation(
                focus_observation(1, GlobalFocusedWindow::Dock(target)),
                PaneFocusIntentGeneration::new(1),
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
                PaneFocusIntentGeneration::new(2),
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

        let installed = FocusDelta::between(&before, &after, &before_effects, &after_effects, &[]);
        assert_installed_focus_delta(&installed, intent, effect);

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
            after_effects.mark_observed_applied(effect, target, InventoryGeneration::new(1),),
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
        assert_settled_focus_delta(&settled, intent, observed);

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
                PaneFocusIntentGeneration::new(1),
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1) || item == ItemId::new(2),
            )
            .expect("focus observation must apply");

        let drop = after
            .request_activation(
                ViewportActivationRequest::drop_committed(target, PanelFocus::Item(ItemId::new(1))),
                PaneFocusIntentGeneration::new(2),
                true,
                |candidate| candidate == target,
                |_, item| item == ItemId::new(1) || item == ItemId::new(2),
            )
            .expect("drop activation must allocate");
        let ActivationStartOutcome::PaneFocusReady { intent: drop } = drop.outcome() else {
            panic!("focused drop target must publish a pane intent");
        };
        assert_eq!(drop.cause(), Some(ViewportActivationCause::DropCommitted));

        let close_request = ViewportCloseRequestId::new(7);
        let close = after
            .request_activation(
                ViewportActivationRequest::close_recovery(
                    target,
                    PanelFocus::Item(ItemId::new(2)),
                    close_request,
                ),
                PaneFocusIntentGeneration::new(3),
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
