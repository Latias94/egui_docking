//! Action-oriented classification for private runtime failure sources.

use crate::backend_ingress::BackendIngressError;
use crate::engine::{
    CoreHostFrameError, EngineError, PointerReceiverGeometryError, SurfaceContributionPrepareError,
};
use crate::layout::LayoutError;
use crate::platform_provider::PlatformObservationAuthorityError;
use crate::pointer_journal::{PointerJournalLedgerError, SurfaceLocalPointerProviderError};
use crate::pointer_receiver::PointerReceiverAttemptError;
use crate::scene_compiler::{PresentationCompilationError, SceneCompilationError};

use super::{
    DockspaceInteractionError, DockspaceRuntimeErrorKind, NativeHostErrorKind,
    PresentationObservationError,
};

pub(super) const fn host_frame_error_kind(error: &CoreHostFrameError) -> DockspaceRuntimeErrorKind {
    use CoreHostFrameError as Error;

    match error {
        Error::SessionOwnedPublicationRequired
        | Error::CausalOrdinalExhausted
        | Error::PresentationAttemptSpaceExhausted
        | Error::PresentationOutputRequestExhausted
        | Error::PointerReceiverRoster { .. }
        | Error::InputPrefixReductionFailed
        | Error::PresentationObligationsAlreadyIssued
        | Error::PresentationObligationsNotIssued
        | Error::PresentationObligationsPartiallyResolved
        | Error::PresentationObligationAttemptMismatch { .. }
        | Error::PresentationObligationOutsideRoster { .. }
        | Error::PresentationObligationAlreadyResolved { .. }
        | Error::PresentationObligationSurfaceMismatch { .. }
        | Error::PresentationInteractionMismatch { .. }
        | Error::PresentationOutputUnavailable { .. }
        | Error::NativeStagingPresentationUnavailable { .. }
        | Error::DuplicatePresentationOutput { .. }
        | Error::RetainedContributionNotPaintable { .. }
        | Error::RetainedContributionStampMismatch { .. }
        | Error::RetainedContributionCoordinateMismatch { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
        Error::BackendIngressRejected { source } => backend_ingress_error_kind(source),
        Error::PointerJournalRejected { source } => pointer_journal_error_kind(source),
        Error::SurfaceLocalPointerProducerRejected { source } => {
            surface_pointer_provider_error_kind(*source)
        }
        Error::PointerReceiverAttempt { source } => pointer_receiver_attempt_error_kind(*source),
        Error::BackendIngressRequired
        | Error::BackendIngressUnexpected
        | Error::DuplicateBackendIngressBatch
        | Error::BackendIngressReceiptsUnexpected
        | Error::BackendIngressIncomplete
        | Error::ReservedInputSource { .. }
        | Error::ItemIdentityOutsideScope { .. }
        | Error::ItemIdentityScopeMismatch
        | Error::InputPhaseMismatch { .. }
        | Error::SemanticAfterConfiguration
        | Error::DuplicatePresentationObservation
        | Error::PresentationObservationHostOutsideScope { .. }
        | Error::DuplicatePresentationObservationForHost { .. }
        | Error::DuplicatePresentationObservationStream { .. }
        | Error::MissingPresentationObservationStream { .. }
        | Error::PresentationObservationStreamOutsideScope { .. }
        | Error::DuplicateSurfaceContribution { .. }
        | Error::SurfaceOutsideRoster { .. }
        | Error::InputAfterPresentation
        | Error::PointerJournalUnexpected
        | Error::SurfaceLocalPointerProducerRequired { .. }
        | Error::PointerJournalMissingBeforePresentation { .. }
        | Error::PointerJournalProviderMismatch { .. }
        | Error::PointerJournalMustBeEdgewise
        | Error::PointerReceiverReceiptBeforeJournal
        | Error::PointerReceiverReceiptsMissingBeforeNextSegment
        | Error::PointerReceiverReceiptsMissingBeforeInput
        | Error::PointerReceiverReceiptsMissingBeforePresentation
        | Error::DuplicatePointerReceiverReceipts
        | Error::PointerReceiverReceiptsUnexpected => DockspaceRuntimeErrorKind::HostProtocol,
    }
}

const fn backend_ingress_error_kind(error: &BackendIngressError) -> DockspaceRuntimeErrorKind {
    match error {
        BackendIngressError::ReplacementIdentityExhausted
        | BackendIngressError::ReplacementTicketGenerationExhausted
        | BackendIngressError::OrdinalExhausted
        | BackendIngressError::RecordIdentityExhausted => DockspaceRuntimeErrorKind::Internal,
        BackendIngressError::ProviderUnavailable
        | BackendIngressError::PointerProviderNotDesktopGlobal { .. } => {
            DockspaceRuntimeErrorKind::Unsupported
        }
        BackendIngressError::DrainReceiptConsumed
        | BackendIngressError::PrefixRetirementReceiptConsumed
        | BackendIngressError::ProviderReplacementTicketConsumed
        | BackendIngressError::ProviderReplacementPending
        | BackendIngressError::ProviderReplacementNotPending
        | BackendIngressError::ProviderAlreadyActive { .. }
        | BackendIngressError::RecordPublicationInProgress { .. }
        | BackendIngressError::RecordAlreadyCommitted { .. }
        | BackendIngressError::RollbackCrossesPublishedRecord { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        BackendIngressError::UnknownReplacementTicket
        | BackendIngressError::BindingQuiescenceAuthorityMismatch { .. }
        | BackendIngressError::DuplicateBindingQuiescence { .. }
        | BackendIngressError::ProviderLeaseMismatch { .. }
        | BackendIngressError::SavepointLeaseMismatch { .. }
        | BackendIngressError::SavepointPrefixReclaimed { .. }
        | BackendIngressError::SavepointAhead { .. }
        | BackendIngressError::SavepointPrefixRewritten { .. }
        | BackendIngressError::PresentationHostLeaseMismatch { .. }
        | BackendIngressError::ForeignAuthorityDomain { .. }
        | BackendIngressError::AuthorityDomainMismatch { .. }
        | BackendIngressError::PresentationAuthorityDomainMismatch { .. }
        | BackendIngressError::RecordRevoked { .. }
        | BackendIngressError::PointerSegmentMustBeEdgewise { .. }
        | BackendIngressError::PointerSegmentPreviousMismatch { .. }
        | BackendIngressError::SemanticInputIsBackendFact
        | BackendIngressError::SemanticInputIsConfiguration
        | BackendIngressError::CommittedWatermarkAhead { .. }
        | BackendIngressError::CommittedWatermarkRetired { .. }
        | BackendIngressError::CommitWatermarkLeaseMismatch { .. }
        | BackendIngressError::CommitWatermarkContentMismatch { .. }
        | BackendIngressError::RetirementWatermarkRegressed { .. }
        | BackendIngressError::PrefixRetirementLeaseMismatch { .. }
        | BackendIngressError::PrefixRetirementWatermarkMismatch { .. }
        | BackendIngressError::BatchLeaseMismatch { .. }
        | BackendIngressError::BatchPreviousMismatch { .. }
        | BackendIngressError::BatchPreviousIdentityMismatch { .. }
        | BackendIngressError::RecordLeaseMismatch { .. }
        | BackendIngressError::RecordOrdinalGap { .. }
        | BackendIngressError::BatchThroughMismatch { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
    }
}

const fn pointer_journal_error_kind(
    error: &PointerJournalLedgerError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PointerJournalLedgerError::ProviderIncarnationExhausted
        | PointerJournalLedgerError::StreamIncarnationExhausted
        | PointerJournalLedgerError::LedgerVersionExhausted => DockspaceRuntimeErrorKind::Internal,
        PointerJournalLedgerError::ProviderAlreadyActive { .. }
        | PointerJournalLedgerError::SurfaceLocalQuiescenceReceiptConsumed { .. }
        | PointerJournalLedgerError::SurfaceLocalProducerNotAbandoned { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        PointerJournalLedgerError::ForeignSurfaceLocalHost { .. }
        | PointerJournalLedgerError::ForeignSurfaceLocalBinding { .. }
        | PointerJournalLedgerError::ForeignLease { .. }
        | PointerJournalLedgerError::UnknownLease { .. }
        | PointerJournalLedgerError::RetiredLease { .. }
        | PointerJournalLedgerError::CompactedLease { .. }
        | PointerJournalLedgerError::SurfaceLocalQuiescenceScopeMismatch { .. }
        | PointerJournalLedgerError::SurfaceLocalQuiescenceWatermarkMismatch { .. }
        | PointerJournalLedgerError::JournalReplay { .. }
        | PointerJournalLedgerError::JournalAhead { .. }
        | PointerJournalLedgerError::ConflictingAuthorityCheckpoint { .. }
        | PointerJournalLedgerError::AuthorityCheckpointAfterPointerEdges { .. }
        | PointerJournalLedgerError::ButtonAlreadyPressed { .. }
        | PointerJournalLedgerError::ButtonNotPressed { .. }
        | PointerJournalLedgerError::StreamEndLeavesButtonsPressed { .. }
        | PointerJournalLedgerError::LocationScopeMismatch { .. }
        | PointerJournalLedgerError::CaptureScopeMismatch { .. }
        | PointerJournalLedgerError::DeliveryScopeMismatch { .. }
        | PointerJournalLedgerError::ForeignDeliveryBinding { .. }
        | PointerJournalLedgerError::ForeignCaptureBinding { .. }
        | PointerJournalLedgerError::ForeignDesktopRouteBinding { .. }
        | PointerJournalLedgerError::ForeignScrollDeliveryHost { .. }
        | PointerJournalLedgerError::ScrollDeliveryScopeMismatch { .. }
        | PointerJournalLedgerError::DesktopScrollDeliveryBindingMissing { .. }
        | PointerJournalLedgerError::ScrollDeliveryOwnerMismatch { .. }
        | PointerJournalLedgerError::InvalidJournal(_)
        | PointerJournalLedgerError::ForeignPreparedJournal
        | PointerJournalLedgerError::PreparedJournalStale { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
    }
}

const fn surface_pointer_provider_error_kind(
    error: SurfaceLocalPointerProviderError,
) -> DockspaceRuntimeErrorKind {
    match error {
        SurfaceLocalPointerProviderError::FrameAttemptExhausted { .. }
        | SurfaceLocalPointerProviderError::FrameSubmissionLost { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
        SurfaceLocalPointerProviderError::FrameSubmissionInFlight { .. }
        | SurfaceLocalPointerProviderError::DrainReceiptConsumed { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        SurfaceLocalPointerProviderError::CommittedWatermarkMismatch { .. }
        | SurfaceLocalPointerProviderError::FrameProviderMismatch { .. }
        | SurfaceLocalPointerProviderError::FrameWatermarkMismatch { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
    }
}

const fn pointer_receiver_attempt_error_kind(
    error: PointerReceiverAttemptError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PointerReceiverAttemptError::ForeignPointerLease { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
        PointerReceiverAttemptError::AttemptSpaceExhausted => DockspaceRuntimeErrorKind::Internal,
    }
}

pub(super) const fn surface_contribution_prepare_error_kind(
    error: &SurfaceContributionPrepareError,
) -> DockspaceRuntimeErrorKind {
    match error {
        SurfaceContributionPrepareError::RetainedCandidateUnavailable { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
        SurfaceContributionPrepareError::Compilation(error) => {
            presentation_compilation_error_kind(error)
        }
        SurfaceContributionPrepareError::Validation(_) => DockspaceRuntimeErrorKind::Internal,
        SurfaceContributionPrepareError::SurfaceOutsideRoster { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
        SurfaceContributionPrepareError::StaleBase { .. }
        | SurfaceContributionPrepareError::TicketMismatch { .. }
        | SurfaceContributionPrepareError::PolicyAuthorityChanged { .. }
        | SurfaceContributionPrepareError::CoordinateAuthorityChanged { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
    }
}

const fn presentation_compilation_error_kind(
    error: &PresentationCompilationError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PresentationCompilationError::WorkspaceVersionMismatch { .. }
        | PresentationCompilationError::Manifest(_) => DockspaceRuntimeErrorKind::Internal,
        PresentationCompilationError::Authority(_)
        | PresentationCompilationError::RequirementsVanished { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
        PresentationCompilationError::Scene(error) => scene_compilation_error_kind(error),
    }
}

const fn scene_compilation_error_kind(error: &SceneCompilationError) -> DockspaceRuntimeErrorKind {
    match error {
        SceneCompilationError::UnrepresentableGuideGeometry { .. }
        | SceneCompilationError::GeometryCountOverflow
        | SceneCompilationError::NonFiniteAggregate { .. }
        | SceneCompilationError::InvalidTabWidthAllocation { .. }
        | SceneCompilationError::InvalidDockFraction
        | SceneCompilationError::LayoutMetrics(_)
        | SceneCompilationError::Geometry(_) => DockspaceRuntimeErrorKind::InvalidConfiguration,
        SceneCompilationError::Layout(error) => layout_error_kind(error),
        SceneCompilationError::TicketMismatch { .. }
        | SceneCompilationError::PolicyRevisionMismatch { .. }
        | SceneCompilationError::EmptySurfaceBounds { .. }
        | SceneCompilationError::EmptyPopupPlaneBounds { .. }
        | SceneCompilationError::MissingSurface { .. }
        | SceneCompilationError::MissingContainedPresentation { .. }
        | SceneCompilationError::ContainedLayerExhausted { .. }
        | SceneCompilationError::MissingRoot { .. }
        | SceneCompilationError::MissingNode { .. }
        | SceneCompilationError::RepeatedNode { .. }
        | SceneCompilationError::MissingLeafMinimum { .. }
        | SceneCompilationError::MissingTabBarRequirement { .. }
        | SceneCompilationError::MissingTabCloseCapability { .. }
        | SceneCompilationError::MissingPaneMinimumMeasurement { .. }
        | SceneCompilationError::MissingTabIntrinsicMeasurement { .. }
        | SceneCompilationError::MissingTabStripMeasurement { .. }
        | SceneCompilationError::TabListMenuRosterMismatch { .. }
        | SceneCompilationError::TabListMenuAnchorOutsidePopupPlane { .. }
        | SceneCompilationError::TabListMenuPopupSpaceUnavailable { .. }
        | SceneCompilationError::ActiveTabListMenuProjectionUnavailable { .. }
        | SceneCompilationError::PopupPlaneStateMismatch
        | SceneCompilationError::MissingSplitProjection { .. }
        | SceneCompilationError::MissingNodeProjection { .. }
        | SceneCompilationError::MissingSplitMinimum { .. }
        | SceneCompilationError::MissingSplitMaximum { .. }
        | SceneCompilationError::Command(_)
        | SceneCompilationError::Scene(_) => DockspaceRuntimeErrorKind::Internal,
    }
}

const fn layout_error_kind(error: &LayoutError) -> DockspaceRuntimeErrorKind {
    match error {
        LayoutError::NonFiniteExtent { .. }
        | LayoutError::NegativeExtent { .. }
        | LayoutError::NonFiniteWeight { .. }
        | LayoutError::NegativeWeight { .. }
        | LayoutError::ZeroWeightTotal
        | LayoutError::NonFiniteAggregate { .. }
        | LayoutError::NonFiniteSplitterExtent { .. }
        | LayoutError::Geometry(_) => DockspaceRuntimeErrorKind::InvalidConfiguration,
        LayoutError::LengthMismatch { .. }
        | LayoutError::CentralIndexOutOfBounds { .. }
        | LayoutError::MissingRoot { .. }
        | LayoutError::MissingNode { .. }
        | LayoutError::MissingLeafConstraints { .. }
        | LayoutError::Cycle { .. }
        | LayoutError::SplitCardinalityMismatch { .. }
        | LayoutError::CentralNodeNotReachable { .. }
        | LayoutError::SplitWeightOverrideRootMismatch { .. }
        | LayoutError::InvalidSplitWeightOverrideSource { .. }
        | LayoutError::DuplicateSplitWeightOverride { .. }
        | LayoutError::SplitWeightOverrideNodeNotSplit { .. }
        | LayoutError::SplitWeightOverrideLengthMismatch { .. }
        | LayoutError::SplitWeightOverrideNotNormalized { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
    }
}

pub(super) const fn presentation_observation_error_kind(
    error: PresentationObservationError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PresentationObservationError::PendingRosterMismatch
        | PresentationObservationError::CaptureGenerationExhausted
        | PresentationObservationError::BackendReportConflict => {
            DockspaceRuntimeErrorKind::Internal
        }
    }
}

pub(super) const fn engine_error_kind(error: &EngineError) -> DockspaceRuntimeErrorKind {
    match error {
        EngineError::InvalidWorkspace(_) => DockspaceRuntimeErrorKind::InvalidConfiguration,
        EngineError::WorkspaceReplacementIdentityRetired { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        EngineError::PointerJournal { source } => pointer_journal_error_kind(source),
        EngineError::SurfaceLocalPointerCommit { source } => {
            surface_pointer_provider_error_kind(*source)
        }
        EngineError::BackendIngress { source } => backend_ingress_error_kind(source),
        EngineError::PlatformProvider { source } => platform_provider_error_kind(*source),
        EngineError::PointerReceiverAttempt { source } => {
            pointer_receiver_attempt_error_kind(*source)
        }
        EngineError::HostFramePoisoned { source } => host_frame_error_kind(source),
        EngineError::PointerReceiverGeometry { source } => {
            pointer_receiver_geometry_error_kind(*source)
        }
        EngineError::SurfaceLocalPointerProviderAbandoned { .. }
        | EngineError::PointerProviderSurfaceHostMismatch { .. }
        | EngineError::PointerProviderSurfaceAuthorityUnavailable { .. }
        | EngineError::PointerProviderSurfaceEndpointMismatch { .. }
        | EngineError::PointerProviderSurfacePresentationMismatch { .. }
        | EngineError::PresentationHostRetired { .. }
        | EngineError::PresentationHostRetiredCompacted { .. }
        | EngineError::GlobalFocusBindingUnavailable { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        EngineError::SurfaceLocalPointerProducerRequired
        | EngineError::PointerReceiverReceipt { .. }
        | EngineError::PointerProviderScope { .. }
        | EngineError::PointerProviderHostOutsideFrameScope { .. }
        | EngineError::HostFramePointerProviderStale { .. }
        | EngineError::HostFramePointerJournalMissing { .. }
        | EngineError::HostFramePointerReceiverReceiptsMissing { .. }
        | EngineError::HostFrameBackendIngressIncomplete
        | EngineError::HostFrameBackendIngressUnexpected
        | EngineError::SourceSequenceNotIncreasing { .. }
        | EngineError::HostFrameAuthorityDomainMismatch { .. }
        | EngineError::HostFrameStale { .. }
        | EngineError::HostFramePredecessorStale { .. }
        | EngineError::HostFramePresentationHostFrontierStale { .. }
        | EngineError::HostFramePlatformProviderFrontierStale { .. }
        | EngineError::HostFrameRuntimeRetentionStale { .. }
        | EngineError::HostFrameRosterStale { .. }
        | EngineError::HostFramePresentationObservationMissing
        | EngineError::HostFrameSupplementaryPresentationObservationMissing { .. }
        | EngineError::HostFrameObserverIsRenderingHost { .. }
        | EngineError::HostFrameObserverDuplicate { .. }
        | EngineError::HostFramePresentationScopeStale { .. }
        | EngineError::HostFrameSupplementaryPresentationScopeStale { .. }
        | EngineError::HostFrameContributionRosterIncomplete { .. }
        | EngineError::HostFramePresentationObligationRosterIncomplete { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
        EngineError::EngineAuthorityDomainExhausted
        | EngineError::InputSequenceExhausted
        | EngineError::ReducerTickExhausted
        | EngineError::PresentationOutputSerialExhausted
        | EngineError::PresentationSurfaceIdentityExhausted
        | EngineError::PresentationRootIdentityExhausted
        | EngineError::PresentationFloatingIdentityExhausted
        | EngineError::PresentationLedger { .. }
        | EngineError::SurfaceLocalPointerRetirementExternalObligation { .. }
        | EngineError::BackendIngressProviderMismatch { .. }
        | EngineError::JournalPresentationSnapshot { .. }
        | EngineError::PointerInteractionInvariant { .. }
        | EngineError::SurfaceRecoveryObligationExhausted { .. }
        | EngineError::RootRecoveryAnchorExhausted { .. }
        | EngineError::RuntimeRetentionRevisionExhausted
        | EngineError::HostPresentationRosterCollision { .. }
        | EngineError::HostPresentationStagingResourceMissing { .. }
        | EngineError::ReductionCauseInvariant { .. }
        | EngineError::WorkspaceEpochExhausted { .. }
        | EngineError::WorkspaceRevisionExhausted { .. }
        | EngineError::PresentationRequirementRevisionExhausted { .. }
        | EngineError::PresentationConfigRevisionExhausted { .. }
        | EngineError::PolicyRevisionExhausted { .. }
        | EngineError::SurfaceRequirementRevisionExhausted { .. }
        | EngineError::PresentationRequirementInvariant { .. }
        | EngineError::ClosePlanInvariant { .. }
        | EngineError::SurfaceContributionInvariant { .. }
        | EngineError::SurfaceContributionBatchInvariant { .. }
        | EngineError::SurfacePresentationObservationBatchInvariant { .. }
        | EngineError::PresentationHostRetirementInvariant { .. }
        | EngineError::Command { .. }
        | EngineError::MissingCommandOutcome { .. }
        | EngineError::SurfaceSceneRevisionExhausted { .. }
        | EngineError::Interaction { .. }
        | EngineError::DropResolution { .. }
        | EngineError::Viewport { .. }
        | EngineError::ViewportFocus { .. }
        | EngineError::CausedViewportFocus { .. }
        | EngineError::SurfaceRoster { .. }
        | EngineError::SurfaceRecovery { .. }
        | EngineError::MissingSurfaceRoster { .. }
        | EngineError::ConflictingSurfaceRecovery { .. } => DockspaceRuntimeErrorKind::Internal,
    }
}

const fn platform_provider_error_kind(
    error: PlatformObservationAuthorityError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PlatformObservationAuthorityError::ProviderAlreadyActive { .. }
        | PlatformObservationAuthorityError::ProviderReplacementPending { .. }
        | PlatformObservationAuthorityError::ProviderAuthorityRetired
        | PlatformObservationAuthorityError::SupersededLease { .. }
        | PlatformObservationAuthorityError::RetiredLease { .. } => {
            DockspaceRuntimeErrorKind::OperationConflict
        }
        PlatformObservationAuthorityError::ForeignLease { .. }
        | PlatformObservationAuthorityError::UnknownLease { .. }
        | PlatformObservationAuthorityError::ForeignReplacementTicket { .. }
        | PlatformObservationAuthorityError::UnknownReplacementTicket => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
        PlatformObservationAuthorityError::ProviderIncarnationExhausted
        | PlatformObservationAuthorityError::ProviderAuthorityFrontierExhausted => {
            DockspaceRuntimeErrorKind::Internal
        }
    }
}

const fn pointer_receiver_geometry_error_kind(
    error: PointerReceiverGeometryError,
) -> DockspaceRuntimeErrorKind {
    match error {
        PointerReceiverGeometryError::InteractionManifestUnavailable { .. }
        | PointerReceiverGeometryError::ReceiverWinnerAmbiguous { .. } => {
            DockspaceRuntimeErrorKind::Internal
        }
        PointerReceiverGeometryError::LogicalPointUnavailable { .. }
        | PointerReceiverGeometryError::AbsenceSurfaceMismatch { .. }
        | PointerReceiverGeometryError::DesktopRoutePresentationMismatch { .. }
        | PointerReceiverGeometryError::SurfaceMismatch { .. }
        | PointerReceiverGeometryError::RegionDoesNotCoverPoint { .. }
        | PointerReceiverGeometryError::ReceiverWinnerMismatch { .. } => {
            DockspaceRuntimeErrorKind::HostProtocol
        }
    }
}

pub(super) const fn interaction_error_kind(
    error: DockspaceInteractionError,
) -> DockspaceRuntimeErrorKind {
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

pub(super) const fn native_error_kind(error: NativeHostErrorKind) -> DockspaceRuntimeErrorKind {
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

#[cfg(test)]
mod tests {
    use super::super::{DockspaceRuntimeError, DockspaceRuntimeErrorKind};
    use crate::backend_ingress::BackendIngressError;
    use crate::engine::{CoreHostFrameError, SurfaceContributionPrepareError};
    use crate::ids::SurfaceId;
    use crate::runtime::PresentationObservationError;
    use crate::scene::SceneBuildError;
    use crate::scene_compiler::{PresentationCompilationError, SceneCompilationError};

    #[test]
    fn host_frame_error_kinds_are_actionable() {
        for (error, expected) in [
            (
                CoreHostFrameError::CausalOrdinalExhausted,
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                CoreHostFrameError::InputPrefixReductionFailed,
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                CoreHostFrameError::BackendIngressRejected {
                    source: BackendIngressError::ProviderUnavailable,
                },
                DockspaceRuntimeErrorKind::Unsupported,
            ),
            (
                CoreHostFrameError::BackendIngressRejected {
                    source: BackendIngressError::ProviderReplacementPending,
                },
                DockspaceRuntimeErrorKind::OperationConflict,
            ),
            (
                CoreHostFrameError::BackendIngressRequired,
                DockspaceRuntimeErrorKind::HostProtocol,
            ),
            (
                CoreHostFrameError::PresentationObligationsNotIssued,
                DockspaceRuntimeErrorKind::Internal,
            ),
        ] {
            assert_eq!(DockspaceRuntimeError::from(error).kind(), expected);
        }
    }

    #[test]
    fn engine_error_kinds_delegate_and_remain_exhaustive() {
        let surface = SurfaceId::new(1);
        for (error, expected) in [
            (
                crate::engine::EngineError::HostFrameContributionRosterIncomplete {
                    expected: vec![surface],
                    submitted: Vec::new(),
                },
                DockspaceRuntimeErrorKind::HostProtocol,
            ),
            (
                crate::engine::EngineError::HostFramePoisoned {
                    source: CoreHostFrameError::PresentationObligationsNotIssued,
                },
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                crate::engine::EngineError::BackendIngress {
                    source: BackendIngressError::ProviderReplacementPending,
                },
                DockspaceRuntimeErrorKind::OperationConflict,
            ),
        ] {
            assert_eq!(DockspaceRuntimeError::from(error).kind(), expected);
        }
    }

    #[test]
    fn surface_prepare_error_kinds_separate_input_from_core_failure() {
        let surface = SurfaceId::new(1);
        for (error, expected) in [
            (
                SurfaceContributionPrepareError::SurfaceOutsideRoster { surface },
                DockspaceRuntimeErrorKind::HostProtocol,
            ),
            (
                SurfaceContributionPrepareError::Compilation(
                    PresentationCompilationError::RequirementsVanished { surface },
                ),
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                SurfaceContributionPrepareError::Compilation(PresentationCompilationError::Scene(
                    SceneCompilationError::InvalidDockFraction,
                )),
                DockspaceRuntimeErrorKind::InvalidConfiguration,
            ),
            (
                SurfaceContributionPrepareError::Validation(
                    SceneBuildError::SurfaceSceneRevisionExhausted { surface },
                ),
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                SurfaceContributionPrepareError::RetainedCandidateUnavailable { surface },
                DockspaceRuntimeErrorKind::Internal,
            ),
            (
                SurfaceContributionPrepareError::CoordinateAuthorityChanged { surface },
                DockspaceRuntimeErrorKind::Internal,
            ),
        ] {
            assert_eq!(DockspaceRuntimeError::from(error).kind(), expected);
        }
    }

    #[test]
    fn presentation_observation_errors_are_internal() {
        for error in [
            PresentationObservationError::PendingRosterMismatch,
            PresentationObservationError::CaptureGenerationExhausted,
            PresentationObservationError::BackendReportConflict,
        ] {
            assert_eq!(
                DockspaceRuntimeError::from(error).kind(),
                DockspaceRuntimeErrorKind::Internal
            );
        }
    }
}
