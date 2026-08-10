//! Narrow renderer-neutral runtime facade for application host frames.
//!
//! This module is the migration boundary for adapters which must not own or
//! access the backend reducer directly. It supports revision-bound product actions,
//! close decisions, complete measurement answers, exact paint settlement, and
//! lossless surface-local pointer batches backed by concrete final-presentation
//! authority.

mod interaction;
mod measurement;
mod native;
mod native_effect;
mod paint;
mod presentation;

use native::NativePlatformError;

pub use crate::model::{PreparedDockAction, WorkspaceVersion};
pub use crate::transition::{ContentCloseRequestRejection, SurfaceCloseRequestRejection};
pub use interaction::{
    DockspaceInteractionError, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePointerRetirement, SurfaceScrollCancelReason, SurfaceScrollDelta, SurfaceScrollDeviceId,
    SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase,
    SurfaceScrollSequenceId,
};
pub use measurement::{
    MeasurementValueError, SurfaceMeasurementAnswer, SurfaceMeasurementRequest, TabListMenuMetrics,
    TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    UniformSurfaceMetrics,
};
pub use native::{
    HostWindowToken, HostWorkAreaToken, NativeCloseState, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeHostErrorKind, NativePointerButton, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativePointerState, NativeProjectedScrollDelta, NativeReceiverAnswer,
    NativeReceiverPurpose, NativeReceiverQuery, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollReceiverChallenge, NativeScrollSequenceId, NativeSurfaceBinding,
    NativeSurfaceCloseRequest, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaBinding, NativeWorkAreaFacts,
    NativeWorkAreaRoster,
};
pub use native_effect::{
    NativeCleanupCorrelationFailure, NativeCleanupObservation, NativeCloseEffectAcknowledgement,
    NativeDispatchFailure, NativeEffectAcknowledgement, NativeEffectOperation, NativeEffectRequest,
    NativeEffectResult, NativeEffectSubmissionError, NativeIndeterminateReason,
    NativeInputEffectAcknowledgement, NativePresentationEffectAcknowledgement, NativeSurfaceRole,
    NativeUnsupportedReason,
};
pub use paint::{
    ContainedPaintRecord, ContainedResizePaintRecord, DockspaceDragPreview, DockspaceGuideScope,
    DockspacePaintLayer, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceVisualId, DockspaceVisualKind, DropGuidePaintRecord,
    DropGuideTargetPaintRecord, PanePaintRecord, SplitterJunctionPaintRecord, SplitterPaintRecord,
    SurfacePaintPlan, TabBarPaintRecord, TabPaintRecord, TabStripMemberPaintRecord,
};
use presentation::PresentationObservationError;
pub use presentation::{
    PaintedSurfaceOutput, SurfacePresentationReportError, SurfacePresentationResult,
};

use std::collections::BTreeSet;

use thiserror::Error;

use crate::command::{CloseCommitOutcome, ContentCloseTarget};
#[cfg(any(feature = "backend", test))]
use crate::command::{CommandOutcome, WorkspaceCommand};
use crate::engine::{
    CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
    HostPresentationUnavailableReason, SurfaceContributionBeginError,
    SurfaceContributionPrepareError,
};
use crate::error::CommandError;
#[cfg(any(feature = "backend", test))]
use crate::graph::Workspace;
use crate::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use crate::interaction::InteractionOutcome;
use crate::model::{
    DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceLayout,
    DockspaceView, PreparedDockActionAuthorityMismatch,
};
use crate::presentation_observation::PresentationHostLease;
use crate::scene_manifest::MeasurementUnavailableReason;
use crate::transition::InputOutcome;
use crate::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use crate::{ClosePlan, CloseResolutionOutcome, SurfaceCloseRequest};

const APPLICATION_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x64_6f_63_6b_73_70_61_63);

