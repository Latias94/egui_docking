//! Typed viewport coordinator failures and resource error conversions.

use super::*;

/// Fatal platform coordinator transition failure.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ViewportCoordinatorError {
    #[error(transparent)]
    PlatformProvider(PlatformObservationAuthorityError),
    #[error("no platform observation provider is active")]
    PlatformProviderUnavailable,
    #[error("platform capability generation is exhausted")]
    CapabilityGenerationExhausted,
    #[error("platform work-area generation is exhausted")]
    WorkAreaGenerationExhausted,
    /// A delayed complete platform batch cannot mutate any newer authority
    /// lane, including the transitional pointer roster.
    #[error("platform snapshot is stale: submitted {submitted:?}, current {current:?}")]
    StalePlatformSnapshot {
        /// Delayed provider batch generation.
        submitted: PlatformSnapshotGeneration,
        /// Last accepted provider batch generation.
        current: PlatformSnapshotGeneration,
    },
    /// A provider reused one complete-batch generation for different facts.
    #[error("platform snapshot generation {generation:?} conflicts with its accepted envelope")]
    ConflictingPlatformSnapshot {
        /// Reused provider batch generation.
        generation: PlatformSnapshotGeneration,
    },
    #[error(
        "platform inventory observation is stale: submitted {submitted:?}, current {current:?}"
    )]
    StaleInventoryObservation {
        submitted: InventoryObservationGeneration,
        current: InventoryObservationGeneration,
    },
    /// One provider reused an accepted inventory generation for a different
    /// complete roster. The whole platform snapshot is rejected before any
    /// binding-scoped fact can publish.
    #[error("platform inventory generation {generation:?} conflicts with its accepted envelope")]
    ConflictingInventoryObservation {
        /// Reused provider generation.
        generation: InventoryObservationGeneration,
    },
    /// A provider omitted one or more complete inventory envelopes. The whole
    /// snapshot is rejected so its binding-scoped facts cannot publish under a
    /// discontinuous roster.
    #[error("platform inventory generation skipped after {previous:?}: submitted {submitted:?}")]
    InventoryObservationGenerationGap {
        /// Last accepted provider generation.
        previous: InventoryObservationGeneration,
        /// Discontinuous submitted generation.
        submitted: InventoryObservationGeneration,
    },
    #[error(transparent)]
    Registry(ViewportRegistryError),
    #[error(transparent)]
    Effect(EffectLedgerError),
    #[error("drag source binding is stale or not ready: {binding:?}")]
    StaleDragSource { binding: ViewportBinding },
    #[error("pointer pass-through saga is missing for drag source {binding:?}")]
    PointerPassthroughSagaMissing { binding: ViewportBinding },
    #[error("pointer-input restore obligation is missing for {binding:?}")]
    PointerInputRestoreObligationMissing { binding: ViewportBinding },
    #[error("authoritative pointer-input observation is unavailable for {binding:?}")]
    PointerInputObservationMissing { binding: ViewportBinding },
    #[error("workspace does not contain logical surface {surface:?}")]
    MissingWorkspaceSurface { surface: SurfaceId },
    /// The predecessor is not an exact current, safe, admitted external root binding.
    #[error("native root continuation predecessor is unavailable: {predecessor:?}")]
    NativeRootContinuationPredecessorUnavailable { predecessor: ViewportBinding },
    /// One physical window cannot continue on two logical surfaces.
    #[error("native root continuation repeats predecessor {predecessor:?}")]
    NativeRootContinuationDuplicatePredecessor { predecessor: ViewportBinding },
    /// Two physical windows cannot acquire the same incoming logical surface.
    #[error("native root continuation repeats target {target:?}")]
    NativeRootContinuationDuplicateTarget { target: SurfaceId },
    /// The replacement workspace does not contain the requested continuation target.
    #[error("native root continuation target is absent from the workspace: {target:?}")]
    NativeRootContinuationTargetMissing { target: SurfaceId },
    /// Another native window already owns the requested continuation target.
    #[error("native root continuation target {target:?} is already bound to {binding:?}")]
    NativeRootContinuationTargetOccupied {
        target: SurfaceId,
        binding: ViewportBinding,
    },
    #[error("docking-owned child surface {surface:?} requires an explicit whole-root recovery")]
    ChildRecoveryRequired { surface: SurfaceId },
    #[error(
        "surface vacancy authority changed for {surface:?}: expected {expected:?}, got {actual:?}"
    )]
    SurfaceVacancyAuthorityChanged {
        surface: SurfaceId,
        expected: Option<ViewportBinding>,
        actual: Option<ViewportBinding>,
    },
    #[error("surface vacancy binding facts changed for {binding:?}")]
    SurfaceVacancyBindingFactsChanged {
        binding: ViewportBinding,
        expected_role: ViewportRole,
        actual_role: ViewportRole,
        expected_ownership: ViewportOwnership,
        actual_ownership: ViewportOwnership,
    },
    #[error("viewport coordinator epoch mismatch: expected {expected:?}, got {actual:?}")]
    WorkspaceEpochMismatch {
        expected: WorkspaceEpoch,
        actual: WorkspaceEpoch,
    },
    #[error("binding retirement is missing: {binding:?}")]
    MissingBindingCleanup { binding: ViewportBinding },
    #[error("retired viewport token remains reserved until exact retirement completion: {token:?}")]
    RetiredTokenReserved { token: WindowToken },
    #[error("retired cleanup ownership mismatch for {binding:?}: {ownership:?}")]
    BindingCleanupOwnershipMismatch {
        binding: ViewportBinding,
        ownership: ViewportOwnership,
    },
    #[error("destroyed viewport binding is still observed: {binding:?}")]
    DestroyedSurfaceStillObserved { binding: ViewportBinding },
    #[error("terminal binding retention guard is missing for {binding:?}")]
    TerminalBindingGuardMissing { binding: ViewportBinding },
    #[error(
        "terminal binding guard {binding:?} belongs to provider {expected:?}, not {submitted:?}"
    )]
    TerminalBindingGuardProviderMismatch {
        binding: ViewportBinding,
        expected: PlatformObservationLease,
        submitted: PlatformObservationLease,
    },
    #[error("native recovery is not pending for surface {surface:?}")]
    MissingRecoveryPending { surface: SurfaceId },
    #[error("staging binding {binding:?} did not retain pending admission: {admission:?}")]
    StagingBindingUnexpectedAdmission {
        binding: ViewportBinding,
        admission: ViewportAdmission,
    },
    #[error("retiring binding {binding:?} has unexpected admission {admission:?}")]
    RetiringBindingUnexpectedAdmission {
        binding: ViewportBinding,
        admission: ViewportAdmission,
    },
    #[error("surface {surface} pending recovery belongs to another binding or obligation")]
    PendingRecoveryObligationMismatch { surface: SurfaceId },
    #[error("registered replacement does not match pending recovery for surface {surface:?}")]
    PendingRecoveryRegistrationMismatch { surface: SurfaceId },
    #[error("native replacement effect is missing for surface {surface:?}")]
    MissingReplacementEffect { surface: SurfaceId },
    #[error("native create saga identity is exhausted")]
    NativeCreateSagaExhausted,
    #[error("native create capability is unavailable: {capability:?}")]
    NativeCapabilityUnavailable { capability: PlatformCapability },
    #[error("native placement proof is stale")]
    StalePlacementProof,
    #[error("native recovery root does not match the created root")]
    RecoveryRootMismatch,
    #[error("native create saga {saga:?} does not exist")]
    MissingCreateSaga { saga: NativeCreateSagaId },
    #[error("native create saga {saga:?} is not ready to commit")]
    CreateSagaNotReady { saga: NativeCreateSagaId },
    #[error("native create saga {saga:?} cannot mint a valid retained staging resource")]
    InvalidNativeStagingResource { saga: NativeCreateSagaId },
    #[error("retained native staging resource {resource:?} already exists")]
    DuplicateNativeStagingResource { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} does not exist")]
    MissingNativeStagingResource { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has another lifecycle owner")]
    NativeStagingResourceOwnerMismatch { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has multiple durable references")]
    DuplicateNativeStagingResourceReference { resource: NativeStagingResourceId },
    #[error("retained native staging resource {resource:?} has no durable lifecycle reference")]
    UnreferencedNativeStagingResource { resource: NativeStagingResourceId },
    #[error("platform effect {effect:?} does not belong to a retryable cleanup")]
    MissingCleanupEffect { effect: EffectId },
    #[error(
        "cleanup continuation {effect:?} does not name a valid emitted predecessor {predecessor:?}"
    )]
    InvalidCleanupContinuation {
        effect: EffectId,
        predecessor: EffectId,
    },
    #[error("cleanup effect {effect:?} cannot be retried from phase {phase:?}")]
    CleanupEffectNotRetryable {
        effect: EffectId,
        phase: EffectPhase,
    },
    #[error("cleanup effect {effect:?} has no authoritatively observed window")]
    CleanupWindowNotObserved { effect: EffectId },
}

