//! Explicit drag and resize session state.

use thiserror::Error;

use crate::command::{CommandOutcome, MovePayload, NodeSource, WorkspaceCommand};
use crate::drop_resolver::DropAffordance;
use crate::drop_target::DropTargetId;
use crate::frame::NativeCreateRequest;
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::SplitWeight;
use crate::ids::{FloatingPresentationId, InputSequence, RootId, SurfaceId, WorkspaceEpoch};
use crate::intent::{
    ContainedPlacementProof, ContainedTransformKind, NativeTearOffProposal, PointerButton,
    PointerId, TargetAuthority, TearOffRequest,
};
use crate::scene::SceneStamp;
use crate::transition::WorkspaceVersion;

macro_rules! interaction_counter {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a counter value from its runtime representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the runtime representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            pub(crate) const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

interaction_counter!(DragGeneration, "Monotonic generation of drag sessions.");
interaction_counter!(
    ResizeGeneration,
    "Monotonic generation of splitter-resize sessions."
);
interaction_counter!(
    ContainedTransformGeneration,
    "Monotonic generation of contained transform sessions."
);
interaction_counter!(
    PreviewSequence,
    "Monotonic identity for previews published by one engine."
);

/// Identity of one drag session within a workspace epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DragSessionId {
    epoch: WorkspaceEpoch,
    generation: DragGeneration,
}

impl DragSessionId {
    /// Creates a typed drag identity.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, generation: DragGeneration) -> Self {
        Self { epoch, generation }
    }

    /// Returns the workspace epoch in which the drag was armed.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the drag generation.
    #[must_use]
    pub const fn generation(self) -> DragGeneration {
        self.generation
    }
}

/// Identity of one resize session within a workspace epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResizeSessionId {
    epoch: WorkspaceEpoch,
    generation: ResizeGeneration,
}

impl ResizeSessionId {
    /// Creates a typed resize identity.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, generation: ResizeGeneration) -> Self {
        Self { epoch, generation }
    }

    /// Returns the workspace epoch in which the resize began.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the resize generation.
    #[must_use]
    pub const fn generation(self) -> ResizeGeneration {
        self.generation
    }
}

/// Identity of one contained transform session within a workspace epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainedTransformSessionId {
    epoch: WorkspaceEpoch,
    generation: ContainedTransformGeneration,
}

impl ContainedTransformSessionId {
    /// Creates a typed contained transform identity.
    #[must_use]
    pub const fn new(epoch: WorkspaceEpoch, generation: ContainedTransformGeneration) -> Self {
        Self { epoch, generation }
    }

    /// Returns the workspace epoch in which the transform began.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the contained transform generation.
    #[must_use]
    pub const fn generation(self) -> ContainedTransformGeneration {
        self.generation
    }
}

/// Opaque identity of one exact preview publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreviewToken {
    session: DragSessionId,
    scene: SceneStamp,
    sequence: PreviewSequence,
}

impl PreviewToken {
    /// Returns the drag session which owns this preview.
    #[must_use]
    pub const fn session(self) -> DragSessionId {
        self.session
    }

    /// Returns the sealed scene against which this preview was resolved.
    #[must_use]
    pub const fn scene(self) -> SceneStamp {
        self.scene
    }
}

/// Exact renderer-neutral visual which must be painted before delivery.
#[derive(Debug, Clone, PartialEq)]
pub enum PreviewVisual {
    /// Highlight one resolved docking target.
    Dock {
        /// Surface containing the target.
        surface: SurfaceId,
        /// Structural target identity.
        target: DropTargetId,
        /// Exact highlight rectangle.
        rect: LogicalRect,
    },
    /// Show an immediate contained-floating placement.
    Contained {
        /// Host logical surface.
        surface: SurfaceId,
        /// Exact logical floating rectangle.
        rect: LogicalRect,
        /// Whether this is an explicitly enabled native fallback.
        fallback: bool,
    },
    /// Show a future native-surface placement.
    Native {
        /// Logical surface identity to be created.
        surface: SurfaceId,
        /// Exact desktop-physical placement.
        placement: PhysicalRect,
    },
}

impl PreviewVisual {
    /// Returns the surface which receives the preview.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        match *self {
            Self::Dock { surface, .. }
            | Self::Contained { surface, .. }
            | Self::Native { surface, .. } => surface,
        }
    }
}

/// Preview published by the core for an adapter to paint exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionPreview {
    token: PreviewToken,
    visual: PreviewVisual,
}

impl InteractionPreview {
    /// Returns the opaque preview token.
    #[must_use]
    pub const fn token(&self) -> PreviewToken {
        self.token
    }

    /// Returns the exact visual to paint.
    #[must_use]
    pub const fn visual(&self) -> &PreviewVisual {
        &self.visual
    }

    /// Constructs the exact acknowledgement submitted only after this visual was painted.
    #[must_use]
    pub fn acknowledgement(&self) -> PaintAcknowledgement {
        PaintAcknowledgement {
            token: self.token,
            visual: self.visual.clone(),
        }
    }
}

/// Exact proof that one published preview was painted.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintAcknowledgement {
    token: PreviewToken,
    visual: PreviewVisual,
}

impl PaintAcknowledgement {
    /// Returns the opaque preview identity.
    #[must_use]
    pub const fn token(&self) -> PreviewToken {
        self.token
    }

    /// Returns the visual claimed to have been painted.
    #[must_use]
    pub const fn visual(&self) -> &PreviewVisual {
        &self.visual
    }
}