/// Why a host could not provide one surface's measurements in the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceUnavailableReason {
    /// The active renderer cannot measure the required values.
    Unsupported,
    /// The owning surface cannot currently be measured.
    SurfaceUnavailable,
    /// Application content required for measurement is not currently available.
    ContentUnavailable,
    /// Font or text shaping data is not ready for this contribution.
    TextMetricsUnavailable,
    /// The host intentionally deferred the contribution to a later frame.
    Deferred,
}

impl From<SurfaceUnavailableReason> for MeasurementUnavailableReason {
    fn from(reason: SurfaceUnavailableReason) -> Self {
        match reason {
            SurfaceUnavailableReason::Unsupported => Self::Unsupported,
            SurfaceUnavailableReason::SurfaceUnavailable => Self::SurfaceUnavailable,
            SurfaceUnavailableReason::ContentUnavailable => Self::ContentUnavailable,
            SurfaceUnavailableReason::TextMetricsUnavailable => Self::TextMetricsUnavailable,
            SurfaceUnavailableReason::Deferred => Self::Deferred,
        }
    }
}

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
    abandoned_native_effects: native_effect::NativeEffectDropQueue,
    committed_source_sequence: u64,
}

impl DockspaceSession {
    /// Creates one product session from stable item, root, and surface layout data.
    ///
    /// Runtime node identities are allocated privately while compiling the layout.
    ///
    /// # Errors
    ///
    /// Returns an error when the core cannot mint its private presentation-host identity.
    pub fn from_layout(
        layout: DockspaceLayout,
        policy: crate::policy::DockPolicy,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace(layout.into_workspace(), policy)
    }

    /// Creates one backend session from a strictly validated runtime workspace and policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the workspace is invalid or the core cannot mint
    /// the private presentation-host identity.
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
    pub fn from_backend_workspace(
        workspace: Workspace,
        policy: crate::policy::DockPolicy,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace(workspace, policy)
    }

