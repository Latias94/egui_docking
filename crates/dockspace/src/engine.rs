//! Single-writer input queue and atomic headless state reducer.

mod close_workflow;
mod contained_adjustment;
mod contained_geometry;
mod contained_projection;
mod deferred_action;
mod error;
mod gesture_cancel;
mod host_frame;
mod input;
#[cfg(feature = "motion")]
mod layout_motion;
mod logical_surface;
mod native_admission;
mod native_input;
mod native_placement;
mod native_projection;
mod native_tear_off;
mod pending_mutation;
mod pointer_contained;
mod pointer_edge_scroll;
mod pointer_handoff;
mod pointer_proof;
mod pointer_session;
mod pointer_splitter;
mod pointer_timed;
mod pointer_touch;
mod pointer_transaction;
mod presentation_authority;
mod presentation_identity;
mod presentation_rehome;
mod presentation_roster;
mod presented_input;
#[cfg(test)]
mod presented_tab_chrome;
mod product_action;
mod provider_lifecycle;
mod reducer;
mod replacement_focus;
mod retention;
mod scroll_interaction;
mod semantic_candidate;
mod splitter_geometry;
mod surface_contribution;
mod surface_runtime;
mod surface_vacancy;
mod tab_strip_input;
mod workspace_publication;

pub use error::EngineError;
use error::presentation_ledger_error;

pub(crate) use native_projection::NativeFirstLiveProjectionStamp;

#[cfg(test)]
use crate::behavior_tests::support as test_support;

pub use self::pointer_proof::PointerReceiverGeometryError;
use self::pointer_proof::{
    is_tab_list_menu_click, journal_position_key, native_window_pointer_probe,
    pointer_receiver_candidate_spec, resolve_native_pointer_edge, validated_desktop_delivery_route,
};

#[cfg(test)]
pub use self::input::{TabListMenuNavigation, TabScrollAdjustmentError};
#[cfg(test)]
pub use self::presentation_roster::HostPresentationSchedule;
#[cfg(test)]
pub use self::surface_contribution::{
    PreparedReadySurfacePaintCandidate, PreparedSurfacePaintCandidate,
};
#[cfg(test)]
use crate::scene::PresentationPlanValidator;

use self::tab_strip_input::TabListMenuActivation;
#[cfg(test)]
use self::tab_strip_input::aligned_tab_strip_scroll_offset;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

use self::contained_geometry::{
    cardinal_contained_resize_requested_rect, clamp_contained_rect, clamp_moved_contained_rect,
    contained_resize_edges, contained_transform_requested_rect, translated_contained_rect,
};
use self::input::TabScrollAdjustmentKind;
#[cfg(any(feature = "serde", test))]
pub use self::input::ValidatedWorkspaceRestore;
pub use self::input::{
    EngineInput, PreparedTabListMenuDismiss, PreparedTabListMenuNavigation,
    PreparedTabListMenuRowActivation, PreparedTabListMenuScroll, PreparedTabStripControlActivation,
    PreparedTabStripScroll, TabScrollAdjustment,
};
pub(crate) use self::input::{LocalSurfaceAction, LocalTabChromeAction, LocalTabSelection};
use self::native_admission::NativeAdmissionState;
use self::pending_mutation::{PendingMutation, PendingPresentationTracker};
use self::presentation_authority::PresentationAuthorityState;
use self::presentation_identity::PresentationIdentityAuthority;
use self::presentation_roster::{
    FrozenSurfacePresentationOutput, HostPresentationObligationSet, HostPresentationRoster,
    presentation_endpoint_from_capture,
};
pub use self::presentation_roster::{
    HostPresentationDisposition, HostPresentationDispositionOutcome, HostPresentationObligation,
    HostPresentationSlot, HostPresentationUnavailableReason,
};
use self::reducer::{
    HostBackendIngressCursor, HostPointerProtocolSegment, PreparedHostPointerProtocol,
    SequencedInput,
};
use self::splitter_geometry::{
    prepare_resize_axis_groups, resize_axis_group_updates, split_resize_updates,
};
use self::surface_contribution::PreparedSurfaceContributionState;
pub use self::surface_contribution::{
    PreparedSurfaceContribution, SurfaceContributionBeginError, SurfaceContributionPrepareError,
    SurfaceContributionToken,
};
use self::surface_vacancy::TickVacancyLedger;

#[cfg(test)]
use crate::backend_ingress::BackendIngressProviderReplacementTicket;
use crate::backend_ingress::{
    BackendIngressAuthority, BackendIngressBatch, BackendIngressCommitGuard,
    BackendIngressCommitWatermark, BackendIngressError, BackendIngressLease, BackendIngressOrdinal,
    BackendIngressPayload, BackendIngressPrefixRetirementReceipt, BackendIngressRecorder,
};
use crate::close_plan::prepare_scoped_content_close as prepare_content_close;
use crate::close_plan::{
    CloseAdvanceOutcome, CloseAuthority, CloseCancellationProof, CloseCoordinator, CloseDecision,
    CloseDecisionToken, CloseDestroyedProof, CloseItemRequirement, CloseNativeSettlement,
    ClosePlan, ClosePlanLookup, ClosePlanPhase, ClosePlanTarget, CloseRequestId,
    CloseResolutionOutcome, DeferredCloseDecision, DeferredCloseToken, NativeCloseEdge,
    SurfaceCloseRequest, SurfaceMainRehomeTarget, SurfaceRehomeTarget,
};
use crate::command::{
    CloseCommitOutcome, CommandOutcome, ContainedPosition, ContentCloseTarget, ItemSource,
    MovePayload, NodeFingerprint, NodeSource, RootContent, RootPresentationTarget, SplitResize,
    WorkspaceCommand,
};
use crate::coordinates::{
    CoordinateSnapshot, RecoveryCoordinateSnapshot, TearOffPlacementRequest,
    solve_tear_off_placement,
};
use crate::drop_resolver::{
    DropAffordance, DropResolution, DropResolutionError, DropTargeting, PreparedDropMutation,
    resolve_presented_drop_with_targeting,
};
use crate::effect::{
    EffectDispatchResult, EffectId, EffectResult, EffectTransition, NativeCloseResolution,
    PlatformEffect, PlatformEffectEmission,
};
use crate::error::{CommandError, ReferenceRole, TransactionError};
use crate::event::{ReductionCause, WorkspaceEvent, WorkspaceEventKind};
use crate::frame::{
    PanelFocus, SurfaceVacancyAuthority, ViewportCoordinator, ViewportCoordinatorError,
};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::{Axis, Node, Workspace};
use crate::ids::{
    EngineAuthorityDomainId, FloatingPresentationId, HostPresentationAttemptId, InputSequence,
    ItemId, NativeCreateSagaId, PresentationIdentityFrontier, ReducerCausalOrdinal, ReducerTickId,
    RootId, SourceSequence, StableInputSourceId, SurfaceId, WorkspaceRevision,
};
use crate::intent::{
    Authority, CloseActivation, CloseSceneTarget, ContainedGestureKind, ContainedPlacementProof,
    ContainedPlacementUnavailable, ContainedTransformKind, NativePlacementProof,
    NativePresentationOffer, NativeTearOffProposal, PointerButton, SurfacePointer,
    TabGestureSource,
};
use crate::interaction::{
    ActiveContainedTransform, ActiveResize, ClickSessionId, ClickStart, ContainedDragIntent,
    ContainedTransformPlacement, ContainedTransformPreview, ContainedTransformSessionId,
    ContainedTransformStart, DragArmStart, DragTimedContinuation, FrozenClickAction,
    FrozenCloseClick, FrozenContainedDragOrigin, FrozenDragOrigin, FrozenPresentationAuthority,
    FrozenTabListMenuBackdropClick, FrozenTabListMenuBlockerClick, FrozenTabListMenuRowClick,
    FrozenTabStripControlClick, GestureCancelDelivery, GestureLane, GestureOwner,
    HoverSelectionState, InteractionCancelReason, InteractionCounterError, InteractionDelivery,
    InteractionEvent, InteractionEventKind, InteractionOutcome, InteractionRejection,
    InteractionState, InteractionStatus, JournalDragSourceGeometry, JournalDragThresholdOrigin,
    ManipulationPositionKey, PointerContinuationRequest, PointerContinuationSession,
    PointerContinuationToken, PreviewProof, PreviewResolutionStatus, PreviewVisual, ResizeStart,
    SceneGestureContinuation, SceneGestureContinuationDraft, SceneGestureContinuationSource,
    SceneGestureSession, ScrollApplication, ScrollReductionOutcome, ScrollSessionId,
    ScrollSuppressionReason, ScrollTerminationReason, TabStripEdgeDirection,
    TabStripEdgeScrollState, TouchPressState, WorkspaceDeliveryKind,
};
use crate::journal_presentation::{JournalPresentationSnapshot, JournalSurfacePresentation};
#[cfg(test)]
use crate::model::DockspaceLayout;
use crate::model::{DockspaceView, ProductAction};
use crate::operation::{
    PreparedSurfaceContentClose, authorize_captured_drag_source, prepare_surface_content_close,
};
use crate::platform::{
    CloseEffectAcknowledgement, PlatformCapability, PlatformSnapshot, WindowCloseObservation,
};
#[cfg(test)]
use crate::platform_provider::PlatformProviderReservation;
use crate::platform_provider::{
    PlatformObservationAuthorityError, PlatformObservationLease, PlatformProviderAuthorityFrontier,
};
use crate::pointer_journal::{
    DesktopRouteFact, DesktopRoutePresentationError, DesktopRouteValidation, PointerCaptureOwner,
    PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerEventDeliveryOwner, PointerInputLease, PointerJournalLedger, PointerJournalLedgerError,
    PointerProviderScope, PointerStreamId, ScrollDeliveryEndpoint, ScrollDelta, ScrollDeviceId,
    ScrollEdge, ScrollPhase, ScrollSequenceToken, SurfaceLocalPointerDrainReceipt,
    SurfaceLocalPointerEndpoint, SurfaceLocalPointerFrameCommit, SurfaceLocalPointerProvider,
    SurfaceLocalPointerProviderError, SurfaceLocalPointerQuiescenceDisposition,
    SurfaceLocalPointerRetirementOutcome, SurfaceLocalPointerScope, ValidatedDesktopRoute,
};
use crate::pointer_receiver::{
    PointerReceiverAttemptError, PointerReceiverAttemptIssuer, PointerReceiverCandidateRoster,
    PointerReceiverCandidateRosterError, PointerReceiverCandidateSpec,
    PointerReceiverDeliveryDisposition, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverObservationError, PointerReceiverPresentedOutput, PointerReceiverProbeReceipt,
    PointerReceiverReceiptBatch, PointerReceiverReceiptValidationError, ScrollReceiverChallenge,
    ValidatedPointerReceiverReceipt, ValidatedPointerReceiverReceiptBatch,
};
use crate::policy::{
    DockPolicy, DockPolicyRequest, DockPolicySnapshot, DockPresentationMode,
    DockResizePolicyRequest, PolicyDecision, PolicyRevision, TabBarInteraction,
    TearOffPresentation,
};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::presentation_hit::{
    PresentationHitRegionId, PresentationHitRegionKind, PresentationHitResolutionError,
    PresentationPointerLane,
};
use crate::presentation_observation::{
    HostFrameKey, HostInteractionPresentation, HostPresentationEmission,
    HostPresentationEmissionRequest, HostPresentationEndpoint, HostPresentationObservation,
    HostPresentationObservationEntry, HostPresentationObservationOutcome,
    HostPresentationOutputPayload, HostPresentationStreamId, NativeStagingResourceId,
    PresentationHostLease, PresentationHostRetirementReason, PresentationHostRetirementStatus,
    PresentationLedger, PresentationLedgerError, PresentationObservationReduction,
    PresentationOutputSerial, PresentationStreamQuiescence, PresentedSurfaceAuthority,
    SurfacePresentationOutputTicket,
};
#[cfg(test)]
use crate::presentation_observation::{
    NativeStagingResourceDescriptor, PresentationLedgerDiagnostics,
};
use crate::retention::PresentationRetentionManifest;
#[cfg(test)]
use crate::retention::{InputSourceRetentionManifest, RuntimeRetentionManifest};
use crate::scene::{
    BootstrapSurfaceSceneReason, PopupGeometryUnavailableReason, PresentationPlan, SceneBuildError,
    SplitterResizeTarget, SplitterSceneId, StaleSurfaceSceneReason, SurfaceCoordinateCapture,
    SurfaceInteractionProjection, SurfaceScene, SurfaceSceneSet, SurfaceSceneStamp, TabBarRecord,
    TabListMenuBackdropRecord, TabListMenuRecord, TabStripMemberRecord,
};
#[cfg(test)]
use crate::scene_compiler::compile_surface_measurements;
use crate::scene_compiler::{
    PresentationCompilationError, compile_surface_plan, derive_scene_requirement_draft,
    derive_scene_requirement_draft_with_index,
};
use crate::scene_manifest::{
    MeasurementUnavailableReason, RequirementRevision, SceneRequirementDraft,
    SceneRequirementManifest, SurfaceMeasurementTicket, SurfaceMeasurements,
    SurfaceRequirementRevision,
};
use crate::surface_recovery::{
    ContainedRootPlacement, ConvertedMainRecovery, RootRecoveryAnchor, RootRecoveryAnchorId,
    SurfaceMainRehome, SurfaceRecoveryBlockedReason, SurfaceRecoveryBootstrap,
    SurfaceRecoveryError, SurfaceRecoveryHostFacts, SurfaceRecoveryObligation,
    SurfaceRecoveryObligationId, SurfaceRecoveryState, SurfaceRecoveryTarget,
    SurfaceRecoveryTransaction, SurfaceRehomePlacement, SurfaceRosterCaptureError,
    SurfaceRosterDisposition,
};
use crate::tab_strip::{
    PopupRoutingRevision, TabListMenuSessionId, TabStripControlId, TabStripInfluenceDomain,
    TabStripStateDelta, TabStripStateError, TabStripStateKey, TabStripStateStore,
};
#[cfg(test)]
use crate::transaction::WorkspaceTransaction;