/// Opaque identity of one exact contained transform preview publication.
///
/// This token is intentionally distinct from [`PreviewToken`], so a docking
/// preview acknowledgement cannot satisfy a contained transform release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainedTransformPreviewToken {
    session: ContainedTransformSessionId,
    scene: SceneStamp,
    sequence: PreviewSequence,
}

impl ContainedTransformPreviewToken {
    /// Returns the transform session which owns this preview.
    #[must_use]
    pub const fn session(self) -> ContainedTransformSessionId {
        self.session
    }

    /// Returns the sealed scene against which this preview was resolved.
    #[must_use]
    pub const fn scene(self) -> SceneStamp {
        self.scene
    }
}

/// Exact contained rectangle which must be painted before transform delivery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedTransformPreview {
    token: ContainedTransformPreviewToken,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
}

impl ContainedTransformPreview {
    /// Returns the opaque preview token.
    #[must_use]
    pub const fn token(self) -> ContainedTransformPreviewToken {
        self.token
    }

    /// Returns the ready host surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the contained root.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the contained presentation identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the exact rectangle which must be painted.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }

    /// Constructs the exact acknowledgement submitted only after this rectangle was painted.
    #[must_use]
    pub const fn acknowledgement(self) -> ContainedTransformPaintAcknowledgement {
        ContainedTransformPaintAcknowledgement {
            token: self.token,
            surface: self.surface,
            root: self.root,
            floating: self.floating,
            rect: self.rect,
        }
    }
}

/// Exact proof that one contained transform preview was painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedTransformPaintAcknowledgement {
    token: ContainedTransformPreviewToken,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
}

impl ContainedTransformPaintAcknowledgement {
    /// Returns the opaque preview identity.
    #[must_use]
    pub const fn token(self) -> ContainedTransformPreviewToken {
        self.token
    }

    /// Returns the exact rectangle claimed to have been painted.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }
}

/// Stable public summary of the currently active gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionStatus {
    /// No gesture is active.
    Idle,
    /// A source was pressed but the renderer has not begun dragging.
    Armed { session: DragSessionId },
    /// An explicit drag is active.
    Dragging { session: DragSessionId },
    /// A splitter resize is active.
    Resizing { session: ResizeSessionId },
    /// A contained floating is being moved or resized.
    ContainedTransforming {
        session: ContainedTransformSessionId,
    },
}

/// Phase of the renderer-visible drag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragPhase {
    /// The source button is down, but the renderer has not crossed its drag threshold.
    Armed,
    /// The renderer explicitly began the drag.
    Dragging,
}

/// Read-only view of the exact drag currently owned by the state machine.
///
/// The view borrows its payload and has no public constructor, so adapters can
/// observe an active drag without forging or mutating core interaction state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveDragView<'state> {
    phase: DragPhase,
    session: DragSessionId,
    pointer: PointerId,
    button: PointerButton,
    payload: &'state MovePayload,
    target: Option<&'state TargetAuthority>,
    tear_off: Option<&'state TearOffRequest>,
}

impl<'state> ActiveDragView<'state> {
    /// Returns whether the drag is armed or actively dragging.
    #[must_use]
    pub const fn phase(self) -> DragPhase {
        self.phase
    }

    /// Returns the exact drag generation.
    #[must_use]
    pub const fn session(self) -> DragSessionId {
        self.session
    }

    /// Returns the pointer which owns the drag.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }

    /// Returns the pointer button which armed the drag.
    #[must_use]
    pub const fn button(self) -> PointerButton {
        self.button
    }

    /// Returns the frozen payload captured when the drag was armed.
    #[must_use]
    pub const fn payload(self) -> &'state MovePayload {
        self.payload
    }

    /// Returns the last authoritative target observation for an active drag.
    ///
    /// Armed drags and active drags without an observation return `None`.
    #[must_use]
    pub const fn target(self) -> Option<&'state TargetAuthority> {
        self.target
    }

    /// Returns the exact tear-off request stored with the last observation.
    ///
    /// Adapters can reuse this request across updates and release without
    /// maintaining a second session-to-proposal cache.
    #[must_use]
    pub const fn tear_off(self) -> Option<&'state TearOffRequest> {
        self.tear_off
    }
}

/// Read-only view of the exact contained transform owned by the core.
///
/// Adapters can render and route the gesture from this view without keeping a
/// second copy of the frozen rectangle, pointer origin, or current preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveContainedTransformView<'state> {
    session: ContainedTransformSessionId,
    pointer: PointerId,
    button: PointerButton,
    surface: SurfaceId,
    root: RootId,
    floating: FloatingPresentationId,
    source_rect: LogicalRect,
    initial_pointer: LogicalPoint,
    current_pointer: LogicalPoint,
    kind: ContainedTransformKind,
    minimum_size: LogicalSize,
    preview: Option<&'state ContainedTransformPreview>,
}

impl<'state> ActiveContainedTransformView<'state> {
    /// Returns the exact transform generation.
    #[must_use]
    pub const fn session(self) -> ContainedTransformSessionId {
        self.session
    }

    /// Returns the pointer which owns the transform.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }

    /// Returns the pointer button which began the transform.
    #[must_use]
    pub const fn button(self) -> PointerButton {
        self.button
    }

    /// Returns the frozen host surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the frozen contained root.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the frozen contained presentation identity.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the exact durable rectangle captured at begin.
    #[must_use]
    pub const fn source_rect(self) -> LogicalRect {
        self.source_rect
    }

    /// Returns the absolute pointer location captured at begin.
    #[must_use]
    pub const fn initial_pointer(self) -> LogicalPoint {
        self.initial_pointer
    }