    fn from_workspace(
        workspace: crate::graph::Workspace,
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
            abandoned_native_effects: native_effect::NativeEffectDropQueue::default(),
            committed_source_sequence: 0,
        })
    }

    /// Returns the currently published, strictly validated workspace.
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        self.engine.workspace()
    }

    /// Returns the published item/surface-centric product view.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        self.engine.product_view()
    }

    /// Returns the current durable workspace version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.engine.version()
    }

    /// Prepares one revision-bound selection action from the published workspace.
    pub const fn prepare_select_item(&self, item: crate::ids::ItemId) -> PreparedDockAction {
        self.engine.prepare_select_item(item)
    }

    /// Prepares one revision-bound open action from the published workspace.
    pub const fn prepare_open_item(
        &self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_open_item(item, placement)
    }

    /// Prepares one revision-bound move action from the published workspace.
    pub const fn prepare_dock_item(
        &self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_dock_item(item, placement)
    }

    /// Prepares one complete-root move against the exact published workspace version.
    pub const fn prepare_dock_root(
        &self,
        root: crate::ids::RootId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_dock_root(root, placement)
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
        self.begin_host_frame_with_native_resolver(None)
    }

    /// Begins one native host frame and synchronously resolves every frozen
    /// receiver question through product-level descriptors.
    ///
    /// The callback never sees pointer leases, provider sequences, backend
    /// ordinals, candidate identities, or receipts. A failed frame retains the
    /// original native edge for an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error when no managed native host is active, presentation
    /// authority is unavailable, or the core rejects the ordered input prefix.
    pub fn begin_native_host_frame(
        &mut self,
        mut resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        let Some(native) = self.native.as_ref() else {
            return Err(NativePlatformError::ProviderUnavailable.into());
        };
        if native.profile != native::NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        self.begin_host_frame_with_native_resolver(Some(&mut resolve))
    }

    fn begin_host_frame_with_native_resolver(
        &mut self,
        mut resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.reconcile_surface_pointer_provider()?;
        if let Some(native) = self.native.as_mut() {
            native.record_abandoned_effects(&self.abandoned_native_effects)?;
            native.reclaim_committed_prefix(&mut self.engine)?;
        }
        let mut prelude = self.engine.begin_host_frame(self.presentation_host)?;
        let submitted_presentation = if let Some(native) = self.native.as_mut() {
            self.presentation.submit_backend_observation(
                &mut prelude,
                &self.engine,
                native.recorder_mut(),
            )?
        } else {
            self.presentation.submit_observation(&mut prelude)?
        };
        let frame = prelude.seal(&self.engine)?;
        let application_base = frame.view().version();
        let next_source_sequence = self.committed_source_sequence;
        let next_pointer_sequence = self
            .pointer
            .as_ref()
            .map(interaction::RuntimePointerState::committed_sequence);
        let mut host_frame = DockspaceHostFrame {
            session: self,
            frame,
            application_base,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: false,
            submitted_presentation,
            painted_surfaces: BTreeSet::new(),
        };
        if let Some(native) = host_frame.session.native.as_mut() {
            let batch = native.prepare_batch(&host_frame.session.engine)?;
            let mut progress = host_frame.frame.submit_backend_ingress(batch)?;
            while progress == crate::engine::BackendIngressProgress::ReceiverReceiptsRequired {
                let candidates = host_frame
                    .frame
                    .pointer_receiver_candidates()
                    .ok_or(NativePlatformError::ProtocolInvariant)?;
                let receipts = candidates
                    .candidates()
                    .iter()
                    .map(|candidate| {
                        let observation = if candidate.receiver_is_applicable() {
                            let resolver = resolver
                                .as_deref_mut()
                                .ok_or(NativePlatformError::ReceiverResolverRequired)?;
                            native::resolve_receiver_observation(
                                &host_frame.frame,
                                candidate,
                                resolver,
                            )?
                        } else {
                            crate::pointer_receiver::PointerReceiverObservation::NotApplicable
                        };
                        Ok(candidate.receipt(observation))
                    })
                    .collect::<Result<Vec<_>, DockspaceRuntimeError>>()?;
                let receipts = crate::pointer_receiver::PointerReceiverReceiptBatch::new(receipts)
                    .map_err(|_| NativePlatformError::ProtocolInvariant)?;
                progress = host_frame
                    .frame
                    .submit_backend_pointer_receiver_receipts(receipts)?;
            }
        }
        Ok(host_frame)
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
    application_base: WorkspaceVersion,
    next_source_sequence: u64,
    next_pointer_sequence: Option<u64>,
    pointer_input_submitted: bool,
    submitted_presentation: presentation::SubmittedPresentationObservation,
    painted_surfaces: BTreeSet<SurfaceId>,
}

