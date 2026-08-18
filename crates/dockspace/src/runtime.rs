//! Narrow renderer-neutral runtime facade for application host frames.
//!
//! This module is the migration boundary for adapters which must not own or
//! access the backend reducer directly. It supports revision-bound product actions,
//! close decisions, complete measurement answers, exact paint settlement, and
//! lossless surface-local pointer batches backed by concrete final-presentation
//! authority.

mod close;
mod error_kind;
mod host_frame;
mod interaction;
mod local_action;
mod measurement;
mod native;
mod native_effect;
mod paint;
#[cfg(feature = "serde")]
mod persistence;
mod presentation;
mod presentation_commands;
mod report;
mod session;

pub use host_frame::DockspaceHostFrame;
pub use session::DockspaceSession;

use error_kind::{
    engine_error_kind, host_frame_error_kind, interaction_error_kind,
    presentation_observation_error_kind, surface_contribution_prepare_error_kind,
};
use native::NativePlatformError;

pub use crate::model::{NativeWindowPlacement, PreparedDockAction, WorkspaceVersion};
pub use crate::presentation_config::{
    DockPresentationConfig, DockPresentationConfigBuilder, DockPresentationConfigError,
};
use close::PreparedCloseRequestAuthorityMismatch;
pub use close::{
    DockspaceCloseInertReason, DockspaceCloseItem, DockspaceClosePhase, DockspaceClosePlan,
    DockspaceCloseRequestRejection, DockspaceCloseResolution, PreparedCloseRequest,
};
pub use interaction::{
    DockspaceInteractionError, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePointerRetirement, SurfaceScrollCancelReason, SurfaceScrollDelta, SurfaceScrollDeviceId,
    SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase,
    SurfaceScrollSequenceId,
};
use local_action::PreparedSurfaceActionAuthorityMismatch;
pub use local_action::{
    PreparedSurfaceAction, SurfaceContainedResizeAdjustment, SurfaceGesturePhase,
    SurfaceSplitterAdjustment, SurfaceTabListNavigation, SurfaceTabNavigation,
};
pub use measurement::{
    MeasurementValueError, SurfaceMeasurementAnswer, SurfaceMeasurementRequest, TabListMenuMetrics,
    TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    UniformSurfaceMetrics,
};
pub use native::{
    HostWindowToken, HostWorkAreaToken, NativeCloseState, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeGlobalFocus, NativeHostCapabilities, NativeHostCapability,
    NativeHostErrorKind, NativePointerButton, NativePointerCancelReason, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativePointerState, NativeProjectedScrollDelta, NativeReceiverAnswer,
    NativeReceiverPurpose, NativeReceiverQuery, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollReceiverChallenge, NativeScrollSequenceId, NativeSurfaceBinding,
    NativeSurfaceCloseAction, NativeSurfaceCloseRejection, NativeSurfaceCloseRequest,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaBinding, NativeWorkAreaFacts, NativeWorkAreaRoster,
};
pub use native_effect::{
    NativeCleanupCorrelationFailure, NativeCleanupObservation, NativeCloseDisposition,
    NativeCloseEffectAcknowledgement, NativeDispatchFailure, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeEffectRequest, NativeEffectResult, NativeEffectSubmissionError,
    NativeIndeterminateReason, NativeInputEffectAcknowledgement,
    NativePresentationEffectAcknowledgement, NativeSurfaceRole, NativeUnsupportedReason,
};
pub use paint::{
    ContainedPaintRecord, ContainedResizeDirection, ContainedResizePaintRecord,
    DockspaceContainedTransformPreview, DockspaceDragDecoration, DockspaceDragPreview,
    DockspaceDragSourceKind, DockspaceDropDirection, DockspaceDropEligibility, DockspaceGuideScope,
    DockspacePaintLayer, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceSemanticOutput, DockspaceVisualId, DockspaceVisualKind,
    DropAffordanceClusterPaintRecord, DropAffordancePaintRecord, DropAffordanceTargetPaintRecord,
    DropGuidePaintRecord, DropGuideTargetPaintRecord, PanePaintRecord, SplitterGapVisibility,
    SplitterJunctionPaintRecord, SplitterPaintRecord, StructuralSplitterGapStatus,
    SurfacePaintPlan, TabBarPaintRecord, TabGroupDragRegionKind, TabGroupDragRegionPaintRecord,
    TabListMenuBackdropPaintRecord, TabListMenuPaintRecord, TabListMenuRowPaintRecord,
    TabPaintRecord, TabStripControlKind, TabStripControlPaintRecord, TabStripMemberPaintRecord,
    TabStripMemberVisibility,
};
#[cfg(feature = "serde")]
pub use persistence::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceError,
    DockspacePersistenceErrorKind, PreparedDockspaceDocumentRestore,
};
use presentation::PresentationObservationError;
pub use presentation::{
    NativeStagingPaintRequest, NativeStagingPresentationPhase,
    NativeStagingPresentationReportError, PaintedNativeStagingOutput, PaintedSurfaceOutput,
    SurfacePresentationReportError, SurfacePresentationResult,
};
pub use presentation_commands::{DockspacePresentationCommand, DockspacePresentationCommands};

