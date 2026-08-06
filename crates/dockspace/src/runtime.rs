//! Narrow renderer-neutral runtime facade for application host frames.
//!
//! This module is the migration boundary for adapters which must not own or
//! access [`crate::engine::DockEngine`] directly. It supports durable commands,
//! close decisions, complete measurement answers, exact paint settlement, and
//! lossless surface-local pointer batches backed by concrete final-presentation
//! authority.

mod interaction;
mod native;
mod paint;
mod presentation;

pub use interaction::{
    DockspaceInteractionError, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePointerRetirement, SurfaceScrollCancelReason, SurfaceScrollDelta, SurfaceScrollDeviceId,
    SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase,
    SurfaceScrollSequenceId,
};
pub use native::{
    HostWindowToken, NativeCloseState, NativePlatformError, NativePlatformSnapshot,
    NativeSurfaceLease, NativeWindowFacts,
};
pub use paint::{
    ContainedPaintRecord, ContainedResizePaintRecord, DockspaceDragPreview, DockspaceGuideScope,
    DockspacePaintLayer, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceVisualId, DockspaceVisualKind, DropGuidePaintRecord,
    DropGuideTargetPaintRecord, PanePaintRecord, SplitterJunctionPaintRecord, SplitterPaintRecord,
    SurfacePaintPlan, TabBarPaintRecord, TabPaintRecord, TabStripMemberPaintRecord,
    UniformSurfaceMetrics,
};
pub use presentation::{
    PaintedSurfaceOutput, PresentationObservationError, PresentationSettlementError,
    PresentationSettlementRejection, SurfacePresentationResult,
};

use std::collections::BTreeSet;

use thiserror::Error;

use crate::command::{CloseCommitOutcome, CommandOutcome, ContentCloseTarget, WorkspaceCommand};
use crate::engine::{
    CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
    HostPresentationUnavailableReason, SurfaceContributionBeginError,
    SurfaceContributionPrepareError,
};
use crate::error::CommandError;
use crate::graph::Workspace;
use crate::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use crate::interaction::InteractionOutcome;
use crate::presentation_observation::PresentationHostLease;
use crate::scene_manifest::MeasurementUnavailableReason;
use crate::transition::{ContentCloseRequestRejection, InputOutcome, WorkspaceVersion};
use crate::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use crate::{ClosePlan, CloseResolutionOutcome};

const APPLICATION_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x64_6f_63_6b_73_70_61_63);

/// Renderer-neutral owner of one docking workspace and one application host.
///
/// The engine and presentation-host lease remain private. A caller can inspect
/// the published workspace and mutate it only through an affine host frame.
#[derive(Debug)]
pub struct DockspaceSession {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    presentation: presentation::RuntimePresentationState,
    pointer: Option<interaction::RuntimePointerState>,
    native: Option<native::RuntimeNativeState>,
    committed_source_sequence: u64,
}

impl DockspaceSession {
    /// Creates one session from a strictly validated workspace and policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the workspace is invalid or the core cannot mint
    /// the private presentation-host identity.
    pub fn new(
        workspace: Workspace,
        policy: crate::policy::DockPolicy,
    ) -> Result<Self, DockspaceRuntimeError> {
        let mut engine = DockEngine::new(workspace, policy)?;
        let presentation_host = engine.create_presentation_host()?;
        Ok(Self {
            engine,
            presentation_host,
            presentation: presentation::RuntimePresentationState::default(),
            pointer: None,
            native: None,
            committed_source_sequence: 0,
        })
    }