impl DockspaceHostFrame<'_> {
    /// Returns the post-input product view without exposing runtime node identities.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(self.frame.view().workspace())
    }

    /// Returns the post-input candidate workspace visible inside this frame.
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
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
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
    pub fn submit_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::WorkspaceCommand {
            expected: self.application_base,
            command,
        })
    }

    /// Selects one currently open item by stable identity.
    ///
    /// The reducer resolves the current tabs source inside this rollback candidate.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn select_item_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::SelectItem {
            expected: self.frame.view().version(),
            item,
        })
    }

    /// Opens one item at a stable product placement.
    ///
    /// Opening an already owned item is a valid no-op and does not reposition it.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn open_item_current(
        &mut self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::OpenItem {
            expected: self.frame.view().version(),
            item,
            placement,
        })
    }

    /// Docks one currently open item at a stable product placement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn dock_item_current(
        &mut self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::DockItem {
            expected: self.frame.view().version(),
            item,
            placement,
        })
    }

    /// Docks one complete root at a stable product placement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn dock_root_current(
        &mut self,
        root: crate::ids::RootId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::DockRoot {
            expected: self.frame.view().version(),
            root,
            placement,
        })
    }

    /// Submits one action against the exact published revision which prepared it.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the action belongs to
    /// another dockspace session. A stale action from this session is accepted
    /// structurally and reported as [`HostInputOutcome::StaleRejected`].
    pub fn submit_prepared_action(
        &mut self,
        prepared: PreparedDockAction,
    ) -> Result<(), DockspaceRuntimeError> {
        let input = self
            .session
            .engine
            .accept_prepared_action(prepared)
            .map_err(DockspaceRuntimeError::prepared_action_authority_mismatch)?;
        self.append(input)
    }

    /// Opens or reuses one core-owned close plan for an item.
    pub fn request_close_item(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.request_close(ContentCloseTarget::Item(item))
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
        self.append(EngineInput::RequestContentClose {
            expected: self.application_base,
            target,
        })
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
        reason: SurfaceUnavailableReason,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let token = self
            .frame
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceRuntimeError::from)?;
        let contribution = self
            .frame
            .view()
            .prepare_surface_unavailable_contribution(token, reason.into())?;
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
            application_base: _,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: _,
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
                .ok_or_else(|| DockspaceRuntimeError::paint_obligation_unavailable(surface))?;
            let obligation = obligations.swap_remove(index);
            let token = frame.view().begin_surface_contribution(surface)?;
            let interaction = frame
                .view()
                .presentation_interaction(surface)
                .ok_or_else(|| DockspaceRuntimeError::paint_obligation_unavailable(surface))?;
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
            native.commit(&session.engine);
        }
        session
            .presentation
            .commit_observation(&transition, &submitted_presentation);
        let painted_outputs = session
            .presentation
            .retain_emissions(transition.presentation_emissions());
        let native_provider = session
            .native
            .as_ref()
            .map(native::RuntimeNativeState::provider);
        Ok(HostFrameReport::from_transition(
            &transition,
            painted_outputs,
            native_provider,
            session.abandoned_native_effects.clone(),
        ))
    }

    fn append(&mut self, input: EngineInput) -> Result<(), DockspaceRuntimeError> {
        self.next_source_sequence = self
            .next_source_sequence
            .checked_add(1)
            .ok_or_else(DockspaceRuntimeError::source_sequence_exhausted)?;
        let sequence = SourceSequence::new(self.next_source_sequence);
        self.frame
            .append_input(APPLICATION_INPUT_SOURCE, sequence, input)?;
        Ok(())
    }
}

/// Product-visible topology change produced by an approved close decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockspaceCloseOutcome {
    /// One item was removed from its root.
    ItemClosed {
        /// Closed item.
        item: crate::ids::ItemId,
        /// Root which owned the item before close.
        root: crate::ids::RootId,
    },
    /// One complete root and all of its items were removed atomically.
    RootClosed {
        /// Closed root.
        root: crate::ids::RootId,
        /// Items removed with the root.
        items: Vec<crate::ids::ItemId>,
    },
    /// One complete surface roster and all of its items were removed atomically.
    SurfaceClosed {
        /// Closed surface.
        surface: SurfaceId,
        /// Items removed with the surface.
        items: Vec<crate::ids::ItemId>,
    },
}

/// Stable rejection category for an approved close whose topology commit failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DockspaceCloseRejection {
    /// Current policy rejected the close mutation.
    #[error("current docking policy rejects the close")]
    PolicyDenied,
    /// Current topology no longer satisfies the frozen close plan.
    #[error("current docking topology conflicts with the close plan")]
    Conflict,
    /// The core detected an internal invariant failure while applying the close.
    #[error("dockspace could not apply the validated close plan")]
    Internal,
}

