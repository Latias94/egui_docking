//! Narrow renderer-neutral runtime facade for application host frames.
//!
//! This module is the migration boundary for adapters which must not own or
//! access the backend reducer directly. It supports revision-bound product actions,
//! close decisions, complete measurement answers, exact paint settlement, and
//! lossless surface-local pointer batches backed by concrete final-presentation
//! authority.

mod close;
mod interaction;
mod local_action;
mod measurement;
mod native;
mod native_effect;
mod paint;
#[cfg(feature = "serde")]
mod persistence;
mod presentation;
mod report;

use native::NativePlatformError;

pub use crate::model::{NativeWindowPlacement, PreparedDockAction, WorkspaceVersion};
pub use crate::presentation_config::DockPresentationConfig;
pub use crate::transition::{ContentCloseRequestRejection, SurfaceCloseRequestRejection};
pub use close::{
    DockspaceCloseInertReason, DockspaceCloseItem, DockspaceClosePlan, DockspaceCloseResolution,
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
    PreparedSurfaceAction, SurfaceGesturePhase, SurfaceSplitterAdjustment, SurfaceTabNavigation,
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
    ContainedPaintRecord, ContainedResizeDirection, ContainedResizePaintRecord,
    DockspaceContainedTransformPreview, DockspaceDragPreview, DockspaceDropDirection,
    DockspaceDropEligibility, DockspaceGuideScope, DockspacePaintLayer, DockspacePreviewVisual,
    DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceSemanticOutput, DockspaceVisualId,
    DockspaceVisualKind, DropAffordanceClusterPaintRecord, DropAffordancePaintRecord,
    DropAffordanceTargetPaintRecord, DropGuidePaintRecord, DropGuideTargetPaintRecord,
    PanePaintRecord, SplitterGapVisibility, SplitterJunctionPaintRecord, SplitterPaintRecord,
    StructuralSplitterGapStatus, SurfacePaintPlan, TabBarPaintRecord,
    TabListMenuBackdropPaintRecord, TabListMenuPaintRecord, TabListMenuRowPaintRecord,
    TabPaintRecord, TabStripControlKind, TabStripControlPaintRecord, TabStripMemberPaintRecord,
    TabStripMemberVisibility,
};
#[cfg(feature = "serde")]
pub use persistence::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceError,
    DockspacePersistenceErrorKind,
};
use presentation::PresentationObservationError;
pub use presentation::{
    NativeStagingPaintRequest, NativeStagingPresentationPhase,
    NativeStagingPresentationReportError, PaintedNativeStagingOutput, PaintedSurfaceOutput,
    SurfacePresentationReportError, SurfacePresentationResult,
};

use std::collections::BTreeSet;

use thiserror::Error;