#[cfg(test)]
use crate::transition::{
    BackendIngressProviderReplacementStart, PresentationHostRetirementOutcome,
};
use crate::transition::{
    ContentCloseRequestRejection, EngineTransition, EngineTransitionParts, InputOutcome,
    InputPriority, ReducedInput, SurfaceCloseRequestRejection, SurfaceContributionOutcome,
    SurfaceContributionRejection, SurfaceContributionUnavailableReason, SurfaceSceneDelta,
    SurfaceSceneStateKind, WorkspaceVersion,
};
use crate::validation::WorkspaceValidationErrors;
use crate::viewport::{ViewportBinding, ViewportRole, WindowToken};
use crate::viewport_focus::{
    ActivationStart, ActivationStartOutcome, FocusCausalStamp, FocusDelta,
    FocusObservationTransition, ObservedPlatformFocusEffect, PaneFocusDisposition, PaneFocusIntent,
    PaneFocusIntentGeneration, PaneFocusObservation, PaneFocusRequestObservation,
    PaneFocusRevealRejection, PanelFocusRecord, PendingPlatformFocus, PlatformFocusEvidence,
    ViewportActivationRequest, ViewportFocusCoordinator, ViewportFocusError,
};
use crate::viewport_registry::{NativeCloseEdgeDisposition, ViewportAdmission, ViewportOwnership};
pub(crate) use host_frame::CoreEncodingCheckpoint;
use pointer_handoff::NativePointerHandoffProjection;

pub(crate) const BACKEND_INGRESS_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(u64::MAX);

/// Explicit phase of one core-owned host frame.
///
/// The semantic phase retains provider append order. Configuration closes that
/// input prefix and updates the private candidate before measurement and paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFrameInputPhase {
    /// Platform, application, renderer, lifecycle, and maintenance input.
    Semantic,
    /// Policy or presentation configuration replacement.
    Configuration,
}

/// Progress while replaying one immutable backend ingress batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendIngressProgress {
    /// The current pointer record has frozen an exact receiver challenge.
    ReceiverReceiptsRequired,
    /// One complete record reduced, and the immutable batch still has a suffix.
    ///
    /// The caller may resume replay, or seal the exact completed prefix when
    /// `can_seal` is true. An atomic append group can pause computation between
    /// records but must publish as a whole. The candidate watermark does not
    /// advance until the prefix is sealed.
    RecordBoundary {
        /// Number of fully reduced records in this host-frame attempt.
        completed_records: usize,
        /// Whether this boundary also ends a complete atomic append group.
        can_seal: bool,
    },
    /// Every record reduced and the candidate ingress watermark advanced.
    Complete,
}

