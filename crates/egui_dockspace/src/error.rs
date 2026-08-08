//! Public failures produced by the egui facade.

use dockspace::backend::engine::{
    CoreHostFrameError, EngineError, HostFrameHoverDropResolutionError,
    SurfaceContributionBeginError, SurfaceContributionPrepareError,
};
use dockspace::backend::ingress::BackendIngressError;
use dockspace::backend::interaction::InteractionRejection;
use dockspace::backend::pointer_journal::{
    PointerEdgeSequence, PointerJournalError, SurfaceLocalPointerProviderError,
};
use dockspace::backend::pointer_receiver::{
    PointerReceiverObservationError, PointerReceiverReceiptBatchError,
};
use dockspace::backend::presentation_observation::HostPresentationEndpoint;
use dockspace::backend::presentation_observation::HostPresentationStreamId;
use dockspace::backend::presentation_observation::NativeStagingPresentation;
use dockspace::backend::scene::SurfaceSceneStamp;
use dockspace::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use dockspace::intent::ContainedPlacementUnavailable;
use dockspace::presentation_config::DockPresentationConfigError;
use egui::ViewportId;
use thiserror::Error;

use crate::DockStyleError;
use crate::facade::{ExactNativeViewport, NativeBindingError};
use crate::projection::ProjectionError;
use crate::render::EguiRendererError;