    /// Returns the currently published, strictly validated workspace.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        self.engine.workspace()
    }

    /// Returns the current durable workspace version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.engine.version()
    }

    /// Begins one affine application host frame.
    ///
    /// Pending output observations are supplied from the facade-owned
    /// presentation sidecar before input authority is frozen.
    ///
    /// # Errors
    ///
    /// Returns an error when presentation output is awaiting an explicit host
    /// observation or the core rejects the frame prelude.
    pub fn begin_host_frame(&mut self) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.reconcile_surface_pointer_provider()?;
        let mut prelude = self.engine.begin_host_frame(self.presentation_host)?;
        let submitted_presentation = self.presentation.submit_observation(&mut prelude)?;
        let frame = prelude.seal(&self.engine)?;
        let next_source_sequence = self.committed_source_sequence;
        let next_pointer_sequence = self
            .pointer
            .as_ref()
            .map(interaction::RuntimePointerState::committed_sequence);
        let next_native_close_generations = self.native.as_ref().map_or_else(
            Default::default,
            native::RuntimeNativeState::close_generations,
        );
        Ok(DockspaceHostFrame {
            session: self,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: false,
            native_snapshot_commit: None,
            next_native_close_generations,
            submitted_presentation,
            painted_surfaces: BTreeSet::new(),
        })
    }
}

/// One affine, rollbackable application host frame.
///
/// Dropping this value discards the private candidate and leaves the published
/// session unchanged.
#[must_use = "dropping a host frame rolls back its uncommitted candidate"]
pub struct DockspaceHostFrame<'session> {
    session: &'session mut DockspaceSession,
    frame: CoreHostFrame,
    next_source_sequence: u64,
    next_pointer_sequence: Option<u64>,
    pointer_input_submitted: bool,
    native_snapshot_commit: Option<native::NativeSnapshotCommit>,
    next_native_close_generations:
        std::collections::BTreeMap<crate::viewport::ViewportBinding, u64>,
    submitted_presentation: presentation::SubmittedPresentationObservation,
    painted_surfaces: BTreeSet<SurfaceId>,
}

impl DockspaceHostFrame<'_> {
    /// Returns the post-input candidate workspace visible inside this frame.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        self.frame.view().workspace()
    }

    /// Returns the complete post-input logical surface roster.
    #[must_use]
    pub fn surfaces(&self) -> Vec<SurfaceId> {
        self.frame.surfaces().collect()
    }

    /// Appends one checked durable command in caller order.
    ///
    /// A structurally accepted input can still produce a typed command
    /// rejection in [`HostFrameReport`].
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn submit_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.frame.view().version();
        self.append(EngineInput::WorkspaceCommand { expected, command })
    }

    /// Opens one core-owned close plan for stable application content.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn request_close(
        &mut self,
        target: ContentCloseTarget,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.frame.view().version();
        self.append(EngineInput::RequestContentClose { expected, target })
    }

    /// Resolves one initial close decision token.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn resolve_close(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::ResolveClose {
            request,
            token,
            decision,
        })
    }

    /// Resolves one deferred close continuation.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn continue_deferred_close(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::ContinueDeferredClose {
            request,
            token,
            decision,
        })
    }

    /// Explicitly marks one frozen surface unavailable for this presentation pass.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is outside the frame roster, was
    /// already answered, or its contribution authority became stale.
    pub fn defer_surface(
        &mut self,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let token = self
            .frame
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceRuntimeError::SurfaceContributionBegin)?;
        let contribution = self
            .frame
            .view()
            .prepare_surface_unavailable_contribution(token, reason)?;
        self.frame.push_surface_contribution(contribution)?;
        Ok(())
    }

    /// Atomically publishes the frame after every surface received an explicit
    /// contribution disposition.
    ///
    /// # Errors
    ///
    /// Returns an error when the contribution roster is incomplete or the core
    /// rejects the final candidate.
    pub fn commit(mut self) -> Result<HostFrameReport, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let Self {
            session,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: _,
            native_snapshot_commit,
            next_native_close_generations,
            submitted_presentation,
            painted_surfaces,
        } = self;
        let mut frame = frame.into_presentation()?;
        let mut obligations = frame.take_presentation_obligations()?;
        for surface in painted_surfaces {
            let index = obligations
                .iter()
                .position(|obligation| {
                    obligation.slot() == (crate::engine::HostPresentationSlot::Surface { surface })
                })
                .ok_or(DockspaceRuntimeError::PaintObligationUnavailable { surface })?;
            let obligation = obligations.swap_remove(index);
            let token = frame.view().begin_surface_contribution(surface)?;
            let interaction = frame
                .view()
                .presentation_interaction(surface)
                .ok_or(DockspaceRuntimeError::PaintObligationUnavailable { surface })?;
            frame.record_painted_surface_contribution(obligation, token, interaction)?;
        }
        for obligation in obligations {
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        let transition = frame.finish(&mut session.engine)?;
        session.committed_source_sequence = next_source_sequence;
        if let (Some(pointer), Some(sequence)) = (&session.pointer, next_pointer_sequence) {
            assert_eq!(
                pointer.committed_sequence(),
                sequence,
                "core commit must advance the exact runtime pointer producer watermark",
            );
        }
        if let Some(native) = &mut session.native {
            native.commit(
                &session.engine,
                &transition,
                native_snapshot_commit,
                next_native_close_generations,
            );
        }
        session
            .presentation
            .commit_observation(&transition, &submitted_presentation);
        let painted_outputs = session
            .presentation
            .retain_emissions(transition.presentation_emissions());
        Ok(HostFrameReport::from_transition(
            &transition,
            painted_outputs,
        ))
    }

    fn append(&mut self, input: EngineInput) -> Result<(), DockspaceRuntimeError> {
        self.next_source_sequence = self
            .next_source_sequence
            .checked_add(1)
            .ok_or(DockspaceRuntimeError::SourceSequenceExhausted)?;
        let sequence = SourceSequence::new(self.next_source_sequence);
        self.frame
            .append_input(APPLICATION_INPUT_SOURCE, sequence, input)?;
        Ok(())
    }
}