/// Public actionable result produced by one facade-owned input.
#[derive(Debug, PartialEq)]
pub enum HostInputOutcome {
    /// One stable item- or root-centric product action committed or produced a valid no-op.
    ProductActionApplied(DockspaceActionOutcome),
    /// One stable item- or root-centric product action was rejected without mutation.
    ProductActionRejected(DockspaceActionRejection),
    /// One checked durable command applied or produced a valid no-op.
    #[cfg(any(feature = "backend", test))]
    CommandApplied {
        /// Structured command result.
        outcome: CommandOutcome,
        /// Whether the complete workspace changed.
        changed: bool,
    },
    /// One checked durable command was consumed without mutation.
    #[cfg(any(feature = "backend", test))]
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
        application: Option<Result<DockspaceCloseOutcome, DockspaceCloseRejection>>,
        /// Whether this input changed durable topology.
        changed: bool,
    },
    /// One existing native root window was bound to a logical surface.
    NativeSurfaceRegistered {
        /// Opaque exact-incarnation binding for later platform facts.
        binding: NativeSurfaceBinding,
    },
    /// Native registration named a surface which was not admissible.
    NativeSurfaceRegistrationRejected {
        /// Stable logical surface rejected by the core.
        surface: SurfaceId,
    },
    /// One complete native platform snapshot was atomically applied.
    NativePlatformSnapshotApplied {
        /// Exact close edges newly observed in this same platform batch.
        close_requests: Vec<NativeSurfaceCloseRequest>,
    },
    /// One exact live-binding close observation was atomically applied.
    NativeCloseObservationApplied {
        /// Exact close edges newly observed by this binding-scoped fact.
        close_requests: Vec<NativeSurfaceCloseRequest>,
    },
    /// One explicit native surface-close request opened a core-owned close plan.
    NativeSurfaceCloseRequested {
        /// Exact surface disposition accepted by the core.
        request: SurfaceCloseRequest,
        /// Frozen close plan shared with the ordinary decision workflow.
        plan: ClosePlan,
    },
    /// One explicit native surface-close request was rejected without mutation.
    NativeSurfaceCloseRejected {
        /// Exact close edge which remains available for a different explicit request.
        close: NativeSurfaceCloseRequest,
        /// Surface disposition rejected by the core.
        request: SurfaceCloseRequest,
        /// Typed fail-closed rejection.
        reason: SurfaceCloseRequestRejection,
    },
    /// A vetoed native close now requires an exact platform cancellation.
    NativeSurfaceCloseCancellationRequired {
        /// Exact close edge which must be cancelled.
        close: NativeSurfaceCloseRequest,
        /// Plan retaining the cancellation obligation.
        plan: ClosePlan,
    },
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
    native_effects: Vec<NativeEffectRequest>,
    repaint_surfaces: Vec<SurfaceId>,
}

