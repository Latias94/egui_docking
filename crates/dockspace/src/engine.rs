//! Single-writer input queue and atomic headless state reducer.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::command::{
    CommandOutcome, MovePayload, RootContent, RootPresentationTarget, WorkspaceCommand,
};
use crate::drop_resolver::{DropResolution, DropResolutionError, resolve_drop};
use crate::effect::{EffectResult, EffectTransition};
use crate::error::TransactionError;
use crate::event::{WorkspaceEvent, WorkspaceEventKind};
use crate::frame::{
    NativeCreateSagaId, ViewportCloseDecision, ViewportCloseRequestId, ViewportCoordinator,
    ViewportCoordinatorError,
};
use crate::graph::Workspace;
use crate::ids::{InputSequence, WorkspaceRevision};
use crate::intent::{
    Authority, PointerButtonState, RendererIntent, TargetAuthority, TearOffRequest,
};
use crate::interaction::{
    InteractionCancelReason, InteractionCounterError, InteractionDelivery, InteractionEvent,
    InteractionEventKind, InteractionOutcome, InteractionRejection, InteractionState,
    InteractionStatus, PreparedNativeTearOff, PreviewProof, PreviewResolutionStatus, PreviewVisual,
    WorkspaceDeliveryKind,
};
use crate::platform::{PlatformCapability, PlatformSnapshot};
use crate::policy::{DockPolicy, TearOffPresentation};
use crate::scene::{
    BuildingScene, SceneBuildError, SceneGeneration, SceneStamp, SealedScene, SurfaceScene,
};
use crate::transaction::WorkspaceTransaction;
use crate::transition::{
    EngineTransition, InputOutcome, InputPriority, ReducedInput, WorkspaceVersion,
};
use crate::validation::WorkspaceValidationErrors;
use crate::viewport::{ViewportRole, WindowToken};

/// Input accepted by the U3 engine boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    /// Bind an existing adapter window to a current logical surface.
    RegisterViewport {
        /// Workspace version whose surface roster was inspected.
        expected: WorkspaceVersion,
        /// Stable logical surface already present in the workspace.
        surface: crate::ids::SurfaceId,
        /// Opaque adapter token, never an operating-system handle.
        token: WindowToken,
        /// Root or docking-owned child close semantics.
        role: ViewportRole,
        /// Whole-root fallback required for docking-owned child windows.
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    },
    /// Publish one complete frame-before-paint platform snapshot.
    PublishPlatformSnapshot {
        /// Workspace epoch against which adapter bindings were observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Complete capabilities, inventory, and pointer facts.
        snapshot: PlatformSnapshot,
    },
    /// Report an adapter dispatch result without claiming the effect was observed applied.
    ReportPlatformEffect {
        /// Workspace epoch in which the result was received.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Exact effect identity and non-observational dispatch result.
        result: EffectResult,
    },
    /// Decide one exact edge-triggered native close request.
    DecideViewportClose {
        /// Workspace version against which the close plan was captured.
        expected: WorkspaceVersion,
        /// Exact close request produced by a platform snapshot transition.
        request: ViewportCloseRequestId,
        /// Application veto or frozen accepted plan.
        decision: ViewportCloseDecision,
    },
    /// Explicitly cancel one unresolved native-create saga.
    CancelNativeCreate {
        /// Workspace version against which the saga was inspected.
        expected: WorkspaceVersion,
        /// Exact create saga to cancel without a timeout heuristic.
        saga: NativeCreateSagaId,
    },
    /// Retry one exact cleanup only after its prior dispatch definitively failed.
    RetryViewportCleanup {
        /// Workspace version against which the failed cleanup was inspected.
        expected: WorkspaceVersion,
        /// Exact failed cleanup effect; indeterminate effects are never retryable.
        failed_effect: crate::effect::EffectId,
    },
    /// Authoritatively replace the complete workspace.
    ReplaceWorkspace(Workspace),
    /// Apply one checked command derived from an exact engine version.
    WorkspaceCommand {
        /// Version from which source and target references were captured.
        expected: WorkspaceVersion,
        /// Checked durable mutation.
        command: WorkspaceCommand,
    },
    /// Replace application policy if the input is still current.
    ReplacePolicy {
        /// Version observed when the application chose the policy.
        expected: WorkspaceVersion,
        /// Complete replacement policy.
        policy: DockPolicy,
    },
    /// Seal and publish the next scene after current renderer intents reduce.
    PublishScene {
        /// Workspace and policy version from which all scene facts were captured.
        expected: WorkspaceVersion,
        /// Complete frozen-roster scene builder.
        scene: BuildingScene,
    },
    /// Reduce one semantic renderer callback input.
    RendererIntent {
        /// Workspace and policy version from which the intent was derived.
        expected: WorkspaceVersion,
        /// Typed interaction input.
        intent: Box<RendererIntent>,
    },
    /// Re-run strict validation without changing state.
    ValidateWorkspace,
}

impl EngineInput {
    /// Returns the fixed source-class priority used during reduction.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        match self {
            Self::RegisterViewport { .. }
            | Self::DecideViewportClose { .. }
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_) => InputPriority::LifecycleControl,
            Self::PublishPlatformSnapshot { .. } | Self::ReportPlatformEffect { .. } => {
                InputPriority::PlatformObservation
            }
            Self::WorkspaceCommand { .. } | Self::ReplacePolicy { .. } => {
                InputPriority::ApplicationCommand
            }
            Self::RendererIntent { .. } => InputPriority::RendererIntent,
            Self::PublishScene { .. } | Self::ValidateWorkspace => InputPriority::Maintenance,
        }
    }

    const fn reduction_rank(&self) -> u8 {
        match self {
            Self::RendererIntent { intent, .. } => intent.reduction_rank(),
            Self::PublishScene { .. } => 1,
            Self::ValidateWorkspace => 2,
            Self::RegisterViewport { .. }
            | Self::PublishPlatformSnapshot { .. }
            | Self::ReportPlatformEffect { .. }
            | Self::DecideViewportClose { .. }
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_)
            | Self::WorkspaceCommand { .. }
            | Self::ReplacePolicy { .. } => 0,
        }
    }
}

/// Input after assignment by the engine's sole sequence writer.
#[derive(Debug, Clone, PartialEq)]
pub struct SequencedInput {
    sequence: InputSequence,
    input: EngineInput,
}

impl SequencedInput {
    /// Returns the monotonic writer sequence.
    #[must_use]
    pub const fn sequence(&self) -> InputSequence {
        self.sequence
    }

    /// Returns the typed input payload.
    #[must_use]
    pub const fn input(&self) -> &EngineInput {
        &self.input
    }
}

/// Failure to construct or atomically reduce engine state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EngineError {
    /// Initial or replacement workspace violated a durable invariant.
    #[error("workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// The single-writer input counter cannot advance without wrapping.
    #[error("engine input sequence is exhausted")]
    InputSequenceExhausted,
    /// A workspace replacement epoch cannot advance without wrapping.
    #[error("workspace epoch is exhausted while reducing input {input}")]
    WorkspaceEpochExhausted {
        /// Input which attempted replacement.
        input: InputSequence,
    },
    /// A state revision cannot advance without wrapping.
    #[error("workspace revision is exhausted while reducing input {input}")]
    WorkspaceRevisionExhausted {
        /// Input which attempted mutation.
        input: InputSequence,
    },
    /// A checked command transaction failed; no engine state was published.
    #[error("workspace command input {input} failed: {source}")]
    Command {
        /// Failing sequenced input.
        input: InputSequence,
        /// Atomic transaction failure.
        source: crate::error::TransactionError,
    },
    /// A successful one-command transaction omitted its required outcome.
    #[error("workspace command input {input} produced no command outcome")]
    MissingCommandOutcome {
        /// Input whose transaction violated the engine contract.
        input: InputSequence,
    },
    /// A scene generation counter cannot advance without wrapping.
    #[error("scene generation is exhausted while reducing input {input}")]
    SceneGenerationExhausted {
        /// Input which attempted to seal the next scene.
        input: InputSequence,
    },
    /// A transient interaction identity cannot advance without wrapping.
    #[error("interaction input {input} failed: {source}")]
    Interaction {
        /// Failing sequenced input.
        input: InputSequence,
        /// Fatal counter or internal state failure.
        source: InteractionCounterError,
    },
    /// Drop prevalidation encountered a fatal internal transaction failure.
    #[error("drop resolution input {input} failed: {source}")]
    DropResolution {
        /// Failing sequenced input.
        input: InputSequence,
        /// Fatal resolver failure.
        source: DropResolutionError,
    },
    /// Platform identity, inventory, route, or effect state could not advance atomically.
    #[error("viewport input {input} failed: {source}")]
    Viewport {
        /// Failing sequenced input.
        input: InputSequence,
        /// Typed coordinator failure.
        source: ViewportCoordinatorError,
    },
}

