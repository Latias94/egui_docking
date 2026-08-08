//! Single-writer input queue and atomic headless state reducer.

mod close_workflow;
mod contained_geometry;
mod host_frame;
mod input;
mod local_response;
mod native_admission;
mod pointer_contained;
mod pointer_proof;
mod pointer_session;
mod pointer_splitter;
mod pointer_transaction;
mod presentation_authority;
mod presentation_identity;
mod presentation_roster;
mod product_action;
mod provider_lifecycle;
mod reducer;
mod retention;
mod scroll_interaction;
mod semantic_keyboard;
mod semantic_receiver;
mod splitter_geometry;
mod surface_contribution;
mod surface_runtime;
mod surface_vacancy;
mod tab_strip_input;

pub use self::pointer_proof::PointerReceiverGeometryError;
use self::pointer_proof::{
    is_tab_list_menu_click, pointer_receiver_candidate_spec, validated_desktop_delivery_route,
};

#[cfg(test)]
use self::tab_strip_input::aligned_tab_strip_scroll_offset;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

use self::contained_geometry::{
    clamp_contained_rect, clamp_moved_contained_rect, contained_resize_edges,
    contained_transform_requested_rect, translated_contained_rect,
};
use self::input::TabScrollAdjustmentKind;
pub use self::input::{
    EngineInput, LocalSplitterGesturePhase, LocalTabGesturePhase, PreparedTabListMenuDismiss,
    PreparedTabListMenuNavigation, PreparedTabListMenuRowActivation, PreparedTabListMenuScroll,
    PreparedTabStripControlActivation, PreparedTabStripScroll, TabListMenuNavigation,
    TabScrollAdjustment, TabScrollAdjustmentError, ValidatedWorkspaceRestore,
};
use self::native_admission::NativeAdmissionState;
use self::presentation_authority::PresentationAuthorityState;
use self::presentation_identity::PresentationIdentityAuthority;
use self::presentation_roster::{
    FrozenSurfacePresentationOutput, HostPresentationObligationSet, HostPresentationRoster,
    presentation_endpoint_from_capture,
};
pub use self::presentation_roster::{
    HostPresentationDisposition, HostPresentationDispositionOutcome, HostPresentationObligation,
    HostPresentationSchedule, HostPresentationSlot, HostPresentationUnavailableReason,
};
use self::reducer::{
    HostBackendIngressCursor, HostPointerProtocolSegment, PreparedHostPointerProtocol,
    SequencedInput,
};
use self::splitter_geometry::{
    prepare_resize_axis_groups, resize_delta_interval, split_resize_update, split_resize_updates,
};
use self::surface_contribution::PreparedSurfaceContributionState;
pub use self::surface_contribution::{
    PreparedReadySurfacePaintCandidate, PreparedSurfaceContribution, PreparedSurfacePaintCandidate,
    SurfaceContributionBeginError, SurfaceContributionPrepareError, SurfaceContributionToken,
};
use self::surface_vacancy::TickVacancyLedger;

use crate::backend_ingress::{
    BackendIngressAuthority, BackendIngressBatch, BackendIngressCommitGuard,
    BackendIngressCommitWatermark, BackendIngressError, BackendIngressLease, BackendIngressOrdinal,
    BackendIngressPayload, BackendIngressPrefixRetirementReceipt,
    BackendIngressProviderReplacementTicket, BackendIngressRecorder,
};
use crate::close_plan::{
    CloseAdvanceOutcome, CloseAuthority, CloseCancellationProof, CloseCoordinator, CloseDecision,
    CloseDecisionToken, CloseDestroyedProof, CloseItemRequirement, CloseNativeSettlement,
    ClosePlan, ClosePlanLookup, ClosePlanPhase, ClosePlanTarget, CloseRequestId,
    CloseResolutionOutcome, DeferredCloseDecision, DeferredCloseToken, NativeCloseEdge,
    SurfaceCloseRequest, SurfaceMainRehomeTarget, SurfaceRehomeTarget,
};
use crate::command::{
    CloseCommitOutcome, CommandOutcome, ContainedPosition, ContentCloseTarget, MovePayload,
    NodeFingerprint, NodeSource, RootContent, RootPresentationTarget, SplitResize,
    WorkspaceCommand,
};
use crate::coordinates::{
    CoordinateSnapshot, RecoveryCoordinateSnapshot, TearOffPlacementRequest,
    solve_tear_off_placement,
};
use crate::drop_resolver::{
    DropAffordance, DropResolution, DropResolutionError, resolve_presented_drop,
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
use crate::graph::{Node, Workspace};
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
    ActiveContainedTransform, ActiveResize, ClickSessionId, ClickStart,
    ContainedTransformPaintAcknowledgement, ContainedTransformPlacement, ContainedTransformPreview,
    ContainedTransformSessionId, ContainedTransformStart, DragArmStart, DragGestureAuthority,
    EscapeDelivery, FrozenClickAction, FrozenCloseClick, FrozenContainedDragOrigin,
    FrozenDragOrigin, FrozenPresentationAuthority, FrozenResizeHandle,
    FrozenTabListMenuBackdropClick, FrozenTabListMenuBlockerClick, FrozenTabListMenuRowClick,
    FrozenTabStripControlClick, GestureOwner, InteractionCancelReason, InteractionCounterError,
    InteractionDelivery, InteractionEvent, InteractionEventKind, InteractionOutcome,
    InteractionRejection, InteractionState, InteractionStatus, JournalDragSourceGeometry,
    JournalDragThresholdOrigin, PaintAcknowledgement, PreparedNativeTearOff, PreviewProof,
    PreviewResolutionStatus, PreviewVisual, ResizeGestureAuthority, ResizeStart,
    SceneGestureContinuation, SceneGestureContinuationDraft, SceneGestureContinuationSource,
    SceneGestureSession, ScrollApplication, ScrollReductionOutcome, ScrollSessionId,
    ScrollSuppressionReason, ScrollTerminationReason, WorkspaceDeliveryKind,
};
use crate::journal_presentation::{JournalPresentationSnapshot, JournalSurfacePresentation};
use crate::model::ProductAction;
use crate::operation::{
    PreparedContentClose, PreparedSurfaceContentClose, prepare_content_close,
    prepare_surface_content_close,
};
use crate::platform::{
    CloseEffectAcknowledgement, PlatformCapability, PlatformSnapshot, WindowCloseObservation,
};
use crate::platform_provider::{
    PlatformObservationAuthorityError, PlatformObservationLease, PlatformProviderAuthorityFrontier,
    PlatformProviderReservation,
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
    DockResizePolicyRequest, DockSurfaceRecoveryPolicyRequest, DockSurfaceRecoveryRootFacts,
    PolicyDecision, PolicyRevision, TabBarInteraction, TearOffPresentation,
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
    HostPresentationOutputPayload, HostPresentationStreamId, NativeStagingPresentation,
    NativeStagingResourceDescriptor, NativeStagingResourceId, PresentationHostLease,
    PresentationHostRetirementReason, PresentationHostRetirementStatus, PresentationLedger,
    PresentationLedgerDiagnostics, PresentationLedgerError, PresentationObservationReduction,
    PresentationOutputSerial, PresentationStreamQuiescence, PresentedNativeStagingPresentation,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::retention::{
    InputSourceRetentionManifest, PresentationRetentionManifest, RuntimeRetentionManifest,
};
use crate::scene::{
    BootstrapSurfaceSceneReason, PopupGeometryUnavailableReason, PresentationPlan,
    PresentationPlanValidator, SceneBuildError, SplitterResizeTarget, SplitterSceneId,
    StaleSurfaceSceneReason, SurfaceCoordinateCapture, SurfaceInteractionProjection, SurfaceScene,
    SurfaceSceneSet, SurfaceSceneStamp, TabBarRecord, TabListMenuBackdropRecord, TabListMenuRecord,
    TabStripMemberRecord,
};
use crate::scene_compiler::{
    PresentationCompilationError, compile_surface_measurements, derive_scene_requirement_draft,
    derive_scene_requirement_draft_with_index,
};
use crate::scene_manifest::{
    MeasurementUnavailableReason, RequirementRevision, SceneRequirementDraft,
    SceneRequirementManifest, SurfaceMeasurementTicket, SurfaceMeasurements,
    SurfaceRequirementRevision,
};
use crate::semantic_input::SemanticReceiverEvent;
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
use crate::transaction::WorkspaceTransaction;
use crate::transition::{
    BackendIngressProviderReplacementStart, ContentCloseRequestRejection, EngineTransition,
    EngineTransitionParts, InputOutcome, InputPriority, PresentationHostRetirementOutcome,
    ReducedInput, SurfaceCloseRequestRejection, SurfaceContributionOutcome,
    SurfaceContributionRejection, SurfaceContributionUnavailableReason, SurfaceSceneDelta,
    SurfaceSceneStateKind, WorkspaceVersion,
};
use crate::validation::WorkspaceValidationErrors;
use crate::viewport::{ViewportBinding, ViewportRole, WindowToken};
use crate::viewport_focus::{
    ActivationStart, ActivationStartOutcome, FocusCausalStamp, FocusDelta,
    FocusObservationTransition, ObservedPlatformFocusEffect, PaneFocusDisposition, PaneFocusIntent,
    PaneFocusIntentGeneration, PaneFocusObservation, PaneFocusRevealRejection, PanelFocusRecord,
    PendingPlatformFocus, PlatformFocusEvidence, PlatformFocusRestoreGate,
    ViewportActivationRequest, ViewportFocusCoordinator, ViewportFocusError,
};
use crate::viewport_registry::{NativeCloseEdgeDisposition, ViewportAdmission};

pub(crate) const BACKEND_INGRESS_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(u64::MAX);

/// Explicit phase of one core-owned host frame.
///
/// The semantic phase retains provider append order. Configuration changes are
/// intentionally applied after surface contributions so one frame has a stable
/// policy/configuration authority for interaction and measurement.
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
    /// Every record reduced and the candidate ingress watermark advanced.
    Complete,
}