use crate::close_plan::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
    SurfaceCloseRequest,
};
use crate::command::ContentCloseTarget;
#[cfg(any(feature = "backend", test))]
use crate::command::{CommandOutcome, WorkspaceCommand};
#[cfg(feature = "serde")]
use crate::document::PreparedRuntimeDocumentRestore;
use crate::engine::{
    CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
    HostPresentationUnavailableReason, SurfaceContributionBeginError,
    SurfaceContributionPrepareError,
};
#[cfg(any(feature = "backend", test))]
use crate::error::CommandError;
#[cfg(any(feature = "backend", test))]
use crate::graph::Workspace;
use crate::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use crate::model::{
    DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceLayout,
    DockspaceView, PreparedDockActionAuthorityMismatch,
};
use crate::presentation_observation::PresentationHostLease;
use crate::scene_manifest::MeasurementUnavailableReason;
use crate::transition::SurfaceContributionOutcome;
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
    #[cfg(feature = "serde")]
    document: Option<crate::document::BoundDocumentState>,
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
        Self::from_workspace_with_presentation_config(
            layout.into_workspace(),
            policy,
            DockPresentationConfig::default(),
        )
    }

    /// Creates one product session with explicit renderer-neutral geometry.
    ///
    /// Visual-only styling remains adapter-owned. This configuration controls
    /// core layout and interaction geometry such as tab bars, splitters, guides,
    /// contained chrome, and drag thresholds.
    ///
    /// # Errors
    ///
    /// Returns an error when the layout is invalid or the core cannot mint its
    /// private presentation-host identity.
    pub fn from_layout_with_presentation_config(
        layout: DockspaceLayout,
        policy: crate::policy::DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace_with_presentation_config(
            layout.into_workspace(),
            policy,
            presentation_config,
        )
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
        Self::from_workspace_with_presentation_config(
            workspace,
            policy,
            DockPresentationConfig::default(),
        )
    }

    fn from_workspace_with_presentation_config(
        workspace: crate::graph::Workspace,
        policy: crate::policy::DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, DockspaceRuntimeError> {
        let engine =
            DockEngine::new_with_presentation_config(workspace, policy, presentation_config)?;
        Ok(Self::from_engine(engine)?)
    }

    fn from_engine(mut engine: DockEngine) -> Result<Self, EngineError> {
        let presentation_host = engine.create_presentation_host()?;
        Ok(Self::from_engine_with_presentation_host(
            engine,
            presentation_host,
        ))
    }

    fn from_engine_with_presentation_host(
        engine: DockEngine,
        presentation_host: PresentationHostLease,
    ) -> Self {
        Self {
            engine,
            presentation_host,
            presentation: presentation::RuntimePresentationState::default(),
            pointer: None,
            native: None,
            #[cfg(feature = "serde")]
            document: None,
            abandoned_native_effects: native_effect::NativeEffectDropQueue::default(),
            committed_source_sequence: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_engine_for_test(
        engine: DockEngine,
        presentation_host: PresentationHostLease,
    ) -> Self {
        Self::from_engine_with_presentation_host(engine, presentation_host)
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

    /// Returns the current declarative docking policy.
    #[must_use]
    pub const fn policy(&self) -> &crate::policy::DockPolicy {
        self.engine.policy()
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

    /// Prepares one complete-root native tear-off against the current published version.
    #[must_use]
    pub const fn prepare_tear_off_root(
        &self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_tear_off_root(root, placement)
    }

    /// Prepares one revision-bound contained-floating action.
    pub const fn prepare_float_item(
        &self,
        item: crate::ids::ItemId,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
    ) -> PreparedDockAction {
        self.engine.prepare_float_item(item, surface, rect)
    }

    /// Prepares one revision-bound contained-bounds update addressed by item identity.
    pub const fn prepare_set_contained_rect(
        &self,
        item: crate::ids::ItemId,
        rect: crate::geometry::LogicalRect,
    ) -> PreparedDockAction {
        self.engine.prepare_set_contained_rect(item, rect)
    }

    /// Prepares one revision-bound contained raise addressed by item identity.
    pub const fn prepare_raise_contained(&self, item: crate::ids::ItemId) -> PreparedDockAction {
        self.engine.prepare_raise_contained(item)
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
        resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_options(resolver, None)
    }

    #[cfg(feature = "serde")]
    fn begin_host_frame_with_document_restore(
        &mut self,
        restore: PreparedRuntimeDocumentRestore,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_options(None, Some(restore))
    }

    #[cfg(feature = "serde")]
    fn ensure_standalone_document_restore_available(&self) -> Result<(), DockspaceRuntimeError> {
        if self.native.is_some() {
            return Err(NativePlatformError::DocumentRestoreRequiresNativeFrame.into());
        }
        Ok(())
    }

    fn begin_host_frame_with_options(
        &mut self,
        mut resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
        #[cfg(feature = "serde")] mut document_restore: Option<PreparedRuntimeDocumentRestore>,
        #[cfg(not(feature = "serde"))] _document_restore: Option<()>,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.reconcile_surface_pointer_provider()?;
        if let Some(native) = self.native.as_mut() {
            native.record_abandoned_effects(&self.abandoned_native_effects)?;
            native.reclaim_committed_prefix(&mut self.engine)?;
        }
        let mut prelude = self.engine.begin_host_frame(self.presentation_host)?;
        #[cfg(feature = "serde")]
        if let Some(restore) = document_restore.as_ref() {
            prelude.restrict_item_identity_scope(restore.item_identity_scope());
        } else if let Some(document) = self.document.as_ref() {
            prelude.restrict_item_identity_scope(document.item_identity_scope());
        }
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
            painted_native_staging: BTreeSet::new(),
            #[cfg(feature = "serde")]
            document_restore: None,
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
        #[cfg(feature = "serde")]
        if let Some(mut restore) = document_restore.take() {
            let input = restore
                .take_engine_input()
                .map_err(DockspacePersistenceError::session)?;
            host_frame.append(input)?;
            host_frame.document_restore = Some(restore);
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
    painted_native_staging: BTreeSet<crate::presentation_observation::NativeStagingPresentation>,
    #[cfg(feature = "serde")]
    document_restore: Option<PreparedRuntimeDocumentRestore>,
}

impl DockspaceHostFrame<'_> {
    /// Returns the post-input product view without exposing runtime node identities.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(self.frame.view().workspace())
    }

    /// Returns the durable candidate workspace version visible inside this frame.
    #[must_use]
    pub fn version(&self) -> WorkspaceVersion {
        self.frame.view().version()
    }

    /// Resolves one stable item identity to its session-owned external key.
    ///
    /// This is available while a candidate frame is open so an adapter can build
    /// its immutable render resources before the candidate is committed. The
    /// mapping remains owned by the document-bound session.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn external_key_for_item(&self, item: crate::ids::ItemId) -> Option<&str> {
        self.session.external_key_for_item(item)
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

    /// Requests a native child window for one complete root.
    ///
    /// Source content remains on its current surface until post-show staging
    /// authorizes the ownership-transfer barrier. First-live presentation then
    /// admits routing, focus, accessibility, and source-resource retirement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn tear_off_root_current(
        &mut self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::TearOffRoot {
            expected: self.frame.view().version(),
            root,
            placement,
        })
    }

    /// Moves one open item into a contained presentation on an existing surface.
    ///
    /// A complete singleton root preserves its root identity. Moving one item out
    /// of a larger root allocates a fresh root and contained-presentation identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn float_item_current(
        &mut self,
        item: crate::ids::ItemId,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::FloatItem {
            expected: self.frame.view().version(),
            item,
            surface,
            rect,
        })
    }

    /// Updates the contained bounds owning one open item.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn set_contained_rect_current(
        &mut self,
        item: crate::ids::ItemId,
        rect: crate::geometry::LogicalRect,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::SetContainedRect {
            expected: self.frame.view().version(),
            item,
            rect,
        })
    }

    /// Raises the contained presentation owning one open item.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn raise_contained_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::RaiseContained {
            expected: self.frame.view().version(),
            item,
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

    /// Submits one action prepared from an exact paintable surface candidate.
    ///
    /// The action must be submitted before this frame publishes measurements or
    /// other presentation contributions. Core revalidates the frozen scene,
    /// workspace revision, policy, and topology when reducing the action.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the action belongs to
    /// another dockspace session. Stale actions from this session are accepted
    /// structurally and reported as [`HostInputOutcome::StaleRejected`].
    pub fn submit_surface_action(
        &mut self,
        prepared: PreparedSurfaceAction,
    ) -> Result<(), DockspaceRuntimeError> {
        let input = prepared
            .into_engine_input(self.session.engine.authority_domain())
            .map_err(DockspaceRuntimeError::prepared_surface_action_authority_mismatch)?;
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

    /// Returns the non-interactive native staging paints required by this frame.
    ///
    /// These requests are lifecycle placeholders, not dock scenes. Hosts must
    /// paint only adapter-owned background or loading chrome and must not invoke
    /// pane UI, publish receivers, or infer focus authority from the callback.
    #[must_use]
    pub fn native_staging_paints(&self) -> Vec<NativeStagingPaintRequest> {
        self.frame
            .view()
            .native_staging_presentations()
            .map(|presentation| {
                NativeStagingPaintRequest::from_core(
                    NativeSurfaceBinding::from_binding(
                        presentation.basis().platform_provider(),
                        presentation.binding(),
                    ),
                    presentation,
                )
            })
            .collect()
    }

    /// Records that the renderer painted one exact native staging placeholder.
    ///
    /// The request must belong to the current frame and may be answered once.
    /// Final renderer presentation is reported later through
    /// [`DockspaceSession::report_native_staging_presentation`].
    ///
    /// # Errors
    ///
    /// Returns an error when the request is stale, foreign, or duplicated.
    pub fn confirm_native_staging_painted(
        &mut self,
        request: NativeStagingPaintRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        if !self
            .frame
            .view()
            .native_staging_presentations()
            .any(|presentation| presentation == request.core())
        {
            return Err(NativePlatformError::StaleSurface {
                surface: request.surface(),
            }
            .into());
        }
        if !self.painted_native_staging.insert(request.core()) {
            return Err(NativePlatformError::ProtocolInvariant.into());
        }
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
            next_pointer_sequence: _,
            pointer_input_submitted: _,
            submitted_presentation,
            painted_surfaces,
            painted_native_staging,
            #[cfg(feature = "serde")]
            document_restore,
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
        for presentation in painted_native_staging {
            let index = obligations
                .iter()
                .position(|obligation| {
                    obligation.slot()
                        == (crate::engine::HostPresentationSlot::NativeStaging { presentation })
                })
                .ok_or_else(|| {
                    DockspaceRuntimeError::paint_obligation_unavailable(
                        presentation.binding().surface(),
                    )
                })?;
            let obligation = obligations.swap_remove(index);
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Painted(
                    crate::presentation_observation::HostInteractionPresentation::default(),
                ),
            )?;
        }
        for obligation in obligations {
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        #[cfg(feature = "serde")]
        let transition = if let Some(restore) = document_restore {
            let prepared = restore
                .prepare_publication(frame, &session.engine)
                .map_err(DockspacePersistenceError::session)?;
            let (transition, binding) = prepared.commit(&mut session.engine)?;
            session.document = Some(binding);
            transition
        } else {
            frame.finish(&mut session.engine)?
        };
        #[cfg(not(feature = "serde"))]
        let transition = frame.finish(&mut session.engine)?;
        session.committed_source_sequence = next_source_sequence;
        let native_admissions = session
            .native
            .as_mut()
            .map_or_else(Vec::new, |native| native.commit(&session.engine));
        session
            .presentation
            .commit_observation(&transition, &submitted_presentation);
        let native_provider = session
            .native
            .as_ref()
            .map(native::RuntimeNativeState::provider);
        let (painted_outputs, painted_native_staging_outputs) = session
            .presentation
            .retain_emissions(transition.presentation_emissions());
        Ok(HostFrameReport::from_transition(
            &transition,
            painted_outputs,
            painted_native_staging_outputs,
            native_admissions,
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
        plan: DockspaceClosePlan,
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
    /// One explicit native surface-close request opened a core-owned close plan.
    NativeSurfaceCloseRequested {
        /// Exact surface disposition accepted by the core.
        request: SurfaceCloseRequest,
        /// Frozen close plan shared with the ordinary decision workflow.
        plan: DockspaceClosePlan,
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
    painted_outputs: Vec<PaintedSurfaceOutput>,
    painted_native_staging_outputs: Vec<PaintedNativeStagingOutput>,
    native_admissions: Vec<NativeSurfaceBinding>,
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
            DockspaceRuntimeErrorSource::HostFrame(_) => DockspaceRuntimeErrorKind::HostProtocol,
            DockspaceRuntimeErrorSource::PreparedActionAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::PreparedSurfaceActionAuthorityMismatch(_) => {
                DockspaceRuntimeErrorKind::OperationConflict
            }
            DockspaceRuntimeErrorSource::SourceSequenceExhausted => {
                DockspaceRuntimeErrorKind::Internal
            }
            DockspaceRuntimeErrorSource::PaintObligationUnavailable { .. } => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::SurfaceContributionBegin(_) => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::SurfaceContributionPrepare(_) => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::Interaction(error) => interaction_error_kind(*error),
            DockspaceRuntimeErrorSource::PresentationObservation(_) => {
                DockspaceRuntimeErrorKind::HostProtocol
            }
            DockspaceRuntimeErrorSource::Native(error) => native_error_kind(error.kind()),
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

    const fn prepared_surface_action_authority_mismatch(
        error: PreparedSurfaceActionAuthorityMismatch,
    ) -> Self {
        Self {
            source: DockspaceRuntimeErrorSource::PreparedSurfaceActionAuthorityMismatch(error),
        }
    }
}

const fn engine_error_kind(error: &EngineError) -> DockspaceRuntimeErrorKind {
    match error {
        EngineError::InvalidWorkspace(_) => DockspaceRuntimeErrorKind::InvalidConfiguration,
        EngineError::WorkspaceReplacementIdentityRetired { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        _ => DockspaceRuntimeErrorKind::Internal,
    }
}

const fn interaction_error_kind(error: DockspaceInteractionError) -> DockspaceRuntimeErrorKind {
    match error {
        DockspaceInteractionError::PointerProviderUnavailable => {
            DockspaceRuntimeErrorKind::Unsupported
        }
        DockspaceInteractionError::PointerProviderAlreadyActive
        | DockspaceInteractionError::PresentationAuthorityUnavailable { .. }
        | DockspaceInteractionError::SurfaceNotPaintable { .. }
        | DockspaceInteractionError::PointerInputAlreadySubmitted
        | DockspaceInteractionError::PointerFrameInFlight => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        DockspaceInteractionError::MeasurementRosterInvariant
        | DockspaceInteractionError::PointerSequenceExhausted
        | DockspaceInteractionError::PointerProtocolInvariant => {
            DockspaceRuntimeErrorKind::Internal
        }
        DockspaceInteractionError::InvalidMeasurementProfile
        | DockspaceInteractionError::SurfaceOutsideRoster { .. }
        | DockspaceInteractionError::SurfaceAlreadyAnswered { .. }
        | DockspaceInteractionError::MeasurementAnswerMismatch
        | DockspaceInteractionError::PointerBatchEmpty
        | DockspaceInteractionError::InvalidScrollSample => DockspaceRuntimeErrorKind::HostProtocol,
    }
}

const fn native_error_kind(error: NativeHostErrorKind) -> DockspaceRuntimeErrorKind {
    match error {
        NativeHostErrorKind::NotEnabled | NativeHostErrorKind::Unsupported => {
            DockspaceRuntimeErrorKind::Unsupported
        }
        NativeHostErrorKind::AlreadyEnabled | NativeHostErrorKind::OperationConflict => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        NativeHostErrorKind::StaleBinding | NativeHostErrorKind::InvalidFacts => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
        NativeHostErrorKind::Internal => DockspaceRuntimeErrorKind::Internal,
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
    PreparedSurfaceActionAuthorityMismatch(PreparedSurfaceActionAuthorityMismatch),
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