    /// Returns the latest absolute pointer location reduced by the core.
    #[must_use]
    pub const fn current_pointer(self) -> LogicalPoint {
        self.current_pointer
    }

    /// Returns the exact move or resize operation.
    #[must_use]
    pub const fn kind(self) -> ContainedTransformKind {
        self.kind
    }

    /// Returns the frozen minimum contained size.
    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.minimum_size
    }

    /// Returns the exact current preview eligible for painting.
    #[must_use]
    pub const fn preview(self) -> Option<&'state ContainedTransformPreview> {
        self.preview
    }
}

/// Why an active gesture was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractionCancelReason {
    /// The user explicitly pressed Escape.
    Escape,
    /// The source button was released before the renderer began dragging.
    ReleasedBeforeDrag,
    /// Pointer capture was authoritatively lost.
    CaptureLost,
    /// The owning surface lost focus.
    FocusLost,
    /// Button state became non-authoritative.
    UnknownButtonState,
    /// Hovered-target authority became unavailable.
    UnknownTargetAuthority,
    /// Native tear-off capability became non-authoritative.
    NativeCapabilityUnknown,
    /// Native tear-off capability became authoritatively unavailable.
    NativeCapabilityUnavailable,
    /// Native placement proof no longer matches current window facts.
    NativePlacementUnavailable,
    /// The frozen source no longer exists.
    SourceVanished,
    /// A durable workspace command invalidated the session.
    WorkspaceChanged,
    /// Application docking policy changed.
    PolicyChanged,
    /// A participating surface closed.
    SurfaceClosed,
    /// The required sealed scene is unavailable.
    SceneUnavailable,
    /// Another mutually exclusive gesture replaced this one.
    ReplacedByNewGesture,
    /// A successful workspace restore advanced the epoch.
    WorkspaceRestored,
}

/// Why a semantic interaction input did not advance or deliver a gesture.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionRejection {
    /// No gesture is active.
    NoActiveGesture,
    /// This release belongs to the most recently consumed drag generation.
    DuplicateRelease { session: DragSessionId },
    /// A non-release input belongs to a drag generation that already settled.
    SessionConsumed { session: DragSessionId },
    /// The input names a different live session.
    SessionMismatch,
    /// The input names a different pointer than the source press.
    PointerMismatch,
    /// The input names a different button than the source press.
    ButtonMismatch,
    /// The drag has been armed but not explicitly begun.
    DragNotBegun,
    /// The matching button is authoritatively still pressed.
    ButtonStillPressed,
    /// No preview was published for the release.
    PreviewMissing,
    /// The exact published preview was not acknowledged as painted.
    PreviewNotPainted,
    /// The preview's sealed scene is no longer current.
    StaleScene,
    /// Release re-resolution selected a different semantic target or command.
    TargetChanged,
    /// The proposed workspace command was rejected during final revalidation.
    CommandRejected(crate::error::CommandError),
    /// The preview acknowledgement does not match the current publication.
    PreviewAcknowledgementMismatch,
    /// Split weights were rejected during candidate validation.
    ResizeRejected(crate::error::CommandError),
    /// A resize was released before any validated weight proposal was supplied.
    ResizeProposalMissing,
    /// This release belongs to the most recently consumed contained transform.
    DuplicateContainedTransformRelease {
        session: ContainedTransformSessionId,
    },
    /// A non-release input belongs to a contained transform that already settled.
    ContainedTransformSessionConsumed {
        session: ContainedTransformSessionId,
    },
    /// The durable rectangle is outside current bounds or violates the requested minimum.
    ContainedTransformInitialRectUnavailable,
    /// Absolute pointer arithmetic could not produce finite transform geometry.
    ContainedTransformGeometryUnavailable,
    /// The current scene cannot authorize the contained transform placement.
    ContainedPlacementUnavailable(crate::intent::ContainedPlacementUnavailable),
    /// The acknowledgement does not match the exact current contained transform preview.
    ContainedTransformPreviewAcknowledgementMismatch,
    /// Release re-resolution no longer matches the exact painted transform proof.
    ContainedTransformChanged,
}

/// Public result class of resolving one drag observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewResolutionStatus {
    /// One exact dock or tear-off preview was published.
    Resolved,
    /// Authority proved there is no dock target and no tear-off was requested.
    KnownNone,
    /// Geometry was hit, but every candidate was explicitly ineligible.
    Rejected,
    /// A required ready surface scene was unavailable.
    Unavailable,
}

/// Kind of an exact workspace delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDeliveryKind {
    /// Move into a sealed docking target.
    Dock,
    /// Create or rehome into a contained floating by explicit request.
    Contained,
    /// Use the explicitly enabled contained fallback for an unavailable native request.
    ContainedFallback,
}

/// Native tear-off plan produced without moving source content.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedNativeTearOff {
    session: DragSessionId,
    source_version: WorkspaceVersion,
    command: WorkspaceCommand,
    proposal: NativeTearOffProposal,
}

impl PreparedNativeTearOff {
    pub(crate) fn new(
        session: DragSessionId,
        source_version: WorkspaceVersion,
        command: WorkspaceCommand,
        proposal: NativeTearOffProposal,
    ) -> Self {
        Self {
            session,
            source_version,
            command,
            proposal,
        }
    }

    /// Returns the consumed drag generation.
    #[must_use]
    pub const fn session(&self) -> DragSessionId {
        self.session
    }

    /// Returns the exact workspace version against which the plan was prepared.
    #[must_use]
    pub const fn source_version(&self) -> WorkspaceVersion {
        self.source_version
    }

