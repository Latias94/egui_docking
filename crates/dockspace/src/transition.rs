//! Atomic engine transition records returned after successful publication.

use crate::command::CommandOutcome;
use crate::effect::EffectRequest;
use crate::effect::{EffectId, EffectTransition};
use crate::error::CommandError;
use crate::event::WorkspaceEvent;
use crate::frame::{
    NativeCreateSagaId, ViewportCloseDecisionRejection, ViewportCloseRequestId,
    ViewportFrameTransition, ViewportReconciliation,
};
use crate::ids::{InputSequence, WorkspaceEpoch, WorkspaceRevision};
use crate::interaction::{InteractionEvent, InteractionOutcome};
use crate::scene::{SceneBuildError, SceneStamp};
use crate::viewport::ViewportBinding;
use crate::viewport_focus::FocusDelta;

/// Version of all workspace and policy state used to derive semantic input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WorkspaceVersion {
    epoch: WorkspaceEpoch,
    revision: WorkspaceRevision,
}

impl WorkspaceVersion {
    /// Creates a version from distinct replacement and mutation counters.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, revision: WorkspaceRevision) -> Self {
        Self { epoch, revision }
    }

    /// Returns the replacement epoch.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the mutation revision within the current epoch.
    #[must_use]
    pub const fn revision(self) -> WorkspaceRevision {
        self.revision
    }
}

/// Normative source-class priority used by the single reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputPriority {
    /// Workspace replacement and native lifecycle control.
    LifecycleControl,
    /// Authoritative facts supplied by a platform provider.
    PlatformObservation,
    /// Checked commands and application policy changes.
    ApplicationCommand,
    /// Semantic input produced while painting a sealed scene.
    RendererIntent,
    /// Validation and other state-neutral upkeep.
    Maintenance,
}

/// Result of reducing one sequenced input.
#[derive(Debug, Clone, PartialEq)]
pub enum InputOutcome {
    /// An existing adapter window was bound to a logical surface.
    ViewportRegistered {
        /// Complete core-owned binding identity.
        binding: ViewportBinding,
    },
    /// Viewport registration named no current logical surface.
    ViewportRegistrationRejected {
        /// Missing stable logical surface.
        surface: crate::ids::SurfaceId,
    },
    /// One complete platform fact snapshot was atomically published.
    PlatformSnapshotPublished {
        /// Structured capability, inventory, and route transition.
        transition: ViewportFrameTransition,
        /// Single global native-focus observation reduced in this same core input.
        focus: crate::viewport_focus::FocusObservationTransition,
        /// Explicit activations created by lifecycle commits in this same core input.
        activations: Vec<crate::viewport_focus::ActivationStart>,
    },
    /// Platform facts from an earlier workspace epoch were consumed without mutation.
    PlatformSnapshotStale {
        expected_epoch: WorkspaceEpoch,
        current_epoch: WorkspaceEpoch,
    },
    /// A correlated adapter dispatch result was reduced.
    PlatformEffectReported {
        effect: EffectId,
        transition: EffectTransition,
        /// Activation state change when the effect belongs to the global focus lane.
        focus: Option<crate::viewport_focus::FocusEffectReportTransition>,
    },
    /// One explicit viewport activation request was reduced.
    ViewportActivationRequested {
        activation: crate::viewport_focus::ActivationStart,
    },
    /// A caller attempted to inject a core-owned lifecycle activation cause.
    ViewportActivationRejected {
        request: crate::viewport_focus::ViewportActivationRequest,
    },
    /// One exact pane-focus observation was reduced.
    PaneFocusObservationPublished {
        transition: crate::viewport_focus::PaneFocusObservationTransition,
    },
    /// Pane focus from an older workspace epoch was consumed without mutation.
    PaneFocusObservationStale {
        expected_epoch: WorkspaceEpoch,
        current_epoch: WorkspaceEpoch,
    },
    /// One exact close request was decided without mutating topology.
    ViewportCloseDecided {
        request: ViewportCloseRequestId,
        effect: EffectId,
    },
    /// An accepted close was rejected and the window was explicitly held.
    ViewportCloseDecisionRejected {
        request: ViewportCloseRequestId,
        reason: ViewportCloseDecisionRejection,
        hold_effect: EffectId,
    },
    /// One unresolved native-create saga was explicitly cancelled.
    NativeCreateCancelled {
        saga: NativeCreateSagaId,
        /// Immediate compensation when the child had already become observable.
        compensation: Option<EffectId>,
    },
    /// One definitively failed cleanup was replaced by a new exact-once effect.
    ViewportCleanupRetried {
        failed_effect: EffectId,
        retry: EffectId,
    },
    /// The complete workspace was replaced and all older derived state became stale.
    WorkspaceReplaced {
        /// Version before replacement.
        before: WorkspaceVersion,
        /// Version after replacement.
        after: WorkspaceVersion,
        /// Exact native binding invalidation and cleanup summary.
        reconciliation: ViewportReconciliation,
    },
    /// One checked command committed or produced a valid no-op.
    CommandProcessed {
        /// Structured command result.
        outcome: CommandOutcome,
        /// Whether the complete workspace changed.
        changed: bool,
        /// Version after processing this input.
        version: WorkspaceVersion,
    },
    /// A checked command was deterministically rejected and consumed.
    CommandRejected {
        /// Typed reason the command could not apply to the candidate state.
        error: CommandError,
        /// Published version, unchanged by this input.
        version: WorkspaceVersion,
    },
    /// Application policy was replaced or found equal.
    PolicyReplaced {
        /// Whether policy state changed.
        changed: bool,
        /// Version after processing this input.
        version: WorkspaceVersion,
    },
    /// One immutable scene generation was sealed and published.
    ScenePublished {
        /// Exact workspace and scene generation stamp.
        stamp: SceneStamp,
        /// Number of surfaces with complete acknowledged scene facts.
        ready_surfaces: usize,
        /// Number of frozen roster surfaces published as non-interactive bootstrap scenes.
        bootstrap_surfaces: usize,
    },
    /// Malformed scene facts were deterministically rejected and consumed.
    SceneRejected {
        /// Typed reason the candidate scene could not be sealed.
        error: SceneBuildError,
    },
    /// One renderer interaction intent was reduced.
    InteractionProcessed {
        /// Structured interaction state-machine result.
        outcome: InteractionOutcome,
        /// Durable workspace version after processing the intent.
        version: WorkspaceVersion,
    },
    /// A maintenance validation completed without mutation.
    WorkspaceValidated {
        /// Version which was validated.
        version: WorkspaceVersion,
    },
    /// Input derived from an old state was rejected without mutation.
    StaleRejected {
        /// Version carried by the input.
        expected: WorkspaceVersion,
        /// Shared state version accepted for application inputs in this boundary.
        accepted_base: WorkspaceVersion,
    },
}