/// Public actionable result produced by one facade-owned input.
#[derive(Debug, Clone, PartialEq)]
pub enum HostInputOutcome {
    /// One checked durable command applied or produced a valid no-op.
    CommandApplied {
        /// Structured command result.
        outcome: CommandOutcome,
        /// Whether the complete workspace changed.
        changed: bool,
    },
    /// One checked durable command was consumed without mutation.
    CommandRejected(CommandError),
    /// One content-close request opened or reused a plan.
    CloseRequested {
        /// Read-only core-owned close plan.
        plan: ClosePlan,
        /// Whether an unresolved plan was reused.
        reused: bool,
        /// Product-level source of the close request.
        origin: HostCloseRequestOrigin,
    },
    /// One content-close request was rejected without mutation.
    CloseRejected {
        /// Stable requested target.
        target: ContentCloseTarget,
        /// Typed fail-closed rejection.
        reason: ContentCloseRequestRejection,
    },
    /// One close decision was consumed.
    CloseDecisionProcessed {
        /// Exact token-resolution result, including typed inert outcomes.
        resolution: CloseResolutionOutcome,
        /// Latest retained close plan, when the request exists.
        plan: Option<ClosePlan>,
        /// Checked topology result after the final allow.
        application: Option<Result<CloseCommitOutcome, CommandError>>,
        /// Whether this input changed durable topology.
        changed: bool,
    },
    /// One existing native root window was bound to a logical surface.
    NativeSurfaceRegistered {
        /// Opaque exact-incarnation capability for later platform facts.
        lease: NativeSurfaceLease,
    },
    /// Native registration named a surface which was not admissible.
    NativeSurfaceRegistrationRejected {
        /// Stable logical surface rejected by the core.
        surface: SurfaceId,
    },
    /// One complete native platform snapshot was atomically applied.
    NativePlatformSnapshotApplied,
    /// One exact live-binding close observation was atomically applied.
    NativeCloseObservationApplied,
    /// Native facts captured against an older workspace epoch were inert.
    NativePlatformSnapshotStale,
    /// Native ingress from a superseded provider was inert.
    NativePlatformProviderRejected,
    /// Input derived from an older workspace version was consumed inertly.
    StaleRejected {
        /// Version supplied by the facade at append time.
        expected: WorkspaceVersion,
        /// Frame-local version accepted by the reducer.
        accepted: WorkspaceVersion,
    },
}