impl HostFrameReport {
    fn from_transition(
        transition: &crate::transition::EngineTransition,
        painted_outputs: Vec<PaintedSurfaceOutput>,
        native_provider: Option<crate::platform_provider::PlatformObservationLease>,
        abandoned_native_effects: native_effect::NativeEffectDropQueue,
    ) -> Self {
        let mut ordered_inputs = Vec::new();
        for reduced in transition.reduced_inputs() {
            let outcome = match reduced.outcome() {
                #[cfg(any(feature = "backend", test))]
                InputOutcome::CommandProcessed {
                    outcome, changed, ..
                } => Some(HostInputOutcome::CommandApplied {
                    outcome: outcome.clone(),
                    changed: *changed,
                }),
                #[cfg(any(feature = "backend", test))]
                InputOutcome::CommandRejected { error, .. } => {
                    Some(HostInputOutcome::CommandRejected(error.clone()))
                }
                InputOutcome::ProductActionProcessed { outcome, .. } => {
                    Some(HostInputOutcome::ProductActionApplied(outcome.clone()))
                }
                InputOutcome::ProductActionRejected { reason, .. } => {
                    Some(HostInputOutcome::ProductActionRejected(*reason))
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
                    application: application.as_ref().map(map_close_application),
                    changed: *changed,
                }),
                InputOutcome::ViewportRegistered { binding } => {
                    native_provider.map(|provider| HostInputOutcome::NativeSurfaceRegistered {
                        binding: NativeSurfaceBinding::from_binding(provider, *binding),
                    })
                }
                InputOutcome::ViewportRegistrationRejected { surface } => {
                    Some(HostInputOutcome::NativeSurfaceRegistrationRejected { surface: *surface })
                }
                InputOutcome::PlatformSnapshotPublished {
                    native_close_edges, ..
                } => native_provider.map(|provider| {
                    HostInputOutcome::NativePlatformSnapshotApplied {
                        close_requests: native_close_edges
                            .iter()
                            .copied()
                            .map(|edge| NativeSurfaceCloseRequest::from_edge(provider, edge))
                            .collect(),
                    }
                }),
                InputOutcome::NativeCloseObservationPublished {
                    native_close_edges, ..
                } => native_provider.map(|provider| {
                    HostInputOutcome::NativeCloseObservationApplied {
                        close_requests: native_close_edges
                            .iter()
                            .copied()
                            .map(|edge| NativeSurfaceCloseRequest::from_edge(provider, edge))
                            .collect(),
                    }
                }),
                InputOutcome::SurfaceCloseRequested { request, plan, .. } => {
                    Some(HostInputOutcome::NativeSurfaceCloseRequested {
                        request: request.clone(),
                        plan: plan.clone(),
                    })
                }
                InputOutcome::SurfaceCloseRejected {
                    edge,
                    request,
                    reason,
                    ..
                } => native_provider.map(|provider| HostInputOutcome::NativeSurfaceCloseRejected {
                    close: NativeSurfaceCloseRequest::from_edge(provider, *edge),
                    request: request.clone(),
                    reason: reason.clone(),
                }),
                InputOutcome::SurfaceCloseCancellationRequested { edge, plan, .. } => {
                    native_provider.map(|provider| {
                        HostInputOutcome::NativeSurfaceCloseCancellationRequired {
                            close: NativeSurfaceCloseRequest::from_edge(provider, *edge),
                            plan: plan.clone(),
                        }
                    })
                }
                InputOutcome::PlatformEffectReported { .. } => None,
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
        let native_effects = transition
            .platform_effects()
            .iter()
            .map(|emission| {
                NativeEffectRequest::from_emission(emission, abandoned_native_effects.clone())
            })
            .collect();
        Self {
            before: transition.before(),
            after: transition.after(),
            inputs,
            painted_outputs,
            native_effects,
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
    /// [`DockspaceSession::report_surface_presentation`] only after its renderer
    /// reports whether that exact output was presented or dropped.
    pub fn take_painted_outputs(&mut self) -> Vec<PaintedSurfaceOutput> {
        std::mem::take(&mut self.painted_outputs)
    }

    /// Takes the exact provider-bound native effects emitted by this frame.
    ///
    /// The vector preserves core order. Each request is affine and must be
    /// dispatched, converted into a typed result, or retained explicitly.
    pub fn take_native_effects(&mut self) -> Vec<NativeEffectRequest> {
        std::mem::take(&mut self.native_effects)
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

fn map_close_application(
    application: &Result<CloseCommitOutcome, CommandError>,
) -> Result<DockspaceCloseOutcome, DockspaceCloseRejection> {
    match application {
        Ok(CloseCommitOutcome::ItemClosed { item, root }) => {
            Ok(DockspaceCloseOutcome::ItemClosed {
                item: *item,
                root: *root,
            })
        }
        Ok(CloseCommitOutcome::RootClosed { root, items }) => {
            Ok(DockspaceCloseOutcome::RootClosed {
                root: *root,
                items: items.clone(),
            })
        }
        Ok(CloseCommitOutcome::SurfaceClosed { surface, items }) => {
            Ok(DockspaceCloseOutcome::SurfaceClosed {
                surface: *surface,
                items: items.clone(),
            })
        }
        Err(CommandError::Policy(_)) => Err(DockspaceCloseRejection::PolicyDenied),
        Err(error) => Err(if error.is_expected_rejection() {
            DockspaceCloseRejection::Conflict
        } else {
            DockspaceCloseRejection::Internal
        }),
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

/// Stable category for one renderer-neutral facade failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspaceRuntimeErrorKind {
    /// Core engine construction or publication failed.
    Engine,
    /// The affine host-frame protocol rejected an operation.
    HostFrame,
    /// A product action capability belongs to another dockspace authority domain.
    ActionAuthority,
    /// The private application input sequence was exhausted.
    SourceSequenceExhausted,
    /// The host named a surface without a matching paint obligation.
    PaintObligationUnavailable,
    /// A surface contribution could not begin under the frozen roster.
    SurfaceContributionBegin,
    /// A surface contribution answer was stale or failed compilation.
    SurfaceContributionPrepare,
    /// A renderer-neutral interaction capability was invalid.
    Interaction,
    /// The presentation sidecar could not synchronize with the core.
    PresentationObservation,
    /// Native lifecycle data was stale, incomplete, or invalid.
    Native,
}

/// Failure at the renderer-neutral facade boundary.
///
/// Backend reducer errors remain available through [`std::error::Error::source`]
/// without becoming part of the default product API. Product-level interaction
/// and native errors have dedicated typed accessors.
#[derive(Debug)]
pub struct DockspaceRuntimeError {
    source: DockspaceRuntimeErrorSource,
}

impl DockspaceRuntimeError {
    /// Returns the stable facade-level error category.
    #[must_use]
    pub const fn kind(&self) -> DockspaceRuntimeErrorKind {
        match &self.source {
            DockspaceRuntimeErrorSource::Engine(_) => DockspaceRuntimeErrorKind::Engine,
            DockspaceRuntimeErrorSource::HostFrame(_) => DockspaceRuntimeErrorKind::HostFrame,
            DockspaceRuntimeErrorSource::PreparedActionAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::ActionAuthority
            }
            DockspaceRuntimeErrorSource::SourceSequenceExhausted => {
                DockspaceRuntimeErrorKind::SourceSequenceExhausted
            }
            DockspaceRuntimeErrorSource::PaintObligationUnavailable { .. } => {
                DockspaceRuntimeErrorKind::PaintObligationUnavailable
            }
            DockspaceRuntimeErrorSource::SurfaceContributionBegin(_) => {
                DockspaceRuntimeErrorKind::SurfaceContributionBegin
            }
            DockspaceRuntimeErrorSource::SurfaceContributionPrepare(_) => {
                DockspaceRuntimeErrorKind::SurfaceContributionPrepare
            }
            DockspaceRuntimeErrorSource::Interaction(_) => DockspaceRuntimeErrorKind::Interaction,
            DockspaceRuntimeErrorSource::PresentationObservation(_) => {
                DockspaceRuntimeErrorKind::PresentationObservation
            }
            DockspaceRuntimeErrorSource::Native(_) => DockspaceRuntimeErrorKind::Native,
        }
    }

    /// Returns the typed interaction failure when this error belongs to that lane.
    #[must_use]
    pub const fn interaction_error(&self) -> Option<&DockspaceInteractionError> {
        match &self.source {
            DockspaceRuntimeErrorSource::Interaction(error) => Some(error),
            _ => None,
        }
    }

    /// Returns the stable native-host failure category when this error belongs to that lane.
    #[must_use]
    pub const fn native_kind(&self) -> Option<NativeHostErrorKind> {
        match &self.source {
            DockspaceRuntimeErrorSource::Native(error) => Some(error.kind()),
            _ => None,
        }
    }

    /// Returns the surface named by a missing paint obligation, when applicable.
    #[must_use]
    pub const fn paint_obligation_surface(&self) -> Option<SurfaceId> {
        match self.source {
            DockspaceRuntimeErrorSource::PaintObligationUnavailable { surface } => Some(surface),
            _ => None,
        }
    }

    const fn source_sequence_exhausted() -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::SourceSequenceExhausted,
        }
    }

    const fn paint_obligation_unavailable(surface: SurfaceId) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PaintObligationUnavailable { surface },
        }
    }

    const fn prepared_action_authority_mismatch(
        error: PreparedDockActionAuthorityMismatch,
    ) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PreparedActionAuthorityMismatch(error),
        }
    }
}