/// Invalid construction of one core-owned host frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreHostFrameError {
    #[error(
        "host focus observation belongs to {submitted:?}, but this frame belongs to {expected:?}"
    )]
    HostFocusOwnerMismatch {
        expected: PresentationHostLease,
        submitted: PresentationHostLease,
    },
    /// Native platform or desktop pointer facts bypassed the joined ingress lane.
    #[error("native backend facts must be submitted through the joined backend ingress batch")]
    BackendIngressRequired,
    /// A backend batch was supplied without an active joined provider pair.
    #[error("host frame has no active backend ingress provider")]
    BackendIngressUnexpected,
    /// A second backend batch was supplied in one host frame.
    #[error("host frame already contains a backend ingress batch")]
    DuplicateBackendIngressBatch,
    /// The submitted backend batch failed provider, interval, or replay validation.
    #[error("backend ingress batch was rejected: {source}")]
    BackendIngressRejected {
        /// Exact backend-ingress validation failure.
        #[source]
        source: BackendIngressError,
    },
    /// Receiver receipts were supplied without a paused backend pointer record.
    #[error("backend pointer receipts require a paused backend ingress pointer record")]
    BackendIngressReceiptsUnexpected,
    /// Replay was resumed or sealed without a completed backend record boundary.
    #[error("backend ingress continuation requires a complete record boundary")]
    BackendIngressBoundaryRequired,
    /// The caller attempted presentation before completing the joined batch.
    #[error("presentation cannot begin before the backend ingress batch completes")]
    BackendIngressIncomplete,
    /// The internal diagnostic source identity is unavailable to adapter input.
    #[error("input source {input_source} is reserved for core-owned backend ingress")]
    ReservedInputSource {
        /// Rejected caller-provided source.
        input_source: StableInputSourceId,
    },
    /// An input attempted to introduce an item outside the session-owned identity roster.
    #[error("application item {item} is outside the identity scope frozen for this host frame")]
    ItemIdentityOutsideScope {
        /// Unadmitted application item.
        item: ItemId,
    },
    /// A document-bound session received a frame without its exact frozen identity roster.
    #[error("host-frame application identity scope is missing or stale")]
    ItemIdentityScopeMismatch,
    /// A session-owned sidecar publication was routed through a borrowed core commit.
    #[error("host frame requires the session-owned atomic publication path")]
    SessionOwnedPublicationRequired,
    /// The core-owned ordinal cannot advance without wrapping.
    #[error("core host-frame causal ordinal is exhausted")]
    CausalOrdinalExhausted,
    /// The caller used the wrong explicit reduction phase for an input.
    #[error("input class {actual:?} cannot be appended in {phase:?} host-frame phase")]
    InputPhaseMismatch {
        /// Phase selected by the caller.
        phase: HostFrameInputPhase,
        /// Diagnostic input class.
        actual: InputPriority,
    },
    /// Semantic input cannot follow an accepted configuration input in one frame.
    ///
    /// Configuration closes the semantic prefix before measurement and paint.
    /// Accepting a later semantic input would mix its new configuration with
    /// the policy and presented authority frozen for the accepted input prefix.
    #[error("a semantic input cannot follow the terminal configuration phase")]
    SemanticAfterConfiguration,
    /// No semantic, configuration, or pointer input may follow overload cancellation.
    #[error("input cannot follow terminal overload cancellation")]
    InputAfterOverloadCancellation,
    /// A host frame may submit exactly one presentation observation.
    #[error("a host frame already contains a presentation observation")]
    DuplicatePresentationObservation,
    /// A supplementary presentation observer is not part of this core-frozen
    /// host-frame scope.
    #[error("presentation observer {host:?} is outside this core-frozen host-frame scope")]
    PresentationObservationHostOutsideScope {
        /// Observer lease supplied by the caller.
        host: PresentationHostLease,
    },
    /// One enrolled observer supplied more than one observation batch.
    #[error("presentation observer {host:?} already supplied an observation batch")]
    DuplicatePresentationObservationForHost {
        /// Observer lease which repeated its submission.
        host: PresentationHostLease,
    },
    /// A complete observation batch repeated one stream identity.
    #[error("presentation observation batch repeats stream {stream:?}")]
    DuplicatePresentationObservationStream {
        /// Stream repeated by the submitted batch.
        stream: HostPresentationStreamId,
    },
    /// A complete observation batch omitted one pending stream frozen at frame begin.
    #[error("presentation observation batch omitted frozen stream {stream:?}")]
    MissingPresentationObservationStream {
        /// Pending stream absent from the submitted batch.
        stream: HostPresentationStreamId,
    },
    /// A complete observation batch named a stream outside the frozen pending scope.
    #[error("presentation observation batch named stream {stream:?} outside the frozen scope")]
    PresentationObservationStreamOutsideScope {
        /// Stream not pending for this host at frame begin.
        stream: HostPresentationStreamId,
    },
    /// A tick can carry one independently compiled contribution for each surface.
    #[error("a host frame already contains a surface contribution for {surface}")]
    DuplicateSurfaceContribution {
        /// Surface already represented in this tick.
        surface: crate::ids::SurfaceId,
    },
    /// A presentation fact belongs to a surface outside this core-frozen roster.
    #[error("surface {surface} is outside the core-frozen host-frame roster")]
    SurfaceOutsideRoster {
        /// Surface supplied by the caller.
        surface: SurfaceId,
    },
    /// Pointer or semantic input was appended after presentation publication
    /// started for the post-input candidate.
    #[error("host-frame input cannot follow presentation staging")]
    InputAfterPresentation,
    /// The non-replayable host presentation attempt counter cannot advance.
    #[error("core host-frame presentation attempt space is exhausted")]
    PresentationAttemptSpaceExhausted,
    /// Presentation tickets have already been issued for this host frame.
    #[error("host-frame presentation obligations were already issued")]
    PresentationObligationsAlreadyIssued,
    /// A disposition was supplied before the core issued this frame's ticket set.
    #[error("host-frame presentation obligations have not been issued")]
    PresentationObligationsNotIssued,
    /// A host tried to replace checked-out presentation tickets with a bulk disposition.
    #[error("host-frame presentation obligations were issued but not completely resolved")]
    PresentationObligationsPartiallyResolved,
    /// A presentation ticket belongs to another non-replayable host-frame attempt.
    #[error(
        "presentation obligation attempt {submitted:?} does not match current attempt {expected:?}"
    )]
    PresentationObligationAttemptMismatch {
        /// Current host-frame attempt.
        expected: HostPresentationAttemptId,
        /// Attempt carried by the submitted ticket.
        submitted: HostPresentationAttemptId,
    },
    /// A presentation ticket names no slot in this frame's exact physical roster.
    #[error("presentation obligation slot {slot:?} is outside the current physical roster")]
    PresentationObligationOutsideRoster {
        /// Unexpected physical slot.
        slot: HostPresentationSlot,
    },
    /// One physical presentation ticket was resolved more than once.
    #[error("presentation obligation slot {slot:?} was already resolved")]
    PresentationObligationAlreadyResolved {
        /// Repeated physical slot.
        slot: HostPresentationSlot,
    },
    /// A surface contribution was paired with a ticket for another physical slot.
    #[error(
        "presentation obligation slot {slot:?} cannot authorize contribution for surface {surface}"
    )]
    PresentationObligationSurfaceMismatch {
        /// Submitted physical presentation slot.
        slot: HostPresentationSlot,
        /// Surface named by the contribution token.
        surface: SurfaceId,
    },
    /// The host claimed transient visuals which differ from the frozen output.
    #[error(
        "surface {surface} painted interaction evidence {submitted:?} does not match frozen output {expected:?}"
    )]
    PresentationInteractionMismatch {
        /// Surface whose paint evidence was rejected.
        surface: SurfaceId,
        /// Exact transient visuals frozen by core.
        expected: HostInteractionPresentation,
        /// Transient visuals reported by the host.
        submitted: HostInteractionPresentation,
    },
    /// The host attempted to stage an output which was not paintable when the
    /// post-observation frame was sealed.
    #[error("surface {surface} has no frozen paintable presentation output")]
    PresentationOutputUnavailable {
        /// Surface whose staged output was rejected.
        surface: SurfaceId,
    },
    /// The supplied native staging request is not current for this frame.

    /// A host frame may stage only one actual presentation output for each
    /// frozen logical surface.
    #[error("a host frame already staged a presentation output for surface {surface}")]
    DuplicatePresentationOutput {
        /// Surface whose second output was rejected.
        surface: SurfaceId,
    },
    /// A retained contribution requires a Ready output that was paintable at
    /// this exact host-frame boundary.
    #[error("surface {surface} has no frozen Ready output eligible for paired retention")]
    RetainedContributionNotPaintable {
        /// Surface whose retained contribution was rejected.
        surface: SurfaceId,
    },
    /// The contribution token was minted for a different scene than the one
    /// whose actual output was paired in this frame.
    #[error(
        "surface {surface} retained contribution stamp {submitted:?} does not match frozen stamp {expected:?}"
    )]
    RetainedContributionStampMismatch {
        /// Surface whose contribution was rejected.
        surface: SurfaceId,
        /// Scene authority frozen in the contribution token.
        submitted: SurfaceSceneStamp,
        /// Scene authority frozen for actual paint in this host frame.
        expected: SurfaceSceneStamp,
    },
    /// The contribution token's coordinate capture differs from the capture
    /// frozen for the paired actual output.
    #[error("surface {surface} retained contribution coordinates differ from frozen paint output")]
    RetainedContributionCoordinateMismatch {
        /// Surface whose contribution was rejected.
        surface: SurfaceId,
    },
    /// The host-frame-local presentation output request counter cannot advance.
    #[error("core host-frame presentation output request ordinal is exhausted")]
    PresentationOutputRequestExhausted,
    /// A pointer journal was supplied although this frame froze no active
    /// pointer provider.
    #[error("host frame has no active pointer provider for a submitted pointer journal")]
    PointerJournalUnexpected,
    /// A surface-local journal attempted to submit through a copied raw lease.
    #[error("surface-local pointer provider {provider:?} requires its affine producer")]
    SurfaceLocalPointerProducerRequired {
        /// Surface-local provider whose copied lease was rejected.
        provider: PointerInputLease,
    },
    /// Presentation cannot begin before a live pointer provider submits its checkpoint.
    #[error("presentation cannot begin before pointer provider {provider:?} submits its journal")]
    PointerJournalMissingBeforePresentation {
        /// Frozen live pointer provider.
        provider: PointerInputLease,
    },
    /// The submitted journal names a provider other than the one frozen when
    /// the post-observation frame was sealed.
    #[error(
        "host frame pointer provider {submitted:?} does not match frozen provider {expected:?}"
    )]
    PointerJournalProviderMismatch {
        /// Provider frozen by the core when the frame was sealed.
        expected: PointerInputLease,
        /// Provider lease supplied with the journal.
        submitted: PointerInputLease,
    },
    /// The pointer journal failed its provider, scope, or watermark checks.
    #[error("pointer journal was rejected while building host frame: {source}")]
    PointerJournalRejected {
        /// Exact ledger rejection retained for diagnostics.
        #[source]
        source: PointerJournalLedgerError,
    },
    /// The affine surface-local producer rejected this frame attempt.
    #[error("surface-local pointer producer rejected the host frame: {source}")]
    SurfaceLocalPointerProducerRejected {
        /// Exact producer-lifecycle rejection.
        #[source]
        source: SurfaceLocalPointerProviderError,
    },
    /// Receiver questions are minted from the exact reducer prefix, so one
    /// challenge may carry at most one pointer edge.
    #[error("host-frame pointer journals must be submitted one edge at a time")]
    PointerJournalMustBeEdgewise,
    /// The engine could not mint the non-replayable receiver attempt for the
    /// frame's frozen provider.
    #[error("host frame could not mint a pointer receiver attempt: {source}")]
    PointerReceiverAttempt {
        /// Exact attempt-issuer rejection retained for diagnostics.
        #[source]
        source: PointerReceiverAttemptError,
    },
    /// Freezing candidates from the validated journal violated the exact
    /// candidate/output roster contract.
    #[error("host frame could not freeze pointer receiver candidates: {source}")]
    PointerReceiverRoster {
        /// Exact candidate-roster failure retained for diagnostics.
        #[source]
        source: PointerReceiverCandidateRosterError,
    },
    /// Receiver receipts require a previously staged pointer journal segment.
    #[error("host frame submitted pointer receiver receipts before its pointer journal")]
    PointerReceiverReceiptBeforeJournal,
    /// A new segment cannot begin until the preceding segment has its exact
    /// receipt batch. Otherwise segment ownership would be ambiguous.
    #[error("previous host-frame pointer journal segment has no receiver receipts")]
    PointerReceiverReceiptsMissingBeforeNextSegment,
    /// No later semantic input may reduce while an earlier pointer challenge is
    /// waiting for its exact receipt batch. Otherwise the core-minted causal
    /// ordinals would disagree with the actual reducer mutation order.
    #[error("host-frame semantic input cannot bypass a pending pointer receiver challenge")]
    PointerReceiverReceiptsMissingBeforeInput,
    /// Presentation cannot begin while one pointer challenge remains unanswered.
    #[error("presentation cannot begin before the pending pointer challenge is answered")]
    PointerReceiverReceiptsMissingBeforePresentation,
    /// A pointer journal segment may receive one exact receipt batch.
    #[error("latest host-frame pointer journal segment already has receiver receipts")]
    DuplicatePointerReceiverReceipts,
    /// Receiver receipts were supplied although this frame froze no active
    /// pointer provider.
    #[error("host frame has no active pointer provider for submitted receiver receipts")]
    PointerReceiverReceiptsUnexpected,
    /// Exact receipt or semantic-input reduction failed inside the private
    /// rollback candidate. The original typed engine error is returned by
    /// `finish`; no later receiver challenge may be minted.
    #[error("host-frame input-prefix reduction failed")]
    InputPrefixReductionFailed,
}

