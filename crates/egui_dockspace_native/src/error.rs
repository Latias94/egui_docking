//! Public runtime failures.

use dockspace::ids::ItemId;
use dockspace::ids::SurfaceId;
use dockspace::viewport::WindowToken;
use egui::ViewportId;
use thiserror::Error;

/// Failure at a native host authority boundary.
#[derive(Debug, Error)]
pub enum NativeRuntimeError {
    /// The configured viewport roster omitted the physical root.
    #[error("native viewport roster must contain egui's root viewport")]
    MissingRootViewport,
    /// More than one viewport was assigned to one logical surface.
    #[error("logical surface {surface} is assigned to more than one native viewport")]
    DuplicateSurface { surface: SurfaceId },
    /// A restored placement was attached to another logical surface.
    #[error("restored placement surface {found} does not match native surface {expected}")]
    RestoredPlacementSurfaceMismatch {
        expected: SurfaceId,
        found: SurfaceId,
    },
    /// A restored placement cannot be represented by egui's logical `f32` viewport builder.
    #[error("restored placement for surface {surface} cannot be represented by egui")]
    RestoredPlacementUnrepresentable { surface: SurfaceId },
    /// More than one viewport reused one adapter window token.
    #[error("native viewport roster repeats window token {token:?}")]
    DuplicateWindowToken { token: WindowToken },
    /// The configured physical roster differs from the workspace surface roster.
    #[error("native viewport roster does not exactly cover the dockspace workspace")]
    WorkspaceRosterMismatch,
    /// Core rejected a bootstrap or replacement registration from the exact native roster.
    #[error("core rejected native viewport registration for surface {surface}")]
    ViewportRegistrationRejected { surface: SurfaceId },
    /// A restored child could not enter the fork's correlated native-create lane.
    #[error(
        "restored native viewport {viewport:?} for surface {surface} could not be scheduled: {source}"
    )]
    RestoredViewportCreateSubmit {
        surface: SurfaceId,
        viewport: ViewportId,
        #[source]
        source: eframe::NativeViewportCreateSubmitError,
    },
    /// The platform accepted a restored child request but failed to create its window.
    #[error("restored native viewport {viewport:?} for surface {surface} failed to materialize")]
    RestoredViewportCreateFailed {
        surface: SurfaceId,
        viewport: ViewportId,
    },
    /// The platform cannot create the restored child window requested by this document.
    #[error(
        "restored native viewport {viewport:?} for surface {surface} is unsupported by the native backend"
    )]
    RestoredViewportCreateUnsupported {
        surface: SurfaceId,
        viewport: ViewportId,
    },
    /// A hosted callback arrived for a viewport outside the configured roster.
    #[error("hosted callback named unconfigured viewport {viewport:?}")]
    UnconfiguredViewport { viewport: ViewportId },
    /// More than one configured entry named the same physical viewport slot.
    #[error("native viewport roster repeats viewport {viewport:?}")]
    DuplicateViewport { viewport: ViewportId },
    /// The fork did not provide its atomic native ingress snapshot.
    #[error("transactional hosted cycle omitted native host ingress")]
    NativeIngressMissing,
    /// The fork-owned cross-lane journal skipped or replayed an ingress prefix.
    #[error(
        "native ingress journal starts after {submitted}, expected the committed prefix {expected}"
    )]
    NativeIngressOrderMismatch { expected: u64, submitted: u64 },
    /// A hosted callback occurred outside the matching begin/end cycle.
    #[error("hosted viewport callback occurred without an active native cycle")]
    CycleMissing,
    /// A new hosted cycle attempted to overtake an unfinished predecessor.
    #[error("a native hosted cycle is already active")]
    CycleAlreadyActive,
    /// The complete hosted callback roster omitted a configured physical viewport.
    #[error("hosted cycle omitted callback for viewport {viewport:?}")]
    MissingViewportCallback { viewport: ViewportId },
    /// A renderer output could not be matched to the exact adapter output.
    #[error("native renderer output for viewport {viewport:?} has no adapter output")]
    AdapterOutputMissing { viewport: ViewportId },
    /// A hosted output belonged to another native viewport incarnation.
    #[error("hosted output for viewport {viewport:?} has the wrong native binding")]
    HostedOutputBindingMismatch { viewport: ViewportId },
    /// An adapter output did not carry an exact native route.
    #[error("adapter output for surface {surface} has no exact native route")]
    NativeRouteMissing { surface: SurfaceId },
    /// An application presentation token would have been overwritten.
    #[error("viewport {viewport:?} already carries a presentation token")]
    PresentationTokenOccupied { viewport: ViewportId },
    /// The renderer returned a token not owned by a pending output.
    #[error("renderer result named unknown presentation serial {serial}")]
    UnknownPresentationToken { serial: u64 },
    /// The renderer result named another physical viewport.
    #[error(
        "renderer result for presentation serial {serial} named viewport {submitted:?}, expected {expected:?}"
    )]
    PresentationViewportMismatch {
        serial: u64,
        expected: ViewportId,
        submitted: ViewportId,
    },
    /// A monotonic runtime identity could not advance without wrapping.
    #[error("native runtime identity space is exhausted")]
    IdentityExhausted,
    /// Native pointer order did not continue the core provider watermark.
    #[error("native pointer sequence cannot be represented by the active core provider")]
    PointerSequenceMismatch,
    /// A close handler attempted to defer an item whose frozen policy permits only an immediate
    /// decision.
    #[error("native close handler deferred immediate-only item {item}")]
    DeferredCloseNotAllowed { item: ItemId },
    /// The current fork ingress cannot preserve a required authority fact.
    #[error("native ingress is unavailable: {0}")]
    IngressUnavailable(&'static str),
    /// A fork hosted-cycle capability rejected an exact lifetime operation.
    #[error("native hosted-cycle protocol rejected an operation: {0}")]
    HostedProtocol(String),
    /// Core rejected one ordered native ingress prefix. The retained detail
    /// identifies the original reducer failure rather than only its poisoned
    /// host-frame wrapper.
    #[error("core rejected native backend ingress: {detail}")]
    CoreIngressRejected {
        detail: String,
        #[source]
        source: egui_dockspace::DockspaceError,
    },
    /// The dockspace adapter rejected an operation.
    #[error(transparent)]
    Dockspace(#[from] egui_dockspace::DockspaceError),
    /// The atomic dockspace document could not be captured or restored.
    #[error(transparent)]
    DocumentPersistence(#[from] egui_dockspace::DockspaceDocumentPersistenceError),
    /// The joined core ingress recorder rejected a fact.
    #[error(transparent)]
    BackendIngress(#[from] dockspace::backend::ingress::BackendIngressError),
    /// The core rejected a translated platform snapshot.
    #[error(transparent)]
    PlatformSnapshot(#[from] dockspace::backend::platform::PlatformSnapshotError),
    /// A translated pointer segment was not contiguous.
    #[error(transparent)]
    PointerJournal(#[from] dockspace::backend::pointer_journal::PointerJournalError),
    /// A native scroll sample violated the core lossless-scroll schema.
    #[error(transparent)]
    ScrollEdge(#[from] dockspace::backend::pointer_journal::ScrollEdgeError),
    /// A fail-closed receiver receipt roster was malformed.
    #[error(transparent)]
    PointerReceipts(#[from] dockspace::backend::pointer_receiver::PointerReceiverReceiptBatchError),
    /// Native geometry could not be represented by the core geometry model.
    #[error(transparent)]
    Geometry(#[from] dockspace::geometry::GeometryError),
    /// The affine adapter settlement rejected a mismatched native lifetime.
    #[error("native presentation settlement rejected its renderer result: {0}")]
    PresentationSettlement(String),
}

#[derive(Debug, Error)]
#[error("{message}")]
pub(crate) struct HostedHookError {
    message: String,
}

impl From<NativeRuntimeError> for HostedHookError {
    fn from(error: NativeRuntimeError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}