impl std::fmt::Display for DockspaceRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(formatter)
    }
}

impl std::error::Error for DockspaceRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.source)
    }
}

#[derive(Debug, Error)]
enum DockspaceRuntimeErrorSource {
    #[error(transparent)]
    Engine(Box<EngineError>),
    #[error(transparent)]
    HostFrame(Box<CoreHostFrameError>),
    #[error(transparent)]
    PreparedActionAuthorityMismatch(PreparedDockActionAuthorityMismatch),
    #[error("application input source sequence is exhausted")]
    SourceSequenceExhausted,
    #[error("surface {surface} has no paintable presentation obligation in this frame")]
    PaintObligationUnavailable { surface: SurfaceId },
    #[error(transparent)]
    SurfaceContributionBegin(SurfaceContributionBeginError),
    #[error(transparent)]
    SurfaceContributionPrepare(Box<SurfaceContributionPrepareError>),
    #[error(transparent)]
    Interaction(DockspaceInteractionError),
    #[error(transparent)]
    PresentationObservation(PresentationObservationError),
    #[error(transparent)]
    Native(NativePlatformError),
}

impl From<EngineError> for DockspaceRuntimeError {
    fn from(error: EngineError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::Engine(Box::new(error)),
        }
    }
}

impl From<CoreHostFrameError> for DockspaceRuntimeError {
    fn from(error: CoreHostFrameError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::HostFrame(Box::new(error)),
        }
    }
}