/// An opaque causal position minted by the core while appending one host input.
///
/// It is intentionally not exposed through a public constructor or reducer
/// input API. Provider-specific global event identity is carried by the
/// forthcoming pointer edge journal; this stamp only records the immutable
/// order accepted inside this exact host frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CoreCausalStamp(u64);

impl CoreCausalStamp {
    const fn ordinal(self) -> ReducerCausalOrdinal {
        ReducerCausalOrdinal::new(self.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StagedPresentationOutput {
    request: HostPresentationEmissionRequest,
    surface: SurfaceId,
    endpoint: HostPresentationEndpoint,
    payload: HostPresentationOutputPayload,
    scene: Option<Arc<crate::scene::SurfacePlanScene>>,
}

/// Observation-only admission stage of one core-owned host frame.
///
/// Beginning a frame freezes only identities that must causally precede
/// presentation observation: the engine domain, predecessor tick, rendering
/// host, platform-provider authority frontier, and every enrolled host's
/// pending stream scope. Scene, interaction, surface roster, and output
/// authority are deliberately unavailable until [`Self::seal`] has reduced
/// those observations into one private candidate.
#[derive(Debug)]
pub struct CoreHostFramePrelude {
    authority_domain: EngineAuthorityDomainId,
    presentation_host: PresentationHostLease,
    predecessor_tick: ReducerTickId,
    presentation_host_frontier: u64,
    platform_provider_frontier: PlatformProviderAuthorityFrontier,
    presentation_scopes: BTreeMap<PresentationHostLease, BTreeSet<HostPresentationStreamId>>,
    presentation_observations: BTreeMap<PresentationHostLease, HostPresentationObservation>,
    item_identity_scope: Option<Arc<BTreeSet<ItemId>>>,
    native_binding_allocation_budget: Option<usize>,
    poison: Option<CoreHostFrameError>,
}

/// Failure to derive one exact HoverDrop receipt from sealed frame authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HostFrameHoverDropResolutionError {
    /// The sealed frame has no receiver-authoritative output for this surface.
    #[error("sealed host frame has no interaction projection for surface {surface}")]
    InteractionProjectionUnavailable {
        /// Surface for which the adapter requested a HoverDrop answer.
        surface: SurfaceId,
    },
    /// Two distinct compiled regions have identical frontmost precedence.
    #[error("sealed HoverDrop receiver is ambiguous between {first:?} and {second:?}")]
    Ambiguous {
        /// First equally frontmost semantic region.
        first: PresentationHitRegionId,
        /// Second equally frontmost semantic region.
        second: PresentationHitRegionId,
    },
    /// The sealed projection could not bind its own resolved receiver.
    #[error("sealed HoverDrop receiver binding is inconsistent: {source}")]
    ObservationBinding {
        /// Exact output/authority binding failure.
        #[source]
        source: PointerReceiverObservationError,
    },
}

/// Narrow read-only authority exposed by a sealed host frame.
///
/// The view references the frame's rollback candidate after presentation
/// observations have reduced. It cannot publish state or access a newer live
/// engine while measurements and receiver facts are being prepared.
#[derive(Debug, Clone, Copy)]
pub struct HostFrameView<'frame> {
    engine: &'frame DockEngine,
    presentation_roster: &'frame HostPresentationRoster,
    receiver_presentations: &'frame BTreeMap<SurfaceId, JournalSurfacePresentation>,
    #[cfg(test)]
    prepared_presentations: &'frame BTreeMap<SurfaceId, JournalSurfacePresentation>,
}

impl<'frame> HostFrameView<'frame> {
    /// Returns the drop guides for this surface's exact paint candidate,
    /// including those frozen in a drag awaiting release presentation.
    #[must_use]
    pub fn presentation_drop_affordance(
        &self,
        surface: SurfaceId,
    ) -> Option<&'frame DropAffordance> {
        let candidate = self.engine.scene().surface(surface)?.ready()?.candidate();
        let scene = candidate.stamp();
        let guide_suppression = candidate
            .plan()
            .contained_projection()
            .map(|projection| projection.drop_source_suppression())
            .unwrap_or_default();
        let drag = match self.engine.interaction.status() {
            InteractionStatus::Dragging { session } => {
                self.engine.interaction.active_drag(session).ok()
            }
            _ => None,
        }
        .or_else(|| {
            self.engine
                .pending_mutation
                .as_ref()
                .and_then(PendingMutation::drag_release)
                .map(|pending| &pending.drag)
        })?;
        drag.drop_affordance().filter(|affordance| {
            affordance.surface() == surface
                // Source exclusion moves guide buttons, but does not invalidate
                // a docking command preview resolved from identical measurements.
                && affordance.guide_suppression() == guide_suppression
                && (affordance.scene() == scene
                    || (self
                        .engine
                        .current_contained_projection_is_owned(drag.source_surface)
                        && self.engine.drag_target_layout_basis(surface, drag.session)
                            == Some(affordance.target_layout_basis())))
        })
    }
}

/// Inputs and independently measured surfaces collected under one sealed,
/// post-observation host-frame authority.
#[derive(Debug)]
pub struct CoreHostFrame {
    /// Optional external report slots available for this frame's effect delivery.
    platform_effect_delivery_limit: Option<usize>,
    authority_domain: EngineAuthorityDomainId,
    presentation_host: PresentationHostLease,
    workspace: WorkspaceVersion,
    requirements: RequirementRevision,
    predecessor_tick: ReducerTickId,
    presentation_host_frontier: u64,
    platform_provider_frontier: PlatformProviderAuthorityFrontier,
    runtime_retention_revision: u64,
    tick: ReducerTickId,
    /// Surface roster frozen from the published engine before observation
    /// reduction. This is the frame capability's stale-admission baseline.
    admission_surface_scope: BTreeSet<SurfaceId>,
    frozen_presentation_roster: HostPresentationRoster,
    /// Exact affine output obligations for this non-replayable frame attempt.
    presentation_obligations: HostPresentationObligationSet,
    /// Exact joined native ingress provider frozen for this frame.
    backend_ingress: Option<BackendIngressLease>,
    backend_ingress_batch_submitted: bool,
    backend_ingress_complete: bool,
    backend_ingress_commit_guard: Option<BackendIngressCommitGuard>,
    pending_backend_ingress: Option<HostBackendIngressCursor>,
    /// Sole provider lease frozen for this frame, when the host runtime has
    /// enrolled pointer delivery. A live provider makes a complete contiguous
    /// sequence of journal segments and one exact receipt batch per segment
    /// mandatory at finish.
    pointer_provider: Option<PointerInputLease>,
    /// Speculative ledger advanced only inside this host frame to validate a
    /// contiguous sequence of submitted edges. It is never published; each
    /// answered edge is reduced immediately against the rollbackable engine
    /// candidate before another receiver question can be minted.
    staged_pointer_journal: PointerJournalLedger,
    /// Producer-side guard retained until this exact frame commits or aborts.
    surface_pointer_commit: Option<SurfaceLocalPointerFrameCommit>,
    /// Interactive outputs visible after the latest accepted input prefix.
    /// Every refresh intersects presented authority with the current physical
    /// roster. Semantic inputs may revoke an output, but cannot retroactively
    /// present newly compiled hit regions to a later pointer segment.
    frozen_pointer_outputs: BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    /// Exact sealed projections backing `frozen_pointer_outputs`. These retain
    /// the presented plan and hit manifest needed to reduce all pointer edges
    /// in this host frame without consulting a causally newer candidate scene.
    frozen_pointer_presentations: BTreeMap<SurfaceId, JournalSurfacePresentation>,
    /// Exact semantic projections for the complete roster. These remain
    /// available to keyboard and accessibility actions even when a local
    /// pointer provider scopes receiver authority to one surface.
    #[cfg(test)]
    frozen_prepared_presentations: BTreeMap<SurfaceId, JournalSurfacePresentation>,
    /// Non-replayable issuer shared with the engine. Every segment receives a
    /// distinct attempt even when the enclosing host frame later fails.
    pointer_receiver_attempt_issuer: Option<Arc<PointerReceiverAttemptIssuer>>,
    pending_pointer_segment: Option<HostPointerProtocolSegment>,
    pointer_segment_submitted: bool,
    presentation_scopes: BTreeMap<PresentationHostLease, BTreeSet<HostPresentationStreamId>>,
    /// Session-owned application identities admitted for the complete frame.
    /// `None` is reserved for engines not bound to durable external identities.
    item_identity_scope: Option<Arc<BTreeSet<ItemId>>>,
    presentation_snapshot_changed: bool,
    presentation_projection_changed: bool,
    candidate: DockEngine,
    tick_policy: DockPolicySnapshot,
    vacancy_ledger: TickVacancyLedger,
    application_base: WorkspaceVersion,
    presentation_observation_outcomes: Vec<HostPresentationObservationOutcome>,
    reduced_inputs: Vec<ReducedInput>,
    reduced_pointer_edges: Vec<crate::transition::ReducedPointerEdge>,
    events: Vec<WorkspaceEvent>,
    interaction_events: Vec<InteractionEvent>,
    last_reduced_input: Option<InputSequence>,
    last_causal_cause: Option<ReductionCause>,
    staged_presentation_outputs: Vec<StagedPresentationOutput>,
    staged_presentation_output_surfaces: BTreeSet<SurfaceId>,
    surface_contributions: BTreeMap<SurfaceId, PreparedSurfaceContribution>,
    /// Measurement results already installed in this update's private candidate.
    compiled_surface_outcomes: BTreeMap<SurfaceId, SurfaceContributionOutcome>,
    presentation_phase_started: bool,
    configuration_phase_started: bool,
    input_terminated_for_overload: bool,
    next_causal_ordinal: u64,
    poison: Option<CoreHostFrameError>,
    input_prefix_error: Option<EngineError>,
}