/// Authoritative renderer-neutral docking engine.
#[derive(Debug, PartialEq)]
pub struct DockEngine {
    workspace: Workspace,
    policy: DockPolicy,
    version: WorkspaceVersion,
    scene: Option<SealedScene>,
    last_scene_generation: SceneGeneration,
    interaction: InteractionState,
    viewport: ViewportCoordinator,
    last_input: InputSequence,
    pending: Vec<SequencedInput>,
}

#[derive(Debug, Clone, PartialEq)]
enum PreviewDecision {
    Publish {
        visual: PreviewVisual,
        proof: Box<PreviewProof>,
    },
    Clear(PreviewResolutionStatus),
    Cancel(InteractionCancelReason),
}

enum ReleaseProofDecision {
    Deliver(Box<PreviewProof>),
    Reject(InteractionRejection),
}

#[derive(Clone, Copy)]
struct DragReleaseInput<'a> {
    session: crate::interaction::DragSessionId,
    pointer: crate::intent::PointerId,
    button: crate::intent::PointerButton,
    button_state: &'a Authority<PointerButtonState>,
    target: &'a TargetAuthority,
    tear_off: Option<&'a TearOffRequest>,
}

#[derive(Clone, Copy)]
struct WorkspaceDeliveryTarget {
    kind: WorkspaceDeliveryKind,
    focus_surface: crate::ids::SurfaceId,
}

#[derive(Default)]
struct PlatformInteractionDependencies {
    surfaces: BTreeSet<crate::ids::SurfaceId>,
    routed: bool,
    native: bool,
}

enum CommandApplication {
    Applied {
        outcome: CommandOutcome,
        changed: bool,
    },
    Rejected(crate::error::CommandError),
}

