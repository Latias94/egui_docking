//! Engine construction and reduction failures.

use super::*;

/// Failure to construct or atomically reduce engine state.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EngineError {
    /// Initial or replacement workspace violated a durable invariant.
    #[error("workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// Current policy prevents an application-requested layout replacement.
    #[error("workspace replacement input {input} rejected by policy: {reason}")]
    WorkspaceReplacementPolicyDenied {
        /// Input which attempted the replacement.
        input: InputSequence,
        /// Exact policy restriction.
        reason: crate::policy::PolicyRejection,
    },
    /// A native document restore could not retain its external root window.
    #[error("native workspace replacement input {input} cannot retain root surface {surface}")]
    NativeWorkspaceRootUnavailable {
        input: InputSequence,
        surface: SurfaceId,
    },
    /// A declared managed child still owns a retained external root window.
    #[error(
        "native workspace replacement input {input} cannot convert retained root {surface} into a child"
    )]
    NativeWorkspaceChildAlreadyHosted {
        input: InputSequence,
        surface: SurfaceId,
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
        binding: crate::viewport::WindowBinding,
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

pub(super) fn presentation_ledger_error(source: PresentationLedgerError) -> EngineError {
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