use thiserror::Error;

use crate::close_plan::SurfaceCloseDisposition;
use crate::engine::{
    CoreHostFrameError, EngineError, SurfaceContributionBeginError, SurfaceContributionPrepareError,
};
use crate::ids::{RootId, SurfaceId};
use crate::model::{
    DockspaceActionOutcome, DockspaceActionRejection, PreparedDockActionAuthorityMismatch,
};
use crate::scene_manifest::MeasurementUnavailableReason;
use crate::transition::SurfaceContributionOutcome;

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
    /// One validated document atomically replaced the complete workspace.
    ///
    /// Earlier input outcomes from the same frame are intentionally omitted
    /// because the replacement superseded their product-visible state.
    DocumentRestored {
        /// Workspace version immediately before replacement.
        previous: WorkspaceVersion,
        /// Workspace version published by the replacement.
        current: WorkspaceVersion,
    },
    /// One stable item- or root-centric product action committed or produced a valid no-op.
    ProductActionApplied(DockspaceActionOutcome),
    /// One product action was accepted and is waiting for an exact presentation terminal.
    ProductPresentationActionRequested {
        /// Stable product result describing the retained source and intended target.
        outcome: DockspaceActionOutcome,
        /// Opaque causal identity shared with the eventual presentation terminal.
        transition: DockspacePresentationTransitionId,
    },
    /// One stable item- or root-centric product action was rejected without mutation.
    ProductActionRejected(DockspaceActionRejection),
    /// One content-close request opened or reused a plan.
    CloseRequested {
        /// Read-only core-owned close plan.
        plan: DockspaceClosePlan,
        /// Whether an unresolved plan was reused.
        reused: bool,
        /// Product-level source of the close request.
        origin: HostCloseRequestOrigin,
    },
    /// One content-close request was rejected without mutation.
    CloseRejected(DockspaceCloseRequestRejection),
    /// One close decision was consumed.
    CloseDecisionProcessed {
        /// Exact token-resolution result, including typed inert outcomes.
        resolution: DockspaceCloseResolution,
        /// Latest retained close plan, when the request exists.
        plan: Option<DockspaceClosePlan>,
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
    /// One globally consistent native-focus observation was applied.
    NativeFocusObservationApplied,
    /// One explicit native surface-close request opened a core-owned close plan.
    NativeSurfaceCloseRequested {
        /// Stable surface disposition accepted by the core.
        disposition: SurfaceCloseDisposition,
        /// Frozen close plan shared with the ordinary decision workflow.
        plan: DockspaceClosePlan,
    },
    /// One explicit native surface-close request was rejected without mutation.
    NativeSurfaceCloseRejected {
        /// Exact close edge which remains available for a different explicit request.
        close: NativeSurfaceCloseRequest,
        /// Stable surface disposition rejected by the core.
        disposition: SurfaceCloseDisposition,
        /// Typed fail-closed rejection.
        reason: NativeSurfaceCloseRejection,
    },
    /// A vetoed native close now requires an exact platform cancellation.
    NativeSurfaceCloseCancellationRequired {
        /// Exact close edge which must be cancelled.
        close: NativeSurfaceCloseRequest,
        /// Plan retaining the cancellation obligation.
        plan: DockspaceClosePlan,
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

/// Product-level result of one surface contribution in a host frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSurfaceCommitStatus {
    /// A complete next presentation candidate was installed.
    Ready,
    /// The current ready candidate was retained unchanged.
    Retained,
    /// The host explicitly left the surface non-interactive.
    Unavailable,
    /// A superseded prepared contribution was consumed inertly.
    Rejected,
}

/// Stable surface identity plus its exact contribution result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSurfaceCommit {
    surface: SurfaceId,
    status: HostSurfaceCommitStatus,
}