/// One input and its outcome in normative reduction order.
#[derive(Debug, Clone, PartialEq)]
pub struct ReducedInput {
    sequence: InputSequence,
    priority: InputPriority,
    outcome: InputOutcome,
}

impl ReducedInput {
    pub(crate) const fn new(
        sequence: InputSequence,
        priority: InputPriority,
        outcome: InputOutcome,
    ) -> Self {
        Self {
            sequence,
            priority,
            outcome,
        }
    }

    /// Returns the writer-assigned sequence.
    #[must_use]
    pub const fn sequence(&self) -> InputSequence {
        self.sequence
    }

    /// Returns the normative source-class priority.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        self.priority
    }

    /// Returns the structured reduction outcome.
    #[must_use]
    pub const fn outcome(&self) -> &InputOutcome {
        &self.outcome
    }
}

/// Complete result of one successfully published engine boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineTransition {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    reduced: Vec<ReducedInput>,
    events: Vec<WorkspaceEvent>,
    interaction_events: Vec<InteractionEvent>,
    platform_effects: Vec<EffectRequest>,
    focus_delta: FocusDelta,
    published_state_changed: bool,
}

pub(crate) struct EngineTransitionParts {
    pub(crate) before: WorkspaceVersion,
    pub(crate) after: WorkspaceVersion,
    pub(crate) reduced: Vec<ReducedInput>,
    pub(crate) events: Vec<WorkspaceEvent>,
    pub(crate) interaction_events: Vec<InteractionEvent>,
    pub(crate) platform_effects: Vec<EffectRequest>,
    pub(crate) focus_delta: FocusDelta,
    pub(crate) published_state_changed: bool,
}

impl EngineTransition {
    pub(crate) fn new(parts: EngineTransitionParts) -> Self {
        let EngineTransitionParts {
            before,
            after,
            reduced,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            published_state_changed,
        } = parts;
        Self {
            before,
            after,
            reduced,
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            published_state_changed,
        }
    }

    /// Returns the state version before reduction.
    #[must_use]
    pub const fn before(&self) -> WorkspaceVersion {
        self.before
    }

    /// Returns the published state version.
    #[must_use]
    pub const fn after(&self) -> WorkspaceVersion {
        self.after
    }

    /// Returns inputs in normative reduction order.
    #[must_use]
    pub fn reduced_inputs(&self) -> &[ReducedInput] {
        &self.reduced
    }

    /// Returns events generated only after the candidate committed.
    #[must_use]
    pub fn events(&self) -> &[WorkspaceEvent] {
        &self.events
    }

    /// Returns committed transient interaction events.
    #[must_use]
    pub fn interaction_events(&self) -> &[InteractionEvent] {
        &self.interaction_events
    }

    /// Returns exact platform effects emitted once after the candidate committed.
    #[must_use]
    pub fn platform_effects(&self) -> &[EffectRequest] {
        &self.platform_effects
    }

    /// Returns the adapter-facing net focus change for this atomic boundary.
    #[must_use]
    pub const fn focus_delta(&self) -> &FocusDelta {
        &self.focus_delta
    }

    /// Returns whether any durable or policy state changed.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.before != self.after
    }

    /// Returns whether any published durable, scene, or interaction state changed.
    ///
    /// Consuming inputs or advancing the private writer sequence alone does not
    /// count as a published change.
    #[must_use]
    pub const fn published_state_changed(&self) -> bool {
        self.published_state_changed
    }
}