impl DockEngine {
    /// Creates an engine from a strictly validated workspace and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted.
    pub fn new(workspace: Workspace, policy: DockPolicy) -> Result<Self, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        Ok(Self {
            workspace,
            policy,
            version: WorkspaceVersion::default(),
            scene: None,
            last_scene_generation: SceneGeneration::default(),
            interaction: InteractionState::default(),
            viewport: ViewportCoordinator::default(),
            last_input: InputSequence::default(),
            pending: Vec::new(),
        })
    }

    /// Returns the published workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns current application policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        &self.policy
    }

    /// Returns the version required by state-derived inputs.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the current immutable scene eligible for painting.
    #[must_use]
    pub const fn scene(&self) -> Option<&SealedScene> {
        self.scene.as_ref()
    }

    /// Returns the published transient interaction state.
    #[must_use]
    pub const fn interaction(&self) -> &InteractionState {
        &self.interaction
    }

    /// Returns the core-owned platform, binding, route, and effect coordinator.
    #[must_use]
    pub const fn viewport(&self) -> &ViewportCoordinator {
        &self.viewport
    }

    /// Returns queued inputs in writer order.
    #[must_use]
    pub fn pending_inputs(&self) -> &[SequencedInput] {
        &self.pending
    }

    /// Assigns the next monotonic sequence and queues an input.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue(&mut self, input: EngineInput) -> Result<InputSequence, EngineError> {
        let sequence = self
            .last_input
            .checked_next()
            .ok_or(EngineError::InputSequenceExhausted)?;
        self.last_input = sequence;
        self.pending.push(SequencedInput { sequence, input });
        Ok(sequence)
    }

    /// Queues a command against the currently published state version.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::WorkspaceCommand {
            expected: self.version,
            command,
        })
    }

    /// Queues a complete authoritative workspace replacement.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_workspace_replacement(
        &mut self,
        workspace: Workspace,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ReplaceWorkspace(workspace))
    }

    /// Queues registration of one existing native adapter window.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_registration(
        &mut self,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RegisterViewport {
            expected: self.version,
            surface,
            token,
            role,
            recovery,
        })
    }

    /// Queues one complete frame-before-paint platform fact snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_platform_snapshot(
        &mut self,
        snapshot: PlatformSnapshot,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::PublishPlatformSnapshot {
            expected_epoch: self.version.epoch(),
            snapshot,
        })
    }

    /// Queues one correlated platform effect dispatch result.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_platform_effect_result(
        &mut self,
        result: EffectResult,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::ReportPlatformEffect {
            expected_epoch: self.version.epoch(),
            result,
        })
    }

    /// Queues an application decision for one exact native close-request edge.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_close_decision(
        &mut self,
        request: ViewportCloseRequestId,
        decision: ViewportCloseDecision,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::DecideViewportClose {
            expected: self.version,
            request,
            decision,
        })
    }

    /// Queues an explicit cancellation for one unresolved native-create saga.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_native_create_cancellation(
        &mut self,
        saga: NativeCreateSagaId,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::CancelNativeCreate {
            expected: self.version,
            saga,
        })
    }

    /// Queues one explicit retry for a cleanup whose dispatch definitively failed.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_viewport_cleanup_retry(
        &mut self,
        failed_effect: crate::effect::EffectId,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RetryViewportCleanup {
            expected: self.version,
            failed_effect,
        })
    }

    /// Produces a native placement proof from current acknowledged platform facts.
    ///
    /// # Errors
    ///
    /// Returns a typed coordinate error when the surface is not ready or its
    /// placement facts are incomplete.
    pub fn viewport_placement(
        &self,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
    ) -> Result<crate::coordinates::ViewportPlacementProof, crate::coordinates::CoordinateUnavailable>
    {
        self.viewport.placement(surface, rect)
    }

    /// Queues complete scene facts against the currently published state.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_scene(&mut self, scene: BuildingScene) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::PublishScene {
            expected: self.version,
            scene,
        })
    }

    /// Queues one renderer intent against the currently published state.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InputSequenceExhausted`] instead of wrapping.
    pub fn enqueue_renderer_intent(
        &mut self,
        intent: RendererIntent,
    ) -> Result<InputSequence, EngineError> {
        self.enqueue(EngineInput::RendererIntent {
            expected: self.version,
            intent: Box::new(intent),
        })
    }

    /// Discards all queued inputs without changing published state.
    pub fn discard_pending(&mut self) -> Vec<SequencedInput> {
        std::mem::take(&mut self.pending)
    }

    /// Reduces queued inputs into one candidate and publishes it atomically.
    ///
    /// Inputs are ordered by [`InputPriority`] and then writer sequence. Events
    /// are returned only if the complete candidate commits. Expected command
    /// rejections are consumed and recorded in the transition. A fatal internal
    /// transaction error leaves the engine, including its pending queue, unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] on counter exhaustion, invalid replacement state,
    /// or an internal transaction failure. No engine state is changed on failure.
    pub fn reduce_pending(&mut self) -> Result<EngineTransition, EngineError> {
        let before = self.version;
        let before_scene = self.scene.clone();
        let before_interaction = self.interaction.clone();
        let before_viewport = self.viewport.clone();
        let mut candidate = self.candidate();
        let mut inputs = std::mem::take(&mut candidate.pending);
        inputs.sort_by_key(|input| {
            (
                input.input.priority(),
                input.input.reduction_rank(),
                input.sequence,
            )
        });

        let mut reduced = Vec::with_capacity(inputs.len());
        let mut events = Vec::new();
        let mut interaction_events = Vec::new();
        let mut application_base = before;
        let mut scene_published = false;
        for input in inputs {
            let priority = input.input.priority();
            let outcome = candidate.reduce_one(
                &input,
                &mut application_base,
                &mut scene_published,
                &mut events,
                &mut interaction_events,
            )?;
            reduced.push(ReducedInput::new(input.sequence, priority, outcome));
        }

        let platform_effects = candidate.viewport.take_new_effects();
        let published_state_changed = before != candidate.version
            || before_scene != candidate.scene
            || before_interaction != candidate.interaction
            || before_viewport != candidate.viewport;
        let transition = EngineTransition::new(
            before,
            candidate.version,
            reduced,
            events,
            interaction_events,
            platform_effects,
            published_state_changed,
        );
        *self = candidate;
        Ok(transition)
    }

    fn reduce_one(
        &mut self,
        input: &SequencedInput,
        application_base: &mut WorkspaceVersion,
        scene_published: &mut bool,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        match &input.input {
            EngineInput::RegisterViewport {
                expected,
                surface,
                token,
                role,
                recovery,
            } => self.reduce_viewport_registration(
                input.sequence,
                *expected,
                *surface,
                *token,
                *role,
                *recovery,
            ),
            EngineInput::PublishPlatformSnapshot {
                expected_epoch,
                snapshot,
            } => self.reduce_platform_snapshot(
                input.sequence,
                *expected_epoch,
                snapshot,
                application_base,
                events,
                interaction_events,
            ),
            EngineInput::ReportPlatformEffect {
                expected_epoch,
                result,
            } => Ok(self.reduce_platform_effect(*expected_epoch, *result)),
            EngineInput::DecideViewportClose {
                expected,
                request,
                decision,
            } => self.reduce_viewport_close_decision(
                input.sequence,
                *expected,
                *request,
                decision.clone(),
            ),
            EngineInput::CancelNativeCreate { expected, saga } => {
                self.reduce_native_create_cancellation(input.sequence, *expected, *saga)
            }
            EngineInput::RetryViewportCleanup {
                expected,
                failed_effect,
            } => self.reduce_viewport_cleanup_retry(input.sequence, *expected, *failed_effect),
            EngineInput::ReplaceWorkspace(workspace) => self.reduce_workspace_replacement(
                input.sequence,
                workspace,
                application_base,
                events,
                interaction_events,
            ),
            EngineInput::WorkspaceCommand { expected, command } => self.reduce_workspace_command(
                input.sequence,
                *expected,
                *application_base,
                command,
                events,
                interaction_events,
            ),
            EngineInput::ReplacePolicy { expected, policy } => self.reduce_policy_replacement(
                input.sequence,
                *expected,
                *application_base,
                policy,
                events,
                interaction_events,
            ),
            EngineInput::PublishScene { expected, scene } => self.reduce_scene(
                input.sequence,
                *expected,
                scene,
                scene_published,
                interaction_events,
            ),
            EngineInput::RendererIntent { expected, intent } => self.reduce_renderer_input(
                input.sequence,
                *expected,
                intent,
                events,
                interaction_events,
            ),
            EngineInput::ValidateWorkspace => {
                self.workspace
                    .validate()
                    .map_err(EngineError::InvalidWorkspace)?;
                Ok(InputOutcome::WorkspaceValidated {
                    version: self.version,
                })
            }
        }
    }

    fn reduce_viewport_registration(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery: Option<crate::intent::ContainedTearOffProposal>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let Some(presentation) = self.workspace.surface(surface) else {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        };
        if role == ViewportRole::Child && recovery.is_none() {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        if let Some(recovery) = recovery
            && (presentation.main_root != recovery.root()
                || recovery.surface() == surface
                || self.workspace.surface(recovery.surface()).is_none())
        {
            return Ok(InputOutcome::ViewportRegistrationRejected { surface });
        }
        let binding = self
            .viewport
            .register_existing(self.version.epoch(), surface, token, role, recovery)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::ViewportRegistered { binding })
    }

    fn reduce_platform_snapshot(
        &mut self,
        input: InputSequence,
        expected_epoch: crate::ids::WorkspaceEpoch,
        snapshot: &PlatformSnapshot,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected_epoch != self.version.epoch() {
            return Ok(InputOutcome::PlatformSnapshotStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            });
        }
        let previous_native = self.viewport.native_tear_off_capability();
        let previous_routing = self.viewport.capabilities().cross_surface_routing();
        let previous_release = self.viewport.capabilities().authoritative_release();
        let interaction_dependencies = self.platform_interaction_dependencies();
        let version_before_actions = self.version;
        let transition = self
            .viewport
            .publish_snapshot(snapshot)
            .map_err(|source| EngineError::Viewport { input, source })?;
        let actions = transition.actions().to_vec();
        self.reduce_viewport_actions(input, &actions, events)?;
        *application_base = self.version;
        if let Some(reason) = self.platform_interaction_cancel_reason(
            &interaction_dependencies,
            version_before_actions,
            &transition,
            previous_native,
            previous_routing,
            previous_release,
        ) {
            self.invalidate_transient(input, reason, interaction_events)?;
        }
        Ok(InputOutcome::PlatformSnapshotPublished { transition })
    }

    fn reduce_platform_effect(
        &mut self,
        expected_epoch: crate::ids::WorkspaceEpoch,
        result: EffectResult,
    ) -> InputOutcome {
        let transition = if expected_epoch == self.version.epoch() {
            self.viewport.report_effect(expected_epoch, result)
        } else {
            EffectTransition::StaleEpoch
        };
        InputOutcome::PlatformEffectReported {
            effect: result.effect(),
            transition,
        }
    }

    fn reduce_viewport_close_decision(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        request: ViewportCloseRequestId,
        decision: ViewportCloseDecision,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if let ViewportCloseDecision::Accept(plan) = &decision
            && let Some(close_request) = self.viewport.viewport_close_request(request)
        {
            let surface = close_request.binding().surface();
            let recovery_matches_surface = self
                .workspace
                .surface(surface)
                .is_some_and(|presentation| presentation.main_root == plan.recovery().root());
            if !recovery_matches_surface {
                return Err(EngineError::Viewport {
                    input,
                    source: crate::frame::ViewportCoordinatorError::CloseRecoveryRootMismatch {
                        request,
                    },
                });
            }
        }
        let effect = self
            .viewport
            .decide_viewport_close(request, decision)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::ViewportCloseDecided { request, effect })
    }

    fn reduce_native_create_cancellation(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        saga: NativeCreateSagaId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let compensation = self
            .viewport
            .cancel_native_create(saga)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::NativeCreateCancelled { saga, compensation })
    }

    fn reduce_viewport_cleanup_retry(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        failed_effect: crate::effect::EffectId,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let retry = self
            .viewport
            .retry_cleanup(failed_effect)
            .map_err(|source| EngineError::Viewport { input, source })?;
        Ok(InputOutcome::ViewportCleanupRetried {
            failed_effect,
            retry,
        })
    }

    fn reduce_viewport_actions(
        &mut self,
        input: InputSequence,
        actions: &[crate::frame::ViewportLifecycleAction],
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<(), EngineError> {
        for action in actions {
            match action {
                crate::frame::ViewportLifecycleAction::CreateReady { saga, prepared } => {
                    match self.apply_interaction_command(input, prepared.command(), events)? {
                        CommandApplication::Applied { .. } => self
                            .viewport
                            .complete_native_create(*saga)
                            .map_err(|source| EngineError::Viewport { input, source })?,
                        CommandApplication::Rejected(_) => {
                            self.viewport
                                .reject_native_create(*saga)
                                .map_err(|source| EngineError::Viewport { input, source })?;
                        }
                    }
                }
                crate::frame::ViewportLifecycleAction::SurfaceDestroyed {
                    binding,
                    resolution,
                } => {
                    self.reduce_destroyed_surface(input, *binding, resolution, events)?;
                }
                crate::frame::ViewportLifecycleAction::RetryRecovery {
                    destroyed_binding,
                    recovery,
                } => {
                    if self.apply_destroyed_surface_recovery(input, *recovery, events)? {
                        self.viewport
                            .complete_pending_recovery(destroyed_binding.surface())
                            .map_err(|source| EngineError::Viewport { input, source })?;
                    }
                }
            }
        }
        Ok(())
    }

    fn reduce_destroyed_surface(
        &mut self,
        input: InputSequence,
        binding: crate::viewport::ViewportBinding,
        resolution: &crate::frame::ViewportDestructionResolution,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<(), EngineError> {
        let mut resolved = false;
        let recovery = match resolution {
            crate::frame::ViewportDestructionResolution::Accepted { plan, .. } => {
                Some(plan.recovery())
            }
            crate::frame::ViewportDestructionResolution::Recover { recovery } => Some(*recovery),
            crate::frame::ViewportDestructionResolution::Unplanned => None,
        };
        match resolution {
            crate::frame::ViewportDestructionResolution::Accepted { plan, .. } => {
                if let Some(primary) = plan.primary() {
                    resolved =
                        self.apply_close_primary(input, binding.surface(), primary, events)?;
                }
                if !resolved {
                    resolved =
                        self.apply_destroyed_surface_recovery(input, plan.recovery(), events)?;
                }
            }
            crate::frame::ViewportDestructionResolution::Recover { recovery } => {
                resolved = self.apply_destroyed_surface_recovery(input, *recovery, events)?;
            }
            crate::frame::ViewportDestructionResolution::Unplanned => {}
        }
        if resolved && self.workspace.surface(binding.surface()).is_none() {
            self.viewport
                .complete_destroyed_surface(binding)
                .map_err(|source| EngineError::Viewport { input, source })?;
        } else if !resolved && let Some(recovery) = recovery {
            self.viewport
                .defer_destroyed_surface_recovery(binding, recovery)
                .map_err(|source| EngineError::Viewport { input, source })?;
        }
        Ok(())
    }

    fn apply_close_primary(
        &mut self,
        input: InputSequence,
        closing_surface: crate::ids::SurfaceId,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        let mut candidate = self.workspace.clone();
        let application =
            Self::run_command_transaction(input, &mut candidate, &self.policy, command)?;
        let CommandApplication::Applied { outcome, changed } = application else {
            return Ok(false);
        };
        if candidate.surface(closing_surface).is_some() {
            return Ok(false);
        }
        self.workspace = candidate;
        if changed {
            self.advance_revision(input)?;
            self.scene = None;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome),
            ));
        }
        Ok(true)
    }

    fn apply_destroyed_surface_recovery(
        &mut self,
        input: InputSequence,
        recovery: crate::intent::ContainedTearOffProposal,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        if !self
            .viewport
            .viewport(recovery.surface())
            .is_some_and(crate::viewport_registry::ViewportRecord::is_ready)
        {
            return Ok(false);
        }
        let Some(root) = self.workspace.root(recovery.root()) else {
            return Ok(false);
        };
        let Ok(source) = self
            .workspace
            .capture_node_source(recovery.root(), root.node)
        else {
            return Ok(false);
        };
        let command = WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::Contained {
                surface: recovery.surface(),
                floating: recovery.floating(),
                rect: recovery.rect(),
                z_order: recovery.z_order(),
            },
        };
        Ok(matches!(
            self.apply_interaction_command(input, &command, events)?,
            CommandApplication::Applied { .. }
        ))
    }

    fn reduce_workspace_replacement(
        &mut self,
        input: InputSequence,
        workspace: &Workspace,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        let before = self.version;
        let epoch = before
            .epoch()
            .checked_next()
            .ok_or(EngineError::WorkspaceEpochExhausted { input })?;
        let desired_surfaces = workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<std::collections::BTreeSet<_>>();
        let reconciliation = self
            .viewport
            .reconcile_workspace_epoch(epoch, &desired_surfaces)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.workspace = workspace.clone();
        self.version = WorkspaceVersion::new(epoch, WorkspaceRevision::default());
        *application_base = self.version;
        self.invalidate_transient(
            input,
            InteractionCancelReason::WorkspaceRestored,
            interaction_events,
        )?;
        events.push(WorkspaceEvent::new(
            input,
            self.version,
            WorkspaceEventKind::WorkspaceReplaced,
        ));
        Ok(InputOutcome::WorkspaceReplaced {
            before,
            after: self.version,
            reconciliation,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_policy_replacement(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        accepted_base: WorkspaceVersion,
        policy: &DockPolicy,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }
        let changed = self.policy != *policy;
        if changed {
            self.policy = policy.clone();
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::PolicyChanged,
                interaction_events,
            )?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::PolicyReplaced,
            ));
        }
        Ok(InputOutcome::PolicyReplaced {
            changed,
            version: self.version,
        })
    }

    fn reduce_renderer_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let outcome = self.reduce_renderer_intent(input, intent, events, interaction_events)?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    fn advance_revision(&mut self, input: InputSequence) -> Result<(), EngineError> {
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        Ok(())
    }

    fn invalidate_transient(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.scene = None;
        if let Some(status) = self.interaction.cancel_active() {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        Ok(())
    }

    fn platform_interaction_dependencies(&self) -> PlatformInteractionDependencies {
        let mut dependencies = PlatformInteractionDependencies::default();
        match self.interaction.status() {
            InteractionStatus::Idle => {}
            InteractionStatus::Armed { session } => {
                if let Ok(drag) = self.interaction.armed_drag(session)
                    && let Some(surface) = self.payload_surface(&drag.payload)
                {
                    dependencies.surfaces.insert(surface);
                }
            }
            InteractionStatus::Dragging { session } => {
                let Ok(drag) = self.interaction.active_drag(session) else {
                    return dependencies;
                };
                if let Some(surface) = self.payload_surface(&drag.payload) {
                    dependencies.surfaces.insert(surface);
                }
                if let Some(target) = &drag.target {
                    match target {
                        TargetAuthority::Local(local) => {
                            dependencies.surfaces.insert(local.observer());
                            if let Authority::Known(Some(target)) = local.target() {
                                dependencies.surfaces.insert(target.surface());
                            }
                        }
                        TargetAuthority::Routed(route) => {
                            dependencies.routed = true;
                            if let Authority::Known(Some(target)) = route.target() {
                                dependencies.surfaces.insert(target.surface());
                            }
                        }
                    }
                } else if self.viewport.drag_source(drag.pointer).is_some() {
                    dependencies.routed = true;
                }
                if let Some(preview) = &drag.preview {
                    match preview.public().visual() {
                        PreviewVisual::Dock { surface, .. }
                        | PreviewVisual::Contained { surface, .. } => {
                            dependencies.surfaces.insert(*surface);
                        }
                        PreviewVisual::Native { .. } => dependencies.native = true,
                    }
                } else if matches!(drag.tear_off, Some(TearOffRequest::Native { .. }))
                    && self.viewport.native_tear_off_capability().is_supported()
                {
                    dependencies.native = true;
                }
            }
            InteractionStatus::Resizing { session } => {
                if let Ok(resize) = self.interaction.active_resize(session)
                    && let Some(surface) = self.root_surface(resize.split.root())
                {
                    dependencies.surfaces.insert(surface);
                }
            }
        }
        dependencies
    }

    #[allow(clippy::too_many_arguments)]
    fn platform_interaction_cancel_reason(
        &self,
        dependencies: &PlatformInteractionDependencies,
        version_before_actions: WorkspaceVersion,
        transition: &crate::frame::ViewportFrameTransition,
        previous_native: PlatformCapability,
        previous_routing: PlatformCapability,
        previous_release: PlatformCapability,
    ) -> Option<InteractionCancelReason> {
        if self.version != version_before_actions {
            return Some(InteractionCancelReason::WorkspaceChanged);
        }
        for event in transition.registry_events() {
            let (binding, destroyed) = match event {
                crate::viewport_registry::RegistryEvent::Destroyed { binding } => (*binding, true),
                crate::viewport_registry::RegistryEvent::FactsUnavailable { binding } => {
                    (*binding, false)
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequested { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequestCleared { .. } => continue,
            };
            if !dependencies.surfaces.contains(&binding.surface()) {
                continue;
            }
            if destroyed {
                return Some(InteractionCancelReason::SurfaceClosed);
            }
            if dependencies.native {
                return Some(InteractionCancelReason::NativePlacementUnavailable);
            }
            if dependencies.routed {
                return Some(InteractionCancelReason::UnknownTargetAuthority);
            }
        }
        let current_native = self.viewport.native_tear_off_capability();
        if dependencies.native && previous_native.is_supported() && !current_native.is_supported() {
            return Some(match current_native {
                PlatformCapability::Unknown(_) => InteractionCancelReason::NativeCapabilityUnknown,
                PlatformCapability::Unsupported(_) => {
                    InteractionCancelReason::NativeCapabilityUnavailable
                }
                PlatformCapability::Supported => return None,
            });
        }
        let current_routing = self.viewport.capabilities().cross_surface_routing();
        if dependencies.routed && previous_routing.is_supported() && !current_routing.is_supported()
        {
            return Some(InteractionCancelReason::UnknownTargetAuthority);
        }
        let current_release = self.viewport.capabilities().authoritative_release();
        if dependencies.routed && previous_release.is_supported() && !current_release.is_supported()
        {
            return Some(InteractionCancelReason::UnknownButtonState);
        }
        None
    }

    fn reduce_scene(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        building: &BuildingScene,
        scene_published: &mut bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if *scene_published {
            return Ok(InputOutcome::SceneRejected {
                error: SceneBuildError::AlreadyPublishedInBoundary,
            });
        }
        let generation = self
            .last_scene_generation
            .checked_next()
            .ok_or(EngineError::SceneGenerationExhausted { input })?;
        let stamp = SceneStamp::new(self.version, generation);
        let sealed = match building.clone().seal(stamp, &self.workspace, &self.policy) {
            Ok(scene) => scene,
            Err(error) => {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::SceneUnavailable,
                    interaction_events,
                )?;
                return Ok(InputOutcome::SceneRejected { error });
            }
        };
        let (ready_surfaces, bootstrap_surfaces) =
            sealed
                .surfaces()
                .fold(
                    (0_usize, 0_usize),
                    |(ready, bootstrap), (_, surface)| match surface {
                        SurfaceScene::Ready(_) => (ready + 1, bootstrap),
                        SurfaceScene::Bootstrap(_) => (ready, bootstrap + 1),
                    },
                );
        self.scene = Some(sealed);
        self.last_scene_generation = generation;
        *scene_published = true;
        self.refresh_drag_preview(input, interaction_events)?;
        Ok(InputOutcome::ScenePublished {
            stamp,
            ready_surfaces,
            bootstrap_surfaces,
        })
    }

    fn reduce_renderer_intent(
        &mut self,
        input: InputSequence,
        intent: &RendererIntent,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match intent {
            RendererIntent::ArmDrag {
                pointer,
                button,
                payload,
            } => self.arm_drag(input, *pointer, *button, payload, interaction_events),
            RendererIntent::BeginDrag {
                session,
                pointer,
                button,
            } => self.begin_drag(input, *session, *pointer, *button),
            RendererIntent::UpdateDrag {
                session,
                target,
                tear_off,
            } => self.update_drag(
                input,
                *session,
                target,
                tear_off.as_ref(),
                interaction_events,
            ),
            RendererIntent::AcknowledgePreview(acknowledgement) => {
                match self.interaction.acknowledge_preview(acknowledgement) {
                    Ok((session, changed)) => {
                        Ok(InteractionOutcome::PreviewAcknowledged { session, changed })
                    }
                    Err(error) => Ok(InteractionOutcome::Rejected(error)),
                }
            }
            RendererIntent::ReleaseDrag {
                session,
                pointer,
                button,
                button_state,
                target,
                tear_off,
            } => self.release_drag(
                input,
                DragReleaseInput {
                    session: *session,
                    pointer: *pointer,
                    button: *button,
                    button_state,
                    target,
                    tear_off: tear_off.as_ref(),
                },
                events,
                interaction_events,
            ),
            RendererIntent::CancelDrag { session, reason } => {
                self.cancel_drag(input, *session, *reason, interaction_events)
            }
            RendererIntent::BeginResize {
                pointer,
                button,
                split,
            } => self.begin_resize(input, *pointer, *button, split, interaction_events),
            RendererIntent::UpdateResize { session, weights } => {
                self.update_resize(input, *session, weights)
            }
            RendererIntent::ReleaseResize {
                session,
                pointer,
                button,
                button_state,
            } => self.release_resize(
                input,
                *session,
                *pointer,
                *button,
                button_state,
                events,
                interaction_events,
            ),
            RendererIntent::CancelResize { session, reason } => {
                Ok(self.cancel_resize(input, *session, *reason, interaction_events))
            }
        }
    }

    fn arm_drag(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        payload: &MovePayload,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_payload(payload) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            ));
        }
        let (session, replaced) = self
            .interaction
            .arm_drag(self.version.epoch(), pointer, button, payload.clone())
            .map_err(|source| EngineError::Interaction { input, source })?;
        if let Some(status) = replaced {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status,
                    reason: InteractionCancelReason::ReplacedByNewGesture,
                },
            ));
        }
        Ok(InteractionOutcome::DragArmed { session, replaced })
    }

    fn begin_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.interaction.begin_drag(session, pointer, button) {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let payload = self
            .interaction
            .active_drag(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .payload
            .clone();
        if let Some(surface) = self.payload_surface(&payload) {
            let _ = self
                .viewport
                .begin_drag_routing(pointer, surface)
                .map_err(|source| EngineError::Viewport { input, source })?;
        }
        Ok(InteractionOutcome::DragBegan { session })
    }

    fn update_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) =
            self.interaction
                .set_drag_observation(session, target.clone(), tear_off.cloned())
        {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let (payload, pointer) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            (drag.payload.clone(), drag.pointer)
        };
        let decision =
            self.resolve_preview_decision(input, session, pointer, &payload, target, tear_off)?;
        self.apply_preview_decision(input, session, decision, interaction_events)
    }

    fn cancel_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let pointer = self
            .interaction
            .active_drag(session)
            .ok()
            .map(|drag| drag.pointer);
        match self.interaction.cancel_drag(session) {
            Ok(status) => {
                if let Some(pointer) = pointer {
                    let _ = self
                        .viewport
                        .end_drag_routing(pointer)
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                Ok(InteractionOutcome::Cancelled { status, reason })
            }
            Err(error) => Ok(InteractionOutcome::Rejected(error)),
        }
    }

    fn begin_resize(
        &mut self,
        input: InputSequence,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        split: &crate::command::NodeSource,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) = self.validate_node_source(split) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            ));
        }
        let (session, replaced) = self
            .interaction
            .begin_resize(self.version.epoch(), pointer, button, split.clone())
            .map_err(|source| EngineError::Interaction { input, source })?;
        if let Some(status) = replaced {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status,
                    reason: InteractionCancelReason::ReplacedByNewGesture,
                },
            ));
        }
        Ok(InteractionOutcome::ResizeBegan { session, replaced })
    }

    fn update_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        weights: &[crate::graph::SplitWeight],
    ) -> Result<InteractionOutcome, EngineError> {
        let split = match self.interaction.active_resize(session) {
            Ok(resize) => resize.split.clone(),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let command = WorkspaceCommand::ResizeSplit {
            split,
            weights: weights.to_vec(),
        };
        match self.preflight_command(input, &command)? {
            CommandApplication::Applied { .. } => {
                self.interaction
                    .set_resize_weights(session, weights.to_vec())
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
                Ok(InteractionOutcome::ResizeUpdated {
                    session,
                    weights: weights.to_vec(),
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    fn cancel_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_resize(session) {
            Ok(status) => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }

    fn apply_preview_decision(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        decision: PreviewDecision,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match decision {
            PreviewDecision::Publish { visual, proof } => {
                let stamp = self.scene.as_ref().map(SealedScene::stamp).ok_or(
                    EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    },
                )?;
                let (preview, changed) = self
                    .interaction
                    .publish_preview(session, stamp, visual, *proof)
                    .map_err(|source| EngineError::Interaction { input, source })?;
                if changed {
                    interaction_events.push(InteractionEvent::new(
                        input,
                        self.version,
                        InteractionEventKind::PreviewPublished {
                            preview: preview.clone(),
                        },
                    ));
                }
                Ok(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: Some(preview),
                    status: PreviewResolutionStatus::Resolved,
                })
            }
            PreviewDecision::Clear(status) => {
                self.interaction
                    .clear_preview(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
                Ok(InteractionOutcome::PreviewUpdated {
                    session,
                    preview: None,
                    status,
                })
            }
            PreviewDecision::Cancel(reason) => {
                self.cancel_drag(input, session, reason, interaction_events)
            }
        }
    }

    fn resolve_preview_decision(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        payload: &MovePayload,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
    ) -> Result<PreviewDecision, EngineError> {
        let Some(scene) = self.scene.as_ref() else {
            return Ok(PreviewDecision::Cancel(
                InteractionCancelReason::SceneUnavailable,
            ));
        };
        if scene.stamp().workspace() != self.version {
            return Ok(PreviewDecision::Cancel(
                InteractionCancelReason::SceneUnavailable,
            ));
        }
        let (target, routed) = match self.target_observation(pointer, target) {
            Ok(target) => target,
            Err(reason) => return Ok(PreviewDecision::Cancel(reason)),
        };
        match target {
            Authority::Unknown(_) => Ok(PreviewDecision::Cancel(
                InteractionCancelReason::UnknownTargetAuthority,
            )),
            Authority::Known(Some(pointer)) => {
                match resolve_drop(
                    scene,
                    &self.workspace,
                    &self.policy,
                    session,
                    payload.clone(),
                    pointer.surface(),
                    pointer.position(),
                )
                .map_err(|source| EngineError::DropResolution { input, source })?
                {
                    DropResolution::Resolved(resolved) => {
                        if resolved.scene_stamp() != scene.stamp()
                            || resolved.session() != session
                            || resolved.source() != payload
                        {
                            return Err(EngineError::Interaction {
                                input,
                                source: InteractionCounterError::StateInvariant,
                            });
                        }
                        let target = resolved.target_id();
                        let visual = PreviewVisual::Dock {
                            surface: pointer.surface(),
                            target,
                            rect: resolved.visual().rect(),
                        };
                        Ok(PreviewDecision::Publish {
                            visual,
                            proof: Box::new(PreviewProof::Dock {
                                target,
                                command: resolved.into_command(),
                            }),
                        })
                    }
                    DropResolution::KnownNone(_) => {
                        Ok(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone))
                    }
                    DropResolution::Rejected(_) => {
                        Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
                    }
                    DropResolution::Unavailable(_) => Ok(PreviewDecision::Cancel(
                        InteractionCancelReason::SceneUnavailable,
                    )),
                }
            }
            Authority::Known(None) => {
                self.resolve_tear_off_decision(input, payload, tear_off, routed)
            }
        }
    }

    fn target_observation<'a>(
        &self,
        pointer: crate::intent::PointerId,
        target: &'a TargetAuthority,
    ) -> Result<(&'a Authority<Option<crate::intent::SurfacePointer>>, bool), InteractionCancelReason>
    {
        match target {
            TargetAuthority::Local(local) => {
                if self.workspace.surface(local.observer()).is_none()
                    || matches!(
                        local.target(),
                        Authority::Known(Some(target))
                            if target.surface() != local.observer()
                    )
                {
                    return Err(InteractionCancelReason::UnknownTargetAuthority);
                }
                Ok((local.target(), false))
            }
            TargetAuthority::Routed(proof) => {
                if proof.pointer() != pointer || !self.viewport.route_is_current(proof) {
                    return Err(InteractionCancelReason::UnknownTargetAuthority);
                }
                Ok((proof.target(), true))
            }
        }
    }

    fn resolve_tear_off_decision(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        request: Option<&TearOffRequest>,
        routed: bool,
    ) -> Result<PreviewDecision, EngineError> {
        let Some(request) = request else {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone));
        };
        match request {
            TearOffRequest::Contained(proposal) => {
                self.resolve_contained_tear_off(input, payload, *proposal, request, false)
            }
            TearOffRequest::Native {
                proposal,
                contained_fallback,
            } => self.resolve_native_tear_off(
                input,
                payload,
                proposal.clone(),
                *contained_fallback,
                request,
                routed,
            ),
        }
    }

    fn resolve_contained_tear_off(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
        request: &TearOffRequest,
        fallback: bool,
    ) -> Result<PreviewDecision, EngineError> {
        if self
            .policy
            .check_tear_off(TearOffPresentation::Contained)
            .is_err()
            || !self.tear_off_root_identity_matches(input, payload, proposal.root())?
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        let command = self.tear_off_command(
            input,
            payload,
            RootPresentationTarget::Contained {
                surface: proposal.surface(),
                floating: proposal.floating(),
                rect: proposal.rect(),
                z_order: proposal.z_order(),
            },
            proposal.root(),
        )?;
        match self.preflight_command(input, &command)? {
            CommandApplication::Applied { .. } => Ok(PreviewDecision::Publish {
                visual: PreviewVisual::Contained {
                    surface: proposal.surface(),
                    rect: proposal.rect(),
                    fallback,
                },
                proof: Box::new(PreviewProof::Contained {
                    command,
                    request: request.clone(),
                    fallback,
                }),
            }),
            CommandApplication::Rejected(_) => {
                Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_native_tear_off(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        proposal: crate::intent::NativeTearOffProposal,
        contained_fallback: Option<crate::intent::ContainedTearOffProposal>,
        request: &TearOffRequest,
        routed: bool,
    ) -> Result<PreviewDecision, EngineError> {
        if self
            .policy
            .check_tear_off(TearOffPresentation::Native)
            .is_err()
        {
            return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
        }
        match self.viewport.native_tear_off_capability() {
            PlatformCapability::Supported => {
                if !routed {
                    return Ok(PreviewDecision::Cancel(
                        InteractionCancelReason::UnknownTargetAuthority,
                    ));
                }
                if !self.viewport.placement_is_current(proposal.placement()) {
                    return Ok(PreviewDecision::Cancel(
                        InteractionCancelReason::NativePlacementUnavailable,
                    ));
                }
                if self.workspace.surface(proposal.surface()).is_some() {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                if !self.tear_off_root_identity_matches(input, payload, proposal.root())? {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                let command = self.tear_off_command(
                    input,
                    payload,
                    RootPresentationTarget::Surface {
                        surface: proposal.surface(),
                    },
                    proposal.root(),
                )?;
                match self.preflight_command(input, &command)? {
                    CommandApplication::Applied { .. } => Ok(PreviewDecision::Publish {
                        visual: PreviewVisual::Native {
                            surface: proposal.surface(),
                            placement: proposal.physical_placement(),
                        },
                        proof: Box::new(PreviewProof::Native {
                            command,
                            request: request.clone(),
                            proposal,
                        }),
                    }),
                    CommandApplication::Rejected(_) => {
                        Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected))
                    }
                }
            }
            PlatformCapability::Unsupported(_) => {
                if self.policy.native_unavailable_fallback().is_err() {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                }
                let Some(proposal) = contained_fallback else {
                    return Ok(PreviewDecision::Clear(PreviewResolutionStatus::Rejected));
                };
                self.resolve_contained_tear_off(input, payload, proposal, request, true)
            }
            PlatformCapability::Unknown(_) => Ok(PreviewDecision::Cancel(
                InteractionCancelReason::NativeCapabilityUnknown,
            )),
        }
    }

    fn release_drag(
        &mut self,
        input: InputSequence,
        release: DragReleaseInput<'_>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(error) =
            self.validate_drag_release_binding(release.session, release.pointer, release.button)
        {
            return Ok(InteractionOutcome::Rejected(error));
        }
        let target_unknown = match self.target_observation(release.pointer, release.target) {
            Ok((target, _)) => matches!(target, Authority::Unknown(_)),
            Err(reason) => {
                return self.cancel_drag(input, release.session, reason, interaction_events);
            }
        };
        let button_state = match release.target {
            TargetAuthority::Local(_) => release.button_state.clone(),
            TargetAuthority::Routed(proof) => proof.button_state(release.button),
        };
        if let Some(outcome) =
            self.require_released_button(input, release.session, &button_state, interaction_events)?
        {
            return Ok(outcome);
        }
        let drag = match self.interaction.take_drag_for_release(
            release.session,
            release.pointer,
            release.button,
        ) {
            Ok(drag) => drag,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if target_unknown {
            let _ = self
                .viewport
                .end_drag_routing(drag.pointer)
                .map_err(|source| EngineError::Viewport { input, source })?;
            return Ok(self.cancel_consumed_drag(
                input,
                release.session,
                InteractionCancelReason::UnknownTargetAuthority,
                interaction_events,
            ));
        }
        let proof = match self.resolve_release_proof(
            input,
            release.session,
            &drag,
            release.target,
            release.tear_off,
        )? {
            ReleaseProofDecision::Deliver(proof) => proof,
            ReleaseProofDecision::Reject(error) => {
                let _ = self
                    .viewport
                    .end_drag_routing(drag.pointer)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                return Ok(InteractionOutcome::Rejected(error));
            }
        };
        let _ = self
            .viewport
            .end_drag_routing(drag.pointer)
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.finish_drag_delivery(input, release.session, *proof, events, interaction_events)
    }

    fn validate_drag_release_binding(
        &self,
        session: crate::interaction::DragSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
    ) -> Result<(), InteractionRejection> {
        let drag = self.interaction.active_drag(session).map_err(|error| {
            if matches!(error, InteractionRejection::SessionConsumed { .. }) {
                InteractionRejection::DuplicateRelease { session }
            } else {
                error
            }
        })?;
        if drag.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if drag.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        Ok(())
    }

    fn require_released_button(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        button_state: &Authority<PointerButtonState>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        match button_state {
            Authority::Known(PointerButtonState::Released) => Ok(None),
            Authority::Known(PointerButtonState::Pressed) => Ok(Some(
                InteractionOutcome::Rejected(InteractionRejection::ButtonStillPressed),
            )),
            Authority::Unknown(_) => {
                let reason = InteractionCancelReason::UnknownButtonState;
                self.cancel_drag(input, session, reason, interaction_events)
                    .map(Some)
            }
        }
    }

    fn cancel_consumed_drag(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        let status = InteractionStatus::Dragging { session };
        interaction_events.push(InteractionEvent::new(
            input,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        InteractionOutcome::Cancelled { status, reason }
    }

    fn resolve_release_proof(
        &self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        drag: &crate::interaction::ActiveDrag,
        target: &TargetAuthority,
        tear_off: Option<&TearOffRequest>,
    ) -> Result<ReleaseProofDecision, EngineError> {
        let Some(preview) = drag.preview.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewMissing,
            ));
        };
        if !preview.painted() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::PreviewNotPainted,
            ));
        }
        let Some(scene) = self.scene.as_ref() else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        };
        if scene.stamp() != preview.public().token().scene()
            || scene.stamp().workspace() != self.version
        {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::StaleScene,
            ));
        }
        let decision = self.resolve_preview_decision(
            input,
            session,
            drag.pointer,
            &drag.payload,
            target,
            tear_off,
        )?;
        let PreviewDecision::Publish { visual, proof } = decision else {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        };
        if preview.public().visual() != &visual || preview.proof() != proof.as_ref() {
            return Ok(ReleaseProofDecision::Reject(
                InteractionRejection::TargetChanged,
            ));
        }
        Ok(ReleaseProofDecision::Deliver(proof))
    }

    fn finish_drag_delivery(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        proof: PreviewProof,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match proof {
            PreviewProof::Dock { target, command } => self.finish_workspace_delivery(
                input,
                session,
                WorkspaceDeliveryTarget {
                    kind: WorkspaceDeliveryKind::Dock,
                    focus_surface: target.surface(),
                },
                &command,
                events,
                interaction_events,
            ),
            PreviewProof::Contained {
                command,
                request,
                fallback,
            } => {
                let kind = if fallback {
                    WorkspaceDeliveryKind::ContainedFallback
                } else {
                    WorkspaceDeliveryKind::Contained
                };
                let focus_surface = match request {
                    TearOffRequest::Contained(proposal)
                    | TearOffRequest::Native {
                        contained_fallback: Some(proposal),
                        ..
                    } => proposal.surface(),
                    TearOffRequest::Native {
                        contained_fallback: None,
                        ..
                    } => {
                        return Err(EngineError::Interaction {
                            input,
                            source: InteractionCounterError::StateInvariant,
                        });
                    }
                };
                self.finish_workspace_delivery(
                    input,
                    session,
                    WorkspaceDeliveryTarget {
                        kind,
                        focus_surface,
                    },
                    &command,
                    events,
                    interaction_events,
                )
            }
            PreviewProof::Native {
                command, proposal, ..
            } => {
                let prepared =
                    PreparedNativeTearOff::new(session, self.version, command, proposal.clone());
                let request = self
                    .viewport
                    .start_native_create(prepared)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::NativeTearOffRequested(request),
                ));
                Ok(InteractionOutcome::DragDelivered {
                    session,
                    delivery: InteractionDelivery::NativeRequested(request),
                })
            }
        }
    }

    fn finish_workspace_delivery(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        delivery: WorkspaceDeliveryTarget,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        match self.apply_interaction_command(input, command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                let _ = self
                    .viewport
                    .request_focus(delivery.focus_surface)
                    .map_err(|source| EngineError::Viewport { input, source })?;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Delivered {
                        session,
                        kind: delivery.kind,
                    },
                ));
                Ok(InteractionOutcome::DragDelivered {
                    session,
                    delivery: InteractionDelivery::Workspace {
                        kind: delivery.kind,
                        outcome,
                        changed,
                    },
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn release_resize(
        &mut self,
        input: InputSequence,
        session: crate::interaction::ResizeSessionId,
        pointer: crate::intent::PointerId,
        button: crate::intent::PointerButton,
        button_state: &Authority<PointerButtonState>,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let binding = match self.interaction.active_resize(session) {
            Ok(resize) => (resize.pointer, resize.button),
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if binding.0 != pointer {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::PointerMismatch,
            ));
        }
        if binding.1 != button {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ButtonMismatch,
            ));
        }
        match button_state {
            Authority::Unknown(_) => {
                let status = self.interaction.cancel_resize(session).map_err(|_| {
                    EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    }
                })?;
                let reason = InteractionCancelReason::UnknownButtonState;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                return Ok(InteractionOutcome::Cancelled { status, reason });
            }
            Authority::Known(PointerButtonState::Pressed) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::ButtonStillPressed,
                ));
            }
            Authority::Known(PointerButtonState::Released) => {}
        }
        let resize = match self
            .interaction
            .take_resize_for_release(session, pointer, button)
        {
            Ok(resize) => resize,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        let Some(weights) = resize.weights else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeProposalMissing,
            ));
        };
        let command = WorkspaceCommand::ResizeSplit {
            split: resize.split,
            weights,
        };
        match self.apply_interaction_command(input, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::ResizeDelivered { session },
                ));
                Ok(InteractionOutcome::ResizeDelivered {
                    session,
                    outcome,
                    changed,
                })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            )),
        }
    }

    fn refresh_drag_preview(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return Ok(());
        };
        let (pointer, payload, target, tear_off) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            (
                drag.pointer,
                drag.payload.clone(),
                drag.target.clone(),
                drag.tear_off.clone(),
            )
        };
        let Some(target) = target else {
            self.interaction
                .clear_preview(session)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
            return Ok(());
        };
        let decision = self.resolve_preview_decision(
            input,
            session,
            pointer,
            &payload,
            &target,
            tear_off.as_ref(),
        )?;
        let _ = self.apply_preview_decision(input, session, decision, interaction_events)?;
        Ok(())
    }

    fn validate_payload(&self, payload: &MovePayload) -> Result<(), crate::error::CommandError> {
        use crate::error::ReferenceRole;

        match payload {
            MovePayload::Item(source) => {
                self.workspace.verify_reference(
                    source.root(),
                    source.tabs(),
                    source.fingerprint(),
                    ReferenceRole::Source,
                )?;
                self.workspace
                    .capture_item_source(source.root(), source.tabs(), source.item())?;
            }
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                self.validate_node_source(source)?;
            }
        }
        Ok(())
    }

    fn validate_node_source(
        &self,
        source: &crate::command::NodeSource,
    ) -> Result<(), crate::error::CommandError> {
        use crate::error::ReferenceRole;

        self.workspace.verify_reference(
            source.root(),
            source.node(),
            source.fingerprint(),
            ReferenceRole::Source,
        )
    }

    fn tear_off_command(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        target: RootPresentationTarget,
        new_root: crate::ids::RootId,
    ) -> Result<WorkspaceCommand, EngineError> {
        let complete_root =
            self.complete_root_source(payload)
                .map_err(|_| EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                })?;
        Ok(match (complete_root, target) {
            (Some(source), target) => WorkspaceCommand::RehomeRoot { source, target },
            (None, RootPresentationTarget::Surface { surface }) => {
                WorkspaceCommand::CreateSurfaceRoot {
                    surface,
                    root: new_root,
                    content: RootContent::Move(payload.clone()),
                }
            }
            (
                None,
                RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    z_order,
                },
            ) => WorkspaceCommand::CreateContainedRoot {
                surface,
                root: new_root,
                floating,
                rect,
                z_order,
                content: RootContent::Move(payload.clone()),
            },
        })
    }

    fn tear_off_root_identity_matches(
        &self,
        input: InputSequence,
        payload: &MovePayload,
        requested: crate::ids::RootId,
    ) -> Result<bool, EngineError> {
        Ok(self
            .complete_root_source(payload)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?
            .is_none_or(|source| source.root() == requested))
    }

    fn complete_root_source(
        &self,
        payload: &MovePayload,
    ) -> Result<Option<crate::command::NodeSource>, crate::error::CommandError> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        let record = self
            .workspace
            .root(root)
            .ok_or(crate::error::CommandError::MissingRoot { root })?;
        let complete = match payload {
            MovePayload::Item(_) => {
                record.central.is_none()
                    && self.workspace.collect_items_in_subtree(record.node).len() == 1
            }
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                source.node() == record.node
            }
        };
        if complete {
            self.workspace
                .capture_node_source(root, record.node)
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn payload_surface(&self, payload: &MovePayload) -> Option<crate::ids::SurfaceId> {
        let root = match payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        self.root_surface(root)
    }

    fn root_surface(&self, root: crate::ids::RootId) -> Option<crate::ids::SurfaceId> {
        match self.workspace.presentation_for_root(root)? {
            crate::workspace::RootPresentation::Main { surface }
            | crate::workspace::RootPresentation::Contained { surface, .. } => Some(surface),
        }
    }

    fn preflight_command(
        &self,
        input: InputSequence,
        command: &WorkspaceCommand,
    ) -> Result<CommandApplication, EngineError> {
        let mut candidate = self.workspace.clone();
        Self::run_command_transaction(input, &mut candidate, &self.policy, command)
    }

    fn apply_interaction_command(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let application =
            Self::run_command_transaction(input, &mut self.workspace, &self.policy, command)?;
        if let CommandApplication::Applied { outcome, changed } = &application
            && *changed
        {
            self.advance_revision(input)?;
            self.scene = None;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(application)
    }

    fn run_command_transaction(
        input: InputSequence,
        workspace: &mut Workspace,
        policy: &DockPolicy,
        command: &WorkspaceCommand,
    ) -> Result<CommandApplication, EngineError> {
        let report =
            match WorkspaceTransaction::from_commands([command.clone()]).apply(workspace, policy) {
                Ok(report) => report,
                Err(TransactionError::Command { index: 0, source })
                    if source.is_expected_rejection() =>
                {
                    return Ok(CommandApplication::Rejected(source));
                }
                Err(source) => return Err(EngineError::Command { input, source }),
            };
        let changed = report.changed();
        let outcome = report
            .into_outcomes()
            .into_iter()
            .next()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        Ok(CommandApplication::Applied { outcome, changed })
    }

    fn reduce_workspace_command(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut self.workspace, &self.policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(InputOutcome::CommandRejected {
                    error: source,
                    version: self.version,
                });
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        let changed = report.changed();
        if changed {
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::WorkspaceChanged,
                interaction_events,
            )?;
        }
        let outcome = report
            .into_outcomes()
            .into_iter()
            .next()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        if changed {
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(InputOutcome::CommandProcessed {
            outcome,
            changed,
            version: self.version,
        })
    }

    fn candidate(&self) -> Self {
        Self {
            workspace: self.workspace.clone(),
            policy: self.policy.clone(),
            version: self.version,
            scene: self.scene.clone(),
            last_scene_generation: self.last_scene_generation,
            interaction: self.interaction.clone(),
            viewport: self.viewport.clone(),
            last_input: self.last_input,
            pending: self.pending.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DockTarget;
    use crate::drop_target::{DropTargetAvailability, DropTargetId, DropTargetRecord, DropVisual};
    use crate::geometry::{LogicalPoint, LogicalRect};
    use crate::graph::{Node, RootRecord, SurfacePresentation};
    use crate::hit_region::HitRegion;
    use crate::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch};
    use crate::intent::{PointerButton, PointerId, RendererIntent, SurfacePointer};
    use crate::interaction::{DragSessionId, InteractionOutcome, InteractionStatus};
    use crate::scene::{NodeSceneId, ReadySurfaceScene, SceneLayerKey, SemanticRect};
    use crate::transition::InputOutcome;

    const SOURCE_ROOT: RootId = RootId::new(1);
    const TARGET_ROOT: RootId = RootId::new(2);
    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(1);
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
    const TEST_POINTER: PointerId = PointerId::new(1);

    struct CounterFixture {
        engine: DockEngine,
        source_tabs: crate::ids::NodeId,
        target_tabs: crate::ids::NodeId,
    }

    fn test_rect() -> LogicalRect {
        LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rectangle must be valid")
    }

    fn counter_fixture() -> CounterFixture {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
        builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
        builder.set_surface(SOURCE_SURFACE, SurfacePresentation::new(SOURCE_ROOT));
        builder.set_surface(TARGET_SURFACE, SurfacePresentation::new(TARGET_ROOT));
        let workspace = builder.build().expect("counter workspace must be valid");
        CounterFixture {
            engine: DockEngine::new(workspace, DockPolicy::default())
                .expect("counter engine must be valid"),
            source_tabs,
            target_tabs,
        }
    }

    fn ready_counter_scene(fixture: &CounterFixture) -> BuildingScene {
        let mut building = BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE])
            .expect("counter roster must be unique");
        building
            .insert_ready(ReadySurfaceScene::new(SOURCE_SURFACE, test_rect()))
            .expect("source facts must be unique");
        let target = fixture
            .engine
            .workspace()
            .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
            .expect("target must be current");
        let mut target_scene = ReadySurfaceScene::new(TARGET_SURFACE, test_rect());
        target_scene.push_drop_target(DropTargetRecord::new(
            DropTargetId::Center {
                surface: TARGET_SURFACE,
                root: TARGET_ROOT,
                tabs: fixture.target_tabs,
            },
            DockTarget::Center(target),
            DropTargetAvailability::Available,
            HitRegion::new(test_rect()),
            SceneLayerKey::new(0),
            DropVisual::new(test_rect()),
        ));
        building
            .insert_ready(target_scene)
            .expect("target facts must be unique");
        building
    }

    fn begin_counter_drag(fixture: &mut CounterFixture) -> DragSessionId {
        let payload = MovePayload::Item(
            fixture
                .engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
                .expect("source must be current"),
        );
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::ArmDrag {
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
                payload,
            })
            .expect("arm sequence must be available");
        let armed = fixture.engine.reduce_pending().expect("arm must reduce");
        let session = match armed.reduced_inputs()[0].outcome() {
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::DragArmed { session, .. },
                ..
            } => *session,
            outcome => panic!("unexpected arm outcome: {outcome:?}"),
        };
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::BeginDrag {
                session,
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
            })
            .expect("begin sequence must be available");
        fixture.engine.reduce_pending().expect("begin must reduce");
        session
    }

    #[test]
    fn fatal_counter_exhaustion_rolls_back_the_complete_boundary() {
        let root = RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::new(root));
        let workspace = builder.build().expect("test workspace must be valid");
        let command = WorkspaceCommand::Select {
            source: workspace
                .capture_item_source(root, tabs, ItemId::new(2))
                .expect("source must be capturable"),
        };
        let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
            .expect("engine must be valid");
        engine.version = WorkspaceVersion::new(
            WorkspaceEpoch::default(),
            WorkspaceRevision::new(u64::MAX - 1),
        );
        let expected = engine.version;
        let mut policy = engine.policy.clone();
        policy.set_allow_native_surfaces(true);
        engine
            .enqueue(EngineInput::ReplacePolicy { expected, policy })
            .expect("first sequence must be available");
        engine
            .enqueue(EngineInput::WorkspaceCommand { expected, command })
            .expect("second sequence must be available");
        let pending = engine.pending.clone();

        assert!(matches!(
            engine.reduce_pending(),
            Err(EngineError::WorkspaceRevisionExhausted { .. })
        ));
        assert_eq!(engine.workspace, workspace);
        assert!(!engine.policy.allows_native_surfaces());
        assert_eq!(engine.version, expected);
        assert_eq!(engine.pending, pending);
    }

    #[test]
    fn scene_generation_exhaustion_rolls_back_scene_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture.engine.last_scene_generation = SceneGeneration::new(u64::MAX);
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("scene sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::SceneGenerationExhausted { .. })
        ));
        assert_eq!(fixture.engine, before);
    }

    #[test]
    fn drag_generation_exhaustion_rolls_back_gesture_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture.engine.interaction.exhaust_drag_generation();
        let payload = MovePayload::Item(
            fixture
                .engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
                .expect("source must be current"),
        );
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::ArmDrag {
                pointer: TEST_POINTER,
                button: PointerButton::Primary,
                payload,
            })
            .expect("arm sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::Interaction {
                source: InteractionCounterError::DragGenerationExhausted,
                ..
            })
        ));
        assert_eq!(fixture.engine, before);
        assert_eq!(
            fixture.engine.interaction().status(),
            InteractionStatus::Idle
        );
    }

    #[test]
    fn preview_sequence_exhaustion_rolls_back_observation_and_pending_inputs() {
        let mut fixture = counter_fixture();
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("scene sequence must be available");
        fixture.engine.reduce_pending().expect("scene must publish");
        let session = begin_counter_drag(&mut fixture);
        fixture.engine.interaction.exhaust_preview_sequence();
        fixture
            .engine
            .enqueue_renderer_intent(RendererIntent::UpdateDrag {
                session,
                target: TargetAuthority::local(
                    TARGET_SURFACE,
                    Authority::Known(Some(SurfacePointer::new(
                        TARGET_SURFACE,
                        LogicalPoint::new(50.0, 50.0).expect("test point must be valid"),
                    ))),
                ),
                tear_off: None,
            })
            .expect("update sequence must be available");
        let before = fixture.engine.candidate();

        assert!(matches!(
            fixture.engine.reduce_pending(),
            Err(EngineError::Interaction {
                source: InteractionCounterError::PreviewSequenceExhausted,
                ..
            })
        ));
        assert_eq!(fixture.engine, before);
        assert!(fixture.engine.interaction().preview().is_none());
    }

    #[test]
    fn malformed_scene_does_not_consume_the_boundary_publication_slot() {
        let mut fixture = counter_fixture();
        let mut malformed = BuildingScene::new([SOURCE_SURFACE, TARGET_SURFACE])
            .expect("malformed roster must be unique");
        let mut source = ReadySurfaceScene::new(SOURCE_SURFACE, test_rect());
        source.push_node(SemanticRect::new(
            NodeSceneId {
                root: TARGET_ROOT,
                node: fixture.target_tabs,
            },
            test_rect(),
            SceneLayerKey::new(0),
        ));
        malformed
            .insert_ready(source)
            .expect("malformed facts are structurally unique");
        fixture
            .engine
            .enqueue_scene(malformed)
            .expect("malformed scene sequence must be available");
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("valid scene sequence must be available");

        let transition = fixture
            .engine
            .reduce_pending()
            .expect("malformed scene is a nonfatal rejection");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::SceneRejected {
                error: SceneBuildError::InvalidNodeSemantic { .. }
            }
        ));
        assert!(matches!(
            transition.reduced_inputs()[1].outcome(),
            InputOutcome::ScenePublished { stamp, .. }
                if stamp.generation() == SceneGeneration::new(1)
        ));
    }

    #[test]
    fn only_one_valid_scene_can_publish_in_a_boundary() {
        let mut fixture = counter_fixture();
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("first scene sequence must be available");
        fixture
            .engine
            .enqueue_scene(ready_counter_scene(&fixture))
            .expect("second scene sequence must be available");

        let transition = fixture
            .engine
            .reduce_pending()
            .expect("duplicate publication is a nonfatal rejection");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::ScenePublished { .. }
        ));
        assert!(matches!(
            transition.reduced_inputs()[1].outcome(),
            InputOutcome::SceneRejected {
                error: SceneBuildError::AlreadyPublishedInBoundary
            }
        ));
        assert_eq!(
            fixture
                .engine
                .scene()
                .expect("first scene must remain published")
                .stamp()
                .generation(),
            SceneGeneration::new(1)
        );
    }
}