/// Product-level origin of one close request.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCloseRequestOrigin {
    /// The application explicitly requested a content close.
    Application,
    /// A presented pointer, keyboard, or accessibility receiver requested close.
    Interaction,
}

/// High-level report from one atomically published facade frame.
#[derive(Debug, PartialEq)]
pub struct HostFrameReport {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    inputs: Vec<HostInputOutcome>,
    painted_outputs: Vec<PaintedSurfaceOutput>,
    repaint_surfaces: Vec<SurfaceId>,
}

impl HostFrameReport {
    fn from_transition(
        transition: &crate::transition::EngineTransition,
        painted_outputs: Vec<PaintedSurfaceOutput>,
    ) -> Self {
        let mut ordered_inputs = Vec::new();
        for reduced in transition.reduced_inputs() {
            let outcome = match reduced.outcome() {
                InputOutcome::CommandProcessed {
                    outcome, changed, ..
                } => Some(HostInputOutcome::CommandApplied {
                    outcome: outcome.clone(),
                    changed: *changed,
                }),
                InputOutcome::CommandRejected { error, .. } => {
                    Some(HostInputOutcome::CommandRejected(error.clone()))
                }
                InputOutcome::ContentCloseRequested { plan, reused, .. } => {
                    Some(HostInputOutcome::CloseRequested {
                        plan: plan.clone(),
                        reused: *reused,
                        origin: HostCloseRequestOrigin::Application,
                    })
                }
                InputOutcome::ContentCloseRejected { target, reason, .. } => {
                    Some(HostInputOutcome::CloseRejected {
                        target: *target,
                        reason: reason.clone(),
                    })
                }
                InputOutcome::CloseDecisionProcessed {
                    resolution,
                    plan,
                    application,
                    changed,
                    ..
                } => Some(HostInputOutcome::CloseDecisionProcessed {
                    resolution: *resolution,
                    plan: plan.clone(),
                    application: application.clone(),
                    changed: *changed,
                }),
                InputOutcome::ViewportRegistered { binding } => {
                    Some(HostInputOutcome::NativeSurfaceRegistered {
                        lease: NativeSurfaceLease::from_binding(*binding),
                    })
                }
                InputOutcome::ViewportRegistrationRejected { surface } => {
                    Some(HostInputOutcome::NativeSurfaceRegistrationRejected { surface: *surface })
                }
                InputOutcome::PlatformSnapshotPublished { .. } => {
                    Some(HostInputOutcome::NativePlatformSnapshotApplied)
                }
                InputOutcome::NativeCloseObservationPublished { .. } => {
                    Some(HostInputOutcome::NativeCloseObservationApplied)
                }
                InputOutcome::PlatformSnapshotStale { .. } => {
                    Some(HostInputOutcome::NativePlatformSnapshotStale)
                }
                InputOutcome::PlatformProviderRejected { .. } => {
                    Some(HostInputOutcome::NativePlatformProviderRejected)
                }
                InputOutcome::StaleRejected {
                    expected,
                    accepted_base,
                } => Some(HostInputOutcome::StaleRejected {
                    expected: *expected,
                    accepted: *accepted_base,
                }),
                InputOutcome::InteractionProcessed { outcome, .. } => {
                    interaction_close_request(outcome)
                }
                _ => None,
            };
            if let Some(outcome) = outcome {
                ordered_inputs.push((reduced.causal_ordinal().get(), 0_usize, 0_usize, outcome));
            }
        }
        for (edge_index, edge) in transition.reduced_pointer_edges().iter().enumerate() {
            for (outcome_index, outcome) in edge.interaction_outcomes().iter().enumerate() {
                if let Some(outcome) = interaction_close_request(outcome) {
                    ordered_inputs.push((
                        edge.causal_ordinal().get(),
                        edge_index,
                        outcome_index,
                        outcome,
                    ));
                }
            }
        }
        let inputs = finish_ordered_inputs(ordered_inputs);
        let repaint_surfaces = transition
            .affected_surfaces()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            before: transition.before(),
            after: transition.after(),
            inputs,
            painted_outputs,
            repaint_surfaces,
        }
    }

    /// Returns the workspace version before the frame.
    #[must_use]
    pub const fn before(&self) -> WorkspaceVersion {
        self.before
    }

    /// Returns the published workspace version after the frame.
    #[must_use]
    pub const fn after(&self) -> WorkspaceVersion {
        self.after
    }

    /// Returns facade-owned outcomes in exact reducer causal order.
    ///
    /// Semantic inputs and actionable pointer results share this sequence.
    /// Pointer edge and outcome indices preserve reducer order when one journal
    /// segment contains multiple edges or one edge produces multiple results.
    #[must_use]
    pub fn inputs(&self) -> &[HostInputOutcome] {
        &self.inputs
    }

    /// Takes the affine capabilities for outputs actually painted by this frame.
    ///
    /// The host must consume each capability through
    /// [`DockspaceSession::settle_presentation`] only after its renderer reports
    /// whether that exact output was presented or dropped.
    pub fn take_painted_outputs(&mut self) -> Vec<PaintedSurfaceOutput> {
        std::mem::take(&mut self.painted_outputs)
    }

    /// Returns every logical surface whose presentation authority changed.
    ///
    /// The roster is sorted and deduplicated. A renderer should schedule a new
    /// paint for each entry; it must not infer repaint from callback absence.
    #[must_use]
    pub fn repaint_surfaces(&self) -> &[SurfaceId] {
        &self.repaint_surfaces
    }
}