impl HostSurfaceCommit {
    fn from_outcome(outcome: &SurfaceContributionOutcome) -> Self {
        let status = match outcome {
            SurfaceContributionOutcome::Ready { .. } => HostSurfaceCommitStatus::Ready,
            SurfaceContributionOutcome::Retained { .. } => HostSurfaceCommitStatus::Retained,
            SurfaceContributionOutcome::Unavailable { .. } => HostSurfaceCommitStatus::Unavailable,
            SurfaceContributionOutcome::Rejected { .. } => HostSurfaceCommitStatus::Rejected,
        };
        Self {
            surface: outcome.surface(),
            status,
        }
    }

    /// Returns the logical surface addressed by this contribution.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact terminal contribution status.
    #[must_use]
    pub const fn status(self) -> HostSurfaceCommitStatus {
        self.status
    }
}

/// One presentation-gated root transition settled by a host frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockspacePresentationTransition {
    id: DockspacePresentationTransitionId,
    root: RootId,
    source_surface: SurfaceId,
    target_surface: SurfaceId,
    result: DockspacePresentationTransitionResult,
    outcome: Option<DockspaceActionOutcome>,
}

impl DockspacePresentationTransition {
    /// Returns the opaque identity minted for the accepted presentation request.
    #[must_use]
    pub const fn id(&self) -> DockspacePresentationTransitionId {
        self.id
    }

    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    #[must_use]
    pub const fn source_surface(&self) -> SurfaceId {
        self.source_surface
    }

    #[must_use]
    pub const fn target_surface(&self) -> SurfaceId {
        self.target_surface
    }

    #[must_use]
    pub const fn result(&self) -> DockspacePresentationTransitionResult {
        self.result
    }

    /// Returns the complete product outcome captured by an applied terminal.
    #[must_use]
    pub const fn outcome(&self) -> Option<&DockspaceActionOutcome> {
        self.outcome.as_ref()
    }
}

/// Opaque causal identity of one presentation-gated product transition.
///
/// Hosts may compare this value for equality, but cannot construct or inspect
/// the core reducer authority encoded by it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DockspacePresentationTransitionId(crate::event::ReductionCause);

impl DockspacePresentationTransitionId {
    pub(crate) const fn from_cause(cause: crate::event::ReductionCause) -> Self {
        Self(cause)
    }
}

impl std::fmt::Debug for DockspacePresentationTransitionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DockspacePresentationTransitionId(..)")
    }
}

/// Stable terminal category for a presentation-gated root transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspacePresentationTransitionResult {
    /// The exact target presentation was observed and ownership moved once.
    Applied,
    /// The target output did not reach a presented terminal.
    TargetNotPresented,
    /// The workspace changed before the transfer barrier.
    WorkspaceChanged,
    /// Dock policy changed before the transfer barrier.
    PolicyChanged,
    /// The exact source presentation or native binding was no longer current.
    SourceUnavailable,
    /// The frozen topology mutation was rejected at the transfer barrier.
    CommandRejected,
}