    /// Returns the exact preflighted mutation frozen before native creation.
    #[must_use]
    pub const fn command(&self) -> &WorkspaceCommand {
        &self.command
    }

    /// Returns the explicit native destination and placement.
    #[must_use]
    pub const fn proposal(&self) -> &NativeTearOffProposal {
        &self.proposal
    }
}

/// Successful delivery produced by one consumed release.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionDelivery {
    /// A checked workspace command committed or produced a valid no-op.
    Workspace {
        /// Semantic delivery mode.
        kind: WorkspaceDeliveryKind,
        /// Structured U3 command result.
        outcome: CommandOutcome,
        /// Whether durable workspace state changed.
        changed: bool,
    },
    /// A native create saga entered the effect ledger; source ownership did not move.
    NativeRequested(NativeCreateRequest),
}

/// Result of reducing one renderer intent.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionOutcome {
    /// A new drag generation was armed.
    DragArmed {
        session: DragSessionId,
        replaced: Option<InteractionStatus>,
    },
    /// An armed drag explicitly began.
    DragBegan { session: DragSessionId },
    /// The current target observation produced a new preview state.
    PreviewUpdated {
        session: DragSessionId,
        preview: Option<InteractionPreview>,
        status: PreviewResolutionStatus,
    },
    /// The exact current preview was acknowledged, idempotently if repeated.
    PreviewAcknowledged {
        session: DragSessionId,
        changed: bool,
    },
    /// The first authoritative matching release delivered exactly once.
    DragDelivered {
        session: DragSessionId,
        delivery: InteractionDelivery,
    },
    /// An active matching gesture was cancelled.
    Cancelled {
        status: InteractionStatus,
        reason: InteractionCancelReason,
    },
    /// A resize gesture began and replaced any previous gesture.
    ResizeBegan {
        session: ResizeSessionId,
        replaced: Option<InteractionStatus>,
    },
    /// The resize proposal was prevalidated and stored for painting.
    ResizeUpdated {
        session: ResizeSessionId,
        weights: Vec<SplitWeight>,
    },
    /// The first authoritative matching resize release committed exactly once.
    ResizeDelivered {
        session: ResizeSessionId,
        outcome: CommandOutcome,
        changed: bool,
    },
    /// One scene-proof-bearing programmatic contained placement committed.
    ContainedPlacementApplied {
        /// Checked workspace command result.
        outcome: CommandOutcome,
        /// Whether durable workspace state changed.
        changed: bool,
    },
    /// A contained transform began and replaced any previous gesture.
    ContainedTransformBegan {
        session: ContainedTransformSessionId,
        replaced: Option<InteractionStatus>,
    },
    /// The core published an exact contained transform rectangle.
    ContainedTransformPreviewUpdated {
        session: ContainedTransformSessionId,
        preview: ContainedTransformPreview,
    },
    /// The exact current contained transform preview was acknowledged.
    ContainedTransformPreviewAcknowledged {
        session: ContainedTransformSessionId,
        changed: bool,
    },
    /// The first authoritative matching transform release committed exactly once.
    ContainedTransformDelivered {
        session: ContainedTransformSessionId,
        outcome: CommandOutcome,
        changed: bool,
    },
    /// A semantic input was consumed without mutating published interaction state.
    Rejected(InteractionRejection),
}

/// Committed interaction event generated only after the engine publishes.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionEvent {
    input: InputSequence,
    version: WorkspaceVersion,
    kind: InteractionEventKind,
}

impl InteractionEvent {
    pub(crate) const fn new(
        input: InputSequence,
        version: WorkspaceVersion,
        kind: InteractionEventKind,
    ) -> Self {
        Self {
            input,
            version,
            kind,
        }
    }

    /// Returns the input which caused this published event.
    #[must_use]
    pub const fn input(&self) -> InputSequence {
        self.input
    }

    /// Returns the engine version published with this event.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the semantic event payload.
    #[must_use]
    pub const fn kind(&self) -> &InteractionEventKind {
        &self.kind
    }
}