fn interaction_close_request(outcome: &InteractionOutcome) -> Option<HostInputOutcome> {
    match outcome {
        InteractionOutcome::CloseRequested { plan, reused } => {
            Some(HostInputOutcome::CloseRequested {
                plan: plan.clone(),
                reused: *reused,
                origin: HostCloseRequestOrigin::Interaction,
            })
        }
        _ => None,
    }
}

fn finish_ordered_inputs(
    mut ordered: Vec<(u64, usize, usize, HostInputOutcome)>,
) -> Vec<HostInputOutcome> {
    ordered.sort_by_key(|(ordinal, edge, outcome, _)| (*ordinal, *edge, *outcome));
    ordered
        .into_iter()
        .map(|(_, _, _, outcome)| outcome)
        .collect()
}

/// Failure at the renderer-neutral facade boundary.
#[derive(Debug, Error)]
pub enum DockspaceRuntimeError {
    /// Core engine construction or frame publication failed.
    #[error(transparent)]
    Engine(Box<EngineError>),
    /// The affine host-frame protocol rejected one operation.
    #[error(transparent)]
    HostFrame(Box<CoreHostFrameError>),
    /// The private application input stream cannot advance without wrapping.
    #[error("application input source sequence is exhausted")]
    SourceSequenceExhausted,
    /// The host claimed a paint for a surface without a matching core slot.
    #[error("surface {surface} has no paintable presentation obligation in this frame")]
    PaintObligationUnavailable {
        /// Surface named by the paint claim.
        surface: SurfaceId,
    },
    /// A surface contribution could not begin under the frozen roster.
    #[error(transparent)]
    SurfaceContributionBegin(#[from] SurfaceContributionBeginError),
    /// A surface contribution answer was stale or failed compilation.
    #[error(transparent)]
    SurfaceContributionPrepare(Box<SurfaceContributionPrepareError>),
    /// A renderer-neutral interaction capability was structurally invalid.
    #[error(transparent)]
    Interaction(#[from] DockspaceInteractionError),
    /// The facade presentation sidecar could not synchronize with the core.
    #[error(transparent)]
    PresentationObservation(#[from] PresentationObservationError),
    /// Native lifecycle data was stale, incomplete, or structurally invalid.
    #[error(transparent)]
    Native(#[from] NativePlatformError),
}

impl From<EngineError> for DockspaceRuntimeError {
    fn from(error: EngineError) -> Self {
        Self::Engine(Box::new(error))
    }
}

impl From<CoreHostFrameError> for DockspaceRuntimeError {
    fn from(error: CoreHostFrameError) -> Self {
        Self::HostFrame(Box::new(error))
    }
}

impl From<SurfaceContributionPrepareError> for DockspaceRuntimeError {
    fn from(error: SurfaceContributionPrepareError) -> Self {
        Self::SurfaceContributionPrepare(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_surface_session() -> DockspaceSession {
        let item = crate::ids::ItemId::new(1);
        let root = crate::ids::RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(crate::graph::Node::tabs([item]));
        builder.set_root(root, crate::graph::RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, crate::graph::SurfacePresentation::with_main(root));
        DockspaceSession::new(
            builder.build().expect("runtime test workspace validates"),
            crate::policy::DockPolicy::default(),
        )
        .expect("runtime test session initializes")
    }

    #[test]
    fn shared_pointer_ordinal_preserves_edge_then_outcome_order() {
        let outcomes = finish_ordered_inputs(vec![
            (7, 1, 0, HostInputOutcome::NativePlatformSnapshotStale),
            (7, 0, 1, HostInputOutcome::NativeCloseObservationApplied),
            (7, 0, 0, HostInputOutcome::NativePlatformSnapshotApplied),
        ]);

        assert!(matches!(
            outcomes.as_slice(),
            [
                HostInputOutcome::NativePlatformSnapshotApplied,
                HostInputOutcome::NativeCloseObservationApplied,
                HostInputOutcome::NativePlatformSnapshotStale,
            ]
        ));
    }

    #[test]
    fn runtime_surface_pointer_disable_drains_and_compacts_the_exact_producer() {
        let mut session = single_surface_session();
        let surface = SurfaceId::new(1);
        let metrics = UniformSurfaceMetrics::new(
            crate::geometry::LogicalRect::new(0.0, 0.0, 640.0, 480.0)
                .expect("runtime test bounds validate"),
            crate::geometry::LogicalSize::new(32.0, 24.0).expect("runtime test minimum validates"),
            80.0,
        )
        .expect("runtime test metrics validate");
        let mut measured = session
            .begin_host_frame()
            .expect("runtime measurement frame begins");
        measured
            .measure_surface(surface, metrics)
            .expect("runtime surface measurements stage");
        measured
            .commit()
            .expect("runtime surface measurements commit");
        let mut painted = session
            .begin_host_frame()
            .expect("runtime paint frame begins");
        painted
            .confirm_surface_painted(surface)
            .expect("runtime surface paint stages");
        let mut report = painted.commit().expect("runtime surface paint emits");
        let output = report
            .take_painted_outputs()
            .pop()
            .expect("runtime paint emits one exact output");
        session
            .settle_presentation(output, SurfacePresentationResult::Presented)
            .expect("runtime renderer presents the exact output");
        let mut observed = session
            .begin_host_frame()
            .expect("runtime presentation observation frame begins");
        observed
            .complete_unpainted_surfaces(
                crate::scene_manifest::MeasurementUnavailableReason::Deferred,
            )
            .expect("runtime presentation observation settles the surface roster");
        observed
            .commit()
            .expect("runtime presentation observation commits");
        session
            .enable_surface_pointer(surface)
            .expect("runtime surface-local producer mints");
        let lease = session
            .engine
            .pointer_provider()
            .expect("runtime retains the active producer lease");

        let retirement = session
            .disable_surface_pointer()
            .expect("runtime producer retires atomically")
            .expect("the active producer returns one retirement result");
        assert_eq!(retirement.surface(), surface);
        assert!(!retirement.interaction_changed());
        assert!(retirement.repaint_required());
        assert_eq!(session.engine.pointer_provider(), None);
        assert!(session.pointer.is_none());
        assert_eq!(
            session
                .disable_surface_pointer()
                .expect("repeated disable is an inert no-op"),
            None
        );
        assert!(matches!(
            session.engine.retire_pointer_provider(lease),
            Err(EngineError::SurfaceLocalPointerProducerRequired)
        ));
    }
}