/// High-level report from one atomically published facade frame.
#[derive(Debug, PartialEq)]
pub struct HostFrameReport {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
    workspace_changed: bool,
    published_state_changed: bool,
    affected_surfaces: Vec<SurfaceId>,
    surface_commits: Vec<HostSurfaceCommit>,
    inputs: Vec<HostInputOutcome>,
    presentation_transitions: Vec<DockspacePresentationTransition>,
    painted_outputs: Vec<PaintedSurfaceOutput>,
    painted_native_staging_outputs: Vec<PaintedNativeStagingOutput>,
    native_admissions: Vec<NativeSurfaceBinding>,
    native_bindings: Vec<NativeSurfaceBinding>,
    native_effects: Vec<NativeEffectRequest>,
    repaint_surfaces: Vec<SurfaceId>,
}

impl HostFrameReport {
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

    /// Returns whether the durable workspace topology changed.
    #[must_use]
    pub const fn workspace_changed(&self) -> bool {
        self.workspace_changed
    }

    /// Returns whether any published scene or interaction state changed.
    #[must_use]
    pub const fn published_state_changed(&self) -> bool {
        self.published_state_changed
    }

    /// Returns the surfaces whose published state changed in this frame.
    #[must_use]
    pub fn affected_surfaces(&self) -> &[SurfaceId] {
        &self.affected_surfaces
    }