/// Published interaction event payload.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionEventKind {
    /// One preview publication replaced the previous preview.
    PreviewPublished { preview: InteractionPreview },
    /// One gesture was explicitly cancelled or invalidated.
    Cancelled {
        status: InteractionStatus,
        reason: InteractionCancelReason,
    },
    /// One exact workspace delivery committed.
    Delivered {
        session: DragSessionId,
        kind: WorkspaceDeliveryKind,
    },
    /// One splitter resize committed.
    ResizeDelivered { session: ResizeSessionId },
    /// One contained transform preview publication replaced the previous one.
    ContainedTransformPreviewPublished { preview: ContainedTransformPreview },
    /// One contained transform committed its exact painted rectangle.
    ContainedTransformDelivered {
        session: ContainedTransformSessionId,
    },
    /// A native create saga entered the effect ledger without moving source content.
    NativeTearOffRequested(NativeCreateRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewProof {
    Dock {
        target: DropTargetId,
        command: WorkspaceCommand,
    },
    Contained {
        command: WorkspaceCommand,
        request: TearOffRequest,
        fallback: bool,
    },
    Native {
        command: WorkspaceCommand,
        request: TearOffRequest,
        proposal: Box<NativeTearOffProposal>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublishedPreview {
    public: InteractionPreview,
    proof: PreviewProof,
    painted: bool,
}

impl PublishedPreview {
    pub(crate) const fn public(&self) -> &InteractionPreview {
        &self.public
    }

    pub(crate) const fn proof(&self) -> &PreviewProof {
        &self.proof
    }

    pub(crate) const fn painted(&self) -> bool {
        self.painted
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ArmedDrag {
    pub(crate) session: DragSessionId,
    pub(crate) pointer: PointerId,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveDrag {
    pub(crate) session: DragSessionId,
    pub(crate) pointer: PointerId,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) target: Option<TargetAuthority>,
    pub(crate) tear_off: Option<TearOffRequest>,
    pub(crate) affordance: Option<DropAffordance>,
    pub(crate) preview: Option<PublishedPreview>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveResize {
    pub(crate) session: ResizeSessionId,
    pub(crate) pointer: PointerId,
    pub(crate) button: PointerButton,
    pub(crate) split: NodeSource,
    pub(crate) weights: Option<Vec<SplitWeight>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublishedContainedTransformPreview {
    public: ContainedTransformPreview,
    placement: ContainedPlacementProof,
    painted: bool,
}

impl PublishedContainedTransformPreview {
    pub(crate) const fn public(&self) -> &ContainedTransformPreview {
        &self.public
    }

    pub(crate) const fn placement(&self) -> ContainedPlacementProof {
        self.placement
    }

    pub(crate) const fn painted(&self) -> bool {
        self.painted
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveContainedTransform {
    pub(crate) session: ContainedTransformSessionId,
    pub(crate) pointer: PointerId,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) current_pointer: LogicalPoint,
    pub(crate) kind: ContainedTransformKind,
    pub(crate) minimum_size: LogicalSize,
    pub(crate) preview: Option<PublishedContainedTransformPreview>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedTransformStart {
    pub(crate) pointer: PointerId,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) kind: ContainedTransformKind,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Debug, Clone, PartialEq)]
enum ActiveGesture {
    Idle,
    Armed(Box<ArmedDrag>),
    Dragging(Box<ActiveDrag>),
    Resizing(Box<ActiveResize>),
    ContainedTransforming(Box<ActiveContainedTransform>),
}

/// Core-owned transient interaction state.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionState {
    active: ActiveGesture,
    last_drag_generation: DragGeneration,
    last_resize_generation: ResizeGeneration,
    last_contained_transform_generation: ContainedTransformGeneration,
    last_preview_sequence: PreviewSequence,
    last_consumed_drag: Option<DragSessionId>,
    last_consumed_contained_transform: Option<ContainedTransformSessionId>,
}

impl InteractionState {
    /// Returns the current public gesture state.
    #[must_use]
    pub const fn status(&self) -> InteractionStatus {
        match &self.active {
            ActiveGesture::Idle => InteractionStatus::Idle,
            ActiveGesture::Armed(drag) => InteractionStatus::Armed {
                session: drag.session,
            },
            ActiveGesture::Dragging(drag) => InteractionStatus::Dragging {
                session: drag.session,
            },
            ActiveGesture::Resizing(resize) => InteractionStatus::Resizing {
                session: resize.session,
            },
            ActiveGesture::ContainedTransforming(transform) => {
                InteractionStatus::ContainedTransforming {
                    session: transform.session,
                }
            }
        }
    }

    /// Returns a read-only view of the current armed or active drag.
    #[must_use]
    pub const fn active_drag_view(&self) -> Option<ActiveDragView<'_>> {
        match &self.active {
            ActiveGesture::Armed(drag) => Some(ActiveDragView {
                phase: DragPhase::Armed,
                session: drag.session,
                pointer: drag.pointer,
                button: drag.button,
                payload: &drag.payload,
                target: None,
                tear_off: None,
            }),
            ActiveGesture::Dragging(drag) => Some(ActiveDragView {
                phase: DragPhase::Dragging,
                session: drag.session,
                pointer: drag.pointer,
                button: drag.button,
                payload: &drag.payload,
                target: drag.target.as_ref(),
                tear_off: drag.tear_off.as_ref(),
            }),
            ActiveGesture::Idle
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    /// Returns a read-only view of the core-owned contained transform.
    #[must_use]
    pub fn active_contained_transform_view(&self) -> Option<ActiveContainedTransformView<'_>> {
        match &self.active {
            ActiveGesture::ContainedTransforming(transform) => Some(ActiveContainedTransformView {
                session: transform.session,
                pointer: transform.pointer,
                button: transform.button,
                surface: transform.surface,
                root: transform.root,
                floating: transform.floating,
                source_rect: transform.source_rect,
                initial_pointer: transform.initial_pointer,
                current_pointer: transform.current_pointer,
                kind: transform.kind,
                minimum_size: transform.minimum_size,
                preview: transform.preview.as_ref().map(|preview| &preview.public),
            }),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_) => None,
        }
    }

    /// Returns the exact contained transform preview eligible for painting.
    #[must_use]
    pub fn contained_transform_preview(&self) -> Option<&ContainedTransformPreview> {
        match &self.active {
            ActiveGesture::ContainedTransforming(transform) => transform
                .preview
                .as_ref()
                .map(PublishedContainedTransformPreview::public),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_) => None,
        }
    }

    /// Returns the exact preview currently eligible for painting.
    #[must_use]
    pub fn preview(&self) -> Option<&InteractionPreview> {
        match &self.active {
            ActiveGesture::Dragging(drag) => drag.preview.as_ref().map(PublishedPreview::public),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    /// Returns the complete docking-guide affordance for the active drag.
    ///
    /// Affordance is independent from [`Self::preview`]: it remains available
    /// while a guide cluster is visible but no exact deliverable button is hit.
    /// It never acts as paint acknowledgement or delivery proof.
    #[must_use]
    pub fn drop_affordance(&self) -> Option<&DropAffordance> {
        match &self.active {
            ActiveGesture::Dragging(drag) => drag.affordance.as_ref(),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    /// Returns the validated resize override eligible for transient projection.
    #[must_use]
    pub fn resize_weights(&self) -> Option<(ResizeSessionId, &NodeSource, &[SplitWeight])> {
        match &self.active {
            ActiveGesture::Resizing(resize) => resize
                .weights
                .as_deref()
                .map(|weights| (resize.session, &resize.split, weights)),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    pub(crate) fn arm_drag(
        &mut self,
        epoch: WorkspaceEpoch,
        pointer: PointerId,
        button: PointerButton,
        payload: MovePayload,
    ) -> Result<(DragSessionId, Option<InteractionStatus>), InteractionCounterError> {
        let generation = self
            .last_drag_generation
            .checked_next()
            .ok_or(InteractionCounterError::DragGenerationExhausted)?;
        self.last_drag_generation = generation;
        let session = DragSessionId::new(epoch, generation);
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::Armed(Box::new(ArmedDrag {
            session,
            pointer,
            button,
            payload,
        }));
        Ok((session, replaced))
    }

    pub(crate) fn begin_drag(
        &mut self,
        session: DragSessionId,
        pointer: PointerId,
        button: PointerButton,
    ) -> Result<(), InteractionRejection> {
        let ActiveGesture::Armed(armed) = &self.active else {
            return Err(self.session_rejection(session));
        };
        if armed.session != session {
            return Err(InteractionRejection::SessionMismatch);
        }
        if armed.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if armed.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        self.active = ActiveGesture::Dragging(Box::new(ActiveDrag {
            session: armed.session,
            pointer: armed.pointer,
            button: armed.button,
            payload: armed.payload.clone(),
            target: None,
            tear_off: None,
            affordance: None,
            preview: None,
        }));
        Ok(())
    }

    pub(crate) fn active_drag(
        &self,
        session: DragSessionId,
    ) -> Result<&ActiveDrag, InteractionRejection> {
        match &self.active {
            ActiveGesture::Dragging(drag) if drag.session == session => Ok(drag),
            ActiveGesture::Armed(armed) if armed.session == session => {
                Err(InteractionRejection::DragNotBegun)
            }
            _ => Err(self.session_rejection(session)),
        }
    }

    pub(crate) fn armed_drag(
        &self,
        session: DragSessionId,
    ) -> Result<&ArmedDrag, InteractionRejection> {
        match &self.active {
            ActiveGesture::Armed(armed) if armed.session == session => Ok(armed),
            _ => Err(self.session_rejection(session)),
        }
    }

    pub(crate) fn active_drag_mut(
        &mut self,
        session: DragSessionId,
    ) -> Result<&mut ActiveDrag, InteractionRejection> {
        let rejection = self.session_rejection(session);
        match &mut self.active {
            ActiveGesture::Dragging(drag) if drag.session == session => Ok(drag),
            ActiveGesture::Armed(armed) if armed.session == session => {
                Err(InteractionRejection::DragNotBegun)
            }
            _ => Err(rejection),
        }
    }

    pub(crate) fn publish_preview(
        &mut self,
        session: DragSessionId,
        scene: SceneStamp,
        visual: PreviewVisual,
        proof: PreviewProof,
    ) -> Result<(InteractionPreview, bool), InteractionCounterError> {
        if let Ok(drag) = self.active_drag(session)
            && let Some(existing) = &drag.preview
            && existing.public.token.scene == scene
            && existing.public.visual == visual
            && existing.proof == proof
        {
            return Ok((existing.public.clone(), false));
        }
        let sequence = self
            .last_preview_sequence
            .checked_next()
            .ok_or(InteractionCounterError::PreviewSequenceExhausted)?;
        let preview = InteractionPreview {
            token: PreviewToken {
                session,
                scene,
                sequence,
            },
            visual,
        };
        self.last_preview_sequence = sequence;
        let drag = self
            .active_drag_mut(session)
            .map_err(|_| InteractionCounterError::StateInvariant)?;
        drag.preview = Some(PublishedPreview {
            public: preview.clone(),
            proof,
            painted: false,
        });
        Ok((preview, true))
    }

    pub(crate) fn clear_preview(
        &mut self,
        session: DragSessionId,
    ) -> Result<bool, InteractionRejection> {
        Ok(self.active_drag_mut(session)?.preview.take().is_some())
    }

    pub(crate) fn set_drop_affordance(
        &mut self,
        session: DragSessionId,
        affordance: Option<DropAffordance>,
    ) -> Result<(), InteractionRejection> {
        self.active_drag_mut(session)?.affordance = affordance;
        Ok(())
    }

    pub(crate) fn acknowledge_preview(
        &mut self,
        acknowledgement: &PaintAcknowledgement,
    ) -> Result<(DragSessionId, bool), InteractionRejection> {
        let session = acknowledgement.token.session;
        let drag = self.active_drag_mut(session)?;
        let Some(preview) = drag.preview.as_mut() else {
            return Err(InteractionRejection::PreviewAcknowledgementMismatch);
        };
        if preview.public.token != acknowledgement.token
            || preview.public.visual != acknowledgement.visual
        {
            return Err(InteractionRejection::PreviewAcknowledgementMismatch);
        }
        let changed = !preview.painted;
        preview.painted = true;
        Ok((session, changed))
    }

    pub(crate) fn set_drag_observation(
        &mut self,
        session: DragSessionId,
        target: TargetAuthority,
        tear_off: Option<TearOffRequest>,
    ) -> Result<(), InteractionRejection> {
        let drag = self.active_drag_mut(session)?;
        drag.target = Some(target);
        drag.tear_off = tear_off;
        Ok(())
    }

    pub(crate) fn take_drag_for_release(
        &mut self,
        session: DragSessionId,
        pointer: PointerId,
        button: PointerButton,
    ) -> Result<ActiveDrag, InteractionRejection> {
        let drag = self.active_drag(session)?;
        if drag.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if drag.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        let ActiveGesture::Dragging(drag) =
            std::mem::replace(&mut self.active, ActiveGesture::Idle)
        else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        self.last_consumed_drag = Some(session);
        Ok(*drag)
    }

    pub(crate) fn begin_resize(
        &mut self,
        epoch: WorkspaceEpoch,
        pointer: PointerId,
        button: PointerButton,
        split: NodeSource,
    ) -> Result<(ResizeSessionId, Option<InteractionStatus>), InteractionCounterError> {
        let generation = self
            .last_resize_generation
            .checked_next()
            .ok_or(InteractionCounterError::ResizeGenerationExhausted)?;
        self.last_resize_generation = generation;
        let session = ResizeSessionId::new(epoch, generation);
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::Resizing(Box::new(ActiveResize {
            session,
            pointer,
            button,
            split,
            weights: None,
        }));
        Ok((session, replaced))
    }

    pub(crate) fn active_resize(
        &self,
        session: ResizeSessionId,
    ) -> Result<&ActiveResize, InteractionRejection> {
        match &self.active {
            ActiveGesture::Resizing(resize) if resize.session == session => Ok(resize),
            ActiveGesture::Idle => Err(InteractionRejection::NoActiveGesture),
            ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => Err(InteractionRejection::SessionMismatch),
        }
    }

    pub(crate) fn set_resize_weights(
        &mut self,
        session: ResizeSessionId,
        weights: Vec<SplitWeight>,
    ) -> Result<(), InteractionRejection> {
        match &mut self.active {
            ActiveGesture::Resizing(resize) if resize.session == session => {
                resize.weights = Some(weights);
                Ok(())
            }
            ActiveGesture::Idle => Err(InteractionRejection::NoActiveGesture),
            ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => Err(InteractionRejection::SessionMismatch),
        }
    }

    pub(crate) fn take_resize_for_release(
        &mut self,
        session: ResizeSessionId,
        pointer: PointerId,
        button: PointerButton,
    ) -> Result<ActiveResize, InteractionRejection> {
        let resize = self.active_resize(session)?;
        if resize.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if resize.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        let ActiveGesture::Resizing(resize) =
            std::mem::replace(&mut self.active, ActiveGesture::Idle)
        else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        Ok(*resize)
    }

    pub(crate) fn begin_contained_transform(
        &mut self,
        epoch: WorkspaceEpoch,
        start: ContainedTransformStart,
    ) -> Result<(ContainedTransformSessionId, Option<InteractionStatus>), InteractionCounterError>
    {
        let generation = self
            .last_contained_transform_generation
            .checked_next()
            .ok_or(InteractionCounterError::ContainedTransformGenerationExhausted)?;
        self.last_contained_transform_generation = generation;
        let session = ContainedTransformSessionId::new(epoch, generation);
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::ContainedTransforming(Box::new(ActiveContainedTransform {
            session,
            pointer: start.pointer,
            button: start.button,
            surface: start.surface,
            root: start.root,
            floating: start.floating,
            source_rect: start.source_rect,
            initial_pointer: start.initial_pointer,
            current_pointer: start.initial_pointer,
            kind: start.kind,
            minimum_size: start.minimum_size,
            preview: None,
        }));
        Ok((session, replaced))
    }

    pub(crate) fn active_contained_transform(
        &self,
        session: ContainedTransformSessionId,
    ) -> Result<&ActiveContainedTransform, InteractionRejection> {
        match &self.active {
            ActiveGesture::ContainedTransforming(transform) if transform.session == session => {
                Ok(transform)
            }
            _ => Err(self.contained_transform_session_rejection(session)),
        }
    }

    pub(crate) fn active_contained_transform_mut(
        &mut self,
        session: ContainedTransformSessionId,
    ) -> Result<&mut ActiveContainedTransform, InteractionRejection> {
        let rejection = self.contained_transform_session_rejection(session);
        match &mut self.active {
            ActiveGesture::ContainedTransforming(transform) if transform.session == session => {
                Ok(transform)
            }
            _ => Err(rejection),
        }
    }

    pub(crate) fn set_contained_transform_pointer(
        &mut self,
        session: ContainedTransformSessionId,
        current_pointer: LogicalPoint,
    ) -> Result<(), InteractionRejection> {
        self.active_contained_transform_mut(session)?
            .current_pointer = current_pointer;
        Ok(())
    }

    pub(crate) fn publish_contained_transform_preview(
        &mut self,
        session: ContainedTransformSessionId,
        scene: SceneStamp,
        placement: ContainedPlacementProof,
    ) -> Result<(ContainedTransformPreview, bool), InteractionCounterError> {
        if let Ok(transform) = self.active_contained_transform(session)
            && let Some(existing) = &transform.preview
            && existing.public.token.scene == scene
            && existing.public.rect == placement.clamped_rect()
            && existing.placement == placement
        {
            return Ok((existing.public, false));
        }
        let sequence = self
            .last_preview_sequence
            .checked_next()
            .ok_or(InteractionCounterError::PreviewSequenceExhausted)?;
        let transform = self
            .active_contained_transform(session)
            .map_err(|_| InteractionCounterError::StateInvariant)?;
        let preview = ContainedTransformPreview {
            token: ContainedTransformPreviewToken {
                session,
                scene,
                sequence,
            },
            surface: transform.surface,
            root: transform.root,
            floating: transform.floating,
            rect: placement.clamped_rect(),
        };
        self.last_preview_sequence = sequence;
        let transform = self
            .active_contained_transform_mut(session)
            .map_err(|_| InteractionCounterError::StateInvariant)?;
        transform.preview = Some(PublishedContainedTransformPreview {
            public: preview,
            placement,
            painted: false,
        });
        Ok((preview, true))
    }

    pub(crate) fn acknowledge_contained_transform_preview(
        &mut self,
        acknowledgement: ContainedTransformPaintAcknowledgement,
    ) -> Result<(ContainedTransformSessionId, bool), InteractionRejection> {
        let session = acknowledgement.token.session;
        let transform = self.active_contained_transform_mut(session)?;
        let Some(preview) = transform.preview.as_mut() else {
            return Err(InteractionRejection::ContainedTransformPreviewAcknowledgementMismatch);
        };
        if preview.public.acknowledgement() != acknowledgement {
            return Err(InteractionRejection::ContainedTransformPreviewAcknowledgementMismatch);
        }
        let changed = !preview.painted;
        preview.painted = true;
        Ok((session, changed))
    }

    pub(crate) fn take_contained_transform_for_release(
        &mut self,
        session: ContainedTransformSessionId,
        pointer: PointerId,
        button: PointerButton,
    ) -> Result<ActiveContainedTransform, InteractionRejection> {
        let transform = self.active_contained_transform(session)?;
        if transform.pointer != pointer {
            return Err(InteractionRejection::PointerMismatch);
        }
        if transform.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        let ActiveGesture::ContainedTransforming(transform) =
            std::mem::replace(&mut self.active, ActiveGesture::Idle)
        else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        self.last_consumed_contained_transform = Some(session);
        Ok(*transform)
    }

    pub(crate) fn cancel_drag(
        &mut self,
        session: DragSessionId,
    ) -> Result<InteractionStatus, InteractionRejection> {
        match &self.active {
            ActiveGesture::Armed(armed) if armed.session == session => {}
            ActiveGesture::Dragging(drag) if drag.session == session => {}
            _ => return Err(self.session_rejection(session)),
        }
        let status = self.status();
        self.active = ActiveGesture::Idle;
        Ok(status)
    }

    pub(crate) fn cancel_resize(
        &mut self,
        session: ResizeSessionId,
    ) -> Result<InteractionStatus, InteractionRejection> {
        self.active_resize(session)?;
        let status = self.status();
        self.active = ActiveGesture::Idle;
        Ok(status)
    }

    pub(crate) fn cancel_contained_transform(
        &mut self,
        session: ContainedTransformSessionId,
    ) -> Result<InteractionStatus, InteractionRejection> {
        self.active_contained_transform(session)?;
        let status = self.status();
        self.active = ActiveGesture::Idle;
        Ok(status)
    }

    pub(crate) fn cancel_active(&mut self) -> Option<InteractionStatus> {
        let status = self.status();
        if status == InteractionStatus::Idle {
            None
        } else {
            self.active = ActiveGesture::Idle;
            Some(status)
        }
    }

    #[cfg(test)]
    pub(crate) fn exhaust_drag_generation(&mut self) {
        self.last_drag_generation = DragGeneration::new(u64::MAX);
    }

    #[cfg(test)]
    pub(crate) fn exhaust_preview_sequence(&mut self) {
        self.last_preview_sequence = PreviewSequence::new(u64::MAX);
    }

    fn session_rejection(&self, session: DragSessionId) -> InteractionRejection {
        if self.last_consumed_drag == Some(session) {
            InteractionRejection::SessionConsumed { session }
        } else if self.status() == InteractionStatus::Idle {
            InteractionRejection::NoActiveGesture
        } else {
            InteractionRejection::SessionMismatch
        }
    }

    fn contained_transform_session_rejection(
        &self,
        session: ContainedTransformSessionId,
    ) -> InteractionRejection {
        if self.last_consumed_contained_transform == Some(session) {
            InteractionRejection::ContainedTransformSessionConsumed { session }
        } else if self.status() == InteractionStatus::Idle {
            InteractionRejection::NoActiveGesture
        } else {
            InteractionRejection::SessionMismatch
        }
    }
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            active: ActiveGesture::Idle,
            last_drag_generation: DragGeneration::default(),
            last_resize_generation: ResizeGeneration::default(),
            last_contained_transform_generation: ContainedTransformGeneration::default(),
            last_preview_sequence: PreviewSequence::default(),
            last_consumed_drag: None,
            last_consumed_contained_transform: None,
        }
    }
}

/// Fatal exhaustion or an impossible internal interaction transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum InteractionCounterError {
    /// Drag generations cannot advance without wrapping.
    #[error("drag session generation is exhausted")]
    DragGenerationExhausted,
    /// Resize generations cannot advance without wrapping.
    #[error("resize session generation is exhausted")]
    ResizeGenerationExhausted,
    /// Contained transform generations cannot advance without wrapping.
    #[error("contained transform session generation is exhausted")]
    ContainedTransformGenerationExhausted,
    /// Preview identities cannot advance without wrapping.
    #[error("preview sequence is exhausted")]
    PreviewSequenceExhausted,
    /// Internal code attempted to publish a preview outside its live drag.
    #[error("interaction state invariant failed while publishing a preview")]
    StateInvariant,
}