/// Failure to construct or advance an egui docking frame.
#[derive(Debug, Error)]
pub enum DockspaceError {
    /// A facade method did not receive its required terminal reducer outcome.
    #[error("dockspace {operation} did not produce its required application outcome")]
    ApplicationOutcomeUnavailable {
        /// Stable facade operation name used for diagnostics.
        operation: &'static str,
    },
    /// The session-owned persistence identity boundary rejected an operation.
    #[cfg(feature = "serde")]
    #[error("dockspace document session failed: {0}")]
    DocumentSession(#[from] dockspace::document::DockspaceDocumentSessionError),
    /// The native runtime supplied a stale, incomplete, or relabeled viewport route.
    #[error("native viewport binding failed: {0}")]
    NativeBinding(#[from] NativeBindingError),
    /// A prepared native route delta lost its owning registry before commit.
    #[error("prepared native viewport bindings have no live registry")]
    NativeBindingRegistryUnavailable,
    /// One logical surface changed native incarnation during one hosted cycle.
    #[error(
        "surface {surface} changed exact native binding from {previous:?} to {submitted:?} within one hosted cycle"
    )]
    NativeSurfaceBindingChanged {
        /// Logical surface whose physical owner changed.
        surface: SurfaceId,
        /// Exact binding that produced the accepted callback.
        previous: ExactNativeViewport,
        /// Exact binding attached to the later callback or output.
        submitted: ExactNativeViewport,
    },
    /// A final output was supplied without a matching exact native UI callback.
    #[error("surface {surface} native output has no matching exact callback")]
    NativeSurfaceOutputWithoutCallback {
        /// Logical surface missing its callback route.
        surface: SurfaceId,
    },
    /// A native callback ran under a different egui viewport slot.
    #[error("native callback for {native:?} executed under egui viewport {submitted:?}")]
    NativeCallbackViewportMismatch {
        /// Exact native lifetime expected by the route.
        native: ExactNativeViewport,
        /// Egui viewport which actually executed the callback.
        submitted: ViewportId,
    },
    /// A receiver output ticket belongs to another logical surface.
    #[error(
        "native receiver route {native:?} owns surface {expected}, but output belongs to {submitted}"
    )]
    NativeReceiverOutputSurfaceMismatch {
        native: ExactNativeViewport,
        expected: SurfaceId,
        submitted: SurfaceId,
    },
    /// A native callback was routed to a live surface without a core staging request.
    #[error("native viewport {native:?} has no current core staging request")]
    NativeStagingRequestUnavailable {
        /// Exact native lifetime submitted by the runtime.
        native: ExactNativeViewport,
    },
    /// A native staging request belongs to another exact core binding.
    #[error("native staging request {presentation:?} does not match callback route for {native:?}")]
    NativeStagingBindingMismatch {
        /// Exact native lifetime submitted by the runtime.
        native: ExactNativeViewport,
        /// Core-authored staging request found for the logical surface.
        presentation: NativeStagingPresentation,
    },
    /// A callback painted a staging request which is no longer in the frozen roster.
    #[error("native staging request {presentation:?} is outside the current host-frame roster")]
    NativeStagingRequestOutsideRoster {
        /// Stale or foreign request supplied by the adapter callback.
        presentation: NativeStagingPresentation,
    },
    /// A core output endpoint disagreed with the exact native callback route.
    #[error(
        "native presentation for {native:?} expected endpoint {expected:?}, submitted {submitted:?}"
    )]
    NativePresentationEndpointMismatch {
        /// Exact native lifetime which produced the callback and `FullOutput`.
        native: ExactNativeViewport,
        /// Endpoint frozen from the exact native/core route.
        expected: HostPresentationEndpoint,
        /// Endpoint carried by the core-minted presentation output.
        submitted: HostPresentationEndpoint,
    },
    /// Owned renderer draft staging or acceptance failed.
    #[error("egui renderer transaction failed: {0}")]
    Renderer(#[from] EguiRendererError),
    /// Style geometry is invalid and cannot produce authoritative scene facts.
    #[error("dock style is invalid: {0}")]
    Style(#[from] DockStyleError),
    /// Style geometry could not be represented by the neutral core configuration.
    #[error("dock presentation configuration is invalid: {0}")]
    PresentationConfig(#[from] DockPresentationConfigError),
    /// The renderer-neutral engine could not publish an atomic boundary.
    #[error("dock engine failed: {0}")]
    Engine(#[from] EngineError),
    /// The joined backend recorder rejected a provider or causal-lane operation.
    #[error("backend ingress failed: {0}")]
    BackendIngress(#[from] BackendIngressError),
    /// The current core scene could not authorize a discrete contained placement.
    #[error("contained placement is unavailable: {0}")]
    ContainedPlacement(#[from] ContainedPlacementUnavailable),
    /// The egui projection could not represent the current workspace.
    #[error("egui projection failed: {0}")]
    Projection(#[from] ProjectionError),
    /// A host frame did not advance beyond the last successfully committed pass.
    #[error(
        "host frame ({submitted_sequence}, {submitted_pass}) is not after committed frame ({previous_sequence}, {previous_pass})"
    )]
    HostFrameNotIncreasing {
        /// Last successfully committed host sequence.
        previous_sequence: u64,
        /// Last successfully committed host pass.
        previous_pass: u32,
        /// Submitted host sequence.
        submitted_sequence: u64,
        /// Submitted host pass.
        submitted_pass: u32,
    },
    /// The automatic single-surface egui frame sequence cannot advance further.
    #[error("automatic egui host-frame sequence exhausted")]
    AutomaticHostFrameSequenceExhausted,
    /// The adapter cannot advance its provider capture generation for one exact core stream.
    #[error("automatic egui presentation capture generation exhausted for stream {stream:?}")]
    AutomaticPresentationCaptureGenerationExhausted {
        /// Core-minted stream whose provider generation cannot advance.
        stream: HostPresentationStreamId,
    },
    /// The outer-host presentation provider exhausted one stream generation.
    #[error("outer egui presentation capture generation exhausted for stream {stream:?}")]
    OuterPresentationCaptureGenerationExhausted {
        /// Core-minted stream whose provider generation cannot advance.
        stream: HostPresentationStreamId,
    },
    /// The single-surface convenience API was used for a multi-surface roster.
    #[error(
        "show_single_surface requires exactly one surface in the frozen roster, found {surface_count}"
    )]
    SingleSurfaceHostFrameRequiresOneSurface {
        /// Number of surfaces frozen at host-frame begin.
        surface_count: usize,
    },
    /// A surface was not present in the host frame's tick-start roster.
    #[error("surface {surface} is outside the frozen host-frame roster")]
    HostFrameSurfaceOutsideRoster {
        /// Unexpected logical surface.
        surface: SurfaceId,
    },
    /// A surface slot was supplied more than once in one host frame.
    #[error("surface {surface} was supplied more than once in one host frame")]
    HostFrameDuplicateSurface {
        /// Repeated logical surface.
        surface: SurfaceId,
    },
    /// One logical surface changed egui viewport within a frozen outer-host frame.
    #[error(
        "surface {surface} changed viewport from {previous:?} to {submitted:?} within one outer-host frame"
    )]
    HostFrameSurfaceViewportChanged {
        /// Logical surface whose viewport binding changed.
        surface: SurfaceId,
        /// Viewport used by the preceding pass.
        previous: ViewportId,
        /// Viewport used by the replacement pass.
        submitted: ViewportId,
    },
    /// A repeated outer-host surface callback did not advance its egui pass.
    #[error(
        "surface {surface} pass {submitted} is not after prior pass {previous} within one outer-host frame"
    )]
    HostFrameSurfacePassNotIncreasing {
        /// Logical surface whose draft could not be replaced.
        surface: SurfaceId,
        /// Previously accepted cumulative egui pass.
        previous: u64,
        /// Submitted cumulative egui pass.
        submitted: u64,
    },
    /// A final-output confirmation was attempted on the callback-only host path.
    #[error("final surface output confirmation requires an outer-host frame")]
    OuterHostFrameRequired,
    /// The final output came from a different egui context than the painted draft.
    #[error("surface {surface} final output belongs to another egui context")]
    OuterHostSurfaceContextMismatch {
        /// Logical surface whose context identity mismatched.
        surface: SurfaceId,
    },
    /// The final output came from a different egui viewport than the painted draft.
    #[error(
        "surface {surface} final output viewport {submitted:?} does not match painted viewport {expected:?}"
    )]
    OuterHostSurfaceViewportMismatch {
        /// Logical surface whose viewport identity mismatched.
        surface: SurfaceId,
        /// Viewport that produced the accepted final draft.
        expected: ViewportId,
        /// Viewport submitted by the outer host.
        submitted: ViewportId,
    },
    /// The context advanced beyond or did not reach the pass that painted this draft.
    #[error(
        "surface {surface} final output pass {submitted} does not match painted pass {expected}"
    )]
    OuterHostSurfaceOutputPassMismatch {
        /// Logical surface whose pass identity mismatched.
        surface: SurfaceId,
        /// Cumulative pass that produced the accepted draft.
        expected: u64,
        /// Cumulative pass visible when the output was confirmed.
        submitted: u64,
    },
    /// The final callback pass cannot advance to a completed-output boundary.
    #[error("surface {surface} final egui pass identity is exhausted")]
    OuterHostSurfaceOutputPassExhausted {
        /// Logical surface whose cumulative pass cannot advance.
        surface: SurfaceId,
    },
    /// The supplied final output did not carry the opaque proof minted by the painted pass.
    #[error("surface {surface} final egui output does not belong to its painted pass")]
    OuterHostSurfaceOutputAuthorityMismatch {
        /// Logical surface whose output proof mismatched.
        surface: SurfaceId,
    },
    /// A final output was submitted more than once for one painted pass.
    #[error("surface {surface} final egui output was already confirmed")]
    OuterHostSurfaceOutputAlreadyConfirmed {
        /// Logical surface whose affine output slot was already consumed.
        surface: SurfaceId,
    },
    /// The supplied egui output did not contain a completed pass for this viewport.
    #[error("surface {surface} has no completed final egui output")]
    OuterHostSurfaceFullOutputMissing {
        /// Logical surface missing a complete output.
        surface: SurfaceId,
    },
    /// A painted outer-host surface reached reduction before final-output confirmation.
    #[error("surface {surface} final egui output was not confirmed before outer-frame finish")]
    OuterHostSurfaceOutputUnconfirmed {
        /// Logical surface whose final output is still unproven.
        surface: SurfaceId,
    },
    /// An internal host-frame slot reached reduction without its required prepared contribution.
    #[error("host frame surface {surface} has no prepared contribution")]
    HostFrameContributionMissing {
        /// Frozen logical surface that was not prepared.
        surface: SurfaceId,
    },
    /// A prior `show_surface` or unavailable-slot call invalidated this host frame.
    #[error("host frame is poisoned after an earlier surface protocol error")]
    HostFramePoisoned,
    /// A backend frame was started without the joined provider required to order native facts.
    #[error("backend frame requires an active joined backend ingress provider")]
    BackendIngressProviderMissing,
    /// A facade mutation attempted to overtake a live owned native session.
    #[error("an owned native session is already active for this dockspace")]
    NativeSessionAlreadyActive,
    /// An owned native session was presented to a different or inactive dockspace.
    #[error("the owned native session does not match this dockspace")]
    NativeSessionLeaseMismatch,
    /// An immediate application mutation raced an active backend input stream.
    #[error("application input must be recorded into the active backend ingress frame")]
    BackendApplicationInputRequiresIngress,
    /// A staged renderer style had no matching accepted core configuration outcome.
    #[error(
        "native style configuration at source sequence {source_sequence} was not accepted by core"
    )]
    NativeStyleConfigurationNotAccepted {
        /// Exact terminal configuration sequence paired with the style sidecar.
        source_sequence: SourceSequence,
    },
    /// A presentation-only backend paint attempted to publish fresh semantic input.
    #[error("backend presentation produced {count} forbidden semantic input(s)")]
    PresentationPhaseProducedSemanticInput {
        /// Number of semantic inputs generated after the input prefix closed.
        count: usize,
    },
    /// A renderer action could not be attributed to exactly one raw egui event.
    #[error(
        "surface {surface} semantic action matched {matching_raw_events} raw egui events instead of exactly one"
    )]
    SemanticActionCausalityUnavailable {
        /// Logical surface which observed the action.
        surface: SurfaceId,
        /// Number of matching events retained in the immutable raw event batch.
        matching_raw_events: usize,
    },
    /// A fork envelope was exact, but its backend derivative correlation was unavailable.
    #[cfg(egui_backend_event_envelope)]
    #[error(
        "surface {surface} semantic action claimed raw egui event {raw_event_index} without backend correlation"
    )]
    SemanticActionBackendCorrelationUnavailable {
        /// Logical surface which observed the action.
        surface: SurfaceId,
        /// Exact raw envelope index whose correlation was unknown.
        raw_event_index: usize,
    },
    /// Surface-local event positions cannot establish one cross-viewport order.
    #[error(
        "{input_count} semantic input(s) span {surface_count} surfaces without backend ordering authority"
    )]
    CrossViewportSemanticInputRequiresBackendAuthority {
        /// Number of surfaces frozen into the host frame.
        surface_count: usize,
        /// Number of event-derived semantic inputs that require a global order.
        input_count: usize,
    },
    /// The crates.io egui adapter cannot establish a complete cross-viewport input boundary.
    #[error(
        "egui_dockspace currently supports one logical surface per host frame, found {surface_count}; use a native runtime provider for multiview"
    )]
    MultiSurfaceHostFrameUnsupported {
        /// Number of surfaces frozen by the core at host-frame begin.
        surface_count: usize,
    },
    /// One explicit egui producer exhausted its monotonic reducer input sequence.
    #[error("egui input source {input_source} sequence exhausted")]
    InputSourceSequenceExhausted {
        /// Stable producer whose source sequence cannot advance further.
        input_source: StableInputSourceId,
    },
    /// The requested logical surface was outside the core-derived presentation roster.
    #[error("surface contribution could not begin: {0}")]
    SurfaceContributionBegin(#[from] SurfaceContributionBeginError),
    /// The core rejected measured surface facts before they could enter a reducer tick.
    #[error("surface contribution could not be prepared: {0}")]
    SurfaceContributionPrepare(#[from] SurfaceContributionPrepareError),
    /// Transitional adapter geometry is missing for a core-retained scene stamp.
    #[error("adapter geometry mirror for surface {surface} and scene {stamp:?} is missing")]
    AdapterGeometryMirrorMissing {
        /// Surface requiring the retained geometry mirror.
        surface: SurfaceId,
        /// Candidate or fallback plan stamp requested by the core.
        stamp: SurfaceSceneStamp,
    },
    /// Core retained a plan stamp for which the adapter no longer owns paint resources.
    #[error("adapter plan resource for surface {surface} and scene {stamp:?} is missing")]
    PaintResourceMissing {
        /// Surface requiring an exact retained resource.
        surface: SurfaceId,
        /// Candidate or fallback plan stamp requested by the core.
        stamp: SurfaceSceneStamp,
    },
    /// Adapter resources no longer match the exact core-compiled geometry for their stamp.
    #[error(
        "adapter plan resource for surface {surface} and scene {stamp:?} mismatches core geometry"
    )]
    AdapterGeometryMirrorMismatch {
        /// Surface requiring an exact retained resource.
        surface: SurfaceId,
        /// Candidate or fallback plan stamp requested by the core.
        stamp: SurfaceSceneStamp,
    },
    /// The core host-frame capability rejected an adapter contribution or acknowledgement.
    #[error("core host frame is invalid: {0}")]
    CoreHostFrame(#[from] CoreHostFrameError),
    /// The adapter could not encode one contiguous raw egui pointer epoch.
    #[error("egui pointer journal is invalid: {0}")]
    PointerJournal(#[from] PointerJournalError),
    /// The affine surface-local producer lifecycle rejected an adapter transition.
    #[error("egui surface-local pointer provider is invalid: {0}")]
    SurfaceLocalPointerProvider(#[from] SurfaceLocalPointerProviderError),
    /// A receiver fact did not match the exact core presentation manifest.
    #[error("egui pointer receiver observation is invalid: {0}")]
    PointerReceiverObservation(#[from] PointerReceiverObservationError),
    /// The sealed core view could not resolve one exact hover-drop receiver.
    #[error("sealed hover-drop receiver could not be resolved: {0}")]
    HostFrameHoverDropResolution(#[from] HostFrameHoverDropResolutionError),
    /// The sealed scene could not prepare an exact semantic tab interaction.
    #[error("semantic tab interaction is unavailable: {0:?}")]
    TabInteractionUnavailable(InteractionRejection),
    /// The adapter did not produce one exact receiver receipt per candidate.
    #[error("egui pointer receiver receipt batch is invalid: {0}")]
    PointerReceiverReceiptBatch(#[from] PointerReceiverReceiptBatchError),
    /// A later egui input epoch arrived before the pending journal committed.
    #[error("egui pointer input advanced before the pending epoch committed")]
    PointerInputEpochAdvancedBeforeCommit,
    /// A pointer provider was used without an adapter binding.
    #[error("egui pointer provider has no active adapter binding")]
    PointerInputBindingMissing,
    /// A pointer epoch belongs to an older adapter incarnation or viewport binding.
    #[error("egui pointer epoch belongs to a stale adapter binding")]
    PointerInputBindingStale,
    /// The adapter could not allocate another incarnation for a pointer binding.
    #[error("egui pointer adapter incarnation exhausted")]
    PointerAdapterIncarnationExhausted,
    /// A new producer attempted to overwrite the adapter's active producer.
    #[error("egui pointer adapter already owns an active producer")]
    PointerInputProviderAlreadyInstalled,
    /// An application control boundary raced an uncommitted egui pointer epoch.
    #[error("application control cannot overtake an uncommitted egui pointer epoch")]
    PointerInputControlDuringPendingEpoch,
    /// An uncommitted host frame still owns the producer lane.
    #[error("pointer input cannot stop while its host frame is uncommitted")]
    PointerInputFrameInFlight,
    /// A frame failure was followed by a second failure while retiring the exact pointer
    /// provider which owned its staged physical edges.
    #[error(
        "egui host frame failed ({frame}); retiring its pointer provider also failed ({retirement})"
    )]
    PointerInputAbortFailed {
        /// Original frame failure.
        frame: Box<DockspaceError>,
        /// Provider-retirement failure. The adapter retains the staged physical edges.
        retirement: Box<DockspaceError>,
    },
    /// The adapter cannot assign another lossless provider-ordered edge identity.
    #[error("egui pointer edge sequence exhausted after {after}")]
    PointerEdgeSequenceExhausted { after: PointerEdgeSequence },
    /// The core did not retain the candidate roster for a staged pointer journal.
    #[error("staged egui pointer journal has no receiver candidate roster")]
    PointerReceiverCandidatesMissing,
    /// A receiver candidate referenced an edge absent from the staged journal.
    #[error("receiver candidate references missing pointer edge sequence {sequence}")]
    PointerReceiverEdgeMissing { sequence: PointerEdgeSequence },
    /// One egui widget or core region was registered with more than one receiver identity.
    #[error("egui pointer receiver registration is not one-to-one")]
    PointerReceiverRegistrationConflict,
    /// The adapter's terminal semantic split did not fall inside its staged journal interval.
    #[error(
        "egui pointer semantic split {split} is outside journal interval {previous}..={through}"
    )]
    PointerSemanticSplitOutsideJournal {
        /// Edge watermark immediately before the semantic segment.
        split: PointerEdgeSequence,
        /// Journal lower watermark.
        previous: PointerEdgeSequence,
        /// Journal upper watermark.
        through: PointerEdgeSequence,
    },
    /// A strict single-surface host frame ended without a paint result.
    #[error("single-surface host frame did not retain its paint result for {surface}")]
    SingleSurfacePaintUnavailable {
        /// Surface expected to have been painted.
        surface: SurfaceId,
    },
}