    /// Returns surface contribution results in reducer order.
    #[must_use]
    pub fn surface_commits(&self) -> &[HostSurfaceCommit] {
        &self.surface_commits
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

    /// Returns presentation-gated product transitions settled by this frame.
    ///
    /// These results may settle an action requested by an earlier frame. They
    /// therefore remain separate from [`Self::inputs`] while preserving the
    /// core event order of the current atomic publication.
    #[must_use]
    pub fn presentation_transitions(&self) -> &[DockspacePresentationTransition] {
        &self.presentation_transitions
    }

    /// Takes the affine capabilities for outputs actually painted by this frame.
    ///
    /// The host must consume each capability through
    /// [`DockspaceSession::report_surface_presentation`] only after its renderer
    /// reports whether that exact output was presented or dropped.
    pub fn take_painted_outputs(&mut self) -> Vec<PaintedSurfaceOutput> {
        std::mem::take(&mut self.painted_outputs)
    }

    /// Takes affine capabilities for native lifecycle staging outputs painted by this frame.
    ///
    /// The host must attach each output to the exact native callback and later
    /// consume it through [`DockspaceSession::report_native_staging_presentation`].
    pub fn take_painted_native_staging_outputs(&mut self) -> Vec<PaintedNativeStagingOutput> {
        std::mem::take(&mut self.painted_native_staging_outputs)
    }

    /// Returns native bindings which crossed the exact first-live admission barrier.
    ///
    /// The roster is emitted only for a `Pending` to `Admitted` transition of
    /// the same native binding. Hosts may use it to retire staging-only
    /// rendering state without inferring admission from workspace changes.
    #[must_use]
    pub fn native_admissions(&self) -> &[NativeSurfaceBinding] {
        &self.native_admissions
    }

    /// Returns the complete current native binding roster after this frame.
    ///
    /// The roster is sorted by logical surface and names exact binding
    /// incarnations. Hosts should atomically replace their route map from this
    /// value after commit instead of inferring rebinding from workspace changes.
    #[must_use]
    pub fn native_bindings(&self) -> &[NativeSurfaceBinding] {
        &self.native_bindings
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

/// Stable category for one renderer-neutral facade failure.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspaceRuntimeErrorKind {
    /// Product layout or renderer-neutral configuration is invalid.
    InvalidConfiguration,
    /// Session-owned persistence validation or publication failed.
    Persistence,
    /// The selected host path does not provide the requested capability.
    Unsupported,
    /// A valid operation conflicts with the current affine or lifecycle state.
    OperationConflict,
    /// A host supplied stale, incomplete, contradictory, or out-of-order facts.
    HostProtocol,
    /// An internal authority, counter, or invariant failed.
    Internal,
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
            DockspaceRuntimeErrorSource::Engine(error) => engine_error_kind(error),
            DockspaceRuntimeErrorSource::HostFrame(error) => host_frame_error_kind(error),
            DockspaceRuntimeErrorSource::PreparedActionAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::PreparedCloseRequestAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::PreparedSurfaceActionAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::SourceSequenceExhausted => {
                DockspaceRuntimeErrorKind::Internal
            }
            #[cfg(feature = "serde")]
            DockspaceRuntimeErrorSource::DocumentRestoreAlreadySubmitted => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            #[cfg(feature = "serde")]
            DockspaceRuntimeErrorSource::PreparedDocumentRestoreMismatch => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::PaintObligationUnavailable { .. } => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::SurfaceContributionBegin(_) => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::SurfaceContributionPrepare(error) => {
                surface_contribution_prepare_error_kind(error)
            }
            DockspaceRuntimeErrorSource::Interaction(error) => interaction_error_kind(*error),
            DockspaceRuntimeErrorSource::PresentationObservation(error) => {
                presentation_observation_error_kind(*error)
            }
            DockspaceRuntimeErrorSource::Native(error) => error.kind().runtime_error_kind(),
            #[cfg(feature = "serde")]
            DockspaceRuntimeErrorSource::Persistence(_) => DockspaceRuntimeErrorKind::Persistence,
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

    /// Returns the stable persistence category when this failure belongs to that lane.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn persistence_kind(&self) -> Option<DockspacePersistenceErrorKind> {
        match &self.source {
            DockspaceRuntimeErrorSource::Persistence(error) => Some(error.kind()),
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

    #[cfg(feature = "serde")]
    const fn document_restore_already_submitted() -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::DocumentRestoreAlreadySubmitted,
        }
    }

    #[cfg(feature = "serde")]
    const fn prepared_document_restore_mismatch() -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PreparedDocumentRestoreMismatch,
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

    const fn prepared_close_request_authority_mismatch(
        error: PreparedCloseRequestAuthorityMismatch,
    ) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PreparedCloseRequestAuthorityMismatch(error),
        }
    }

    const fn prepared_surface_action_authority_mismatch(
        error: PreparedSurfaceActionAuthorityMismatch,
    ) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PreparedSurfaceActionAuthorityMismatch(error),
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
    #[error(transparent)]
    PreparedCloseRequestAuthorityMismatch(PreparedCloseRequestAuthorityMismatch),
    #[error(transparent)]
    PreparedSurfaceActionAuthorityMismatch(PreparedSurfaceActionAuthorityMismatch),
    #[error("application input source sequence is exhausted")]
    SourceSequenceExhausted,
    #[cfg(feature = "serde")]
    #[error("document replacement is the final semantic mutation in its host frame")]
    DocumentRestoreAlreadySubmitted,
    #[cfg(feature = "serde")]
    #[error("prepared document restore belongs to an older or different session state")]
    PreparedDocumentRestoreMismatch,
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
    #[cfg(feature = "serde")]
    #[error(transparent)]
    Persistence(DockspacePersistenceError),
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

#[cfg(feature = "serde")]
impl From<DockspacePersistenceError> for DockspaceRuntimeError {
    fn from(error: DockspacePersistenceError) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::Persistence(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineError;
    use crate::graph::Workspace;

    fn single_surface_session() -> DockspaceSession {
        let item = crate::ids::ItemId::new(1);
        let root = crate::ids::RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(crate::graph::Node::tabs([item]));
        builder.set_root(root, crate::graph::RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, crate::graph::SurfacePresentation::with_main(root));
        DockspaceSession::from_workspace_for_test(
            builder.build().expect("runtime test workspace validates"),
            crate::policy::DockPolicy::default(),
        )
        .expect("runtime test session initializes")
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