/// Invalid construction of one core-owned host frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CoreHostFrameError {
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
    /// Configuration is a terminal phase because it is deliberately reduced
    /// after semantic inputs and surface contributions. Accepting a later
    /// semantic input would silently reorder the core-minted append sequence.
    #[error("a semantic input cannot follow the terminal configuration phase")]
    SemanticAfterConfiguration,
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
    #[error("native staging presentation {presentation:?} is not requested by this host frame")]
    NativeStagingPresentationUnavailable {
        presentation: NativeStagingPresentation,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StagedPresentationOutput {
    request: HostPresentationEmissionRequest,
    surface: SurfaceId,
    endpoint: HostPresentationEndpoint,
    payload: HostPresentationOutputPayload,
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
    item_identity_scope: Option<BTreeSet<ItemId>>,
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
    semantic_presentations: &'frame BTreeMap<SurfaceId, JournalSurfacePresentation>,
}

/// Inputs and independently measured surfaces collected under one sealed,
/// post-observation host-frame authority.
#[derive(Debug)]
pub struct CoreHostFrame {
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
    frozen_semantic_presentations: BTreeMap<SurfaceId, JournalSurfacePresentation>,
    /// Non-replayable issuer shared with the engine. Every segment receives a
    /// distinct attempt even when the enclosing host frame later fails.
    pointer_receiver_attempt_issuer: Option<Arc<PointerReceiverAttemptIssuer>>,
    pending_pointer_segment: Option<HostPointerProtocolSegment>,
    pointer_segment_submitted: bool,
    presentation_scopes: BTreeMap<PresentationHostLease, BTreeSet<HostPresentationStreamId>>,
    /// Session-owned application identities admitted for the complete frame.
    /// `None` is reserved for engines not bound to durable external identities.
    item_identity_scope: Option<BTreeSet<ItemId>>,
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
    configuration_inputs: Vec<SequencedInput>,
    staged_presentation_outputs: Vec<StagedPresentationOutput>,
    staged_presentation_output_surfaces: BTreeSet<SurfaceId>,
    surface_contributions: Vec<PreparedSurfaceContribution>,
    presentation_phase_started: bool,
    configuration_phase_started: bool,
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
    #[cfg(feature = "serde")]
    item_identity_scope: Option<BTreeSet<ItemId>>,
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

/// Failure to construct or atomically reduce engine state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EngineError {
    /// Initial or replacement workspace violated a durable invariant.
    #[error("workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// A replacement attempted to revive a presentation identity retired by this engine.
    #[error("workspace replacement input {input} rejected: {source}")]
    WorkspaceReplacementIdentityRetired {
        /// Input which attempted the replacement.
        input: InputSequence,
        /// Exact retired identity rejection.
        #[source]
        source: CommandError,
    },
    /// Process-local engine authority identities cannot advance without wrapping.
    #[error("engine authority domain identity is exhausted")]
    EngineAuthorityDomainExhausted,
    /// The single-writer input counter cannot advance without wrapping.
    #[error("engine input sequence is exhausted")]
    InputSequenceExhausted,
    /// The reducer tick counter cannot advance without wrapping.
    #[error("engine reducer tick is exhausted")]
    ReducerTickExhausted,
    /// The core-owned committed-presentation output counter cannot advance without wrapping.
    #[error("presentation output ticket counter is exhausted")]
    PresentationOutputSerialExhausted,
    /// The document-local surface identity counter cannot advance without wrapping.
    #[error("core presentation surface identity frontier is exhausted")]
    PresentationSurfaceIdentityExhausted,
    /// The document-local root identity counter cannot advance without wrapping.
    #[error("core presentation root identity frontier is exhausted")]
    PresentationRootIdentityExhausted,
    /// The document-local contained-floating identity counter cannot advance without wrapping.
    #[error("core contained-floating identity frontier is exhausted")]
    PresentationFloatingIdentityExhausted,
    /// Core-owned presentation host, stream, or emission state rejected an invariant.
    #[error("presentation ledger invariant failed: {detail}")]
    PresentationLedger {
        /// Stable diagnostic for the rejected ledger operation.
        detail: String,
    },
    /// Pointer-provider ledger rejected a lifecycle or commit operation.
    #[error("pointer input ledger invariant failed: {source}")]
    PointerJournal {
        /// Exact pointer-ledger rejection retained for diagnostics.
        #[source]
        source: PointerJournalLedgerError,
    },
    /// A surface-local provider attempted to bypass its affine producer lifecycle.
    #[error("surface-local pointer providers require the affine producer API")]
    SurfaceLocalPointerProducerRequired,
    /// A prepared host frame lost the exact producer attempt before publication.
    #[error("prepared host frame lost its surface-local pointer producer attempt: {source}")]
    SurfaceLocalPointerCommit {
        /// Exact producer-side validation failure.
        #[source]
        source: SurfaceLocalPointerProviderError,
    },
    /// Every producer-side capability disappeared before the active lane retired.
    #[error(
        "surface-local pointer provider {provider:?} was abandoned; reclaim it before continuing"
    )]
    SurfaceLocalPointerProviderAbandoned {
        /// Exact active lease whose provider, frame guard, and drain receipt vanished.
        provider: PointerInputLease,
    },
    /// A surface-local pointer provider named a host which does not own the surface output.
    #[error(
        "surface-local pointer host {host:?} cannot authorize surface {surface:?}; active presentation owner is {owner:?}"
    )]
    PointerProviderSurfaceHostMismatch {
        /// Host frozen into the pointer provider scope.
        host: PresentationHostLease,
        /// Logical surface whose presentation authority is owned elsewhere.
        surface: SurfaceId,
        /// Host which owns the current active presentation stream.
        owner: PresentationHostLease,
    },
    /// A surface-local provider was requested before any presentation stream
    /// owned the exact surface endpoint.
    #[error(
        "surface-local pointer host {host:?} cannot authorize surface {surface:?} before an active presentation stream exists"
    )]
    PointerProviderSurfaceAuthorityUnavailable {
        /// Host requesting a local input lane.
        host: PresentationHostLease,
        /// Surface whose first concrete output has not established authority.
        surface: SurfaceId,
    },
    /// A surface-local provider named a different endpoint incarnation from the
    /// active presentation stream.
    #[error(
        "surface-local pointer endpoint {submitted:?} for host {host:?} and surface {surface:?} does not match active presentation endpoint {active:?}"
    )]
    PointerProviderSurfaceEndpointMismatch {
        /// Host requesting a local input lane.
        host: PresentationHostLease,
        /// Surface shared by both endpoint descriptions.
        surface: SurfaceId,
        /// Endpoint supplied by the pointer producer.
        submitted: SurfaceLocalPointerEndpoint,
        /// Endpoint owned by the active presentation stream.
        active: HostPresentationEndpoint,
    },
    /// The presentation stream currently owning a surface endpoint has not
    /// crossed the final-presentation boundary for the retained hit graph.
    #[error(
        "surface-local pointer host {host:?} cannot authorize surface {surface:?}: active presentation {active_stream:?}/{active_endpoint:?} differs from presented interaction authority {presented_stream:?}/{presented_endpoint:?}"
    )]
    PointerProviderSurfacePresentationMismatch {
        /// Host requesting a local input lane.
        host: PresentationHostLease,
        /// Surface whose active and finally presented streams disagree.
        surface: SurfaceId,
        /// Stream currently selected by the presentation ledger.
        active_stream: HostPresentationStreamId,
        /// Endpoint currently selected by the presentation ledger.
        active_endpoint: HostPresentationEndpoint,
        /// Stream which owns the retained interaction projection.
        presented_stream: HostPresentationStreamId,
        /// Endpoint which owns the retained interaction projection.
        presented_endpoint: HostPresentationEndpoint,
    },
    /// A surface-local retirement attempted to delegate work that its local adapter cannot own.
    #[error(
        "surface-local pointer retirement produced {platform_effect_count} platform effects or a focus change ({focus_changed})"
    )]
    SurfaceLocalPointerRetirementExternalObligation {
        /// Number of platform effects that would require an external dispatcher.
        platform_effect_count: usize,
        /// Whether retirement would publish an adapter-visible focus delta.
        focus_changed: bool,
    },
    /// Joined backend ingress rejected provider pairing, ordering, or replay.
    #[error("backend ingress invariant failed: {source}")]
    BackendIngress {
        /// Exact backend-ingress rejection retained for diagnostics.
        #[source]
        source: BackendIngressError,
    },
    /// Joined backend authority no longer matches the active lane providers.
    #[error(
        "backend ingress {ingress:?} does not match active platform {platform:?} and pointer {pointer:?} providers"
    )]
    BackendIngressProviderMismatch {
        /// Joined provider pair frozen by the engine.
        ingress: BackendIngressLease,
        /// Currently active platform provider.
        platform: Option<PlatformObservationLease>,
        /// Currently active pointer provider.
        pointer: Option<PointerInputLease>,
    },
    /// Platform-observation provider authority rejected a lifecycle operation.
    #[error("platform observation provider invariant failed: {source}")]
    PlatformProvider {
        /// Exact provider-authority rejection retained for diagnostics.
        #[source]
        source: PlatformObservationAuthorityError,
    },
    /// The engine could not mint a non-replayable receiver attempt.
    #[error("pointer receiver attempt issuer failed: {source}")]
    PointerReceiverAttempt {
        /// Exact attempt issuer rejection retained for diagnostics.
        #[source]
        source: PointerReceiverAttemptError,
    },
    /// A pointer receipt was incomplete, foreign, stale, or otherwise unable
    /// to prove delivery against the current interactive output roster.
    #[error("pointer receiver receipt validation failed: {source}")]
    PointerReceiverReceipt {
        /// Exact receipt validation rejection retained for diagnostics.
        #[source]
        source: PointerReceiverReceiptValidationError,
    },
    /// A validated pointer receipt could not be bound to the exact presented
    /// projection that it references for this journal reduction.
    #[error("pointer journal presentation snapshot failed: {detail}")]
    JournalPresentationSnapshot {
        /// Exact snapshot failure retained for protocol diagnostics.
        detail: String,
    },
    /// A journal edge reached an internal counter, transaction, or scene
    /// invariant after its receipt and presentation authority were validated.
    #[error("pointer interaction reduction for {cause:?} failed: {detail}")]
    PointerInteractionInvariant {
        /// Exact core-minted journal cause being reduced.
        cause: ReductionCause,
        /// Stable diagnostic for the failed internal boundary.
        detail: String,
    },
    /// A receipt named a region which does not prove the semantic receiver at
    /// its own exact journal edge point.
    #[error("pointer receiver geometry validation failed: {source}")]
    PointerReceiverGeometry {
        /// Exact geometry rejection retained for diagnostics.
        #[source]
        source: PointerReceiverGeometryError,
    },
    /// A surface-local provider named an endpoint which the current core state
    /// cannot admit.
    #[error("pointer provider scope is unavailable: {detail}")]
    PointerProviderScope { detail: String },
    /// A frame froze a surface-local provider whose owning presentation host
    /// was not enrolled to report presentation facts in that frame.
    #[error(
        "pointer provider {provider:?} requires presentation host {host:?} outside the frozen frame scope"
    )]
    PointerProviderHostOutsideFrameScope {
        /// Exact frozen provider lease.
        provider: PointerInputLease,
        /// Required local presentation host.
        host: PresentationHostLease,
    },
    /// A live provider changed after a host frame froze its authority scope.
    #[error(
        "host frame pointer provider {submitted:?} no longer matches active provider {current:?}"
    )]
    HostFramePointerProviderStale {
        /// Provider frozen by the frame, when any.
        submitted: Option<PointerInputLease>,
        /// Provider active when the frame tried to finish.
        current: Option<PointerInputLease>,
    },
    /// A frame with a live pointer provider omitted its mandatory complete
    /// journal.
    #[error("host frame omitted the mandatory pointer journal for provider {provider:?}")]
    HostFramePointerJournalMissing {
        /// Frozen live provider lease.
        provider: PointerInputLease,
    },
    /// A frame with a staged pointer journal omitted its mandatory exact
    /// receiver receipt batch.
    #[error("host frame omitted mandatory pointer receiver receipts for provider {provider:?}")]
    HostFramePointerReceiverReceiptsMissing {
        /// Frozen live provider lease.
        provider: PointerInputLease,
    },
    /// A frame froze joined backend authority but did not complete one exact batch.
    #[error("host frame omitted or did not complete its mandatory backend ingress batch")]
    HostFrameBackendIngressIncomplete,
    /// A frame staged backend state although no joined provider was frozen.
    #[error("host frame staged backend ingress without a frozen joined provider")]
    HostFrameBackendIngressUnexpected,
    /// A terminal presentation host was used after its retirement boundary.
    #[error("presentation host {host:?} was retired at reducer tick {retired_at:?} ({reason:?})")]
    PresentationHostRetired {
        /// Exact terminal host lease.
        host: PresentationHostLease,
        /// Reducer boundary which recorded the retirement.
        retired_at: ReducerTickId,
        /// Original terminal lifecycle reason.
        reason: PresentationHostRetirementReason,
    },
    /// A terminal presentation host was used after its detailed tombstone was compacted.
    #[error(
        "presentation host {host:?} was retired and its detailed terminal record was compacted"
    )]
    PresentationHostRetiredCompacted {
        /// Exact terminal host lease.
        host: PresentationHostLease,
    },
    /// The engine-local recovery-obligation identity cannot advance without wrapping.
    #[error("surface recovery obligation identity is exhausted at input {input}")]
    SurfaceRecoveryObligationExhausted {
        /// Input which attempted to mint the obligation.
        input: InputSequence,
    },
    /// The engine-local root recovery anchor identity cannot advance without wrapping.
    #[error("root recovery anchor identity is exhausted at input {input}")]
    RootRecoveryAnchorExhausted {
        /// Input which attempted to mint the anchor.
        input: InputSequence,
    },
    /// One source repeated or moved backwards from its last submitted sequence.
    #[error(
        "input source {input_source} sequence {submitted} must be greater than previous sequence {previous}"
    )]
    SourceSequenceNotIncreasing {
        /// Stable semantic input producer.
        input_source: StableInputSourceId,
        /// Last accepted sequence or preceding sequence in this batch.
        previous: SourceSequence,
        /// Duplicate or decreasing sequence supplied by the producer.
        submitted: SourceSequence,
    },
    /// A host frame minted by one engine cannot reduce through another engine.
    #[error("host frame authority domain {submitted:?} does not match engine domain {expected:?}")]
    HostFrameAuthorityDomainMismatch {
        /// Domain of the engine receiving `finish`.
        expected: EngineAuthorityDomainId,
        /// Domain frozen by the frame at `begin_host_frame`.
        submitted: EngineAuthorityDomainId,
    },
    /// Core state changed after a host frame froze its exact roster and requirements.
    #[error(
        "host frame is stale: workspace {submitted_workspace:?}/{current_workspace:?}, requirements {submitted_requirements:?}/{current_requirements:?}"
    )]
    HostFrameStale {
        /// Workspace version frozen when the post-observation frame was sealed.
        submitted_workspace: WorkspaceVersion,
        /// Workspace version at frame finish.
        current_workspace: WorkspaceVersion,
        /// Requirement revision frozen when the post-observation frame was sealed.
        submitted_requirements: RequirementRevision,
        /// Requirement revision at frame finish.
        current_requirements: RequirementRevision,
    },
    /// A host frame was constructed against an earlier reducer boundary.
    ///
    /// A successful host-frame finish advances the reducer tick even when it
    /// has no semantic inputs. Reusing an independently begun capability after
    /// that boundary would make callback completion order part of the protocol.
    #[error(
        "host frame began after reducer tick {submitted:?}, but the current reducer tick is {current:?}"
    )]
    HostFramePredecessorStale {
        /// Reducer tick observed while the capability was minted.
        submitted: ReducerTickId,
        /// Last successfully committed reducer tick at finish time.
        current: ReducerTickId,
    },
    /// Presentation-host identity allocation advanced outside the frozen frame.
    ///
    /// Host creation does not consume a reducer tick. Publishing an older
    /// candidate across this boundary would otherwise discard the new lease
    /// and permit its serial to be minted again.
    #[error(
        "host frame presentation-host frontier {submitted} no longer matches current frontier {current}"
    )]
    HostFramePresentationHostFrontierStale {
        /// Host identity frontier frozen by the frame prelude.
        submitted: u64,
        /// Host identity frontier observed at seal or finish.
        current: u64,
    },
    /// Platform-provider authority changed outside the frozen frame.
    ///
    /// Provider activation does not consume a reducer tick. Publishing an
    /// older candidate across this boundary would otherwise revoke the live
    /// provider and restore a stale replacement state.
    #[error(
        "host frame platform-provider frontier {submitted} no longer matches current frontier {current}"
    )]
    HostFramePlatformProviderFrontierStale {
        /// Provider authority frontier frozen by the frame prelude.
        submitted: u64,
        /// Provider authority frontier observed at seal or finish.
        current: u64,
    },
    /// Retention-only state changed after an owned host frame was prepared.
    #[error(
        "host frame retention revision {submitted} no longer matches current revision {current}"
    )]
    HostFrameRuntimeRetentionStale {
        /// Retention revision frozen by the prepared candidate.
        submitted: u64,
        /// Current published retention revision.
        current: u64,
    },
    /// The engine-local retention revision cannot advance without wrapping.
    #[error("runtime retention revision is exhausted")]
    RuntimeRetentionRevisionExhausted,
    /// One physical surface was derived as both semantic content and native staging.
    #[error("physical surface {surface} appears in multiple host presentation slots")]
    HostPresentationRosterCollision {
        /// Surface whose lifecycle and semantic ownership overlap.
        surface: SurfaceId,
    },
    /// A staging output referenced a resource absent from the exact retained roster.
    #[error("native staging request references missing retained resource {resource:?}")]
    HostPresentationStagingResourceMissing {
        /// Resource which must stay pinned across this native-create saga.
        resource: NativeStagingResourceId,
    },
    /// The core-derived roster no longer matches the capability frozen at seal.
    #[error("core host-frame roster {submitted:?} no longer matches current roster {current:?}")]
    HostFrameRosterStale {
        /// Exact roster frozen by the capability.
        submitted: Vec<SurfaceId>,
        /// Current core-derived roster.
        current: Vec<SurfaceId>,
    },
    /// A host-frame prelude did not submit the mandatory presentation observation before sealing.
    #[error("host-frame prelude sealed without its mandatory presentation observation")]
    HostFramePresentationObservationMissing,
    /// An explicitly enrolled presentation observer did not submit its
    /// mandatory observation before the prelude was sealed.
    #[error("host-frame prelude sealed without the mandatory observation from {host:?}")]
    HostFrameSupplementaryPresentationObservationMissing {
        /// Observer lease that was frozen into this host frame.
        host: PresentationHostLease,
    },
    /// A caller attempted to enrol the rendering host as a supplementary
    /// observer as well.
    #[error("rendering host {host:?} cannot be enrolled as a supplementary observer")]
    HostFrameObserverIsRenderingHost {
        /// Duplicate rendering host lease.
        host: PresentationHostLease,
    },
    /// A caller repeated one supplementary observer lease while constructing a
    /// core-frozen host-frame scope.
    #[error("supplementary observer {host:?} was enrolled more than once")]
    HostFrameObserverDuplicate {
        /// Repeated observer lease.
        host: PresentationHostLease,
    },
    /// The core-owned presentation stream scope changed after host-frame begin.
    #[error(
        "host frame presentation stream scope {submitted:?} no longer matches current scope {current:?}"
    )]
    HostFramePresentationScopeStale {
        /// Exact pending stream scope frozen by the capability.
        submitted: Vec<HostPresentationStreamId>,
        /// Current pending stream scope for the same host lease.
        current: Vec<HostPresentationStreamId>,
    },
    /// A supplementary observer's pending stream scope changed after the
    /// frame began.
    #[error(
        "supplementary observer {host:?} presentation scope {submitted:?} no longer matches current scope {current:?}"
    )]
    HostFrameSupplementaryPresentationScopeStale {
        /// Observer lease whose scope changed.
        host: PresentationHostLease,
        /// Exact scope frozen by the capability.
        submitted: Vec<HostPresentationStreamId>,
        /// Scope at frame finish.
        current: Vec<HostPresentationStreamId>,
    },
    /// One or more core-frozen surfaces were not represented by exactly one contribution.
    ///
    /// Missing callbacks are not an implicit `KnownNone`: an adapter must
    /// submit an exact unavailable contribution when it cannot measure a
    /// surface during this host frame.
    #[error(
        "host frame contribution roster is incomplete; expected {expected:?}, submitted {submitted:?}"
    )]
    HostFrameContributionRosterIncomplete {
        /// Complete core-frozen roster.
        expected: Vec<SurfaceId>,
        /// Surfaces represented by submitted contributions.
        submitted: Vec<SurfaceId>,
    },
    /// One or more core-issued physical presentation obligations were omitted.
    ///
    /// Missing callbacks are never interpreted as an unavailable output. Each
    /// physical slot must be resolved as either painted or explicitly unavailable.
    #[error(
        "host frame presentation obligation roster is incomplete; expected {expected:?}, resolved {resolved:?}"
    )]
    HostFramePresentationObligationRosterIncomplete {
        /// Complete core-derived physical output roster.
        expected: Vec<HostPresentationSlot>,
        /// Physical slots carrying an explicit disposition.
        resolved: Vec<HostPresentationSlot>,
    },
    /// A structural host-frame construction error poisoned the entire batch.
    ///
    /// The original typed error is retained so callers can diagnose the first
    /// invalid action without allowing an accepted prefix to commit.
    #[error("host frame was poisoned while being constructed: {source}")]
    HostFramePoisoned {
        /// First structural construction error retained by the capability.
        #[source]
        source: CoreHostFrameError,
    },
    /// An internal event escaped reduction without its exact core-minted cause.
    #[error("reduction event cause invariant failed: {detail}")]
    ReductionCauseInvariant {
        /// Static diagnostic for the broken binding boundary.
        detail: &'static str,
    },
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
    /// The aggregate semantic-requirement counter cannot advance without wrapping.
    #[error("presentation requirement revision is exhausted while reducing input {input}")]
    PresentationRequirementRevisionExhausted {
        /// Input which attempted to replace presentation requirements.
        input: InputSequence,
    },
    /// The renderer-neutral presentation configuration counter cannot advance.
    #[error("presentation configuration revision is exhausted while reducing input {input}")]
    PresentationConfigRevisionExhausted {
        /// Input which attempted to replace semantic geometry.
        input: InputSequence,
    },
    /// The presentation policy counter cannot advance without wrapping.
    #[error("presentation policy revision is exhausted while reducing input {input}")]
    PolicyRevisionExhausted {
        /// Input which attempted to replace presentation policy.
        input: InputSequence,
    },
    /// One surface's independent requirement counter cannot advance without wrapping.
    #[error("surface {surface} requirement revision is exhausted while reducing input {input}")]
    SurfaceRequirementRevisionExhausted {
        /// Input which attempted to replace presentation requirements.
        input: InputSequence,
        /// Surface whose independent counter was exhausted.
        surface: crate::ids::SurfaceId,
    },
    /// Validated engine state could not produce its core-owned presentation manifest.
    #[error("presentation requirement invariant failed at input {input:?}: {detail}")]
    PresentationRequirementInvariant {
        /// Reducing input, or `None` during engine construction.
        input: Option<InputSequence>,
        /// Internal derivation detail retained for diagnostics.
        detail: String,
    },
    /// The internal close coordinator could not preserve its typed protocol invariants.
    #[error("close plan invariant failed at input {input}: {detail}")]
    ClosePlanInvariant {
        /// Input which attempted the invalid close transition.
        input: InputSequence,
        /// Internal protocol detail retained for diagnostics.
        detail: String,
    },
    /// A surface contribution could not advance core presentation state atomically.
    #[error("surface contribution {surface} from {base:?} failed: {detail}")]
    SurfaceContributionInvariant {
        /// Surface whose exact contribution was being reduced.
        surface: crate::ids::SurfaceId,
        /// Exact pre-measurement authority.
        base: SurfaceSceneStamp,
        /// Typed failure rendered without inventing an input sequence.
        detail: String,
    },
    /// A batch-level interaction reconciliation failed after surface contributions.
    #[error("surface contribution batch {tick} invariant failed: {detail}")]
    SurfaceContributionBatchInvariant {
        /// Reducer boundary containing the contributing surface batch.
        tick: ReducerTickId,
        /// Low-level invariant diagnostic.
        detail: String,
    },
    /// A batch-level interaction reconciliation failed after presentation observations.
    #[error("surface presentation observation batch {tick} invariant failed: {detail}")]
    SurfacePresentationObservationBatchInvariant {
        /// Reducer boundary containing the observation batch.
        tick: ReducerTickId,
        /// Low-level invariant diagnostic.
        detail: String,
    },
    /// Interaction reconciliation failed during terminal host retirement.
    #[error("presentation host retirement at reducer tick {tick} failed: {detail}")]
    PresentationHostRetirementInvariant {
        /// Reducer boundary which attempted the retirement.
        tick: ReducerTickId,
        /// Internal invariant diagnostic.
        detail: String,
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
    /// One surface presentation authority revision cannot advance without wrapping.
    #[error("surface {surface} scene revision is exhausted while reducing input {input}")]
    SurfaceSceneRevisionExhausted {
        /// Input whose state change required the revision.
        input: InputSequence,
        /// Surface whose private revision tombstone was exhausted.
        surface: crate::ids::SurfaceId,
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
    /// Viewport activation or pane-focus identity could not advance atomically.
    #[error("viewport focus input {input} failed: {source}")]
    ViewportFocus {
        /// Failing sequenced input.
        input: InputSequence,
        /// Typed activation and pane-focus coordinator failure.
        source: ViewportFocusError,
    },
    /// A provider named a docking binding which is not observable in the exact registry state.
    #[error("global focus input {input} names an unavailable binding {binding:?}")]
    GlobalFocusBindingUnavailable {
        /// Failing sequenced input.
        input: InputSequence,
        /// Exact stale, destroyed, or otherwise unobservable binding.
        binding: ViewportBinding,
    },
    /// A pointer edge or another already-bound reducer fact could not advance focus state.
    #[error("viewport focus reduction {cause:?} failed: {source}")]
    CausedViewportFocus {
        /// Exact core-minted reducer fact which attempted the transition.
        cause: ReductionCause,
        /// Typed activation and pane-focus coordinator failure.
        source: ViewportFocusError,
    },
    /// A lifecycle edge could not freeze the complete logical surface roster.
    #[error("surface roster input {input} failed: {source}")]
    SurfaceRoster {
        /// Failing sequenced input.
        input: InputSequence,
        /// Exact roster capture failure.
        source: SurfaceRosterCaptureError,
    },
    /// Complete-roster recovery failed after exact target facts were frozen.
    #[error("surface recovery input {input} failed: {source}")]
    SurfaceRecovery {
        /// Failing sequenced input.
        input: InputSequence,
        /// Typed recovery compiler failure.
        source: SurfaceRecoveryError,
    },
    /// An accepted close reached destruction without its edge-frozen roster.
    #[error("surface {surface} destroyed at input {input} without a frozen close roster")]
    MissingSurfaceRoster {
        /// Failing sequenced input.
        input: InputSequence,
        /// Destroyed logical surface.
        surface: crate::ids::SurfaceId,
    },
    /// Two lifecycle paths attempted to retain different rosters for one surface.
    #[error("surface {surface} has conflicting pending recovery rosters at input {input}")]
    ConflictingSurfaceRecovery {
        /// Failing sequenced input.
        input: InputSequence,
        /// Logical surface whose recovery ownership conflicted.
        surface: crate::ids::SurfaceId,
    },
}

fn presentation_ledger_error(source: PresentationLedgerError) -> EngineError {
    match source {
        PresentationLedgerError::HostRetired {
            host,
            retired_at,
            reason,
        } => EngineError::PresentationHostRetired {
            host,
            retired_at,
            reason,
        },
        PresentationLedgerError::HostRetiredCompacted { host } => {
            EngineError::PresentationHostRetiredCompacted { host }
        }
        source => EngineError::PresentationLedger {
            detail: source.to_string(),
        },
    }
}

/// Authoritative renderer-neutral docking engine.
#[derive(Debug, PartialEq)]
pub struct DockEngine {
    authority_domain: EngineAuthorityDomainId,
    workspace: Workspace,
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
    pending_drag_release: Option<PendingDragRelease>,
    pending_contained_transform_release: Option<PendingContainedTransformRelease>,
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
    workspace: Workspace,
    root_recovery_anchors: BTreeMap<crate::ids::SurfaceId, RootRecoveryAnchor>,
    bound_surface_recoveries: BTreeMap<crate::ids::SurfaceId, BoundSurfaceRecovery>,
}

#[derive(Debug, Clone, PartialEq)]
struct BoundSurfaceRecovery {
    binding: crate::viewport::ViewportBinding,
    obligation: SurfaceRecoveryObligation,
    retained_staging_resource: Option<NativeStagingResourceId>,
}

impl BoundSurfaceRecovery {
    const fn new(
        binding: crate::viewport::ViewportBinding,
        obligation: SurfaceRecoveryObligation,
    ) -> Self {
        Self {
            binding,
            obligation,
            retained_staging_resource: None,
        }
    }

    const fn retaining_staging_resource(mut self, resource: NativeStagingResourceId) -> Self {
        self.retained_staging_resource = Some(resource);
        self
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
}

#[derive(Debug, Clone, PartialEq)]
enum PreparedCloseOperation {
    Content(PreparedContentClose),
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
    semantic_presentations: Option<&'a BTreeMap<SurfaceId, JournalSurfacePresentation>>,
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
    release_decision: PreviewDecision,
    preview: crate::interaction::PreviewToken,
    presentation_outputs: BTreeSet<HostFrameKey>,
    presented_output: Option<HostFrameKey>,
    presentation_failed: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingContainedTransformRelease {
    source_version: WorkspaceVersion,
    policy_revision: PolicyRevision,
    cause: ReductionCause,
    session: ContainedTransformSessionId,
    transform: ActiveContainedTransform,
    placement: ContainedTransformPlacement,
    preview: crate::interaction::ContainedTransformPreviewToken,
    presentation_outputs: BTreeSet<HostFrameKey>,
    presented_output: Option<HostFrameKey>,
    presentation_failed: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct PreviewEvaluation {
    decision: PreviewDecision,
    affordance: Option<DropAffordance>,
}

struct StagedJournalWorkspaceCommand {
    publication: StagedWorkspacePublication,
    outcome: CommandOutcome,
    changed: bool,
}

impl PreviewEvaluation {
    const fn new(decision: PreviewDecision, affordance: Option<DropAffordance>) -> Self {
        Self {
            decision,
            affordance,
        }
    }

    const fn without_affordance(decision: PreviewDecision) -> Self {
        Self::new(decision, None)
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
    presentation: DragGestureAuthority,
    source_layout_facts: Option<std::sync::Arc<crate::scene::PresentationLayoutFacts>>,
}

#[derive(Clone)]
struct PreparedContainedTabOrigin {
    surface: crate::ids::SurfaceId,
    floating: crate::ids::FloatingPresentationId,
    source_rect: crate::geometry::LogicalRect,
    minimum_size: crate::geometry::LogicalSize,
    expected_roster: crate::command::ContainedRosterSource,
}

#[derive(Default)]
struct PlatformInteractionDependencies {
    owner_bindings: BTreeSet<crate::viewport::ViewportBinding>,
    target_bindings: BTreeSet<crate::viewport::ViewportBinding>,
    routed: bool,
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

enum DestroyedSurfaceRecoveryApplication {
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
    /// Creates an engine from a strictly validated workspace and explicit policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if `workspace` is corrupted.
    pub fn new(workspace: Workspace, policy: DockPolicy) -> Result<Self, EngineError> {
        Self::new_with_presentation_state(
            workspace,
            policy,
            DockPresentationConfig::default(),
            PresentationIdentityFrontier::empty(),
        )
    }

    /// Creates an engine from one atomic, document-validated restore.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::InvalidWorkspace`] if the restored workspace is corrupted.
    pub fn from_validated_restore(
        restore: ValidatedWorkspaceRestore,
        policy: DockPolicy,
    ) -> Result<Self, EngineError> {
        let (workspace, presentation_identity_frontier) = restore.into_parts();
        Self::new_with_presentation_state(
            workspace,
            policy,
            DockPresentationConfig::default(),
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
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        let presentation_identity =
            PresentationIdentityAuthority::new(&workspace, presentation_identity_frontier);
        let authority_domain =
            EngineAuthorityDomainId::mint().ok_or(EngineError::EngineAuthorityDomainExhausted)?;
        let version = WorkspaceVersion::default();
        let presentation_config_revision = PresentationConfigRevision::default();
        let policy = policy.snapshot(PolicyRevision::default());
        let requirement_revision = RequirementRevision::default();
        let interaction = InteractionState::default();
        let surface_semantic_snapshots = Self::capture_surface_semantic_snapshots(&workspace)
            .map_err(|source| EngineError::PresentationRequirementInvariant {
                input: None,
                detail: format!("{source:?}"),
            })?;
        let surface_requirement_revisions = workspace
            .surfaces()
            .map(|(surface, _)| (surface, SurfaceRequirementRevision::default()))
            .collect::<BTreeMap<_, _>>();
        let mut tab_strip_states = TabStripStateStore::default();
        let requirement_draft = derive_scene_requirement_draft(
            authority_domain,
            &workspace,
            version,
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
            workspace,
            presentation_identity,
            policy,
            version,
            presentation_authority: PresentationAuthorityState::new(
                authority_domain,
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
            pending_drag_release: None,
            pending_contained_transform_release: None,
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
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub(crate) const fn authority_domain(&self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the durable frontier required for an atomic dockspace document capture.
    #[must_use]
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

    /// Returns one exact interaction projection after the workspace-global
    /// popup presentation gate authorizes the complete surface roster.
    #[must_use]
    pub fn interaction_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        self.presentation_authority
            .scene
            .interaction_projection(surface)
    }

    /// Returns one exact final-presentation authority after the global gate.
    #[must_use]
    pub fn interaction_authority(&self, surface: SurfaceId) -> Option<PresentedSurfaceAuthority> {
        self.presentation_authority
            .scene
            .interaction_authority(surface)
    }

    fn freeze_interaction_projection(
        projection: SurfaceInteractionProjection<'_>,
    ) -> FrozenPresentationAuthority {
        FrozenPresentationAuthority::new(projection.authority(), projection.popup_gate_revision())
    }

    fn freeze_journal_presentation(
        presentation: &JournalSurfacePresentation,
    ) -> FrozenPresentationAuthority {
        FrozenPresentationAuthority::new(
            presentation.authority(),
            presentation.popup_gate_revision(),
        )
    }

    fn presentation_authority_is_current(&self, frozen: FrozenPresentationAuthority) -> bool {
        self.presentation_authority
            .scene
            .interaction_projection(frozen.surface())
            .is_some_and(|current| {
                current.popup_gate_revision() == frozen.popup_gate_revision
                    && current
                        .authority()
                        .same_interaction_semantics(frozen.presented)
            })
    }

    fn resize_continuation_is_current(&self, frozen: FrozenPresentationAuthority) -> bool {
        let InteractionStatus::Resizing { session } = self.interaction.status() else {
            return false;
        };
        let Ok(resize) = self.interaction.active_resize(session) else {
            return false;
        };
        if resize.authority.presented() != Some(frozen)
            || resize.surface != frozen.surface()
            || self
                .presentation_authority
                .presentation_requirements
                .surface(resize.surface)
                .is_none_or(|requirements| requirements.ticket() != resize.scene.requirement())
            || !Self::coordinate_capture_matches_current(
                resize.coordinate_capture,
                self.viewport.viewport(resize.surface),
                self.viewport.surface_coordinate_authority(resize.surface),
            )
        {
            return false;
        }

        resize.axis_groups.iter().all(|group| {
            group.handles.iter().all(|handle| {
                self.workspace
                    .capture_node_source(handle.source.root(), handle.source.node())
                    .is_ok_and(|current| current == handle.source)
                    && matches!(
                        self.workspace.presentation_for_root(handle.source.root()),
                        Some(crate::RootPresentationOwner::Main { surface })
                            | Some(crate::RootPresentationOwner::Contained { surface, .. })
                            if surface == resize.surface
                    )
            })
        })
    }

    fn scene_gesture_continuation_draft(
        &self,
        activation: ReductionCause,
        owner: GestureOwner,
        origin: FrozenPresentationAuthority,
        source: SceneGestureContinuationSource,
    ) -> Option<SceneGestureContinuationDraft> {
        Some(SceneGestureContinuationDraft {
            activation,
            owner,
            origin,
            workspace: self.version,
            policy: self.policy.revision(),
            config: self.presentation_authority.presentation_config_revision,
            requirements: self
                .presentation_authority
                .presentation_requirements
                .revision(),
            popup_routing: self
                .presentation_authority
                .presentation_requirements
                .popup()
                .revision(),
            source,
        })
    }

    fn scene_gesture_continuation_is_current(
        &self,
        continuation: &SceneGestureContinuation,
    ) -> bool {
        let draft = &continuation.draft;
        if self.version != draft.workspace
            || self.policy.revision() != draft.policy
            || self.presentation_authority.presentation_config_revision != draft.config
            || self
                .presentation_authority
                .presentation_requirements
                .revision()
                != draft.requirements
            || self
                .presentation_authority
                .presentation_requirements
                .popup()
                .revision()
                != draft.popup_routing
            || self.interaction.active_owner() != Some(draft.owner)
        {
            return false;
        }

        match (&continuation.session, &draft.source) {
            (
                SceneGestureSession::Drag(session),
                SceneGestureContinuationSource::Drag {
                    payload,
                    source_surface,
                    complete_root,
                    origin,
                    coordinates,
                },
            ) => {
                let active_source_matches = match self.interaction.status() {
                    InteractionStatus::Armed { session: active } if active == *session => {
                        self.interaction.armed_drag(*session).is_ok_and(|drag| {
                            drag.payload == *payload
                                && drag.source_surface == *source_surface
                                && drag.complete_root == *complete_root
                                && drag.origin == *origin
                                && drag.presentation.presented() == Some(draft.origin)
                        })
                    }
                    InteractionStatus::Dragging { session: active } if active == *session => {
                        self.interaction.active_drag(*session).is_ok_and(|drag| {
                            drag.payload == *payload
                                && drag.source_surface == *source_surface
                                && drag.complete_root == *complete_root
                                && drag.origin == *origin
                                && drag.presentation.presented() == Some(draft.origin)
                        })
                    }
                    InteractionStatus::Idle
                    | InteractionStatus::Pressed { .. }
                    | InteractionStatus::Armed { .. }
                    | InteractionStatus::Dragging { .. }
                    | InteractionStatus::Resizing { .. }
                    | InteractionStatus::ContainedTransforming { .. } => false,
                };
                let source_coordinates_are_current = match (self.interaction.status(), origin) {
                    (
                        InteractionStatus::Dragging { session: active },
                        FrozenDragOrigin::Workspace,
                    ) if active == *session => Self::coordinate_capture_incarnation_is_current(
                        *coordinates,
                        self.viewport.viewport(*source_surface),
                        self.viewport.surface_coordinate_authority(*source_surface),
                    ),
                    _ => Self::coordinate_capture_matches_current(
                        *coordinates,
                        self.viewport.viewport(*source_surface),
                        self.viewport.surface_coordinate_authority(*source_surface),
                    ),
                };
                if draft.origin.surface() != *source_surface
                    || !active_source_matches
                    || !source_coordinates_are_current
                {
                    return false;
                }
                let Ok(prepared) = self.prepare_drag_source(payload) else {
                    return false;
                };
                if prepared.source_surface != *source_surface
                    || prepared.complete_root != *complete_root
                    || !self.frozen_drag_origin_is_current(origin.clone())
                {
                    return false;
                }
                match origin {
                    FrozenDragOrigin::Workspace => true,
                    FrozenDragOrigin::Contained(origin) => self
                        .workspace
                        .capture_contained_roster(origin.surface)
                        .is_ok_and(|current| current == origin.source_roster),
                }
            }
            (
                SceneGestureSession::ContainedTransform(session),
                SceneGestureContinuationSource::ContainedTransform {
                    source,
                    surface,
                    root,
                    floating,
                    source_rect,
                    expected_roster,
                    coordinates,
                },
            ) => {
                let Ok(transform) = self.interaction.active_contained_transform(*session) else {
                    return false;
                };
                draft.origin.surface() == *surface
                    && transform.presentation == draft.origin
                    && transform.surface == *surface
                    && transform.root == *root
                    && transform.floating == *floating
                    && transform.source_rect == *source_rect
                    && Self::coordinate_capture_matches_current(
                        *coordinates,
                        self.viewport.viewport(*surface),
                        self.viewport.surface_coordinate_authority(*surface),
                    )
                    && self
                        .workspace
                        .capture_node_source(*root, source.node())
                        .is_ok_and(|current| current == *source)
                    && self.workspace.presentation_for_root(*root)
                        == Some(crate::RootPresentationOwner::Contained {
                            surface: *surface,
                            floating: *floating,
                        })
                    && self
                        .workspace
                        .contained_floating(*floating)
                        .is_some_and(|record| record.root == *root && record.rect == *source_rect)
                    && self
                        .workspace
                        .capture_contained_roster(*surface)
                        .is_ok_and(|current| current == *expected_roster)
            }
            (
                SceneGestureSession::Drag(_),
                SceneGestureContinuationSource::ContainedTransform { .. },
            )
            | (
                SceneGestureSession::ContainedTransform(_),
                SceneGestureContinuationSource::Drag { .. },
            ) => false,
        }
    }

    fn cancel_active_if_presentation_revoked(
        &mut self,
        input_for_error: InputSequence,
    ) -> Result<Option<InteractionStatus>, EngineError> {
        let Some(frozen) = self.interaction.active_presentation_authority() else {
            return Ok(None);
        };
        if self.presentation_authority_is_current(frozen)
            || self.resize_continuation_is_current(frozen)
            || self
                .interaction
                .active_scene_gesture_continuation()
                .is_some_and(|continuation| {
                    self.scene_gesture_continuation_is_current(continuation)
                })
        {
            return Ok(None);
        }
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport {
                input: input_for_error,
                source,
            })?;
        Ok(self.interaction.cancel_active())
    }

    fn cancel_revoked_presentation_caused(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(status) = self.cancel_active_if_presentation_revoked(self.last_input)? else {
            return Ok(None);
        };
        let reason = InteractionCancelReason::SceneUnavailable;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }

    fn cancel_revoked_presentation_input(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(status) = self.cancel_active_if_presentation_revoked(input)? else {
            return Ok(None);
        };
        let reason = InteractionCancelReason::SceneUnavailable;
        interaction_events.push(InteractionEvent::new(
            input,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }

    /// Returns count-only presentation-ledger diagnostics for conformance tests.
    #[doc(hidden)]
    #[must_use]
    pub fn presentation_ledger_diagnostics(&self) -> PresentationLedgerDiagnostics {
        self.presentation_authority.presentation.diagnostics()
    }

    /// Derives the exact concrete presentation emissions whose adapter resources remain live.
    ///
    /// This is the sole reclamation authority for renderer receiver sidecars. In particular, a
    /// frame age, a newer semantic ticket, or a viewport callback disappearing does not make an
    /// older emission reclaimable while core still retains it for settlement or interaction.
    #[must_use]
    pub fn presentation_retention_manifest(&self) -> PresentationRetentionManifest {
        PresentationRetentionManifest::from_resources(
            self.retained_presentation_emissions(),
            self.presentation_authority
                .presentation
                .retained_stream_ids(),
        )
    }

    /// Returns complete core-owned accounting for resources retained across host frames.
    ///
    /// The presentation subset is an executable adapter reclamation allow-list. The remaining
    /// subsets expose every retained structure and the protocol barrier which still prevents its
    /// safe compaction; no entry is hidden behind frame age or a fixed cache capacity.
    #[must_use]
    pub fn runtime_retention_manifest(&self) -> RuntimeRetentionManifest {
        RuntimeRetentionManifest::new(
            self.presentation_retention_manifest(),
            self.presentation_authority
                .presentation
                .retention_manifest(),
            self.viewport.effect_retention_manifest(),
            self.close.retention_manifest(),
            self.pointer_journal.retention_manifest(),
            self.viewport.binding_retention_manifest(),
            InputSourceRetentionManifest::new(usize::from(self.semantic_input_watermark.is_some())),
            self.scroll_interaction.retention_manifest(),
        )
    }

    /// Prepares an affine acknowledgement for a renderer that has externally quiesced a retiring
    /// presentation stream.
    ///
    /// The host must stop every path that could submit another observation for `stream` before
    /// calling this method. The returned acknowledgement is consumed by
    /// [`Self::confirm_presentation_stream_quiescence`], which revalidates core retention before
    /// reclaiming the stream record.
    ///
    /// # Errors
    ///
    /// Returns an error when the stream is foreign, still active, unsettled, or lacks a live
    /// same-host successor for its surface.
    pub fn prepare_presentation_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<PresentationStreamQuiescence, EngineError> {
        self.presentation_authority
            .presentation
            .prepare_stream_quiescence(host, stream)
            .map_err(presentation_ledger_error)
    }

    /// Tries to prepare stream reclamation without treating normal lifecycle progress as failure.
    ///
    /// `Ok(None)` means the exact stream is still active, has unsettled output, lacks its
    /// same-host successor, remains referenced by another core authority, or belongs to a host
    /// whose whole retirement path now owns cleanup. Those conditions are expected to change at a
    /// later host boundary. Foreign identities and host mismatches remain typed errors rather than
    /// being hidden as retryable state.
    ///
    /// # Errors
    ///
    /// Returns an error when `host` or `stream` is foreign, unknown, or belongs to a different
    /// host.
    pub fn try_prepare_presentation_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError> {
        let quiescence = match self
            .presentation_authority
            .presentation
            .prepare_stream_quiescence(host, stream)
        {
            Ok(quiescence) => quiescence,
            Err(
                PresentationLedgerError::HostRetired { .. }
                | PresentationLedgerError::StreamNotRetiring { .. }
                | PresentationLedgerError::StreamHasPendingOutputs { .. }
                | PresentationLedgerError::StreamQuiescenceRequiresSameHostSuccessor { .. },
            ) => return Ok(None),
            Err(source) => return Err(presentation_ledger_error(source)),
        };
        if self.retained_presentation_streams().contains(&stream) {
            return Ok(None);
        }
        Ok(Some(quiescence))
    }

    /// Consumes a renderer's affine quiescence acknowledgement for one retiring stream.
    ///
    /// No age or capacity policy participates in this reclamation. The exact stream remains in
    /// the retention manifest until this acknowledgement is accepted and every core reference has
    /// disappeared.
    ///
    /// # Errors
    ///
    /// Returns an error when core still retains the stream or its ledger state changed after the
    /// acknowledgement was prepared.
    pub fn confirm_presentation_stream_quiescence(
        &mut self,
        quiescence: PresentationStreamQuiescence,
    ) -> Result<(), EngineError> {
        self.confirm_presentation_stream_quiescence_batch([quiescence])
    }

    /// Atomically consumes renderer quiescence acknowledgements for retiring streams.
    ///
    /// Every acknowledgement is revalidated against one candidate engine. If any stream changed
    /// after preparation, none of the stream records are reclaimed.
    ///
    /// # Errors
    ///
    /// Returns an error when core still retains any stream, an acknowledgement is duplicated, or
    /// a ledger state changed after the acknowledgement was prepared.
    #[doc(hidden)]
    pub fn confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: impl IntoIterator<Item = PresentationStreamQuiescence>,
    ) -> Result<(), EngineError> {
        let mut candidate = self.candidate();
        let retained_streams = candidate.retained_presentation_streams();
        for quiescence in quiescences {
            candidate
                .presentation_authority
                .presentation
                .compact_quiesced_retiring_stream(quiescence, &retained_streams)
                .map_err(presentation_ledger_error)?;
        }
        candidate.advance_runtime_retention_revision()?;
        self.publish_candidate(candidate);
        Ok(())
    }

    /// Permanently retires one presentation host and all streams it ever owned.
    ///
    /// The core enumerates the host's complete stream roster, discards every
    /// pending output without promotion, releases only active ownership still
    /// pointing at those streams, and revokes matching scene interaction
    /// authority atomically. No rendering host or contribution roster is
    /// required because this is a terminal control boundary rather than a host
    /// frame.
    ///
    /// Repeating retirement is explicitly idempotent: the original tombstone is
    /// returned and the reducer tick does not advance.
    ///
    /// # Errors
    ///
    /// Returns a typed ledger or reducer error when the lease is foreign,
    /// unknown, or the atomic candidate cannot be published.
    pub fn retire_presentation_host(
        &mut self,
        host: PresentationHostLease,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirementOutcome, EngineError> {
        match self
            .presentation_authority
            .presentation
            .retirement_status(host)
            .map_err(presentation_ledger_error)?
        {
            PresentationHostRetirementStatus::Detailed(tombstone) => {
                return Ok(PresentationHostRetirementOutcome::AlreadyRetired { host, tombstone });
            }
            PresentationHostRetirementStatus::Compacted => {
                return Ok(PresentationHostRetirementOutcome::Compacted { host });
            }
            PresentationHostRetirementStatus::Live => {}
        }

        let before = self.version;
        let before_scene = self.presentation_authority.scene.clone();
        let before_viewport = self.viewport.clone();
        let before_viewport_focus = self.viewport_focus.clone();
        let effect_boundary = before_viewport.latest_effect_id();
        let mut candidate = self.candidate();
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;

        let retirement = candidate
            .presentation_authority
            .presentation
            .retire_host(host, tick, reason)
            .map_err(presentation_ledger_error)?;
        candidate.observe_pending_release_host_retirement(retirement.retired_outputs());
        let retired_pointer_provider =
            candidate.pointer_journal.active_lease().filter(|provider| {
                provider
                    .scope()
                    .surface_local()
                    .is_some_and(|scope| scope.host() == host)
            });
        if let Some(provider) = retired_pointer_provider {
            candidate
                .pointer_journal
                .retire_provider(provider)
                .map_err(|source| EngineError::PointerJournal { source })?;
        }
        let affected_surfaces = candidate
            .presentation_authority
            .scene
            .revoke_interaction_authority_for_streams(retirement.streams());
        let mut interaction_events = Vec::new();
        candidate.reconcile_interaction_after_host_retirement(
            tick,
            host,
            &affected_surfaces,
            &mut interaction_events,
        )?;
        if let Some(provider) = retired_pointer_provider {
            candidate.cancel_retired_pointer_owner(
                ReductionCause::PresentationHostRetirement { tick, host },
                provider,
                InteractionCancelReason::SceneUnavailable,
                &mut interaction_events,
            )?;
        }
        let mut events = Vec::new();
        let drag_release_settled = candidate
            .settle_presented_pending_drag_release(&mut events, &mut interaction_events)?;
        let contained_release_settled = candidate
            .settle_presented_pending_contained_transform_release(
                &mut events,
                &mut interaction_events,
            )?;
        if drag_release_settled || contained_release_settled {
            candidate.rebuild_presentation_requirements(candidate.last_input)?;
        }
        candidate.settle_retired_presentation_hosts()?;

        let platform_effects = candidate
            .viewport
            .try_take_new_effects_after(effect_boundary)
            .map_err(|source| EngineError::Viewport {
                input: candidate.last_input,
                source,
            })?;
        candidate
            .record_emitted_native_surface_close_effects(candidate.last_input, &platform_effects)?;
        let focus_delta = FocusDelta::between(
            &before_viewport_focus,
            &candidate.viewport_focus,
            before_viewport.effects(),
            candidate.viewport.effects(),
            &[],
        );
        let surface_scene_deltas =
            Self::surface_scene_deltas(&before_scene, &candidate.presentation_authority.scene);
        // The durable host tombstone always changes published core state, even
        // when the host never emitted a stream and no scene delta is visible.
        let published_state_changed = true;
        let transition = EngineTransition::new(EngineTransitionParts {
            authority_domain: candidate.authority_domain,
            tick,
            before,
            after: candidate.version,
            reduced: Vec::new(),
            reduced_pointer_edges: Vec::new(),
            events,
            interaction_events,
            platform_effects,
            focus_delta,
            presentation_observations: Vec::new(),
            presentation_emissions: Vec::new(),
            presentation_dispositions: Vec::new(),
            surface_contributions: Vec::new(),
            surface_scene_deltas,
            published_state_changed,
        });
        let outcome = PresentationHostRetirementOutcome::Retired {
            host: retirement.host(),
            reason: retirement.tombstone().reason(),
            retired_stream_count: retirement.streams().len(),
            retired_output_count: retirement.retired_output_count(),
            released_active_surfaces: retirement
                .released_active_surfaces()
                .iter()
                .copied()
                .collect(),
            affected_surfaces: affected_surfaces.iter().copied().collect(),
            transition,
        };
        self.publish_candidate(candidate);
        Ok(outcome)
    }

    /// Returns the published transient interaction state.
    #[must_use]
    pub const fn interaction(&self) -> &InteractionState {
        &self.interaction
    }

    /// Returns the exact release preview retained until the host proves that it
    /// was presented.
    #[must_use]
    pub fn pending_release_preview(
        &self,
    ) -> Option<(
        crate::interaction::DragSessionId,
        crate::interaction::PreviewToken,
    )> {
        self.pending_drag_release
            .as_ref()
            .map(|pending| (pending.session, pending.preview))
    }

    /// Returns the exact contained-transform release preview retained until the
    /// host proves that it was presented.
    #[must_use]
    pub fn pending_contained_transform_release_preview(
        &self,
    ) -> Option<(
        ContainedTransformSessionId,
        crate::interaction::ContainedTransformPreviewToken,
    )> {
        self.pending_contained_transform_release
            .as_ref()
            .map(|pending| (pending.session, pending.preview))
    }

    /// Returns the drag preview which must be represented by the next host
    /// presentation, including a release preview whose gesture is already idle.
    #[must_use]
    pub fn presentation_preview(&self) -> Option<&crate::interaction::InteractionPreview> {
        self.interaction.preview().or_else(|| {
            self.pending_drag_release
                .as_ref()
                .and_then(|pending| pending.drag.preview.as_ref())
                .map(crate::interaction::PublishedPreview::public)
        })
    }

    /// Returns the contained-transform preview which must be represented by the
    /// next host presentation, including a release preview after gesture end.
    #[must_use]
    pub fn presentation_contained_transform_preview(&self) -> Option<&ContainedTransformPreview> {
        self.interaction.contained_transform_preview().or_else(|| {
            self.pending_contained_transform_release
                .as_ref()
                .and_then(|pending| pending.transform.preview.as_ref())
                .map(crate::interaction::PublishedContainedTransformPreview::public)
        })
    }

    /// Returns the latest public state of one exact close request.
    #[must_use]
    pub fn close_plan(&self, request: CloseRequestId) -> Option<&ClosePlan> {
        self.close.plan(request)
    }

    /// Classifies a close request without retaining every terminal payload indefinitely.
    #[must_use]
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
                pending.destroyed_binding() == bound.binding
                    && pending.recovery_obligation() == bound.obligation.id()
            });
        (live || retained).then(|| bound.obligation.target())
    }

    /// Returns the complete roster retained for one unresolved destroyed surface.
    #[must_use]
    pub fn pending_surface_recovery(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Option<&SurfaceRosterDisposition> {
        self.surface_recovery.pending(surface)
    }

    /// Returns why one destroyed child surface is retained for a later exact retry.
    #[must_use]
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
    pub const fn last_input_sequence(&self) -> InputSequence {
        self.last_input
    }

    /// Returns the last successfully committed sequence for the semantic writer lane.
    ///
    /// [`StableInputSourceId`] values attached to individual inputs are diagnostic labels. They
    /// do not partition replay authority: every non-backend semantic input submitted to this
    /// engine must advance this one session-owned sequence.
    #[must_use]
    pub const fn semantic_input_watermark(&self) -> Option<SourceSequence> {
        self.semantic_input_watermark
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
        work_area: crate::viewport::WorkAreaToken,
    ) -> Result<crate::coordinates::ViewportPlacementProof, crate::coordinates::CoordinateUnavailable>
    {
        self.viewport.placement(surface, rect, work_area)
    }

    #[must_use]
    pub fn native_placement_is_current(&self, proof: &crate::intent::NativePlacementProof) -> bool {
        match proof {
            NativePlacementProof::TearOff(proof) => {
                self.pointer_provider() == Some(proof.pointer_provider())
                    && self.platform_provider() == Some(proof.platform_provider())
                    && self.viewport.work_area_generation() == proof.work_area_generation()
                    && self.viewport.work_area(proof.work_area()).is_some()
                    && self.viewport.native_tear_off_capability().is_supported()
            }
            NativePlacementProof::Surface(_) => self.viewport.native_placement_is_current(proof),
        }
    }

    /// Produces a deterministic contained placement from current ready surface bounds.
    ///
    /// The returned proof is valid only for the exact sealed scene generation from which it was
    /// derived. The requested size is first expanded to `minimum_size`, capped by the surface
    /// bounds, and then translated into those bounds without any history-based heuristic.
    ///
    /// # Errors
    ///
    /// Returns [`ContainedPlacementUnavailable`] when no current ready scene exists, the surface
    /// has no usable area, or finite corners cannot represent a strictly positive finite clamp.
    pub fn contained_placement(
        &self,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        Self::contained_placement_from_scene(
            &self.presentation_authority.scene,
            self.version,
            surface,
            requested_rect,
            minimum_size,
        )
    }

    fn contained_placement_from_scene(
        scene: &SurfaceSceneSet,
        workspace: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        let (stamp, bounds) = Self::ready_surface_bounds(scene, workspace, surface)?;
        let clamped_rect = clamp_contained_rect(surface, bounds, requested_rect, minimum_size)?;
        Ok(ContainedPlacementProof::new(
            stamp,
            surface,
            requested_rect,
            minimum_size,
            bounds,
            clamped_rect,
        ))
    }

    fn ready_surface_bounds(
        scene: &SurfaceSceneSet,
        workspace: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
    ) -> Result<(SurfaceSceneStamp, crate::geometry::LogicalRect), ContainedPlacementUnavailable>
    {
        let ready = match scene.ready_surface(surface) {
            Some(ready) => ready,
            None if matches!(scene.surface(surface), Some(SurfaceScene::Ready(_))) => {
                return Err(ContainedPlacementUnavailable::PendingPaintSurface { surface });
            }
            None if matches!(scene.surface(surface), Some(SurfaceScene::Stale(_))) => {
                return Err(ContainedPlacementUnavailable::StaleSurface { surface });
            }
            None if matches!(scene.surface(surface), Some(SurfaceScene::Bootstrap(_))) => {
                return Err(ContainedPlacementUnavailable::BootstrapSurface { surface });
            }
            None => return Err(ContainedPlacementUnavailable::MissingSurface { surface }),
        };
        if ready.stamp().requirement().workspace_epoch() != workspace.epoch() {
            return Err(ContainedPlacementUnavailable::SceneUnavailable);
        }
        Ok((ready.stamp(), ready.plan().bounds()))
    }

    fn validate_contained_placement(
        &self,
        proof: ContainedPlacementProof,
    ) -> Result<(), ContainedPlacementUnavailable> {
        let scene = &self.presentation_authority.scene;
        let current = scene
            .ready_surface(proof.surface())
            .map(|ready| ready.stamp());
        if current != Some(proof.scene())
            || proof.scene().requirement().workspace_epoch() != self.version.epoch()
        {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: proof.scene(),
                current,
            });
        }
        let ready = match scene.ready_surface(proof.surface()) {
            Some(ready) => ready,
            None if matches!(scene.surface(proof.surface()), Some(SurfaceScene::Ready(_))) => {
                return Err(ContainedPlacementUnavailable::PendingPaintSurface {
                    surface: proof.surface(),
                });
            }
            None if matches!(scene.surface(proof.surface()), Some(SurfaceScene::Stale(_))) => {
                return Err(ContainedPlacementUnavailable::StaleSurface {
                    surface: proof.surface(),
                });
            }
            None if matches!(
                scene.surface(proof.surface()),
                Some(SurfaceScene::Bootstrap(_))
            ) =>
            {
                return Err(ContainedPlacementUnavailable::BootstrapSurface {
                    surface: proof.surface(),
                });
            }
            None => {
                return Err(ContainedPlacementUnavailable::MissingSurface {
                    surface: proof.surface(),
                });
            }
        };
        if ready.plan().bounds() != proof.surface_bounds() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        let reproduced = clamp_contained_rect(
            proof.surface(),
            ready.plan().bounds(),
            proof.requested_rect(),
            proof.minimum_size(),
        )?;
        if reproduced != proof.clamped_rect() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        Ok(())
    }

    fn apply_journal_workspace_command(
        &mut self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Result<(CommandOutcome, bool), CommandError>, EngineError> {
        let staged = match self.stage_journal_workspace_command(cause, command, policy)? {
            Ok(staged) => staged,
            Err(source) => return Ok(Err(source)),
        };
        self.publish_workspace(staged.publication);
        self.reconcile_viewport_focus_authority();
        if staged.changed {
            self.advance_revision_caused(cause)?;
            events.push(WorkspaceEvent::new_caused(
                cause,
                self.version,
                WorkspaceEventKind::CommandCommitted(staged.outcome.clone()),
            ));
        }
        Ok(Ok((staged.outcome, staged.changed)))
    }

    fn stage_journal_workspace_command(
        &self,
        cause: ReductionCause,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<StagedJournalWorkspaceCommand, CommandError>, EngineError> {
        let mut workspace = self.clone_workspace_candidate();
        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut workspace, policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(Err(source));
            }
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        if let Some(surface) = self.first_workspace_publication_mismatch(&workspace, None, None) {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        let changed = report.changed();
        let mut outcomes = report.into_outcomes();
        if outcomes.len() != 1 {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "one journal drop command produced a non-unit outcome roster".to_owned(),
            });
        }
        let outcome = outcomes
            .pop()
            .expect("unit outcome roster was checked before extraction");
        let publication = match self.stage_workspace_publication(workspace, policy) {
            Ok(publication) => publication,
            Err(source) if source.is_expected_rejection() => return Ok(Err(source)),
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        Ok(Ok(StagedJournalWorkspaceCommand {
            publication,
            outcome,
            changed,
        }))
    }

    fn capture_surface_coordinates(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> SurfaceCoordinateCapture {
        let authority_generation = self.viewport.surface_coordinate_authority(surface);
        match self.viewport.viewport(surface) {
            None => SurfaceCoordinateCapture::Headless {
                authority_generation,
            },
            Some(record) if record.has_coordinate_authority() => match record.coordinates() {
                Some(coordinates) => SurfaceCoordinateCapture::NativeReady {
                    coordinates,
                    authority_generation,
                },
                None => SurfaceCoordinateCapture::NativeUnavailable {
                    binding: record.binding(),
                    lifecycle: record.lifecycle(),
                    authority_generation,
                },
            },
            Some(record) => SurfaceCoordinateCapture::NativeUnavailable {
                binding: record.binding(),
                lifecycle: record.lifecycle(),
                authority_generation,
            },
        }
    }

    fn coordinate_capture_matches_current(
        capture: SurfaceCoordinateCapture,
        current: Option<&crate::viewport_registry::ViewportRecord>,
        current_authority: crate::viewport::CoordinateGeneration,
    ) -> bool {
        match capture {
            SurfaceCoordinateCapture::Headless {
                authority_generation,
            } => current.is_none() && authority_generation == current_authority,
            SurfaceCoordinateCapture::NativeUnavailable {
                binding,
                lifecycle,
                authority_generation,
            } => {
                authority_generation == current_authority
                    && current.is_some_and(|record| {
                        !record.has_coordinate_authority()
                            && record.binding() == binding
                            && record.lifecycle() == lifecycle
                            && record.coordinate_generation() == authority_generation
                    })
            }
            SurfaceCoordinateCapture::NativeReady {
                coordinates,
                authority_generation,
            } => {
                authority_generation == current_authority
                    && current.is_some_and(|record| {
                        record.has_coordinate_authority()
                            && record.coordinate_generation() == authority_generation
                            && record.coordinates().is_some_and(|current| {
                                current.binding() == coordinates.binding()
                                    && current.coordinate_generation()
                                        == coordinates.coordinate_generation()
                                    && current.content_bounds() == coordinates.content_bounds()
                                    && current.native_scale_factor()
                                        == coordinates.native_scale_factor()
                                    && current.presentation_scale_factor()
                                        == coordinates.presentation_scale_factor()
                            })
                    })
            }
        }
    }

    fn coordinate_capture_incarnation_is_current(
        capture: SurfaceCoordinateCapture,
        current: Option<&crate::viewport_registry::ViewportRecord>,
        current_authority: crate::viewport::CoordinateGeneration,
    ) -> bool {
        match Self::coordinate_capture_binding(capture) {
            Some(binding) => current.is_some_and(|record| record.binding() == binding),
            None => Self::coordinate_capture_matches_current(capture, current, current_authority),
        }
    }

    const fn coordinate_capture_binding(
        capture: SurfaceCoordinateCapture,
    ) -> Option<crate::viewport::ViewportBinding> {
        match capture {
            SurfaceCoordinateCapture::Headless { .. } => None,
            SurfaceCoordinateCapture::NativeUnavailable { binding, .. } => Some(binding),
            SurfaceCoordinateCapture::NativeReady { coordinates, .. } => {
                Some(coordinates.binding())
            }
        }
    }

    /// Finishes one sealed core-minted host frame and publishes it atomically.
    ///
    /// The capability must come from [`Self::begin_host_frame`] on this exact
    /// engine. Its frozen domain, authority frontiers, workspace version,
    /// requirement revision, and complete roster are revalidated before any
    /// tick or source watermark can advance. Semantic input then reduces
    /// strictly in append order; only the separately explicit configuration
    /// phase follows surface contributions.
    fn platform_provider_rejection(
        &self,
        provider: PlatformObservationLease,
    ) -> Option<InputOutcome> {
        self.viewport
            .require_platform_provider(provider)
            .err()
            .map(|error| InputOutcome::PlatformProviderRejected { provider, error })
    }

    fn reduce_workspace_replacement(
        &mut self,
        input: InputSequence,
        workspace: &Workspace,
        restored_identity_frontier: Option<PresentationIdentityFrontier>,
        application_base: &mut WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        workspace
            .validate()
            .map_err(EngineError::InvalidWorkspace)?;
        self.validate_workspace_replacement_identity_freshness(workspace)
            .map_err(|source| EngineError::WorkspaceReplacementIdentityRetired { input, source })?;
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
        let rebound_root_surfaces = reconciliation
            .rebound()
            .iter()
            .filter_map(|(_, binding)| {
                self.viewport
                    .viewport(binding.surface())
                    .filter(|record| {
                        record.binding() == *binding && record.role() == ViewportRole::Root
                    })
                    .map(|_| binding.surface())
            })
            .collect::<BTreeSet<_>>();
        match restored_identity_frontier {
            Some(restored) => self
                .presentation_identity
                .merge_restored(workspace, restored),
            None => self.presentation_identity.observe_workspace(workspace),
        }
        self.workspace = workspace.clone();
        self.surface_recovery.clear();
        self.root_recovery_anchors.clear();
        self.bound_surface_recoveries.clear();
        self.native_admission.clear();
        self.issue_root_recovery_anchors(input, rebound_root_surfaces)?;
        self.viewport_focus.reconcile_workspace_replacement();
        self.version = WorkspaceVersion::new(epoch, WorkspaceRevision::default());
        self.close.invalidate_stale(self.close_authority());
        self.rebuild_presentation_requirements(input)?;
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
            restored_identity_frontier,
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
        let changed = !self.policy.has_same_rules(policy);
        if changed {
            let policy_revision = self
                .policy
                .revision()
                .checked_next()
                .ok_or(EngineError::PolicyRevisionExhausted { input })?;
            self.policy = policy.snapshot(policy_revision);
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

    fn reduce_presentation_config_replacement(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        accepted_base: WorkspaceVersion,
        config: &DockPresentationConfig,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }
        let changed = self.presentation_authority.presentation_config != *config;
        if changed {
            let revision = self
                .presentation_authority
                .presentation_config_revision
                .checked_next()
                .ok_or(EngineError::PresentationConfigRevisionExhausted { input })?;
            self.presentation_authority.presentation_config = config.clone();
            self.presentation_authority.presentation_config_revision = revision;
            self.rebuild_presentation_requirements(input)?;
            self.terminate_all_scroll_sessions_input(
                input,
                ScrollTerminationReason::PresentationConfigChanged,
                interaction_events,
            );
            self.invalidate_transient(
                input,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            )?;
        }
        Ok(InputOutcome::PresentationConfigReplaced {
            changed,
            revision: self.presentation_authority.presentation_config_revision,
        })
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
            engine.apply_contained_placement(input, placement, policy, events, interaction_events)
        })
    }

    fn reduce_preview_acknowledgement(
        &mut self,
        expected: WorkspaceVersion,
        acknowledgement: &PaintAcknowledgement,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            Ok(
                match engine.interaction.acknowledge_preview(acknowledgement) {
                    Ok((session, changed)) => {
                        InteractionOutcome::PreviewAcknowledged { session, changed }
                    }
                    Err(error) => InteractionOutcome::Rejected(error),
                },
            )
        })
    }

    fn reduce_contained_transform_preview_acknowledgement(
        &mut self,
        expected: WorkspaceVersion,
        acknowledgement: ContainedTransformPaintAcknowledgement,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            Ok(
                match engine
                    .interaction
                    .acknowledge_contained_transform_preview(acknowledgement)
                {
                    Ok((session, changed)) => {
                        InteractionOutcome::ContainedTransformPreviewAcknowledged {
                            session,
                            changed,
                        }
                    }
                    Err(error) => InteractionOutcome::Rejected(error),
                },
            )
        })
    }

    fn apply_contained_placement(
        &mut self,
        input: InputSequence,
        update: ContainedPlacementInput,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let command = match self.checked_contained_placement_command(update) {
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
    ) -> Result<WorkspaceCommand, InteractionRejection> {
        self.validate_contained_placement(update.placement)
            .map_err(|error| match error {
                ContainedPlacementUnavailable::StaleScene { .. }
                | ContainedPlacementUnavailable::SceneUnavailable => {
                    InteractionRejection::StaleScene
                }
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

    fn prepare_drag_source(
        &self,
        payload: &MovePayload,
    ) -> Result<PreparedDragSource, InteractionRejection> {
        self.validate_payload(payload)
            .map_err(InteractionRejection::CommandRejected)?;
        let complete_root = self
            .complete_root_source(payload)
            .map_err(InteractionRejection::CommandRejected)?;
        let partial_detachable =
            complete_root.is_some() || self.partial_payload_is_detachable(payload);
        let source_surface =
            self.payload_surface(payload)
                .ok_or(InteractionRejection::CommandRejected(
                    CommandError::Invariant {
                        stage: "freeze drag source surface",
                    },
                ))?;
        Ok(PreparedDragSource {
            source_surface,
            complete_root,
            partial_detachable,
        })
    }

    #[cfg(test)]
    fn native_create_reserves_root(&self, root: crate::ids::RootId) -> bool {
        self.viewport
            .native_create_sagas()
            .any(|(_, saga)| saga.prepared().proposal().root() == root)
    }

    fn native_create_reserves_floating(
        &self,
        floating: crate::ids::FloatingPresentationId,
    ) -> bool {
        self.viewport
            .native_create_sagas()
            .any(|(_, saga)| saga.prepared().proposal().converted_main().floating() == floating)
    }

    fn drag_source_is_current(
        &mut self,
        session: crate::interaction::DragSessionId,
        input: InputSequence,
    ) -> Result<bool, EngineError> {
        let (payload, complete_root, origin) = {
            let drag =
                self.interaction
                    .active_drag(session)
                    .map_err(|_| EngineError::Interaction {
                        input,
                        source: InteractionCounterError::StateInvariant,
                    })?;
            if drag.source_validated_at == self.version {
                return Ok(true);
            }
            (
                drag.payload.clone(),
                drag.complete_root.clone(),
                drag.origin.clone(),
            )
        };
        let Ok(prepared) = self.prepare_drag_source(&payload) else {
            return Ok(false);
        };
        let valid =
            prepared.complete_root == complete_root && self.frozen_drag_origin_is_current(origin);
        if valid {
            let drag = self.interaction.active_drag_mut(session).map_err(|_| {
                EngineError::Interaction {
                    input,
                    source: InteractionCounterError::StateInvariant,
                }
            })?;
            drag.source_validated_at = self.version;
            drag.partial_detachable = prepared.partial_detachable;
        }
        Ok(valid)
    }

    fn drag_source_presentation_allows_targeting(
        &self,
        drag: &crate::interaction::ActiveDrag,
    ) -> bool {
        if let Some(coordinates) = drag.presentation.local_coordinates() {
            let surface = drag.presentation.surface();
            return surface == drag.source_surface
                && Self::coordinate_capture_matches_current(
                    coordinates,
                    self.viewport.viewport(surface),
                    self.viewport.surface_coordinate_authority(surface),
                )
                && self
                    .presentation_authority
                    .scene
                    .surface(surface)
                    .and_then(SurfaceScene::ready)
                    .map(crate::scene::ReadySurfaceScene::candidate)
                    .is_some_and(|candidate| {
                        candidate.stamp().requirement().workspace_epoch() == self.version.epoch()
                            && candidate.coordinate_capture() == coordinates
                    });
        }
        if !matches!(drag.origin, FrozenDragOrigin::Workspace) {
            return true;
        }
        let surface = drag.source_surface;
        let captured_coordinates = drag.continuation.as_ref().and_then(|continuation| {
            match (&continuation.session, &continuation.draft.source) {
                (
                    SceneGestureSession::Drag(session),
                    SceneGestureContinuationSource::Drag {
                        source_surface,
                        coordinates,
                        ..
                    },
                ) if *session == drag.session && *source_surface == surface => Some(*coordinates),
                _ => None,
            }
        });
        if captured_coordinates.is_some_and(|coordinates| {
            Self::coordinate_capture_matches_current(
                coordinates,
                self.viewport.viewport(surface),
                self.viewport.surface_coordinate_authority(surface),
            )
        }) {
            return true;
        }
        self.presentation_authority
            .scene
            .interaction_projection(surface)
            .is_some_and(|projection| {
                Self::coordinate_capture_matches_current(
                    projection.output().coordinate_capture(),
                    self.viewport.viewport(surface),
                    self.viewport.surface_coordinate_authority(surface),
                )
            })
    }

    fn frozen_drag_origin_is_current(&self, origin: FrozenDragOrigin) -> bool {
        let FrozenDragOrigin::Contained(origin) = origin else {
            return true;
        };
        self.workspace.presentation_for_root(origin.root)
            == Some(crate::RootPresentationOwner::Contained {
                surface: origin.surface,
                floating: origin.floating,
            })
            && self
                .workspace
                .contained_floating(origin.floating)
                .is_some_and(|record| {
                    record.root == origin.root && record.rect == origin.source_rect
                })
    }

    fn valid_core_contained_command(
        &self,
        drag: &crate::interaction::ActiveDrag,
        proposal: crate::intent::ContainedTearOffProposal,
        command: &WorkspaceCommand,
    ) -> bool {
        if self
            .validate_contained_placement(proposal.placement())
            .is_err()
        {
            return false;
        }
        match command {
            WorkspaceCommand::UpdateContainedPresentation {
                source,
                floating,
                expected_rect,
                expected_roster,
                rect,
                position,
            } => {
                let FrozenDragOrigin::Contained(origin) = &drag.origin else {
                    return false;
                };
                let source_matches = drag.complete_root.as_ref().is_some_and(|source| {
                    source.root() == origin.root
                        && match &drag.payload {
                            MovePayload::Item(item) => {
                                item.root() == source.root() && item.tabs() == source.node()
                            }
                            MovePayload::Tabs(payload) | MovePayload::Subtree(payload) => {
                                payload == source
                            }
                        }
                });
                let owner_matches = self.workspace.presentation_for_root(origin.root)
                    == Some(crate::RootPresentationOwner::Contained {
                        surface: origin.surface,
                        floating: origin.floating,
                    });
                let record_matches = self
                    .workspace
                    .contained_floating(origin.floating)
                    .is_some_and(|record| {
                        record.root == origin.root && record.rect == origin.source_rect
                    });
                drag.source_validated_at == self.version
                    && source_matches
                    && owner_matches
                    && record_matches
                    && proposal.surface() == origin.surface
                    && proposal.root() == origin.root
                    && proposal.floating() == origin.floating
                    && drag.complete_root.as_ref() == Some(source)
                    && *floating == origin.floating
                    && *expected_rect == origin.source_rect
                    && expected_roster == &origin.source_roster
                    && *rect == proposal.rect()
                    && *position == proposal.position()
            }
            WorkspaceCommand::CreateContainedRoot {
                surface,
                root,
                floating,
                rect,
                position,
                content: RootContent::Move(payload),
            } => {
                drag.complete_root.is_none()
                    && payload == &drag.payload
                    && *surface == proposal.surface()
                    && *root == proposal.root()
                    && *floating == proposal.floating()
                    && *rect == proposal.rect()
                    && *position == proposal.position()
                    && self.workspace.surface(*surface).is_some()
                    && self.workspace.root(*root).is_none()
                    && self.workspace.contained_floating(*floating).is_none()
                    && drag.partial_detachable
            }
            WorkspaceCommand::RehomeRoot { source, target } => {
                drag.complete_root.as_ref() == Some(source)
                    && self.core_contained_rehome_is_valid(source, *target, proposal)
            }
            WorkspaceCommand::CreateContainedRoot { .. }
            | WorkspaceCommand::UpdateContainedRect { .. }
            | WorkspaceCommand::Select { .. }
            | WorkspaceCommand::Reorder { .. }
            | WorkspaceCommand::Open { .. }
            | WorkspaceCommand::Move { .. }
            | WorkspaceCommand::ResizeSplits { .. }
            | WorkspaceCommand::CreateSurfaceRoot { .. }
            | WorkspaceCommand::InstallMainRoot { .. }
            | WorkspaceCommand::PromoteContained { .. }
            | WorkspaceCommand::RaiseContained { .. }
            | WorkspaceCommand::RemoveEmptyRoot { .. } => false,
        }
    }

    fn partial_payload_is_detachable(&self, payload: &MovePayload) -> bool {
        #[cfg(test)]
        PARTIAL_DETACHABILITY_EVALUATIONS.with(|evaluations| {
            evaluations.set(evaluations.get().saturating_add(1));
        });
        let (MovePayload::Tabs(source) | MovePayload::Subtree(source)) = payload else {
            return true;
        };
        let Some(root) = self.workspace.root(source.root()) else {
            return false;
        };
        root.central
            .is_none_or(|central| !self.workspace.subtree_contains(source.node(), central))
    }

    fn core_contained_rehome_is_valid(
        &self,
        source: &NodeSource,
        target: RootPresentationTarget,
        proposal: crate::intent::ContainedTearOffProposal,
    ) -> bool {
        let RootPresentationTarget::Contained {
            surface,
            floating,
            rect,
            position,
        } = target
        else {
            return false;
        };
        if source.root() != proposal.root()
            || surface != proposal.surface()
            || floating != proposal.floating()
            || rect != proposal.rect()
            || position != proposal.position()
            || self.workspace.surface(surface).is_none()
        {
            return false;
        }
        match self.workspace.presentation_for_root(source.root()) {
            Some(crate::RootPresentationOwner::Main {
                surface: source_surface,
            }) => {
                self.workspace.surface(source_surface).is_some()
                    && self.workspace.contained_floating(floating).is_none()
            }
            Some(crate::RootPresentationOwner::Contained {
                surface: source_surface,
                floating: source_floating,
            }) => {
                source_floating == floating
                    && (source_surface != surface
                        || self
                            .workspace
                            .contained_floating(floating)
                            .is_some_and(|record| record.rect == rect))
            }
            None => false,
        }
    }

    fn core_contained_candidate(
        &self,
        drag: &crate::interaction::ActiveDrag,
        current_pointer: SurfacePointer,
    ) -> CoreContainedCandidate {
        let (root, floating, source_rect, initial_pointer, minimum_size, position) =
            match &drag.origin {
                FrozenDragOrigin::Contained(origin) => {
                    if current_pointer.surface() != origin.surface {
                        return CoreContainedCandidate::None;
                    }
                    let Some(record) = self.workspace.contained_floating(origin.floating) else {
                        return CoreContainedCandidate::Rejected;
                    };
                    if record.root != origin.root || record.rect != origin.source_rect {
                        return CoreContainedCandidate::Rejected;
                    }
                    (
                        origin.root,
                        origin.floating,
                        origin.source_rect,
                        origin.initial_pointer,
                        origin.minimum_size,
                        ContainedPosition::Front,
                    )
                }
                FrozenDragOrigin::Workspace => {
                    let Some(offer) = drag.contained_offer else {
                        return CoreContainedCandidate::None;
                    };
                    if current_pointer.surface() != offer.anchor().surface() {
                        return CoreContainedCandidate::None;
                    }
                    (
                        offer.root(),
                        offer.floating(),
                        offer.requested_rect(),
                        offer.anchor().position(),
                        offer.minimum_size(),
                        offer.position(),
                    )
                }
            };
        let Ok(requested_rect) =
            translated_contained_rect(source_rect, initial_pointer, current_pointer.position())
        else {
            return CoreContainedCandidate::Rejected;
        };
        let placement =
            match self.contained_placement(current_pointer.surface(), requested_rect, minimum_size)
            {
                Ok(placement) => placement,
                Err(
                    ContainedPlacementUnavailable::UnrepresentableGeometry { .. }
                    | ContainedPlacementUnavailable::ProofMismatch { .. },
                ) => {
                    return CoreContainedCandidate::Rejected;
                }
                Err(
                    ContainedPlacementUnavailable::SceneUnavailable
                    | ContainedPlacementUnavailable::StaleScene { .. }
                    | ContainedPlacementUnavailable::MissingSurface { .. }
                    | ContainedPlacementUnavailable::BootstrapSurface { .. },
                ) => {
                    return CoreContainedCandidate::Cancel(
                        InteractionCancelReason::SceneUnavailable,
                    );
                }
                Err(
                    ContainedPlacementUnavailable::StaleSurface { .. }
                    | ContainedPlacementUnavailable::PendingPaintSurface { .. },
                ) => {
                    return CoreContainedCandidate::None;
                }
            };
        CoreContainedCandidate::Proposal(crate::intent::ContainedTearOffProposal::new(
            root, floating, placement, position,
        ))
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
            .and_then(|drag| drag.owner.pointer_if_physical());
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

    fn cancel_click(
        &mut self,
        input: InputSequence,
        session: ClickSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_click(session) {
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

    fn check_resize_policy(
        &self,
        split: &NodeSource,
        policy: &DockPolicySnapshot,
    ) -> Result<(), CommandError> {
        self.validate_node_source(split)?;
        let axis = match self.workspace.node(split.node()) {
            Some(Node::Split { axis, .. }) => *axis,
            Some(Node::Tabs { .. }) => {
                return Err(CommandError::NodeIsNotSplit { node: split.node() });
            }
            None => {
                return Err(CommandError::MissingNode {
                    role: ReferenceRole::Source,
                    node: split.node(),
                });
            }
        };
        let surface = self.root_surface(split.root());
        match policy.evaluate(&DockPolicyRequest::Resize(DockResizePolicyRequest::new(
            axis, surface,
        ))) {
            PolicyDecision::Allow => Ok(()),
            PolicyDecision::Reject(reason) => Err(reason.into()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn adjust_splitter_resize(
        &mut self,
        input: InputSequence,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let surface = scene.surface();
        let painted = self
            .presentation_authority
            .scene
            .ready_surface(surface)
            .filter(|painted| {
                painted.stamp() == scene
                    && scene.requirement().workspace_epoch() == self.version.epoch()
            })
            .ok_or(InteractionRejection::StaleScene);
        let painted = match painted {
            Ok(painted) => painted,
            Err(error) => return Ok(InteractionOutcome::Rejected(error)),
        };
        if !Self::coordinate_capture_matches_current(
            painted.coordinate_capture(),
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable { surface },
            ));
        }
        let record = match painted.plan().splitter_record(splitter).cloned() {
            Some(record) if record.operable() => record,
            None => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::SplitterGestureHitUnavailable { surface },
                ));
            }
            Some(_) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::SplitterGestureHitUnavailable { surface },
                ));
            }
        };
        let source = match self
            .workspace
            .capture_node_source(splitter.root, splitter.split)
        {
            Ok(source) => source,
            Err(error) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::ResizeRejected(error),
                ));
            }
        };
        if let Err(error) = self.check_resize_policy(&source, policy) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::ResizeRejected(error),
            ));
        }
        let Some(allowed_delta) = resize_delta_interval(&record) else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterResizeGeometryUnavailable,
            ));
        };
        let clamped_delta = delta.clamp(allowed_delta.minimum, allowed_delta.maximum);
        let handle = FrozenResizeHandle {
            source,
            record,
            allowed_delta,
        };
        let Some(update) = split_resize_update(&handle, clamped_delta) else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SplitterResizeGeometryUnavailable,
            ));
        };
        let command = WorkspaceCommand::ResizeSplits {
            splits: vec![update],
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
                Ok(InteractionOutcome::SplitterAdjusted { outcome, changed })
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

    fn freeze_payload_focus(
        &self,
        payload: &MovePayload,
    ) -> Result<PaneFocusDisposition, crate::error::CommandError> {
        self.validate_payload(payload)?;
        let surface =
            self.payload_surface(payload)
                .ok_or(crate::error::CommandError::Invariant {
                    stage: "freezing pane focus for an unpresented payload",
                })?;
        let record = self.viewport_focus.panel_focus(surface);
        let PanelFocusRecord::Item(item) = record else {
            return Ok(PaneFocusDisposition::from_record(record));
        };
        let payload_contains_item = match payload {
            MovePayload::Item(source) => source.item() == item,
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => self
                .workspace
                .collect_items_in_subtree(source.node())
                .contains(&item),
        };
        if payload_contains_item {
            Ok(PaneFocusDisposition::Set(item))
        } else {
            Ok(PaneFocusDisposition::Clear)
        }
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
            MovePayload::Tabs(source) => {
                self.validate_node_source(source)?;
                match self.workspace.node(source.node()) {
                    Some(crate::graph::Node::Tabs { items, .. }) if !items.is_empty() => {}
                    Some(crate::graph::Node::Tabs { .. }) => {
                        return Err(crate::error::CommandError::EmptyPayload {
                            node: source.node(),
                        });
                    }
                    Some(crate::graph::Node::Split { .. }) => {
                        return Err(crate::error::CommandError::NodeIsNotTabs {
                            node: source.node(),
                        });
                    }
                    None => {
                        return Err(crate::error::CommandError::MissingNode {
                            role: ReferenceRole::Source,
                            node: source.node(),
                        });
                    }
                }
            }
            MovePayload::Subtree(source) => {
                self.validate_node_source(source)?;
                if self
                    .workspace
                    .collect_items_in_subtree(source.node())
                    .is_empty()
                {
                    return Err(crate::error::CommandError::EmptyPayload {
                        node: source.node(),
                    });
                }
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

    fn tear_off_command_from_complete_root(
        payload: &MovePayload,
        target: RootPresentationTarget,
        new_root: crate::ids::RootId,
        complete_root: Option<NodeSource>,
    ) -> WorkspaceCommand {
        match (complete_root, target) {
            (Some(source), target) => WorkspaceCommand::RehomeRoot { source, target },
            (None, RootPresentationTarget::NewSurface { surface }) => {
                WorkspaceCommand::CreateSurfaceRoot {
                    surface,
                    root: new_root,
                    content: RootContent::Move(payload.clone()),
                }
            }
            (None, RootPresentationTarget::Main { surface }) => WorkspaceCommand::InstallMainRoot {
                surface,
                root: new_root,
                content: RootContent::Move(payload.clone()),
            },
            (
                None,
                RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    position,
                },
            ) => WorkspaceCommand::CreateContainedRoot {
                surface,
                root: new_root,
                floating,
                rect,
                position,
                content: RootContent::Move(payload.clone()),
            },
        }
    }

    fn contained_presentation_command(
        &self,
        payload: &MovePayload,
        proposal: crate::intent::ContainedTearOffProposal,
        complete_root: Option<NodeSource>,
        frozen_origin: Option<&FrozenContainedDragOrigin>,
    ) -> Option<WorkspaceCommand> {
        if let Some(source) = complete_root.as_ref()
            && let Some(current) = self.workspace.contained_floating(proposal.floating())
            && current.root == source.root()
            && self.workspace.presentation_for_root(source.root())
                == Some(crate::RootPresentationOwner::Contained {
                    surface: proposal.surface(),
                    floating: proposal.floating(),
                })
        {
            let expected_roster = match frozen_origin {
                Some(origin)
                    if origin.surface == proposal.surface()
                        && origin.root == source.root()
                        && origin.floating == proposal.floating()
                        && origin.source_rect == current.rect =>
                {
                    origin.source_roster.clone()
                }
                Some(_) => return None,
                None => self
                    .workspace
                    .capture_contained_roster(proposal.surface())
                    .ok()?,
            };
            return Some(WorkspaceCommand::UpdateContainedPresentation {
                source: source.clone(),
                floating: proposal.floating(),
                expected_rect: current.rect,
                expected_roster,
                rect: proposal.rect(),
                position: proposal.position(),
            });
        }

        Some(Self::tear_off_command_from_complete_root(
            payload,
            RootPresentationTarget::Contained {
                surface: proposal.surface(),
                floating: proposal.floating(),
                rect: proposal.rect(),
                position: proposal.position(),
            },
            proposal.root(),
            complete_root,
        ))
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
            crate::RootPresentationOwner::Main { surface }
            | crate::RootPresentationOwner::Contained { surface, .. } => Some(surface),
        }
    }

    fn apply_interaction_command(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let policy = self.policy.clone();
        self.apply_interaction_command_with_policy(input, &policy, command, events)
    }

    fn apply_interaction_command_with_policy(
        &mut self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        self.apply_interaction_command_with_policy_and_barrier(input, policy, command, None, events)
    }

    #[cfg(test)]
    fn apply_interaction_command_with_barrier(
        &mut self,
        input: InputSequence,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let policy = self.policy.clone();
        self.apply_interaction_command_with_policy_and_barrier(
            input,
            &policy,
            command,
            action_barrier,
            events,
        )
    }

    fn apply_interaction_command_with_policy_and_barrier(
        &mut self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        let (candidate, application) =
            self.stage_workspace_command(input, policy, command, action_barrier)?;
        if let Some(candidate) = candidate {
            self.publish_workspace(candidate);
            self.reconcile_viewport_focus_authority();
        }
        self.record_interaction_command_application(input, application, events)
    }

    fn record_interaction_command_application(
        &mut self,
        input: InputSequence,
        application: CommandApplication,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<CommandApplication, EngineError> {
        if let CommandApplication::Applied { outcome, changed } = &application
            && *changed
        {
            self.advance_revision(input)?;
            events.push(WorkspaceEvent::new(
                input,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        }
        Ok(application)
    }

    fn recovery_policy_request(
        workspace: &Workspace,
        source_surface: crate::ids::SurfaceId,
        host_surface: crate::ids::SurfaceId,
    ) -> Result<DockSurfaceRecoveryPolicyRequest, CommandError> {
        let presentation =
            workspace
                .surface(source_surface)
                .ok_or(CommandError::MissingSurface {
                    surface: source_surface,
                })?;
        let mut roots = BTreeMap::new();
        if let Some(root) = presentation.main_root {
            Self::insert_recovery_root_facts(
                workspace,
                &mut roots,
                root,
                DockPresentationMode::Native,
            )?;
        }
        for floating in &presentation.contained {
            let contained =
                workspace
                    .contained_floating(*floating)
                    .ok_or(CommandError::MissingFloating {
                        floating: *floating,
                    })?;
            Self::insert_recovery_root_facts(
                workspace,
                &mut roots,
                contained.root,
                DockPresentationMode::Contained,
            )?;
        }
        Ok(DockSurfaceRecoveryPolicyRequest::new(
            source_surface,
            host_surface,
            roots,
        ))
    }

    fn insert_recovery_root_facts(
        workspace: &Workspace,
        roots: &mut BTreeMap<crate::ids::RootId, DockSurfaceRecoveryRootFacts>,
        root: crate::ids::RootId,
        presentation: DockPresentationMode,
    ) -> Result<(), CommandError> {
        let record = workspace
            .root(root)
            .ok_or(CommandError::MissingRoot { root })?;
        let facts = DockSurfaceRecoveryRootFacts::new(
            presentation,
            workspace.collect_items_in_subtree(record.node),
        );
        if roots.insert(root, facts).is_some() {
            return Err(CommandError::Invariant {
                stage: "normalize surface recovery roots",
            });
        }
        Ok(())
    }

    fn next_surface_recovery_obligation_id(
        &self,
        input: InputSequence,
    ) -> Result<SurfaceRecoveryObligationId, EngineError> {
        self.last_surface_recovery_obligation
            .checked_next()
            .ok_or(EngineError::SurfaceRecoveryObligationExhausted { input })
    }

    fn authorize_surface_recovery_obligation(
        &self,
        input: InputSequence,
        id: SurfaceRecoveryObligationId,
        workspace: &Workspace,
        source_surface: crate::ids::SurfaceId,
        target: SurfaceRecoveryTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceRecoveryObligation, CommandError> {
        if workspace.surface(target.host_surface()).is_none() {
            return Err(CommandError::SurfaceLifecycleFrozen {
                surface: target.host_surface(),
            });
        }
        let request =
            Self::recovery_policy_request(workspace, source_surface, target.host_surface())?;
        self.authorize_surface_recovery_obligation_for_request(input, id, request, target, policy)
    }

    fn authorize_surface_recovery_obligation_for_request(
        &self,
        _input: InputSequence,
        id: SurfaceRecoveryObligationId,
        request: DockSurfaceRecoveryPolicyRequest,
        target: SurfaceRecoveryTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceRecoveryObligation, CommandError> {
        if let PolicyDecision::Reject(reason) =
            policy.evaluate(&DockPolicyRequest::RecoverSurface(request.clone()))
        {
            return Err(CommandError::Policy(reason));
        }
        SurfaceRecoveryObligation::new(
            id,
            self.authority_domain,
            request,
            target,
            policy.revision(),
        )
        .map_err(|_| CommandError::SurfaceLifecycleFrozen {
            surface: target.host_surface(),
        })
    }

    fn stage_workspace_publication(
        &self,
        candidate: Workspace,
        policy: &DockPolicySnapshot,
    ) -> Result<StagedWorkspacePublication, CommandError> {
        self.stage_workspace_publication_with_authority(
            candidate,
            policy,
            WorkspacePublicationAuthority::Ordinary,
        )
    }

    fn stage_workspace_publication_with_authority(
        &self,
        candidate: Workspace,
        policy: &DockPolicySnapshot,
        authority: WorkspacePublicationAuthority,
    ) -> Result<StagedWorkspacePublication, CommandError> {
        let anchors = self.root_recovery_anchors.clone();
        let mut recoveries = self.bound_surface_recoveries.clone();
        let recovery_commit = match authority {
            WorkspacePublicationAuthority::RecoveryCommit {
                source_surface,
                obligation,
            } => Some((source_surface, obligation)),
            WorkspacePublicationAuthority::Ordinary
            | WorkspacePublicationAuthority::NativeCommit { .. } => None,
        };
        if let Some((source_surface, obligation)) = recovery_commit
            && !recoveries
                .get(&source_surface)
                .is_some_and(|current| current.obligation.id() == obligation)
        {
            return Err(CommandError::SurfaceLifecycleFrozen {
                surface: source_surface,
            });
        }
        let live_surfaces = recoveries.keys().copied().collect::<Vec<_>>();
        for surface in live_surfaces {
            let bound = recoveries
                .get(&surface)
                .cloned()
                .ok_or(CommandError::Invariant {
                    stage: "load recovery obligation",
                })?;
            if candidate.surface(surface).is_none() {
                continue;
            }
            let obligation = &bound.obligation;
            let host = obligation.request().host_surface();
            if candidate.surface(host).is_none()
                || anchors.get(&host) != Some(&obligation.target().anchor())
            {
                return Err(CommandError::SurfaceLifecycleFrozen { surface: host });
            }
            let request = Self::recovery_policy_request(&candidate, surface, host)?;
            if request != *obligation.request() {
                if let PolicyDecision::Reject(reason) =
                    policy.evaluate(&DockPolicyRequest::RecoverSurface(request.clone()))
                {
                    return Err(CommandError::Policy(reason));
                }
                let next = obligation
                    .reauthorized_for(request, policy.revision())
                    .map_err(|_| CommandError::SurfaceLifecycleFrozen { surface })?;
                recoveries.insert(surface, bound.reauthorize(next));
            }
        }

        for (surface, bound) in &recoveries {
            let obligation = &bound.obligation;
            if let Some(converted) = obligation.target().converted_main()
                && candidate.contained_floating(converted.floating()).is_some()
            {
                let exact_recovery_consumption = matches!(
                    authority,
                    WorkspacePublicationAuthority::RecoveryCommit {
                        source_surface,
                        obligation: obligation_id,
                    } if source_surface == *surface && obligation_id == obligation.id()
                ) && candidate
                    .presentation_for_root(converted.source_root())
                    == Some(crate::RootPresentationOwner::Contained {
                        surface: obligation.target().host_surface(),
                        floating: converted.floating(),
                    });
                if !exact_recovery_consumption {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface: *surface });
                }
            }
        }

        if let Some((source_surface, obligation)) = recovery_commit {
            if candidate.surface(source_surface).is_some() {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                });
            }
            let removed =
                recoveries
                    .remove(&source_surface)
                    .ok_or(CommandError::SurfaceLifecycleFrozen {
                        surface: source_surface,
                    })?;
            if removed.obligation.id() != obligation {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                });
            }
        }
        for (saga_id, saga) in self.viewport.native_create_sagas() {
            let prepared = saga.prepared();
            let obligation = prepared.recovery_obligation();
            let surface = obligation.request().source_surface();
            if let Some(converted) = obligation.target().converted_main()
                && candidate.contained_floating(converted.floating()).is_some()
            {
                return Err(CommandError::SurfaceLifecycleFrozen { surface });
            }
            if candidate.surface(surface).is_some() {
                if !matches!(
                    authority,
                    WorkspacePublicationAuthority::NativeCommit { saga } if saga == saga_id
                ) || Self::recovery_policy_request(
                    &candidate,
                    surface,
                    obligation.request().host_surface(),
                )? != *obligation.request()
                {
                    return Err(CommandError::SurfaceLifecycleFrozen { surface });
                }
            }
        }

        let referenced_anchors = recoveries
            .iter()
            .filter(|(surface, _)| candidate.surface(**surface).is_some())
            .map(|(_, bound)| bound.obligation.target().anchor())
            .chain(
                self.viewport
                    .native_create_sagas()
                    .map(|(_, saga)| saga.prepared().recovery_obligation().target().anchor()),
            )
            .collect::<BTreeSet<_>>();
        for anchor in &referenced_anchors {
            if anchors.get(&anchor.surface()) != Some(anchor)
                || candidate.surface(anchor.surface()).is_none()
            {
                return Err(CommandError::SurfaceLifecycleFrozen {
                    surface: anchor.surface(),
                });
            }
        }
        Ok(StagedWorkspacePublication {
            workspace: candidate,
            root_recovery_anchors: anchors,
            bound_surface_recoveries: recoveries,
        })
    }

    fn stage_workspace_command(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        self.stage_workspace_command_with_authority(
            input,
            policy,
            command,
            action_barrier,
            WorkspacePublicationAuthority::Ordinary,
        )
    }

    fn stage_workspace_command_for_native_commit(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        saga: NativeCreateSagaId,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        self.stage_workspace_command_with_authority(
            input,
            policy,
            command,
            action_barrier,
            WorkspacePublicationAuthority::NativeCommit { saga },
        )
    }

    fn stage_workspace_command_with_authority(
        &self,
        input: InputSequence,
        policy: &DockPolicySnapshot,
        command: &WorkspaceCommand,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        authority: WorkspacePublicationAuthority,
    ) -> Result<(Option<StagedWorkspacePublication>, CommandApplication), EngineError> {
        let mut candidate = self.clone_workspace_candidate();
        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut candidate, policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok((None, CommandApplication::Rejected(source)));
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&candidate, action_barrier, None)
        {
            return Ok((
                None,
                CommandApplication::Rejected(crate::error::CommandError::SurfaceLifecycleFrozen {
                    surface,
                }),
            ));
        }
        let application = Self::command_application_from_report(input, report)?;
        let publication =
            match self.stage_workspace_publication_with_authority(candidate, policy, authority) {
                Ok(publication) => publication,
                Err(source) if source.is_expected_rejection() => {
                    return Ok((None, CommandApplication::Rejected(source)));
                }
                Err(source) => {
                    return Err(EngineError::Command {
                        input,
                        source: TransactionError::Command { index: 0, source },
                    });
                }
            };
        Ok((Some(publication), application))
    }

    fn first_workspace_publication_mismatch(
        &self,
        candidate: &Workspace,
        action_barrier: Option<&BTreeMap<crate::ids::SurfaceId, SurfaceRosterDisposition>>,
        excluded: Option<crate::ids::SurfaceId>,
    ) -> Option<crate::ids::SurfaceId> {
        let persistent = match excluded {
            Some(surface) => self
                .surface_recovery
                .first_workspace_mismatch_excluding(candidate, surface),
            None => self.surface_recovery.first_workspace_mismatch(candidate),
        };
        let transient = action_barrier.and_then(|barrier| {
            barrier.values().find_map(|roster| {
                (Some(roster.surface()) != excluded && !roster.matches_workspace(candidate))
                    .then_some(roster.surface())
            })
        });
        persistent.or(transient)
    }

    fn publish_workspace(&mut self, publication: StagedWorkspacePublication) {
        self.presentation_identity
            .observe_workspace(&publication.workspace);
        self.workspace = publication.workspace;
        self.root_recovery_anchors = publication.root_recovery_anchors;
        self.bound_surface_recoveries = publication.bound_surface_recoveries;
    }

    fn settle_semantic_surface_vacancies(
        &mut self,
        _input: InputSequence,
        authorities: &[SurfaceVacancyAuthority],
    ) -> Result<(), EngineError> {
        for authority in authorities {
            let surface = authority.surface();
            if self
                .bound_surface_recoveries
                .get(&surface)
                .is_some_and(|bound| Some(bound.binding) == authority.binding())
            {
                if let Some(bound) = self.bound_surface_recoveries.remove(&surface)
                    && let Some(resource) = bound.retained_staging_resource
                {
                    self.viewport
                        .settle_vacated_native_staging_resource(
                            resource,
                            bound.binding,
                            bound.obligation.id(),
                        )
                        .map_err(|source| EngineError::Viewport {
                            input: self.last_input,
                            source,
                        })?;
                }
            }
        }
        let referenced_anchors = self
            .bound_surface_recoveries
            .values()
            .map(|bound| bound.obligation.target().anchor())
            .chain(
                self.viewport
                    .native_create_sagas()
                    .map(|(_, saga)| saga.prepared().recovery_obligation().target().anchor()),
            )
            .collect::<BTreeSet<_>>();
        for authority in authorities {
            let surface = authority.surface();
            let Some(anchor) = self.root_recovery_anchors.get(&surface).copied() else {
                continue;
            };
            if referenced_anchors.contains(&anchor) {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "tick-final vacancy retained a referenced root recovery anchor",
                });
            }
            self.root_recovery_anchors.remove(&surface);
        }
        Ok(())
    }

    fn command_application_from_report(
        input: InputSequence,
        report: crate::transaction::TransactionReport,
    ) -> Result<CommandApplication, EngineError> {
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
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let (candidate, application) =
            self.stage_workspace_command(input, policy, command, None)?;
        let CommandApplication::Applied { outcome, changed } = application else {
            let CommandApplication::Rejected(error) = application else {
                unreachable!("command application variants are exhaustive")
            };
            return Ok(InputOutcome::CommandRejected {
                error,
                version: self.version,
            });
        };
        if let Err(error) = self.validate_application_command_identity_freshness(command) {
            return Ok(InputOutcome::CommandRejected {
                error,
                version: self.version,
            });
        }
        self.adopt_application_command_identities(command);
        self.publish_workspace(candidate.ok_or(EngineError::MissingCommandOutcome { input })?);
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::WorkspaceChanged,
                interaction_events,
            )?;
        }
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

    #[inline]
    fn clone_workspace_candidate(&self) -> Workspace {
        #[cfg(test)]
        crate::drop_resolver::structural_work::record_engine_workspace_candidate_clone(
            &self.workspace,
        );
        self.workspace.clone()
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
            pending_drag_release: self.pending_drag_release.clone(),
            pending_contained_transform_release: self.pending_contained_transform_release.clone(),
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