/// Presentation-only phase of one rollbackable core host frame.
///
/// The wrapper deliberately exposes no semantic or pointer append methods. Its
/// view is the exact post-input candidate used for measurement and paint.
#[derive(Debug)]
pub struct CoreHostPresentationFrame {
    frame: CoreHostFrame,
}

/// Fully reduced host-frame candidate awaiting one infallible publication.
///
/// Adapters may inspect the exact transition to complete their own fallible
/// preflight. They cannot mutate the original engine until this capability is
/// either committed or dropped.
#[must_use = "dropping a prepared host frame rolls back the candidate"]
#[cfg(test)]
pub struct PreparedHostFrameCommit<'a> {
    engine: &'a mut DockEngine,
    candidate: DockEngine,
    transition: EngineTransition,
    backend_ingress_commit_guard: Option<BackendIngressCommitGuard>,
    surface_pointer_commit: Option<SurfaceLocalPointerFrameCommit>,
}

/// Fully reduced host-frame candidate which does not borrow its destination engine.
///
/// This affine capability is intended for hosts whose own transaction must seal
/// after the docking reducer has completed its fallible work but before semantic
/// state may publish. Committing revalidates the exact engine authority frozen by
/// preparation, so an intervening mutation fails closed instead of overwriting it.
#[must_use = "dropping an owned prepared host frame rolls back the candidate"]
pub struct OwnedPreparedHostFrameCommit {
    fence: HostFrameCommitFence,
    candidate: DockEngine,
    transition: EngineTransition,
    backend_ingress_commit_guard: Option<BackendIngressCommitGuard>,
    surface_pointer_commit: Option<SurfaceLocalPointerFrameCommit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HostFrameCommitFence {
    authority_domain: EngineAuthorityDomainId,
    predecessor_tick: ReducerTickId,
    workspace: WorkspaceVersion,
    requirements: RequirementRevision,
    presentation_host_frontier: u64,
    platform_provider_frontier: PlatformProviderAuthorityFrontier,
    pointer_provider: Option<PointerInputLease>,
    backend_ingress: Option<BackendIngressLease>,
    runtime_retention_revision: u64,
}

/// Authoritative renderer-neutral docking engine.
#[derive(Debug, PartialEq)]
pub struct DockEngine {
    authority_domain: EngineAuthorityDomainId,
    /// Canonical immutable snapshot shared by rollback candidates. Checked
    /// commands own the existing staging copy and publish a new snapshot.
    workspace: crate::operation::WorkspaceSnapshot,
    presentation_identity: PresentationIdentityAuthority,
    policy: DockPolicySnapshot,
    version: WorkspaceVersion,
    presentation_authority: PresentationAuthorityState,
    /// Sole live pointer-provider ledger. Its speculative clones share an
    /// identity allocation so prepared journals cannot cross engine domains or
    /// escape an abandoned candidate transaction.
    pointer_journal: PointerJournalLedger,
    /// Sole joined native backend ingress lane. The watermark advances only in
    /// the rollback candidate of a complete host frame.
    backend_ingress: BackendIngressAuthority,
    /// Monotonic receiver-attempt authority deliberately shared with
    /// speculative candidates. Issuing an attempt is a non-replayable boundary
    /// even when a later host frame fails.
    pointer_receiver_attempt_issuer: Arc<PointerReceiverAttemptIssuer>,
    scroll_interaction: scroll_interaction::ScrollInteractionState,
    interaction: InteractionState,
    pending_mutation: Option<PendingMutation>,
    close: CloseCoordinator<PreparedCloseOperation>,
    viewport: ViewportCoordinator,
    viewport_focus: ViewportFocusCoordinator,
    last_focus_reducer_generation: PaneFocusIntentGeneration,
    surface_recovery: SurfaceRecoveryState,
    last_root_recovery_anchor: RootRecoveryAnchorId,
    root_recovery_anchors: BTreeMap<crate::ids::SurfaceId, RootRecoveryAnchor>,
    last_surface_recovery_obligation: SurfaceRecoveryObligationId,
    bound_surface_recoveries: BTreeMap<crate::ids::SurfaceId, BoundSurfaceRecovery>,
    native_admission: NativeAdmissionState,
    /// Publication fence for retention-only mutations outside reducer ticks.
    runtime_retention_revision: u64,
    last_reducer_tick: ReducerTickId,
    semantic_input_watermark: Option<SourceSequence>,
    last_input: InputSequence,
}

#[derive(Debug)]
struct StagedWorkspacePublication {
    workspace: crate::operation::WorkspaceSnapshot,
    root_recovery_anchors: BTreeMap<crate::ids::SurfaceId, RootRecoveryAnchor>,
    bound_surface_recoveries: BTreeMap<crate::ids::SurfaceId, BoundSurfaceRecovery>,
}

#[derive(Debug, Clone, PartialEq)]
struct BoundSurfaceRecovery {
    binding: crate::viewport::ViewportBinding,
    obligation: SurfaceRecoveryObligation,
}

impl BoundSurfaceRecovery {
    const fn new(
        binding: crate::viewport::ViewportBinding,
        obligation: SurfaceRecoveryObligation,
    ) -> Self {
        Self {
            binding,
            obligation,
        }
    }