impl From<SurfaceContributionBeginError> for DockspaceRuntimeError {
    fn from(error: SurfaceContributionBeginError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::SurfaceContributionBegin(error),
        }
    }
}

impl From<SurfaceContributionPrepareError> for DockspaceRuntimeError {
    fn from(error: SurfaceContributionPrepareError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::SurfaceContributionPrepare(Box::new(error)),
        }
    }
}

impl From<DockspaceInteractionError> for DockspaceRuntimeError {
    fn from(error: DockspaceInteractionError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::Interaction(error),
        }
    }
}

impl From<PresentationObservationError> for DockspaceRuntimeError {
    fn from(error: PresentationObservationError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PresentationObservation(error),
        }
    }
}

impl From<NativePlatformError> for DockspaceRuntimeError {
    fn from(error: NativePlatformError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::Native(error),
        }
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
        DockspaceSession::from_backend_workspace(
            builder.build().expect("runtime test workspace validates"),
            crate::policy::DockPolicy::default(),
        )
        .expect("runtime test session initializes")
    }

    #[test]
    fn shared_pointer_ordinal_preserves_edge_then_outcome_order() {
        let outcomes = finish_ordered_inputs(vec![
            (7, 1, 0, HostInputOutcome::NativePlatformSnapshotStale),
            (
                7,
                0,
                1,
                HostInputOutcome::NativeCloseObservationApplied {
                    close_requests: Vec::new(),
                },
            ),
            (
                7,
                0,
                0,
                HostInputOutcome::NativePlatformSnapshotApplied {
                    close_requests: Vec::new(),
                },
            ),
        ]);

        assert!(matches!(
            outcomes.as_slice(),
            [
                HostInputOutcome::NativePlatformSnapshotApplied { .. },
                HostInputOutcome::NativeCloseObservationApplied { .. },
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
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("runtime renderer presents the exact output");
        let mut observed = session
            .begin_host_frame()
            .expect("runtime presentation observation frame begins");
        observed
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
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