impl From<NativeStagingResourceLedgerError> for ViewportCoordinatorError {
    fn from(source: NativeStagingResourceLedgerError) -> Self {
        match source {
            NativeStagingResourceLedgerError::DuplicateResource { resource } => {
                Self::DuplicateNativeStagingResource { resource }
            }
            NativeStagingResourceLedgerError::MissingResource { resource } => {
                Self::MissingNativeStagingResource { resource }
            }
            NativeStagingResourceLedgerError::OwnerMismatch { resource, .. } => {
                Self::NativeStagingResourceOwnerMismatch { resource }
            }
        }
    }
}

impl From<NativeStagingResourceConservationError> for ViewportCoordinatorError {
    fn from(source: NativeStagingResourceConservationError) -> Self {
        match source {
            NativeStagingResourceConservationError::DuplicateReference { resource, .. } => {
                Self::DuplicateNativeStagingResourceReference { resource }
            }
            NativeStagingResourceConservationError::ReferencedResourceMissing {
                resource, ..
            } => Self::MissingNativeStagingResource { resource },
            NativeStagingResourceConservationError::ReferenceOwnerMismatch { resource, .. } => {
                Self::NativeStagingResourceOwnerMismatch { resource }
            }
            NativeStagingResourceConservationError::UnreferencedResource { resource, .. } => {
                Self::UnreferencedNativeStagingResource { resource }
            }
        }
    }
}