    fn reauthorize(mut self, obligation: SurfaceRecoveryObligation) -> Self {
        self.obligation = obligation;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspacePublicationAuthority {
    Ordinary,
    NativeCommit {
        saga: NativeCreateSagaId,
    },
    RecoveryCommit {
        source_surface: crate::ids::SurfaceId,
        obligation: SurfaceRecoveryObligationId,
    },
    WindowRecoveryCommit {
        window: crate::viewport::WindowBinding,
    },
    PresentationRehome {
        source_surface: crate::ids::SurfaceId,
        obligation: SurfaceRecoveryObligationId,
        root: crate::ids::RootId,
        floating: crate::ids::FloatingPresentationId,
    },
    PendingPresentationRehome {
        root: crate::ids::RootId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationRehomeCommitAuthority {
    Ordinary,
    BoundConvertedMain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitterAdjustmentAuthority {
    PresentedCoordinates,
    CurrentCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainedPlacementAuthority {
    PresentedCoordinates,
    CurrentCandidate,
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedCloseOperation {
    Content(crate::close_plan::ContentCloseScope),
    Window {
        bindings: Vec<ViewportBinding>,
        operations: Vec<PreparedCloseOperation>,
    },
    SurfaceRetain {
        roster: SurfaceRosterDisposition,
        recovery_focus: PaneFocusDisposition,
    },
    SurfaceRehome {
        roster: SurfaceRosterDisposition,
        transaction: SurfaceRecoveryTransaction,
        recovery_focus: PaneFocusDisposition,
    },
    SurfaceContent {
        roster: SurfaceRosterDisposition,
        prepared: PreparedSurfaceContentClose,
        recovery_focus: PaneFocusDisposition,
    },
}

/// Opaque surface-close facts frozen at the exact native close edge.
///
/// The public request is intentionally descriptive only. This capture is the
/// actual authority used after native destruction: it contains every source
/// root, item sequence, presentation identity, target program, and recorded
/// focus disposition needed to apply without re-reading mutable UI state.
#[derive(Debug, Clone, PartialEq)]
struct SurfaceCloseCapture {
    requirements: Vec<CloseItemRequirement>,
    prepared: PreparedCloseOperation,
}

/// One fully staged post-destruction publication. The workspace candidate is
/// built before the close coordinator advances to `Applied`, so a malformed
/// payload can never consume a native proof and then fail halfway through its
/// topology commit.
#[derive(Debug)]
struct PreparedSurfaceCloseCommit {
    candidate: Workspace,
    events: Vec<WorkspaceEventKind>,
    focus_target: Option<crate::ids::SurfaceId>,
    recovery_focus: PaneFocusDisposition,
}

#[derive(Debug, Clone)]
struct SurfaceRecoveryBatchContext {
    active_rosters: BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    targets: BTreeMap<crate::ids::SurfaceId, SurfaceRecoveryHostFacts>,
}

#[derive(Clone, Copy)]
struct SurfaceRecoveryTargetContext<'a> {
    action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    target_facts: Option<&'a SurfaceRecoveryHostFacts>,
}

impl<'a> SurfaceRecoveryTargetContext<'a> {
    const fn new(
        action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
        target_facts: Option<&'a SurfaceRecoveryHostFacts>,
    ) -> Self {
        Self {
            action_barrier,
            target_facts,
        }
    }
}

struct DestroyedSurfaceContext<'a> {
    roster: Option<&'a SurfaceRosterDisposition>,
    action_barrier: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>,
    recovery_targets: &'a BTreeMap<crate::ids::SurfaceId, SurfaceRecoveryHostFacts>,
    events: &'a mut Vec<WorkspaceEvent>,
}

struct PlatformSnapshotReductionContext<'a> {
    application_base: &'a mut WorkspaceVersion,
    events: &'a mut Vec<WorkspaceEvent>,
    interaction_events: &'a mut Vec<InteractionEvent>,
}

#[derive(Clone, Copy)]
struct TickStartAuthority<'a> {
    policy: &'a DockPolicySnapshot,
}

#[derive(Clone, Copy)]
struct CandidatePaneSelection {
    surface: crate::ids::SurfaceId,
    item: crate::ids::ItemId,
}

#[derive(Debug, Clone, PartialEq)]
enum PreviewDecision {
    Publish {
        scene: SurfaceSceneStamp,
        visual: PreviewVisual,
        proof: Box<PreviewProof>,
        tab_strip: Option<std::sync::Arc<crate::scene_compiler::FutureTabStrip>>,
    },
    Clear(PreviewResolutionStatus),
    Cancel(InteractionCancelReason),
}

#[derive(Debug, Clone, PartialEq)]
struct PendingDragRelease {
    source_version: WorkspaceVersion,
    policy_revision: PolicyRevision,
    cause: ReductionCause,
    focus_causal: FocusCausalStamp,
    session: crate::interaction::DragSessionId,
    drag: crate::interaction::ActiveDrag,
    physical_route: Option<(
        crate::viewport::ViewportBinding,
        crate::platform::NativeDragSourceInput,
    )>,
    release_decision: PreviewDecision,
    preview: crate::interaction::PreviewToken,
    presentation: PendingPresentationTracker,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingPresentationRehome {
    source_version: WorkspaceVersion,
    policy_revision: PolicyRevision,
    cause: ReductionCause,
    focus_causal: FocusCausalStamp,
    root: RootId,
    source: crate::command::NodeSource,
    source_owner: crate::RootPresentationOwner,
    source_surface: SurfaceId,
    source_binding: crate::viewport::ViewportBinding,
    source_recovery: SurfaceRecoveryObligationId,
    source_presentation: PresentedSurfaceAuthority,
    commit_authority: PresentationRehomeCommitAuthority,
    target_surface: SurfaceId,
    floating: Option<FloatingPresentationId>,
    context: product_action::ProductCommandContext,
    pane_focus: PaneFocusDisposition,
    preview: crate::interaction::PublishedPreview,
    presentation: PendingPresentationTracker,
}

#[derive(Debug, Clone, PartialEq)]
struct PreviewEvaluation {
    decision: PreviewDecision,
    affordance: Option<DropAffordance>,
    observation: crate::interaction::targeting::TargetObservation,
    target_layout_basis: Option<SurfaceSceneStamp>,
    prepared_mutation: Option<PreparedDropMutation>,
}

struct StagedJournalWorkspaceCommand {
    publication: StagedWorkspacePublication,
    outcome: CommandOutcome,
    activation: Vec<CommandOutcome>,
    changed: bool,
}

#[derive(Clone, Copy)]
enum JournalDeliveryActivation {
    None,
    SelectItem { item: ItemId, source_root: RootId },
    RaiseContained(crate::ids::FloatingPresentationId),
}

impl PreviewEvaluation {
    const fn new(
        decision: PreviewDecision,
        affordance: Option<DropAffordance>,
        observation: crate::interaction::targeting::TargetObservation,
        target_layout_basis: Option<SurfaceSceneStamp>,
    ) -> Self {
        Self {
            decision,
            affordance,
            observation,
            target_layout_basis,
            prepared_mutation: None,
        }
    }

    const fn without_affordance(decision: PreviewDecision) -> Self {
        Self::new(
            decision,
            None,
            crate::interaction::targeting::TargetObservation::Unavailable,
            None,
        )
    }

    const fn outside_all(decision: PreviewDecision) -> Self {
        Self::new(
            decision,
            None,
            crate::interaction::targeting::TargetObservation::OutsideAll,
            None,
        )
    }
}

enum CoreContainedCandidate {
    None,
    Proposal(crate::intent::ContainedTearOffProposal),
    Rejected,
    Cancel(InteractionCancelReason),
}

struct PreparedDragSource {
    source_surface: crate::ids::SurfaceId,
    complete_root: Option<NodeSource>,
    partial_detachable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalCaptureActionGate {
    Authorized,
    Unavailable,
    Lost,
}

#[cfg(test)]
thread_local! {
    static PARTIAL_DETACHABILITY_EVALUATIONS: std::cell::Cell<usize> = const {
        std::cell::Cell::new(0)
    };
}

#[derive(Clone, Copy)]
struct ContainedPlacementInput {
    root: crate::ids::RootId,
    floating: crate::ids::FloatingPresentationId,
    expected_rect: crate::geometry::LogicalRect,
    placement: ContainedPlacementProof,
}

#[derive(Clone)]
struct PreparedContainedGesture {
    surface: crate::ids::SurfaceId,
    root: crate::ids::RootId,
    floating: crate::ids::FloatingPresentationId,
    button: crate::intent::PointerButton,
    initial_pointer: crate::geometry::LogicalPoint,
    minimum_size: crate::geometry::LogicalSize,
    source_rect: crate::geometry::LogicalRect,
    source: NodeSource,
    expected_roster: crate::command::ContainedRosterSource,
    kind: ContainedGestureKind,
    scene: SurfaceSceneStamp,
    surface_bounds: crate::geometry::LogicalRect,
    coordinate_capture: SurfaceCoordinateCapture,
    presentation: FrozenPresentationAuthority,
    source_layout_facts: Option<std::sync::Arc<crate::scene::PresentationLayoutFacts>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourcePressIntent {
    Activate,
    Manipulate,
}

#[derive(Clone)]
struct PreparedTabGesture {
    source: TabGestureSource,
    surface: crate::ids::SurfaceId,
    root: crate::ids::RootId,
    tabs: crate::ids::NodeId,
    source_node: NodeSource,
    source_geometry: JournalDragSourceGeometry,
    contained: Option<PreparedContainedTabOrigin>,
    initial_pointer: crate::geometry::LogicalPoint,
    presentation: FrozenPresentationAuthority,
    source_layout_facts: Option<std::sync::Arc<crate::scene::PresentationLayoutFacts>>,
}

#[derive(Clone)]
struct PreparedContainedTabOrigin {
    surface: crate::ids::SurfaceId,
    floating: crate::ids::FloatingPresentationId,
    root_source: NodeSource,
    source_rect: crate::geometry::LogicalRect,
    minimum_size: crate::geometry::LogicalSize,
    expected_roster: crate::command::ContainedRosterSource,
}

#[derive(Default)]
struct PlatformInteractionDependencies {
    owner_bindings: BTreeSet<crate::viewport::ViewportBinding>,
    target_bindings: BTreeSet<crate::viewport::ViewportBinding>,
    route_input: Option<crate::platform::NativeDragSourceInput>,
    native: bool,
}

#[derive(Default)]
struct InvalidatedSurfaceSceneAuthorities {
    surfaces: BTreeSet<crate::ids::SurfaceId>,
    bindings: BTreeSet<crate::viewport::ViewportBinding>,
}

enum PlatformInteractionReconciliation {
    Preserve,
    ClearDragFeedback {
        workspace_changed: bool,
        end_routing: bool,
    },
    Cancel(InteractionCancelReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
enum PresentationHostRetirementInteractionImpact {
    None,
    ClearFeedback { end_routing: bool },
    CancelOwner,
}

enum CommandApplication {
    Applied {
        outcome: CommandOutcome,
        changed: bool,
    },
    Rejected(crate::error::CommandError),
}

enum ObligatedSurfaceRecoveryApplication {
    Applied,
    Blocked(SurfaceRecoveryBlockedReason),
}

enum ContentCloseApplication {
    Applied {
        outcome: CloseCommitOutcome,
        changed: bool,
    },
    Rejected(CommandError),
}

enum JournalClickDelivery<'snapshot> {
    Dock(
        PresentationHitRegionId,
        &'snapshot JournalSurfacePresentation,
    ),
    KnownMismatch,
    Blocked,
    Unknown,
}

impl DockEngine {
    /// Creates an engine from stable product layout data and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns an invariant error if exact semantic requirements cannot be derived.
    #[cfg(test)]
    pub fn from_layout_with_presentation_config(
        layout: DockspaceLayout,
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, EngineError> {
        Self::new_with_presentation_config(layout.into_workspace(), policy, presentation_config)
    }

    /// Creates an engine from a strictly validated workspace and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted.
    #[cfg(test)]
    pub fn new(workspace: Workspace, policy: DockPolicy) -> Result<Self, EngineError> {
        Self::new_with_presentation_state(
            workspace,
            policy,
            DockPresentationConfig::default(),
            PresentationIdentityFrontier::empty(),
        )
    }

    #[cfg(feature = "serde")]
    pub(crate) fn from_validated_restore_with_presentation_config(
        restore: ValidatedWorkspaceRestore,
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, EngineError> {
        let (workspace, presentation_identity_frontier) = restore.into_parts();
        Self::new_with_presentation_state(
            workspace,
            policy,
            presentation_config,
            presentation_identity_frontier,
        )
    }

    /// Creates an engine with explicit validated renderer-neutral presentation geometry.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted,
    /// or an invariant error if its exact semantic requirements cannot be derived.
    pub fn new_with_presentation_config(
        workspace: Workspace,
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, EngineError> {
        Self::new_with_presentation_state(
            workspace,
            policy,
            presentation_config,
            PresentationIdentityFrontier::empty(),
        )
    }

    fn new_with_presentation_state(
        workspace: Workspace,
        policy: DockPolicy,
        presentation_config: DockPresentationConfig,
        presentation_identity_frontier: PresentationIdentityFrontier,
    ) -> Result<Self, EngineError> {
        let workspace = crate::operation::WorkspaceSnapshot::validated(workspace)
            .map_err(EngineError::InvalidWorkspace)?;
        let presentation_identity =
            PresentationIdentityAuthority::new(&workspace, presentation_identity_frontier);
        let authority_domain =
            EngineAuthorityDomainId::mint().ok_or(EngineError::EngineAuthorityDomainExhausted)?;
        let version = WorkspaceVersion::initial();
        let presentation_config_revision = PresentationConfigRevision::default();
        let policy = policy.snapshot(PolicyRevision::default());
        let requirement_revision = RequirementRevision::default();
        let interaction = InteractionState::default();
        let workspace_index = Arc::new(
            crate::workspace::WorkspaceIndex::build(&workspace, version).map_err(|source| {
                EngineError::PresentationRequirementInvariant {
                    input: None,
                    detail:
                        crate::scene_compiler::SceneRequirementDerivationError::WorkspaceIndex {
                            source,
                        }
                        .to_string(),
                }
            })?,
        );
        let surface_semantic_snapshots =
            Self::capture_surface_semantic_snapshots(&workspace, &workspace_index).map_err(
                |source| EngineError::PresentationRequirementInvariant {
                    input: None,
                    detail: format!("{source:?}"),
                },
            )?;
        let surface_requirement_revisions = workspace
            .surfaces()
            .map(|(surface, _)| (surface, SurfaceRequirementRevision::default()))
            .collect::<BTreeMap<_, _>>();
        let mut tab_strip_states = TabStripStateStore::default();
        let requirement_draft = derive_scene_requirement_draft_with_index(
            authority_domain,
            &workspace,
            version,
            workspace_index,
            presentation_config_revision,
            &policy,
            requirement_revision,
            &surface_requirement_revisions,
        )
        .map_err(|source| EngineError::PresentationRequirementInvariant {
            input: None,
            detail: source.to_string(),
        })?;
        Self::reconcile_tab_strip_state_store(
            &workspace,
            &requirement_draft,
            &mut tab_strip_states,
            false,
        )
        .map_err(|source| EngineError::PresentationRequirementInvariant {
            input: None,
            detail: source.to_string(),
        })?;
        let presentation_requirements = requirement_draft
            .finalize(tab_strip_states.popup_requirement())
            .map_err(|source| EngineError::PresentationRequirementInvariant {
                input: None,
                detail: source.to_string(),
            })?;
        let scene = SurfaceSceneSet::new(&presentation_requirements).map_err(|source| {
            EngineError::PresentationRequirementInvariant {
                input: None,
                detail: source.to_string(),
            }
        })?;
        Ok(Self {
            authority_domain,
            workspace: workspace.clone(),
            presentation_identity,
            policy,
            version,
            presentation_authority: PresentationAuthorityState::new(
                authority_domain,
                workspace.as_arc(),
                presentation_config,
                presentation_config_revision,
                presentation_requirements,
                tab_strip_states,
                surface_semantic_snapshots,
                surface_requirement_revisions,
                scene,
                PresentationLedger::new(authority_domain),
            ),
            pointer_journal: PointerJournalLedger::new(authority_domain),
            backend_ingress: BackendIngressAuthority::new(authority_domain),
            pointer_receiver_attempt_issuer: Arc::new(PointerReceiverAttemptIssuer::new(
                authority_domain,
            )),
            scroll_interaction: scroll_interaction::ScrollInteractionState::default(),
            interaction,
            pending_mutation: None,
            close: CloseCoordinator::new(authority_domain),
            viewport: ViewportCoordinator::new(authority_domain),
            viewport_focus: ViewportFocusCoordinator::default(),
            last_focus_reducer_generation: PaneFocusIntentGeneration::default(),
            surface_recovery: SurfaceRecoveryState::default(),
            last_root_recovery_anchor: RootRecoveryAnchorId::default(),
            root_recovery_anchors: BTreeMap::new(),
            last_surface_recovery_obligation: SurfaceRecoveryObligationId::default(),
            bound_surface_recoveries: BTreeMap::new(),
            native_admission: NativeAdmissionState::default(),
            runtime_retention_revision: 0,
            last_reducer_tick: ReducerTickId::default(),
            semantic_input_watermark: None,
            last_input: InputSequence::default(),
        })
    }

    /// Returns the published workspace.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Returns the published item/surface-centric product view.
    #[must_use]
    pub fn product_view(&self) -> DockspaceView<'_> {
        DockspaceView::new(&self.workspace)
    }

    pub(crate) const fn authority_domain(&self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the durable frontier required for an atomic dockspace document capture.
    #[must_use]
    #[cfg(any(feature = "serde", test))]
    pub const fn presentation_identity_frontier(&self) -> PresentationIdentityFrontier {
        self.presentation_identity.frontier()
    }

    /// Returns current application policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        self.policy.policy()
    }

    /// Returns the exact immutable policy revision used by semantic consumers.
    #[must_use]
    pub const fn policy_snapshot(&self) -> &DockPolicySnapshot {
        &self.policy
    }

    /// Returns the version required by state-derived inputs.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the validated renderer-neutral geometry used by scene compilation.
    #[must_use]
    pub const fn presentation_config(&self) -> &DockPresentationConfig {
        &self.presentation_authority.presentation_config
    }

    /// Returns the current complete core-derived semantic measurement inventory.
    #[must_use]
    pub const fn presentation_requirements(&self) -> &SceneRequirementManifest {
        &self.presentation_authority.presentation_requirements
    }

    /// Returns the complete independently revisioned presentation roster.
    #[must_use]
    pub const fn scene(&self) -> &SurfaceSceneSet {
        &self.presentation_authority.scene
    }

    /// Returns the published transient interaction state.
    #[must_use]
    pub const fn interaction(&self) -> &InteractionState {
        &self.interaction
    }

    pub(crate) fn has_pending_presentation(&self) -> bool {
        self.pending_mutation.is_some()
    }

    pub(crate) fn surface_requires_presentation(&self, surface: SurfaceId) -> bool {
        self.pending_presentation_targets()
            .any(|(target, _)| target == surface)
    }

    pub(crate) fn surfaces_requiring_presentation_paint(&self) -> impl Iterator<Item = SurfaceId> {
        self.pending_presentation_targets()
            .filter_map(|(surface, needs_paint)| needs_paint.then_some(surface))
    }

    // The boolean distinguishes a missing paint from an exact output awaiting its terminal.
    fn pending_presentation_targets(&self) -> impl Iterator<Item = (SurfaceId, bool)> {
        self.pending_mutation
            .as_ref()
            .filter(|pending| pending.presentation().awaiting_presentation())
            .map(|pending| (pending.surface(), pending.presentation().outputs.is_empty()))
            .into_iter()
    }

    /// Returns the exact release preview retained until the host proves that it
    /// was presented.
    #[must_use]
    #[cfg(test)]
    pub fn pending_release_preview(
        &self,
    ) -> Option<(
        crate::interaction::DragSessionId,
        crate::interaction::PreviewToken,
    )> {
        self.pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
            .map(|pending| (pending.session, pending.preview))
    }

    /// Returns the drag preview which must be represented by the next host
    /// presentation, including a release preview whose gesture is already idle.
    #[must_use]
    pub fn presentation_preview(&self) -> Option<&crate::interaction::InteractionPreview> {
        self.interaction
            .preview()
            .or_else(|| {
                self.pending_mutation
                    .as_ref()
                    .and_then(PendingMutation::drag_release)
                    .and_then(|pending| pending.drag.preview())
                    .map(crate::interaction::PublishedPreview::public)
            })
            .or_else(|| {
                self.pending_mutation
                    .as_ref()
                    .and_then(PendingMutation::rehome)
                    .map(|pending| pending.preview.public())
            })
    }

    /// Returns the contained-transform preview which must be represented by the
    /// next host presentation, including a release preview after gesture end.
    #[must_use]
    pub fn presentation_contained_transform_preview(&self) -> Option<&ContainedTransformPreview> {
        self.interaction.contained_transform_preview()
    }

    /// Returns the latest public state of one exact close request.
    #[must_use]
    pub fn close_plan(&self, request: CloseRequestId) -> Option<&ClosePlan> {
        self.close.plan(request)
    }

    /// Classifies a close request without retaining every terminal payload indefinitely.
    #[must_use]
    #[cfg(test)]
    pub fn lookup_close_plan(&self, request: CloseRequestId) -> ClosePlanLookup<'_> {
        self.close.lookup(request)
    }

    /// Returns every retained close-plan snapshot in stable request order.
    ///
    /// Terminal plans remain visible through the boundary which publishes their final state.
    /// Later mutations may compact them; use [`Self::lookup_close_plan`] to distinguish a retired
    /// terminal identity from an identity this engine never allocated.
    pub fn close_plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.close.plans()
    }

    /// Returns every currently non-terminal close-plan snapshot.
    #[cfg(test)]
    pub fn active_close_plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.close.active_plans()
    }

    /// Returns the core-owned platform, binding, route, and effect coordinator.
    #[must_use]
    pub const fn viewport(&self) -> &ViewportCoordinator {
        &self.viewport
    }

    /// Returns adapter-neutral native activation and pane-focus state.
    #[must_use]
    pub const fn viewport_focus(&self) -> &ViewportFocusCoordinator {
        &self.viewport_focus
    }

    /// Returns the exact current native binding eligible to publish pane-focus facts.
    #[must_use]
    pub fn viewport_focus_binding(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<crate::viewport::ViewportBinding> {
        self.workspace.surface(surface)?;
        self.viewport
            .registry()
            .record(surface)
            .filter(|record| record.can_observe_focus())
            .map(crate::viewport_registry::ViewportRecord::binding)
    }

    /// Returns the current engine-issued recovery anchor for one registered Root surface.
    #[must_use]
    pub fn root_recovery_anchor(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<RootRecoveryAnchor> {
        self.root_recovery_anchors.get(&surface).copied()
    }

    /// Returns the durable recovery target authorized for one live or retained Child surface.
    ///
    /// A retained recovery remains queryable while its original binding is destroyed and until a
    /// replacement is visible and admitted. The target itself is not an admission proof; the
    /// engine still validates the exact obligation at every registration and lifecycle edge.
    #[must_use]
    pub fn surface_recovery_target(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<SurfaceRecoveryTarget> {
        let bound = self.bound_surface_recoveries.get(&surface)?;
        let live = self.viewport.viewport(surface).is_some_and(|record| {
            record.binding() == bound.binding && record.admission() == ViewportAdmission::Admitted
        });
        let retained = self
            .viewport
            .recovery_pending(surface)
            .is_some_and(|pending| {
                pending.source_binding() == bound.binding
                    && pending.recovery_obligation() == bound.obligation.id()
            });
        (live || retained).then(|| bound.obligation.target())
    }

    /// Returns a hosting preference only for an exact child recovery owner.
    #[cfg(feature = "serde")]
    pub(crate) fn native_child_recovery_host(
        &self,
        binding: crate::viewport::ViewportBinding,
    ) -> Option<crate::ids::SurfaceId> {
        let bound = self.bound_surface_recoveries.get(&binding.surface())?;
        if bound.obligation.authority_domain() != self.authority_domain {
            return None;
        }
        let pending = self
            .viewport
            .recovery_pending(binding.surface())
            .filter(|pending| {
                pending.role() == ViewportRole::Child
                    && pending.source_binding() == bound.binding
                    && pending.recovery_obligation() == bound.obligation.id()
            });
        let exact = match self.viewport.viewport(binding.surface()) {
            Some(record) => {
                record.binding() == binding
                    && record.role() == ViewportRole::Child
                    && (bound.binding == binding
                        || pending
                            .is_some_and(|pending| pending.replacement_binding() == Some(binding)))
            }
            None => pending.is_some_and(|pending| pending.source_binding() == binding),
        };
        exact.then(|| bound.obligation.target().host_surface())
    }

    /// Returns the complete roster retained for one unresolved destroyed surface.
    #[must_use]
    #[cfg(test)]
    pub fn pending_surface_recovery(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<&SurfaceRosterDisposition> {
        self.surface_recovery.pending(surface)
    }

    /// Returns why one destroyed child surface is retained for a later exact retry.
    #[must_use]
    #[cfg(test)]
    pub fn blocked_surface_recovery(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<&SurfaceRecoveryBlockedReason> {
        self.surface_recovery.blocked(surface)
    }

    /// Returns the last successfully committed reducer tick.
    #[must_use]
    pub const fn last_reducer_tick(&self) -> ReducerTickId {
        self.last_reducer_tick
    }

    /// Returns the last globally assigned input sequence.
    #[must_use]
    #[cfg(test)]
    pub const fn last_input_sequence(&self) -> InputSequence {
        self.last_input
    }

    /// Returns the last successfully committed sequence for the semantic writer lane.
    ///
    /// [`StableInputSourceId`] values attached to individual inputs are diagnostic labels. They
    /// do not partition replay authority: every non-backend semantic input submitted to this
    /// engine must advance this one session-owned sequence.
    #[must_use]
    #[cfg(test)]
    pub const fn semantic_input_watermark(&self) -> Option<SourceSequence> {
        self.semantic_input_watermark
    }

    /// Produces a native placement proof from current acknowledged platform facts.
    ///
    /// # Errors
    ///
    /// Returns a typed coordinate error when the surface is not ready or its
    /// placement facts are incomplete.
    #[cfg(test)]
    pub fn viewport_placement(
        &self,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
        work_area: crate::viewport::WorkAreaToken,
    ) -> Result<crate::coordinates::ViewportPlacementProof, crate::coordinates::CoordinateUnavailable>
    {
        self.viewport.placement(surface, rect, work_area)
    }

    #[must_use]
    #[cfg(test)]
    pub fn native_placement_is_current(&self, proof: &crate::intent::NativePlacementProof) -> bool {
        match proof {
            NativePlacementProof::TearOff(proof) => {
                self.pointer_provider() == Some(proof.pointer_provider())
                    && self.platform_provider() == Some(proof.platform_provider())
                    && self.viewport.work_area_generation() == proof.work_area_generation()
                    && self.viewport.work_area(proof.work_area()).is_some()
                    && self
                        .viewport
                        .physical_native_drag_capability(proof.source_input())
                        .is_supported()
            }
            NativePlacementProof::Surface(_) => self.viewport.native_placement_is_current(proof),
        }
    }

    fn reduce_versioned_interaction(
        &mut self,
        expected: WorkspaceVersion,
        reduce: impl FnOnce(&mut Self) -> Result<InteractionOutcome, EngineError>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let outcome = reduce(self)?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    fn reduce_scene_close_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.request_close_plan(input, scene, target, CloseActivation::Semantic, policy)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_splitter_adjustment_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.adjust_splitter_resize(
                input,
                scene,
                splitter,
                delta,
                SplitterAdjustmentAuthority::PresentedCoordinates,
                policy,
                events,
                interaction_events,
            )
        })
    }

    fn reduce_contained_placement_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        placement: ContainedPlacementInput,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.apply_contained_placement(
                input,
                placement,
                ContainedPlacementAuthority::PresentedCoordinates,
                policy,
                events,
                interaction_events,
            )
        })
    }

    fn apply_contained_placement(
        &mut self,
        input: InputSequence,
        update: ContainedPlacementInput,
        authority: ContainedPlacementAuthority,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let command = match self.checked_contained_placement_command(update, authority) {
            Ok(command) => command,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        match self.apply_interaction_command_with_policy(input, policy, &command, events)? {
            CommandApplication::Applied { outcome, changed } => {
                if changed {
                    self.invalidate_transient(
                        input,
                        InteractionCancelReason::WorkspaceChanged,
                        interaction_events,
                    )?;
                }
                Ok(InteractionOutcome::ContainedPlacementApplied { outcome, changed })
            }
            CommandApplication::Rejected(error) => Ok(InteractionOutcome::Rejected(
                InteractionRejection::CommandRejected(error),
            )),
        }
    }

    fn checked_contained_placement_command(
        &self,
        update: ContainedPlacementInput,
        authority: ContainedPlacementAuthority,
    ) -> Result<WorkspaceCommand, InteractionRejection> {
        let validated = match authority {
            ContainedPlacementAuthority::PresentedCoordinates => {
                self.validate_contained_placement(update.placement)
            }
            ContainedPlacementAuthority::CurrentCandidate => self
                .semantic_candidate(update.placement.scene())
                .map_err(|_| ContainedPlacementUnavailable::StaleScene {
                    expected: update.placement.scene(),
                    current: self
                        .presentation_authority
                        .scene
                        .surface(update.placement.surface())
                        .map(SurfaceScene::stamp),
                })
                .and_then(|candidate| {
                    Self::validate_contained_placement_geometry(update.placement, candidate)
                }),
        };
        validated.map_err(|error| match error {
            ContainedPlacementUnavailable::StaleScene { .. }
            | ContainedPlacementUnavailable::SceneUnavailable => InteractionRejection::StaleScene,
            other => InteractionRejection::ContainedPlacementUnavailable(other),
        })?;
        Ok(WorkspaceCommand::UpdateContainedRect {
            surface: update.placement.surface(),
            root: update.root,
            floating: update.floating,
            expected_rect: update.expected_rect,
            rect: update.placement.clamped_rect(),
        })
    }

    fn cancel_contained_transform(
        &mut self,
        input: InputSequence,
        session: ContainedTransformSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_contained_transform(session) {
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

    fn clone_workspace_candidate(&self) -> Workspace {
        #[cfg(test)]
        crate::drop_resolver::structural_work::record_engine_workspace_candidate_clone(
            &self.workspace,
        );
        self.workspace.as_ref().clone()
    }

    fn candidate(&self) -> Self {
        #[cfg(test)]
        {
            let presentation = self.presentation_authority.presentation.diagnostics();
            let scene_surfaces = self.presentation_authority.scene.surfaces().len();
            let scene_retained_plans = self
                .presentation_authority
                .scene
                .surfaces()
                .map(|(_, scene)| scene.retained_plan_stamps().unique().count())
                .sum();
            let scene_paint_hit_regions = self
                .presentation_authority
                .scene
                .surfaces()
                .filter_map(|(_, scene)| scene.paint_projection())
                .map(|projection| projection.hit_manifest().regions().len())
                .sum();
            crate::drop_resolver::structural_work::record_engine_atomic_candidate_clone(
                &self.workspace,
                crate::drop_resolver::structural_work::EngineCloneVolume {
                    scene_surfaces,
                    scene_retained_plans,
                    scene_paint_hit_regions,
                    presentation_hosts: presentation.retained_host_states(),
                    presentation_streams: presentation.retained_stream_states(),
                    presentation_pending_outputs: presentation.pending_outputs(),
                    live_pointer_providers: usize::from(self.pointer_provider().is_some()),
                    semantic_input_watermark_guards: usize::from(
                        self.semantic_input_watermark.is_some(),
                    ),
                },
            );
        }
        let mut candidate = Self {
            authority_domain: self.authority_domain,
            workspace: self.workspace.clone(),
            presentation_identity: self.presentation_identity,
            policy: self.policy.clone(),
            version: self.version,
            presentation_authority: self.presentation_authority.clone(),
            pointer_journal: self.pointer_journal.clone(),
            backend_ingress: self.backend_ingress.clone(),
            pointer_receiver_attempt_issuer: Arc::clone(&self.pointer_receiver_attempt_issuer),
            scroll_interaction: self.scroll_interaction.clone(),
            interaction: self.interaction.clone(),
            pending_mutation: self.pending_mutation.clone(),
            close: self.close.clone(),
            viewport: self.viewport.clone(),
            viewport_focus: self.viewport_focus.clone(),
            last_focus_reducer_generation: self.last_focus_reducer_generation,
            surface_recovery: self.surface_recovery.clone(),
            last_root_recovery_anchor: self.last_root_recovery_anchor,
            root_recovery_anchors: self.root_recovery_anchors.clone(),
            last_surface_recovery_obligation: self.last_surface_recovery_obligation,
            bound_surface_recoveries: self.bound_surface_recoveries.clone(),
            native_admission: self.native_admission.clone(),
            runtime_retention_revision: self.runtime_retention_revision,
            last_reducer_tick: self.last_reducer_tick,
            semantic_input_watermark: self.semantic_input_watermark,
            last_input: self.last_input,
        };
        candidate.close.compact_published_terminal();
        let mut retained_effects = BTreeSet::new();
        candidate
            .close
            .extend_referenced_effects(&mut retained_effects);
        candidate
            .viewport_focus
            .extend_referenced_effects(&mut retained_effects);
        candidate
            .viewport
            .extend_referenced_effects(&mut retained_effects);
        candidate
            .viewport
            .compact_published_terminal_effects(&retained_effects);
        candidate.viewport.compact_surface_coordinate_authority(
            candidate.workspace.surfaces().map(|(surface, _)| surface),
        );
        candidate
    }

    fn mark_runtime_boundary_published(&mut self) {
        self.close.mark_boundary_published();
        self.viewport.mark_effect_boundary_published();
    }

    fn advance_runtime_retention_revision(&mut self) -> Result<(), EngineError> {
        self.runtime_retention_revision = self
            .runtime_retention_revision
            .checked_add(1)
            .ok_or(EngineError::RuntimeRetentionRevisionExhausted)?;
        Ok(())
    }

    fn publish_candidate(&mut self, mut candidate: Self) {
        candidate.mark_runtime_boundary_published();
        *self = candidate;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod drag_source_index_tests;
