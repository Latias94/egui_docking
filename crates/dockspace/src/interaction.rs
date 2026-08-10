//! Explicit drag and resize session state.

use std::sync::Arc;

use thiserror::Error;

use crate::close_plan::{CloseItemRequirement, ClosePlan, ClosePlanTarget};
use crate::command::{
    CommandOutcome, ContainedRosterSource, MovePayload, NodeSource, SplitResize, WorkspaceCommand,
};
use crate::drop_resolver::DropAffordance;
use crate::drop_target::DropTargetId;
use crate::event::{PendingReductionCause, ReductionCause};
use crate::frame::NativeCreateRequest;
use crate::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use crate::graph::Axis;
use crate::ids::{
    FloatingPresentationId, InputSequence, ItemId, RootId, SurfaceId, WorkspaceEpoch,
};
use crate::intent::{
    Authority, CloseSceneTarget, ContainedGestureKind, ContainedPresentationOffer,
    ContainedTearOffProposal, ContainedTransformKind, NativePresentationOffer,
    NativeTearOffProposal, PointerButton, PointerId, SurfaceBackgroundRootOffer, TabGestureSource,
};
use crate::operation::PreparedContentClose;
use crate::pointer_journal::{
    PointerCaptureOwner, PointerEventDeliveryOwner, PointerStreamId, ScrollCancelReason,
    ScrollPhase, ScrollSequenceToken,
};
use crate::policy::PolicyRevision;
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_hit::PresentationHitRegionId;
use crate::presentation_observation::PresentedSurfaceAuthority;
use crate::scene::{
    PopupInteractionGateRevision, PresentationLayoutFacts, SplitterRecord, SplitterResizeTarget,
    SurfaceCoordinateCapture, SurfaceSceneStamp, TabBarSceneId, TabListMenuBackdropRecord,
    TabListMenuRecord, TabListMenuRowRecord, TabSceneId, TabStripControlRecord,
};
use crate::scene_manifest::{RequirementRevision, SurfaceMeasurementTicket};
use crate::surface_recovery::SurfaceRecoveryObligation;
use crate::tab_strip::{
    PopupRoutingRevision, TabListMenuSessionId, TabStripControlId, TabStripStateKey,
};
use crate::transition::WorkspaceVersion;
use crate::viewport::ViewportBinding;
use crate::viewport_focus::{FocusCausalStamp, PaneFocusDisposition};

macro_rules! interaction_counter {
    ($constructor_visibility:vis $name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a counter value from its runtime representation.
            #[must_use]
            $constructor_visibility const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the runtime representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            pub(crate) const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self::new(value)),
                    None => None,
                }
            }
        }
    };
}

interaction_counter!(pub DragGeneration, "Monotonic generation of drag sessions.");
interaction_counter!(
    pub(crate) ClickGeneration,
    "Monotonic generation of click sessions."
);
interaction_counter!(
    pub ResizeGeneration,
    "Monotonic generation of splitter-resize sessions."
);
interaction_counter!(
    pub(crate) ScrollSessionId,
    "Core-owned identity of one admitted smooth-scroll session."
);

/// Identity of one press/release click session within a workspace epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClickSessionId {
    epoch: WorkspaceEpoch,
    generation: ClickGeneration,
}

impl ClickSessionId {
    pub(crate) const fn new(epoch: WorkspaceEpoch, generation: ClickGeneration) -> Self {
        Self { epoch, generation }
    }

    /// Returns the workspace epoch in which the click was pressed.
    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    /// Returns the click generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation.get()
    }
}
interaction_counter!(
    pub ContainedTransformGeneration,
    "Monotonic generation of contained transform sessions."
);
interaction_counter!(
    pub PreviewSequence,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PreviewToken {
    session: DragSessionId,
    scene: SurfaceSceneStamp,
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
    pub const fn scene(self) -> SurfaceSceneStamp {
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
    /// Show a source-hosted cue for a future native-surface placement.
    Native {
        /// Existing surface whose presented output carries this cue.
        ///
        /// The prospective native target has no presentation stream before its
        /// lifecycle starts, so it cannot acknowledge a preview. Keeping this
        /// separate from [`Self::target_surface`] makes that boundary explicit.
        host_surface: SurfaceId,
        /// Reserved logical surface which the later native lifecycle may create.
        target_surface: SurfaceId,
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
            | Self::Native {
                host_surface: surface,
                ..
            } => surface,
        }
    }

    /// Returns the reserved native target when this is a native tear-off cue.
    #[must_use]
    pub const fn target_surface(&self) -> Option<SurfaceId> {
        match *self {
            Self::Native { target_surface, .. } => Some(target_surface),
            Self::Dock { .. } | Self::Contained { .. } => None,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContainedTransformPreviewToken {
    session: ContainedTransformSessionId,
    scene: SurfaceSceneStamp,
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
    pub const fn scene(self) -> SurfaceSceneStamp {
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
    /// A Click-lane receiver was pressed and awaits its matching release.
    Pressed { session: ClickSessionId },
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

/// Authoritative endpoint which delivered one Escape key press.
///
/// Surface-local immediate-mode adapters may only cancel a gesture from its
/// frozen source surface. Native event loops instead name the exact current
/// window binding which received the key edge, allowing a target window to
/// cancel a cross-window docking gesture without guessing its source callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeDelivery {
    /// Escape was delivered by one logical surface callback.
    Surface(SurfaceId),
    /// Escape was delivered by one exact native window incarnation.
    NativeBinding(ViewportBinding),
}

/// Core-private close action frozen by an authoritative Click-lane press.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenCloseClick {
    pub(crate) target: ClosePlanTarget,
    pub(crate) requirements: Vec<CloseItemRequirement>,
    pub(crate) prepared: PreparedContentClose,
}

/// Exact tab-strip control action frozen by an authoritative Click-lane press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenTabStripControlClick {
    pub(crate) key: TabStripStateKey,
    pub(crate) record: TabStripControlRecord,
}

/// Exact menu-row action frozen by an authoritative Click-lane press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenTabListMenuRowClick {
    pub(crate) session: TabListMenuSessionId,
    pub(crate) record: TabListMenuRowRecord,
    pub(crate) revision: PopupRoutingRevision,
}

/// Exact menu-frame blocker action frozen by an authoritative Click-lane press.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenTabListMenuBlockerClick {
    pub(crate) session: TabListMenuSessionId,
    pub(crate) record: TabListMenuRecord,
    pub(crate) revision: PopupRoutingRevision,
}

/// Exact popup-backdrop dismissal frozen by an authoritative Click-lane press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenTabListMenuBackdropClick {
    pub(crate) session: TabListMenuSessionId,
    pub(crate) record: TabListMenuBackdropRecord,
    pub(crate) revision: PopupRoutingRevision,
}

/// Immutable semantic action owned by one press/release click session.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrozenClickAction {
    Close(FrozenCloseClick),
    TabStripControl(FrozenTabStripControlClick),
    TabListMenuRow(FrozenTabListMenuRowClick),
    TabListMenuBlocker(FrozenTabListMenuBlockerClick),
    TabListMenuBackdrop(FrozenTabListMenuBackdropClick),
}

/// Exact global presentation epoch and surface proof which authorized a gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrozenPresentationAuthority {
    pub(crate) presented: PresentedSurfaceAuthority,
    pub(crate) popup_gate_revision: PopupInteractionGateRevision,
}

impl FrozenPresentationAuthority {
    pub(crate) const fn new(
        presented: PresentedSurfaceAuthority,
        popup_gate_revision: PopupInteractionGateRevision,
    ) -> Self {
        Self {
            presented,
            popup_gate_revision,
        }
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.presented.surface()
    }
}

/// Exact source facts retained across a presentation invalidation caused by
/// activating the same gesture.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SceneGestureContinuationSource {
    Drag {
        payload: MovePayload,
        source_surface: SurfaceId,
        complete_root: Option<NodeSource>,
        origin: FrozenDragOrigin,
        coordinates: SurfaceCoordinateCapture,
    },
    ContainedTransform {
        source: NodeSource,
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        source_rect: LogicalRect,
        expected_roster: ContainedRosterSource,
        coordinates: SurfaceCoordinateCapture,
    },
}

/// Core-owned continuation facts captured after an activation mutation and
/// bound to the newly minted gesture session by [`InteractionState`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneGestureContinuationDraft {
    pub(crate) activation: ReductionCause,
    pub(crate) owner: GestureOwner,
    pub(crate) origin: FrozenPresentationAuthority,
    pub(crate) workspace: WorkspaceVersion,
    pub(crate) policy: PolicyRevision,
    pub(crate) config: PresentationConfigRevision,
    pub(crate) requirements: RequirementRevision,
    pub(crate) popup_routing: PopupRoutingRevision,
    pub(crate) source: SceneGestureContinuationSource,
}

/// Exact active session to which one continuation belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SceneGestureSession {
    Drag(DragSessionId),
    ContainedTransform(ContainedTransformSessionId),
}

/// Unforgeable core-owned capability permitting only the source gesture to
/// survive a transient global presentation-gate loss while its own semantic
/// source and coordinate facts remain current.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SceneGestureContinuation {
    pub(crate) session: SceneGestureSession,
    pub(crate) draft: SceneGestureContinuationDraft,
}

impl SceneGestureContinuationDraft {
    fn bind(self, session: SceneGestureSession) -> SceneGestureContinuation {
        SceneGestureContinuation {
            session,
            draft: self,
        }
    }
}

/// Exact facts required to arm one journal-owned click.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClickStart {
    pub(crate) epoch: WorkspaceEpoch,
    pub(crate) owner: GestureOwner,
    pub(crate) capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) region: PresentationHitRegionId,
    pub(crate) measurement: SurfaceMeasurementTicket,
    pub(crate) coordinates: SurfaceCoordinateCapture,
    pub(crate) presentation: FrozenPresentationAuthority,
    pub(crate) action: Arc<FrozenClickAction>,
}

/// Core-private active press/release click state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveClick {
    pub(crate) session: ClickSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) region: PresentationHitRegionId,
    pub(crate) measurement: SurfaceMeasurementTicket,
    pub(crate) coordinates: SurfaceCoordinateCapture,
    pub(crate) presentation: FrozenPresentationAuthority,
    pub(crate) action: Arc<FrozenClickAction>,
}

/// Read-only view of one pressed Click-lane receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveClickView {
    session: ClickSessionId,
    region: PresentationHitRegionId,
}

impl ActiveClickView {
    /// Returns the core-minted click session.
    #[must_use]
    pub const fn session(self) -> ClickSessionId {
        self.session
    }

    /// Returns the exact stable receiver pressed by the owning stream.
    #[must_use]
    pub const fn region(self) -> PresentationHitRegionId {
        self.region
    }

    /// Returns the logical surface owning the pressed receiver.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.region.surface()
    }
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
    owner: GestureOwner,
    button: PointerButton,
    payload: &'state MovePayload,
    contained_offer: Option<&'state ContainedPresentationOffer>,
    surface_background_offer: Option<&'state SurfaceBackgroundRootOffer>,
    native_offer: Option<&'state NativePresentationOffer>,
}

/// Read-only view of one scene-bound splitter resize session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveResizeView<'state> {
    session: ResizeSessionId,
    owner: GestureOwner,
    button: PointerButton,
    surface: SurfaceId,
    scene: SurfaceSceneStamp,
    target: SplitterResizeTarget,
    updates: &'state [SplitResize],
}

impl<'state> ActiveResizeView<'state> {
    /// Returns the exact resize generation.
    #[must_use]
    pub const fn session(self) -> ResizeSessionId {
        self.session
    }

    /// Returns the physical pointer which owns this resize, when journal-backed.
    #[must_use]
    pub const fn pointer(self) -> Option<PointerId> {
        self.owner.pointer_if_physical()
    }

    /// Returns the exact journal stream when this resize came from the
    /// pointer-journal protocol.
    #[must_use]
    pub const fn journal_stream(self) -> Option<PointerStreamId> {
        self.owner.stream()
    }

    /// Returns the button which owns this resize.
    #[must_use]
    pub const fn button(self) -> PointerButton {
        self.button
    }

    /// Returns the frozen logical owner surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the painted scene which authorized activation.
    #[must_use]
    pub const fn scene(self) -> SurfaceSceneStamp {
        self.scene
    }

    /// Returns the exact structural splitter target frozen at press time.
    #[must_use]
    pub const fn target(self) -> SplitterResizeTarget {
        self.target
    }

    /// Returns the local-response owner surface when no pointer provider owns the gesture.
    #[must_use]
    pub const fn local_response_surface(self) -> Option<SurfaceId> {
        match self.owner {
            GestureOwner::Stream(_) => None,
            GestureOwner::LocalResponse { surface } => Some(surface),
        }
    }

    /// Returns the latest validated atomic transient updates.
    #[must_use]
    pub const fn updates(self) -> &'state [SplitResize] {
        self.updates
    }
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

    /// Returns the physical pointer which owns the drag, when journal-backed.
    #[must_use]
    pub const fn pointer(self) -> Option<PointerId> {
        self.owner.pointer_if_physical()
    }

    /// Returns the exact journal stream when this drag came from the
    /// pointer-journal protocol.
    #[must_use]
    pub const fn journal_stream(self) -> Option<PointerStreamId> {
        self.owner.stream()
    }

    /// Returns the local-response owner surface when no physical pointer
    /// provider owns this drag.
    #[must_use]
    pub const fn local_response_surface(self) -> Option<SurfaceId> {
        match self.owner {
            GestureOwner::Stream(_) => None,
            GestureOwner::LocalResponse { surface } => Some(surface),
        }
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

    /// Returns the first contained-presentation offer frozen for this session.
    #[must_use]
    pub const fn contained_offer(self) -> Option<&'state ContainedPresentationOffer> {
        self.contained_offer
    }

    /// Returns the first background-root identity offer frozen for this session.
    #[must_use]
    pub const fn surface_background_offer(self) -> Option<&'state SurfaceBackgroundRootOffer> {
        self.surface_background_offer
    }

    /// Returns the first native presentation offer frozen for this session.
    #[must_use]
    pub const fn native_offer(self) -> Option<&'state NativePresentationOffer> {
        self.native_offer
    }
}

/// Read-only view of the exact contained transform owned by the core.
///
/// Adapters can render and route the gesture from this view without keeping a
/// second copy of the frozen rectangle, pointer origin, or current preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveContainedTransformView<'state> {
    session: ContainedTransformSessionId,
    owner: GestureOwner,
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

    /// Returns the physical pointer which owns the transform, when journal-backed.
    #[must_use]
    pub const fn pointer(self) -> Option<PointerId> {
        self.owner.pointer_if_physical()
    }

    /// Returns the exact journal stream when this transform came from the
    /// pointer-journal protocol.
    #[must_use]
    pub const fn journal_stream(self) -> Option<PointerStreamId> {
        self.owner.stream()
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
    /// Pointer capture authority was unavailable at a terminal edge.
    CaptureAuthorityUnavailable,
    /// The edge-local delivery endpoint became unavailable while a gesture was active.
    DeliveryAuthorityUnavailable,
    /// An edge was delivered by an endpoint other than the gesture's frozen source.
    DeliveryOwnerLost,
    /// The platform explicitly terminated the owning pointer stream.
    PointerStreamCancelled,
    /// The current framework response explicitly cancelled its local gesture.
    LocalResponseCancelled,
    /// The provider reported the normal terminal release of an ephemeral stream.
    PointerStreamEnded,
    /// Button state became non-authoritative.
    UnknownButtonState,
    /// Hovered-target authority became unavailable.
    UnknownTargetAuthority,
    /// A foreign native window authoritatively blocked the pointer route.
    OpaquePointerBlocker,
    /// The primary release did not deliver to the exact pressed Click-lane receiver.
    ClickReceiverMismatch,
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
    /// The exact pointer-provider incarnation owning the gesture was retired.
    PointerProviderRetired,
    /// Another mutually exclusive gesture replaced this one.
    ReplacedByNewGesture,
    /// A successful workspace restore advanced the epoch.
    WorkspaceRestored,
}

/// Why a semantic interaction input did not advance or deliver a gesture.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionRejection {
    /// The exact surface has no currently presented semantic projection.
    SemanticPresentationUnavailable { surface: SurfaceId },
    /// The semantic event named an output other than the currently presented output.
    SemanticOutputUnavailable { surface: SurfaceId },
    /// The semantic event named an emission other than the currently presented emission.
    SemanticEmissionUnavailable { surface: SurfaceId },
    /// The event delivery endpoint does not own the named presented output.
    SemanticDeliveryMismatch {
        expected: crate::semantic_input::SemanticDelivery,
        actual: crate::presentation_observation::HostPresentationEndpoint,
    },
    /// The claimed receiver is absent from the exact presented hit manifest.
    SemanticReceiverUnavailable {
        target: crate::presentation_hit::PresentationHitRegionKind,
    },
    /// The requested semantic action is not defined for this receiver class.
    SemanticActionUnsupported {
        target: crate::presentation_hit::PresentationHitRegionKind,
        action: crate::semantic_input::SemanticReceiverAction,
    },
    /// The close intent named a target absent from the exact painted scene.
    CloseSceneTargetUnavailable { target: CloseSceneTarget },
    /// The exact painted target exposes no close control under frozen policy.
    CloseControlUnavailable { target: CloseSceneTarget },
    /// Pointer activation used a button which cannot activate a close control.
    CloseActivationButtonUnsupported { button: PointerButton },
    /// Pointer activation named a different surface than the painted scene.
    CloseActivationSurfaceMismatch {
        expected: SurfaceId,
        actual: SurfaceId,
    },
    /// Pointer activation missed the exact half-open close-control rectangle.
    CloseActivationHitMismatch { target: CloseSceneTarget },
    /// A structurally higher presentation layer owns the activation point.
    CloseActivationOccluded { target: CloseSceneTarget },
    /// The scene's coordinate capture no longer matches current platform authority.
    CloseCoordinateAuthorityUnavailable { surface: SurfaceId },
    /// Current topology cannot reproduce the source represented by the close control.
    CloseSourceUnavailable(crate::error::CommandError),
    /// The claimed tab-strip control is absent from the exact current plan.
    TabStripControlUnavailable { control: TabStripControlId },
    /// The exact current plan exposes this control only as a disabled blocker.
    TabStripControlDisabled { control: TabStripControlId },
    /// Pointer activation missed the exact tab-strip control rectangle.
    TabStripControlHitMismatch { control: TabStripControlId },
    /// The tab-strip control record changed after its press.
    TabStripControlRecordChanged { control: TabStripControlId },
    /// The control's structural tab strip is no longer live.
    TabStripSourceUnavailable { key: TabStripStateKey },
    /// The exact current tab bar does not admit semantic scrolling.
    TabStripScrollDisabled { key: TabStripStateKey },
    /// The requested reveal item is absent from the exact current tab bar.
    TabStripScrollItemUnavailable { key: TabStripStateKey, item: ItemId },
    /// The exact tab-bar geometry changed after the scroll proof was prepared.
    TabStripRecordChanged { key: TabStripStateKey },
    /// The claimed menu instance is no longer the sole active core-owned menu.
    TabListMenuSessionUnavailable { session: TabListMenuSessionId },
    /// The requested reveal item is absent from the exact active menu.
    TabListMenuScrollItemUnavailable {
        session: TabListMenuSessionId,
        item: ItemId,
    },
    /// The requested focus item is absent from the exact active menu.
    TabListMenuFocusItemUnavailable {
        session: TabListMenuSessionId,
        item: ItemId,
    },
    /// The exact menu geometry changed after the scroll proof was prepared.
    TabListMenuRecordChanged { session: TabListMenuSessionId },
    /// The claimed row is absent from the exact current menu record.
    TabListMenuRowUnavailable {
        session: TabListMenuSessionId,
        tab: TabSceneId,
    },
    /// Pointer activation missed the exact clipped menu-row rectangle.
    TabListMenuRowHitMismatch {
        session: TabListMenuSessionId,
        tab: TabSceneId,
    },
    /// The exact menu-row record changed after its press.
    TabListMenuRowRecordChanged {
        session: TabListMenuSessionId,
        tab: TabSceneId,
    },
    /// The popup routing revision changed after the exact press was frozen.
    TabListMenuPopupRevisionChanged {
        session: TabListMenuSessionId,
        expected: PopupRoutingRevision,
        actual: PopupRoutingRevision,
    },
    /// The exact current plan no longer exposes the claimed menu-frame blocker.
    TabListMenuBlockerUnavailable { session: TabListMenuSessionId },
    /// Pointer activation missed the exact current menu-frame bounds.
    TabListMenuBlockerHitMismatch { session: TabListMenuSessionId },
    /// The exact menu-frame record changed after its press.
    TabListMenuBlockerRecordChanged { session: TabListMenuSessionId },
    /// The exact current plan no longer exposes the claimed popup backdrop.
    TabListMenuBackdropUnavailable { session: TabListMenuSessionId },
    /// Pointer activation missed the exact current popup-backdrop bounds.
    TabListMenuBackdropHitMismatch { session: TabListMenuSessionId },
    /// The exact popup-backdrop record changed after its press.
    TabListMenuBackdropRecordChanged { session: TabListMenuSessionId },
    /// No gesture is active.
    NoActiveGesture,
    /// Escape was not delivered by the active gesture's source surface or by
    /// an exact current native window binding.
    EscapeDeliveryUnavailable { delivery: EscapeDelivery },
    /// This release belongs to the most recently consumed drag generation.
    DuplicateRelease { session: DragSessionId },
    /// A non-release input belongs to a drag generation that already settled.
    SessionConsumed { session: DragSessionId },
    /// The input names a different live session.
    SessionMismatch,
    /// Another stream already owns the engine's mutually exclusive docking
    /// gesture state. Journal input never replaces it implicitly.
    GestureBusy { status: InteractionStatus },
    /// A primary press reported a known capture owner incompatible with its
    /// immutable provider endpoint or exact presented native binding.
    CaptureOwnerMismatch {
        expected: PointerCaptureOwner,
        actual: PointerCaptureOwner,
    },
    /// A primary press did not identify an authoritative delivery endpoint.
    DeliveryAuthorityUnavailable,
    /// A primary press was delivered by an endpoint other than the exact
    /// immutable provider endpoint or presented native binding.
    DeliveryOwnerMismatch {
        expected: PointerEventDeliveryOwner,
        actual: PointerEventDeliveryOwner,
    },
    /// The input names a different pointer than the source press.
    PointerMismatch,
    /// The input names a different journal stream incarnation than the source
    /// press. A reused physical pointer id is deliberately not sufficient.
    PointerStreamMismatch {
        /// Stream which owns the live gesture.
        expected: PointerStreamId,
        /// Stream supplied by the later edge.
        actual: PointerStreamId,
    },
    /// The input names a different button than the source press.
    ButtonMismatch,
    /// The drag has been armed but not explicitly begun.
    DragNotBegun,
    /// Transitional contained-origin facts do not identify the exact complete root.
    #[doc(hidden)]
    ContainedDragOriginMismatch,
    /// The pressed point is outside the exact painted surface bounds.
    ContainedGesturePointerOutsideSurface { surface: SurfaceId },
    /// The pressed point does not belong to the claimed exact chrome hit region.
    ContainedGestureHitMismatch {
        floating: FloatingPresentationId,
        kind: ContainedGestureKind,
    },
    /// A structurally higher contained layer owns the exact pressed point.
    ContainedGestureOccluded {
        floating: FloatingPresentationId,
        occluding: FloatingPresentationId,
    },
    /// The scene's coordinate capture no longer matches current platform authority.
    ContainedGestureCoordinateAuthorityUnavailable { surface: SurfaceId },
    /// The pressed point is outside the exact painted surface bounds.
    TabGesturePointerOutsideSurface { surface: SurfaceId },
    /// The pressed point does not belong to the claimed exact tab drag region.
    TabGestureHitMismatch { source: TabGestureSource },
    /// A structurally higher contained layer owns the exact pressed point.
    TabGestureOccluded {
        source: TabGestureSource,
        occluding: FloatingPresentationId,
    },
    /// The scene's coordinate capture no longer matches current platform authority.
    TabGestureCoordinateAuthorityUnavailable { surface: SurfaceId },
    /// The painted tab no longer belongs to the claimed workspace presentation.
    TabGestureSourceUnavailable { source: TabGestureSource },
    /// A new-presentation offer was supplied for an already contained title drag.
    ContainedPresentationOfferUnexpected,
    /// A later observation attempted to replace the session's frozen contained offer.
    ContainedPresentationOfferChanged,
    /// A later observation attempted to replace the session's frozen background-root offer.
    SurfaceBackgroundRootOfferChanged,
    /// A later observation attempted to replace the session's frozen native offer.
    NativePresentationOfferChanged,
    /// A native offer's recovery plan names a different root than its native proposal.
    NativePresentationRecoveryRootMismatch,
    /// The observation's local or routed provenance is stale or internally inconsistent.
    TargetAuthorityInvalid,
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
    /// The pressed point lies outside the claimed painted surface.
    SplitterGesturePointerOutsideSurface { surface: SurfaceId },
    /// No splitter target owns the pressed point on the authoritative layer.
    SplitterGestureHitUnavailable { surface: SurfaceId },
    /// The authoritative presentation plan contains ambiguous splitter geometry.
    SplitterGestureHitAmbiguous(crate::scene::SplitterResizeHitError),
    /// The scene's coordinate capture no longer matches current platform authority.
    SplitterGestureCoordinateAuthorityUnavailable { surface: SurfaceId },
    /// An update named a surface other than the session's frozen owner.
    SplitterGestureSurfaceMismatch {
        expected: SurfaceId,
        actual: SurfaceId,
    },
    /// Frozen splitter geometry could not produce a valid resize proposal.
    SplitterResizeGeometryUnavailable,
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
    /// The exact contained presentation is absent from the current painted plan.
    ContainedTransformPresentationUnavailable {
        /// Surface expected to paint the contained presentation.
        surface: SurfaceId,
        /// Stable contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// Absolute pointer arithmetic could not produce finite transform geometry.
    ContainedTransformGeometryUnavailable,
    /// The current scene cannot authorize the contained transform placement.
    ContainedPlacementUnavailable(crate::intent::ContainedPlacementUnavailable),
    /// The acknowledgement does not match the exact current contained transform preview.
    ContainedTransformPreviewAcknowledgementMismatch,
    /// Release re-resolution no longer matches the exact painted transform proof.
    ContainedTransformChanged,
}

impl InteractionRejection {
    fn owner_mismatch(expected: GestureOwner, actual: GestureOwner) -> Self {
        match (expected, actual) {
            (GestureOwner::Stream(expected), GestureOwner::Stream(actual)) => {
                Self::PointerStreamMismatch { expected, actual }
            }
            (GestureOwner::LocalResponse { .. }, GestureOwner::LocalResponse { .. })
            | (GestureOwner::Stream(_), GestureOwner::LocalResponse { .. })
            | (GestureOwner::LocalResponse { .. }, GestureOwner::Stream(_)) => {
                Self::SessionMismatch
            }
        }
    }
}

/// Public result class of resolving one drag observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewResolutionStatus {
    /// One exact dock or tear-off preview was published.
    Resolved,
    /// Authority proved there is no dock target and no eligible non-docking request.
    KnownNone,
    /// Geometry was hit, but every candidate was explicitly ineligible.
    Rejected,
    /// A required ready surface scene was unavailable.
    Unavailable,
    /// The provider could not authoritatively classify the current pointer target.
    UnknownAuthority,
    /// A foreign native window authoritatively owns the current pointer route.
    OpaqueBlocker,
    /// Native capability is temporarily non-authoritative.
    NativeCapabilityUnknown,
    /// The offered native placement no longer matches current route or work-area facts.
    NativePlacementUnavailable,
}

/// Kind of an exact workspace delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceDeliveryKind {
    /// Move into a sealed docking target.
    Dock,
    /// Create, rehome, or move a contained floating by explicit request.
    Contained,
    /// Use the explicitly enabled contained fallback for an unavailable native request.
    ContainedFallback,
}

/// Native tear-off plan produced without moving source content.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedNativeTearOff {
    session: DragSessionId,
    source_presentation: PresentedSurfaceAuthority,
    payload: MovePayload,
    source_version: WorkspaceVersion,
    command: WorkspaceCommand,
    proposal: NativeTearOffProposal,
    recovery_obligation: SurfaceRecoveryObligation,
    focus_causal: FocusCausalStamp,
    pane_focus: PaneFocusDisposition,
}

impl PreparedNativeTearOff {
    pub(crate) fn new(
        session: DragSessionId,
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
        source_version: WorkspaceVersion,
        command: WorkspaceCommand,
        proposal: NativeTearOffProposal,
        recovery_obligation: SurfaceRecoveryObligation,
        focus_causal: FocusCausalStamp,
        pane_focus: PaneFocusDisposition,
    ) -> Self {
        Self {
            session,
            source_presentation,
            payload,
            source_version,
            command,
            proposal,
            recovery_obligation,
            focus_causal,
            pane_focus,
        }
    }

    /// Returns the consumed drag generation.
    #[must_use]
    pub const fn session(&self) -> DragSessionId {
        self.session
    }

    /// Returns the exact logical surface which owned the payload at release.
    #[must_use]
    pub const fn source_surface(&self) -> SurfaceId {
        self.source_presentation.surface()
    }

    /// Returns the exact final-presentation authority frozen at release.
    #[must_use]
    pub const fn source_presentation(&self) -> PresentedSurfaceAuthority {
        self.source_presentation
    }

    pub(crate) const fn payload(&self) -> &MovePayload {
        &self.payload
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

    pub(crate) const fn recovery_obligation(&self) -> &SurfaceRecoveryObligation {
        &self.recovery_obligation
    }

    /// Returns the release-edge causal position reserved for eventual focus admission.
    #[must_use]
    pub const fn focus_causal(&self) -> FocusCausalStamp {
        self.focus_causal
    }

    /// Returns the source pane focus frozen at the release edge.
    #[must_use]
    pub const fn pane_focus(&self) -> PaneFocusDisposition {
        self.pane_focus
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

/// One canonical offset mutation produced by an ordered scroll edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollApplication {
    session: Option<ScrollSessionId>,
    receiver: PresentationHitRegionId,
    phase: ScrollPhase,
    requested_delta: f64,
    applied_delta: f64,
    unapplied_delta: f64,
    offset: f64,
}

impl ScrollApplication {
    pub(crate) const fn new(
        session: Option<ScrollSessionId>,
        receiver: PresentationHitRegionId,
        phase: ScrollPhase,
        requested_delta: f64,
        applied_delta: f64,
        unapplied_delta: f64,
        offset: f64,
    ) -> Self {
        Self {
            session,
            receiver,
            phase,
            requested_delta,
            applied_delta,
            unapplied_delta,
            offset,
        }
    }

    /// Returns the core-owned smooth-session identity, or `None` for a discrete edge.
    #[must_use]
    pub const fn session(self) -> Option<ScrollSessionId> {
        self.session
    }

    /// Returns the exact semantic receiver which consumed the edge.
    #[must_use]
    pub const fn receiver(self) -> PresentationHitRegionId {
        self.receiver
    }

    /// Returns the native phase carried by the reduced edge.
    #[must_use]
    pub const fn phase(self) -> ScrollPhase {
        self.phase
    }

    /// Returns the requested signed canonical offset change before clamping.
    #[must_use]
    pub const fn requested_delta(self) -> f64 {
        self.requested_delta
    }

    /// Returns the signed canonical offset change applied after clamping.
    #[must_use]
    pub const fn applied_delta(self) -> f64 {
        self.applied_delta
    }

    /// Returns the signed requested change which could not be applied.
    #[must_use]
    pub const fn unapplied_delta(self) -> f64 {
        self.unapplied_delta
    }

    /// Returns the resulting core-owned logical offset.
    #[must_use]
    pub const fn offset(self) -> f64 {
        self.offset
    }
}

/// Why one ordered scroll edge consumed no docking semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollSuppressionReason {
    /// The adapter could not authoritatively determine the top receiver.
    ReceiverUnknown,
    /// A known framework receiver blocked docking delivery.
    FrameworkBlocked,
    /// The framework authoritatively reported no receiver.
    NoReceiver,
    /// The docking canvas received the edge but owns no scroll capability.
    DockCanvas,
    /// A core-owned blocker was the exact top scroll receiver.
    DockBlocker(PresentationHitRegionId),
}

/// Exact terminal cause of one core-owned smooth-scroll session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollTerminationReason {
    /// The provider emitted the matching terminal `End` edge.
    Completed,
    /// The provider explicitly cancelled the native sequence.
    Cancelled(ScrollCancelReason),
    /// The pointer stream was explicitly cancelled.
    StreamCancelled,
    /// The pointer stream completed with its normal terminal release.
    StreamEnded,
    /// The pointer-provider incarnation was retired.
    ProviderRetired,
    /// Known receiver evidence no longer corroborated the locked owner.
    ReceiverLost,
    /// The physical delivery endpoint changed during the sequence.
    DeliveryEndpointChanged,
    /// The exact native viewport binding was retired or replaced.
    BindingRetired,
    /// The presentation host which owned delivery was retired.
    PresentationHostRetired,
    /// The logical receiver surface no longer exists.
    SurfaceRemoved,
    /// Popup routing changed while a popup receiver was locked.
    PopupRoutingChanged,
    /// Policy changed while the semantic receiver was locked.
    PolicyChanged,
    /// Presentation configuration changed while conversion geometry was locked.
    PresentationConfigChanged,
}

/// Canonical reduction result for one ordered scroll edge or lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollReductionOutcome {
    /// A smooth begin locked one exact core-owned receiver.
    Began {
        /// Core-owned session identity.
        session: ScrollSessionId,
        /// Exact semantic receiver locked for the sequence.
        receiver: PresentationHitRegionId,
    },
    /// A receiver consumed the edge and updated its core-owned offset, possibly by zero at a bound.
    Applied(ScrollApplication),
    /// The edge was consumed without changing docking state.
    Suppressed {
        /// Core-owned smooth session, when this edge belongs to one.
        session: Option<ScrollSessionId>,
        /// Provider-owned sequence token, when this is a smooth edge.
        sequence: Option<ScrollSequenceToken>,
        /// Native phase carried by the edge.
        phase: ScrollPhase,
        /// Exact fail-closed reason.
        reason: ScrollSuppressionReason,
    },
    /// One smooth session reached its sole terminal transition.
    Terminated {
        /// Core-owned session identity.
        session: ScrollSessionId,
        /// Locked semantic receiver, absent for a sequence suppressed at `Begin`.
        receiver: Option<PresentationHitRegionId>,
        /// Exact terminal cause.
        reason: ScrollTerminationReason,
    },
}

/// Result of reducing one renderer intent.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionOutcome {
    /// One pointer-journal scroll reduction or lifecycle terminal transition.
    Scroll(ScrollReductionOutcome),
    /// One exact close plan was opened without mutating workspace topology.
    CloseRequested {
        plan: ClosePlan,
        /// True when repeated activation reused the same unresolved request.
        reused: bool,
    },
    /// A core-owned tab-strip control settled on matching release.
    TabStripControlActivated {
        /// Exact control whose press and release matched.
        control: TabStripControlId,
        /// Whether core-owned transient presentation state changed.
        changed: bool,
        /// The resulting sole active menu, when this was a menu toggle/open.
        menu: Option<TabListMenuSessionId>,
    },
    /// One exact menu row selected its current item and closed the menu atomically.
    TabListMenuItemSelected {
        /// Menu instance which owned the selected row.
        session: TabListMenuSessionId,
        /// Stable row/tab identity selected by the release.
        tab: TabSceneId,
        /// Checked workspace selection result.
        outcome: CommandOutcome,
        /// Whether the durable workspace selection changed.
        changed: bool,
    },
    /// A matching backdrop release atomically closed the exact active menu.
    TabListMenuDismissed {
        /// Menu instance closed by the backdrop release.
        session: TabListMenuSessionId,
    },
    /// A matching release inside the menu frame was consumed without dismissal.
    TabListMenuFrameConsumed {
        /// Menu instance whose frame consumed the release.
        session: TabListMenuSessionId,
    },
    /// One exact tab-strip scroll adjustment updated core-owned presentation state.
    TabStripScrolled {
        /// Structural bar whose offset was adjusted.
        bar: TabBarSceneId,
        /// Resulting core-clamped logical offset.
        offset: f64,
        /// Whether the core-owned offset changed.
        changed: bool,
    },
    /// One exact active tab-list menu scroll adjustment updated core-owned state.
    TabListMenuScrolled {
        /// Exact popup session whose offset was adjusted.
        session: TabListMenuSessionId,
        /// Resulting core-clamped logical offset.
        offset: f64,
        /// Whether the core-owned offset changed.
        changed: bool,
    },
    /// One exact menu navigation atomically updated focus and reveal scroll.
    TabListMenuFocusMoved {
        /// Exact popup session whose focus was adjusted.
        session: TabListMenuSessionId,
        /// Resulting core-owned focus item.
        item: ItemId,
        /// Resulting core-clamped logical scroll offset.
        offset: f64,
        /// Whether focus or scroll state changed.
        changed: bool,
    },
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
    /// Release was consumed, but delivery is waiting for this exact preview to
    /// be observed as presented by the host.
    ReleasePending {
        session: DragSessionId,
        preview: PreviewToken,
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
        splits: Vec<SplitResize>,
    },
    /// One scene-bound keyboard or accessibility resize committed.
    SplitterAdjusted {
        outcome: CommandOutcome,
        changed: bool,
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
    /// Release was consumed, but the exact contained rectangle is waiting for
    /// a final-presentation observation before it can become durable.
    ContainedTransformReleasePending {
        session: ContainedTransformSessionId,
        preview: ContainedTransformPreviewToken,
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
    cause: PendingReductionCause,
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
            cause: PendingReductionCause::input(input),
            version,
            kind,
        }
    }

    pub(crate) const fn new_caused(
        cause: ReductionCause,
        version: WorkspaceVersion,
        kind: InteractionEventKind,
    ) -> Self {
        Self {
            cause: PendingReductionCause::bound(cause),
            version,
            kind,
        }
    }

    pub(crate) fn bind_input(&mut self, cause: ReductionCause) -> bool {
        self.cause.bind_input(cause)
    }

    /// Returns the exact core-minted cause of this published event.
    #[must_use]
    pub fn cause(&self) -> ReductionCause {
        self.cause
            .published()
            .expect("published interaction events always have a bound reduction cause")
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
    /// A smooth-scroll session reached one authoritative lifecycle terminal.
    ScrollTerminated {
        /// Core-owned session identity.
        session: ScrollSessionId,
        /// Locked semantic receiver, absent for a sequence suppressed at `Begin`.
        receiver: Option<PresentationHitRegionId>,
        /// Exact lifecycle terminal cause.
        reason: ScrollTerminationReason,
    },
    /// A semantic tab action committed and requests adapter focus continuation.
    SemanticFocusRequested { tab: TabSceneId },
    /// One preview publication replaced the previous preview.
    PreviewPublished { preview: InteractionPreview },
    /// A scene authority change invalidated preview facts while preserving the source drag.
    PreviewCleared { session: DragSessionId },
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
    NativePresentationRequested(NativeCreateRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewProof {
    Dock {
        target: DropTargetId,
        command: WorkspaceCommand,
    },
    Contained {
        command: WorkspaceCommand,
        proposal: ContainedTearOffProposal,
        fallback: bool,
    },
    Native {
        command: WorkspaceCommand,
        offer: NativePresentationOffer,
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

    pub(crate) fn acknowledge(
        &mut self,
        acknowledgement: &PaintAcknowledgement,
    ) -> Result<bool, InteractionRejection> {
        if self.public.token != acknowledgement.token
            || self.public.visual != acknowledgement.visual
        {
            return Err(InteractionRejection::PreviewAcknowledgementMismatch);
        }
        let changed = !self.painted;
        self.painted = true;
        Ok(changed)
    }
}

/// Exact journal stream owner of one transient gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GestureOwner {
    Stream(PointerStreamId),
    LocalResponse { surface: SurfaceId },
}

impl GestureOwner {
    pub(crate) const fn pointer(self) -> PointerId {
        match self {
            Self::Stream(stream) => stream.pointer(),
            Self::LocalResponse { .. } => {
                panic!("a local-response gesture has no physical pointer identity")
            }
        }
    }

    pub(crate) const fn pointer_if_physical(self) -> Option<PointerId> {
        match self {
            Self::Stream(stream) => Some(stream.pointer()),
            Self::LocalResponse { .. } => None,
        }
    }

    pub(crate) const fn stream(self) -> Option<PointerStreamId> {
        match self {
            Self::Stream(stream) => Some(stream),
            Self::LocalResponse { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResizeGestureAuthority {
    Presented(FrozenPresentationAuthority),
    LocalReady,
}

impl ResizeGestureAuthority {
    pub(crate) const fn presented(self) -> Option<FrozenPresentationAuthority> {
        match self {
            Self::Presented(authority) => Some(authority),
            Self::LocalReady => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragGestureAuthority {
    Presented(FrozenPresentationAuthority),
    LocalReady {
        surface: SurfaceId,
        coordinates: SurfaceCoordinateCapture,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ContainedTransformGestureAuthority {
    Presented(FrozenPresentationAuthority),
    LocalReady {
        surface: SurfaceId,
        coordinates: SurfaceCoordinateCapture,
    },
}

impl ContainedTransformGestureAuthority {
    pub(crate) const fn presented(self) -> Option<FrozenPresentationAuthority> {
        match self {
            Self::Presented(authority) => Some(authority),
            Self::LocalReady { .. } => None,
        }
    }

    pub(crate) const fn local_coordinates(self) -> Option<SurfaceCoordinateCapture> {
        match self {
            Self::Presented(_) => None,
            Self::LocalReady { coordinates, .. } => Some(coordinates),
        }
    }

    pub(crate) const fn into_drag(self) -> DragGestureAuthority {
        match self {
            Self::Presented(authority) => DragGestureAuthority::Presented(authority),
            Self::LocalReady {
                surface,
                coordinates,
            } => DragGestureAuthority::LocalReady {
                surface,
                coordinates,
            },
        }
    }
}

impl DragGestureAuthority {
    pub(crate) const fn presented(self) -> Option<FrozenPresentationAuthority> {
        match self {
            Self::Presented(authority) => Some(authority),
            Self::LocalReady { .. } => None,
        }
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        match self {
            Self::Presented(authority) => authority.surface(),
            Self::LocalReady { surface, .. } => surface,
        }
    }

    pub(crate) const fn local_coordinates(self) -> Option<SurfaceCoordinateCapture> {
        match self {
            Self::Presented(_) => None,
            Self::LocalReady { coordinates, .. } => Some(coordinates),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ArmedDrag {
    pub(crate) session: DragSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) journal_threshold_origin: Option<JournalDragThresholdOrigin>,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: DragGestureAuthority,
    pub(crate) source_layout_facts: Option<Arc<PresentationLayoutFacts>>,
    pub(crate) journal_source_geometry: Option<JournalDragSourceGeometry>,
    pub(crate) continuation: Option<SceneGestureContinuation>,
}

/// Press-time coordinate authority used only to decide when a journal drag starts.
///
/// A desktop-global provider never creates a synthetic logical coordinate
/// system across surfaces. Its threshold remains source-logical policy, but
/// the comparison is performed in desktop physical pixels using the frozen
/// source scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum JournalDragThresholdOrigin {
    SurfaceLocal(LogicalPoint),
    DesktopPhysical {
        position: PhysicalPoint,
        source_scale: ScaleFactor,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct JournalDragSourceGeometry {
    pub(crate) source_rect: LogicalRect,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenContainedDragOrigin {
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) source_roster: ContainedRosterSource,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrozenDragOrigin {
    Workspace,
    Contained(FrozenContainedDragOrigin),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragArmStart {
    pub(crate) epoch: WorkspaceEpoch,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) journal_threshold_origin: Option<JournalDragThresholdOrigin>,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: DragGestureAuthority,
    pub(crate) source_layout_facts: Option<Arc<PresentationLayoutFacts>>,
    pub(crate) journal_source_geometry: Option<JournalDragSourceGeometry>,
    pub(crate) continuation: Option<SceneGestureContinuationDraft>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveDrag {
    pub(crate) session: DragSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: DragGestureAuthority,
    pub(crate) source_layout_facts: Option<Arc<PresentationLayoutFacts>>,
    pub(crate) journal_source_geometry: Option<JournalDragSourceGeometry>,
    pub(crate) continuation: Option<SceneGestureContinuation>,
    pub(crate) journal_presentation_reservation: Option<JournalPresentationReservation>,
    pub(crate) contained_offer: Option<ContainedPresentationOffer>,
    pub(crate) surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    pub(crate) native_offer: Option<NativePresentationOffer>,
    pub(crate) affordance: Option<DropAffordance>,
    pub(crate) preview: Option<PublishedPreview>,
}

/// Core-minted identities frozen to one journal drag session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JournalPresentationReservation {
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveResize {
    pub(crate) session: ResizeSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) target: SplitterResizeTarget,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) authority: ResizeGestureAuthority,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) current_pointer: LogicalPoint,
    pub(crate) axis_groups: Vec<FrozenResizeAxisGroup>,
    pub(crate) updates: Vec<SplitResize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenResizeHandle {
    pub(crate) source: NodeSource,
    pub(crate) record: SplitterRecord,
    pub(crate) allowed_delta: ResizeDeltaInterval,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenResizeAxisGroup {
    pub(crate) axis: Axis,
    pub(crate) handles: Vec<FrozenResizeHandle>,
    pub(crate) common_delta: ResizeDeltaInterval,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResizeDeltaInterval {
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResizeStart {
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) target: SplitterResizeTarget,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) authority: ResizeGestureAuthority,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) axis_groups: Vec<FrozenResizeAxisGroup>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublishedContainedTransformPreview {
    public: ContainedTransformPreview,
    placement: ContainedTransformPlacement,
    painted: bool,
}

impl PublishedContainedTransformPreview {
    pub(crate) const fn public(&self) -> &ContainedTransformPreview {
        &self.public
    }

    pub(crate) const fn placement(&self) -> ContainedTransformPlacement {
        self.placement
    }

    pub(crate) const fn painted(&self) -> bool {
        self.painted
    }

    pub(crate) fn acknowledge(
        &mut self,
        acknowledgement: ContainedTransformPaintAcknowledgement,
    ) -> Result<bool, InteractionRejection> {
        if self.public.acknowledgement() != acknowledgement {
            return Err(InteractionRejection::ContainedTransformPreviewAcknowledgementMismatch);
        }
        let changed = !self.painted;
        self.painted = true;
        Ok(changed)
    }
}

/// Exact core-resolved geometry for one contained transform update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedTransformPlacement {
    scene: SurfaceSceneStamp,
    surface: SurfaceId,
    surface_bounds: LogicalRect,
    rect: LogicalRect,
}

impl ContainedTransformPlacement {
    pub(crate) const fn new(
        scene: SurfaceSceneStamp,
        surface: SurfaceId,
        surface_bounds: LogicalRect,
        rect: LogicalRect,
    ) -> Self {
        Self {
            scene,
            surface,
            surface_bounds,
            rect,
        }
    }

    pub(crate) const fn scene(self) -> SurfaceSceneStamp {
        self.scene
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn rect(self) -> LogicalRect {
        self.rect
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveContainedTransform {
    pub(crate) session: ContainedTransformSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) current_pointer: LogicalPoint,
    pub(crate) kind: ContainedTransformKind,
    pub(crate) minimum_size: LogicalSize,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) surface_bounds: LogicalRect,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) presentation: ContainedTransformGestureAuthority,
    pub(crate) continuation: Option<SceneGestureContinuation>,
    pub(crate) preview: Option<PublishedContainedTransformPreview>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainedTransformStart {
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Option<Authority<PointerCaptureOwner>>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) kind: ContainedTransformKind,
    pub(crate) minimum_size: LogicalSize,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) surface_bounds: LogicalRect,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) presentation: ContainedTransformGestureAuthority,
    pub(crate) continuation: Option<SceneGestureContinuationDraft>,
}

#[derive(Debug, Clone, PartialEq)]
enum ActiveGesture {
    Idle,
    Pressed(Box<ActiveClick>),
    Armed(Box<ArmedDrag>),
    Dragging(Box<ActiveDrag>),
    Resizing(Box<ActiveResize>),
    ContainedTransforming(Box<ActiveContainedTransform>),
}

/// Core-owned transient interaction state.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionState {
    active: ActiveGesture,
    last_click_generation: ClickGeneration,
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
            ActiveGesture::Pressed(click) => InteractionStatus::Pressed {
                session: click.session,
            },
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

    /// Returns the exact owner of the current gesture, if any.
    pub(crate) const fn active_owner(&self) -> Option<GestureOwner> {
        match &self.active {
            ActiveGesture::Idle => None,
            ActiveGesture::Pressed(click) => Some(click.owner),
            ActiveGesture::Armed(drag) => Some(drag.owner),
            ActiveGesture::Dragging(drag) => Some(drag.owner),
            ActiveGesture::Resizing(resize) => Some(resize.owner),
            ActiveGesture::ContainedTransforming(transform) => Some(transform.owner),
        }
    }

    /// Returns the current journal stream, if the active gesture is journal
    /// owned rather than a legacy renderer interaction.
    pub(crate) const fn active_stream(&self) -> Option<PointerStreamId> {
        match self.active_owner() {
            Some(owner) => owner.stream(),
            None => None,
        }
    }

    pub(crate) const fn active_journal_capture_authority(
        &self,
    ) -> Option<Authority<PointerCaptureOwner>> {
        match &self.active {
            ActiveGesture::Pressed(click) => Some(click.capture_authority),
            ActiveGesture::Armed(drag) => drag.journal_capture_authority,
            ActiveGesture::Dragging(drag) => drag.journal_capture_authority,
            ActiveGesture::Resizing(resize) => resize.journal_capture_authority,
            ActiveGesture::ContainedTransforming(transform) => transform.journal_capture_authority,
            ActiveGesture::Idle => None,
        }
    }

    pub(crate) fn update_journal_capture_authority(
        &mut self,
        stream: PointerStreamId,
        capture: Authority<PointerCaptureOwner>,
    ) -> bool {
        if self.active_stream() != Some(stream) {
            return false;
        }
        match &mut self.active {
            ActiveGesture::Pressed(click) => click.capture_authority = capture,
            ActiveGesture::Armed(drag) => drag.journal_capture_authority = Some(capture),
            ActiveGesture::Dragging(drag) => drag.journal_capture_authority = Some(capture),
            ActiveGesture::Resizing(resize) => resize.journal_capture_authority = Some(capture),
            ActiveGesture::ContainedTransforming(transform) => {
                transform.journal_capture_authority = Some(capture);
            }
            ActiveGesture::Idle => return false,
        }
        true
    }

    /// Returns the exact presentation proof frozen by the active gesture.
    pub(crate) const fn active_presentation_authority(
        &self,
    ) -> Option<FrozenPresentationAuthority> {
        match &self.active {
            ActiveGesture::Idle => None,
            ActiveGesture::Pressed(click) => Some(click.presentation),
            ActiveGesture::Armed(drag) => drag.presentation.presented(),
            ActiveGesture::Dragging(drag) => drag.presentation.presented(),
            ActiveGesture::Resizing(resize) => resize.authority.presented(),
            ActiveGesture::ContainedTransforming(transform) => transform.presentation.presented(),
        }
    }

    /// Returns the exact self-invalidation continuation held by the active
    /// source gesture, if one was minted during activation.
    pub(crate) const fn active_scene_gesture_continuation(
        &self,
    ) -> Option<&SceneGestureContinuation> {
        match &self.active {
            ActiveGesture::Armed(drag) => drag.continuation.as_ref(),
            ActiveGesture::Dragging(drag) => drag.continuation.as_ref(),
            ActiveGesture::ContainedTransforming(transform) => transform.continuation.as_ref(),
            ActiveGesture::Idle | ActiveGesture::Pressed(_) | ActiveGesture::Resizing(_) => None,
        }
    }

    /// Returns the public stable identity of the active click, if one is pressed.
    #[must_use]
    pub const fn active_click_view(&self) -> Option<ActiveClickView> {
        match &self.active {
            ActiveGesture::Pressed(click) => Some(ActiveClickView {
                session: click.session,
                region: click.region,
            }),
            ActiveGesture::Idle
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    pub(crate) fn begin_click(
        &mut self,
        start: ClickStart,
    ) -> Result<(ClickSessionId, Option<InteractionStatus>), InteractionCounterError> {
        if start.owner.stream().is_none() || start.button != PointerButton::Primary {
            return Err(InteractionCounterError::StateInvariant);
        }
        let generation = self
            .last_click_generation
            .checked_next()
            .ok_or(InteractionCounterError::ClickGenerationExhausted)?;
        self.last_click_generation = generation;
        let session = ClickSessionId::new(start.epoch, generation);
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::Pressed(Box::new(ActiveClick {
            session,
            owner: start.owner,
            capture_authority: start.capture_authority,
            button: start.button,
            region: start.region,
            measurement: start.measurement,
            coordinates: start.coordinates,
            presentation: start.presentation,
            action: start.action,
        }));
        Ok((session, replaced))
    }

    pub(crate) fn active_click(
        &self,
        session: ClickSessionId,
    ) -> Result<&ActiveClick, InteractionRejection> {
        match &self.active {
            ActiveGesture::Pressed(click) if click.session == session => Ok(click),
            ActiveGesture::Idle => Err(InteractionRejection::NoActiveGesture),
            _ => Err(InteractionRejection::SessionMismatch),
        }
    }

    pub(crate) fn take_click_for_release(
        &mut self,
        session: ClickSessionId,
        owner: GestureOwner,
        button: PointerButton,
    ) -> Result<ActiveClick, InteractionRejection> {
        let click = self.active_click(session)?;
        if click.owner != owner {
            return Err(InteractionRejection::owner_mismatch(click.owner, owner));
        }
        if click.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        let ActiveGesture::Pressed(click) =
            std::mem::replace(&mut self.active, ActiveGesture::Idle)
        else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        Ok(*click)
    }

    /// Returns a read-only view of the current armed or active drag.
    #[must_use]
    pub const fn active_drag_view(&self) -> Option<ActiveDragView<'_>> {
        match &self.active {
            ActiveGesture::Armed(drag) => Some(ActiveDragView {
                phase: DragPhase::Armed,
                session: drag.session,
                owner: drag.owner,
                button: drag.button,
                payload: &drag.payload,
                contained_offer: None,
                surface_background_offer: None,
                native_offer: None,
            }),
            ActiveGesture::Dragging(drag) => Some(ActiveDragView {
                phase: DragPhase::Dragging,
                session: drag.session,
                owner: drag.owner,
                button: drag.button,
                payload: &drag.payload,
                contained_offer: drag.contained_offer.as_ref(),
                surface_background_offer: drag.surface_background_offer.as_ref(),
                native_offer: drag.native_offer.as_ref(),
            }),
            ActiveGesture::Idle
            | ActiveGesture::Pressed(_)
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
                owner: transform.owner,
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
            | ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_) => None,
        }
    }

    /// Returns a read-only view of the active splitter resize.
    #[must_use]
    pub fn active_resize_view(&self) -> Option<ActiveResizeView<'_>> {
        match &self.active {
            ActiveGesture::Resizing(resize) => Some(ActiveResizeView {
                session: resize.session,
                owner: resize.owner,
                button: resize.button,
                surface: resize.surface,
                scene: resize.scene,
                target: resize.target,
                updates: &resize.updates,
            }),
            ActiveGesture::Idle
            | ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::ContainedTransforming(_) => None,
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
            | ActiveGesture::Pressed(_)
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
            | ActiveGesture::Pressed(_)
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
            | ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    /// Returns the validated resize override eligible for transient projection.
    #[must_use]
    pub fn resize_overrides(&self) -> Option<(ResizeSessionId, &[SplitResize])> {
        match &self.active {
            ActiveGesture::Resizing(resize) if !resize.updates.is_empty() => {
                Some((resize.session, &resize.updates))
            }
            ActiveGesture::Idle
            | ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => None,
        }
    }

    pub(crate) fn arm_drag(
        &mut self,
        start: DragArmStart,
    ) -> Result<(DragSessionId, Option<InteractionStatus>), InteractionCounterError> {
        let generation = self
            .last_drag_generation
            .checked_next()
            .ok_or(InteractionCounterError::DragGenerationExhausted)?;
        self.last_drag_generation = generation;
        let session = DragSessionId::new(start.epoch, generation);
        let continuation = start
            .continuation
            .map(|draft| draft.bind(SceneGestureSession::Drag(session)));
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::Armed(Box::new(ArmedDrag {
            session,
            owner: start.owner,
            journal_capture_authority: start.journal_capture_authority,
            button: start.button,
            payload: start.payload,
            source_surface: start.source_surface,
            initial_pointer: start.initial_pointer,
            journal_threshold_origin: start.journal_threshold_origin,
            complete_root: start.complete_root,
            partial_detachable: start.partial_detachable,
            origin: start.origin,
            source_validated_at: start.source_validated_at,
            presentation: start.presentation,
            source_layout_facts: start.source_layout_facts,
            journal_source_geometry: start.journal_source_geometry,
            continuation,
        }));
        Ok((session, replaced))
    }

    pub(crate) fn begin_drag(
        &mut self,
        session: DragSessionId,
        owner: GestureOwner,
        button: PointerButton,
    ) -> Result<(), InteractionRejection> {
        let ActiveGesture::Armed(armed) = &self.active else {
            return Err(self.session_rejection(session));
        };
        if armed.session != session {
            return Err(InteractionRejection::SessionMismatch);
        }
        if armed.owner != owner {
            return Err(InteractionRejection::owner_mismatch(armed.owner, owner));
        }
        if armed.button != button {
            return Err(InteractionRejection::ButtonMismatch);
        }
        self.active = ActiveGesture::Dragging(Box::new(ActiveDrag {
            session: armed.session,
            owner: armed.owner,
            journal_capture_authority: armed.journal_capture_authority,
            button: armed.button,
            payload: armed.payload.clone(),
            source_surface: armed.source_surface,
            initial_pointer: armed.initial_pointer,
            complete_root: armed.complete_root.clone(),
            partial_detachable: armed.partial_detachable,
            origin: armed.origin.clone(),
            source_validated_at: armed.source_validated_at,
            presentation: armed.presentation,
            source_layout_facts: armed.source_layout_facts.clone(),
            journal_source_geometry: armed.journal_source_geometry,
            continuation: armed.continuation.clone(),
            journal_presentation_reservation: None,
            contained_offer: None,
            surface_background_offer: None,
            native_offer: None,
            affordance: None,
            preview: None,
        }));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn freeze_journal_presentation_reservation(
        &mut self,
        session: DragSessionId,
        owner: GestureOwner,
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        anchor: crate::intent::SurfacePointer,
        requested_rect: LogicalRect,
        minimum_size: LogicalSize,
        reserve_background_root: bool,
    ) -> Result<(), InteractionCounterError> {
        let drag = self
            .active_drag_mut(session)
            .map_err(|_| InteractionCounterError::StateInvariant)?;
        if drag.owner != owner
            || owner.stream().is_none()
            || drag.journal_presentation_reservation.is_some()
            || drag.contained_offer.is_some()
            || drag.surface_background_offer.is_some()
            || drag.native_offer.is_some()
        {
            return Err(InteractionCounterError::StateInvariant);
        }
        match &drag.origin {
            FrozenDragOrigin::Workspace => {
                if drag
                    .complete_root
                    .as_ref()
                    .is_some_and(|source| source.root() != root)
                    || (drag.complete_root.is_some() && reserve_background_root)
                    || (drag.complete_root.is_none() && !reserve_background_root)
                {
                    return Err(InteractionCounterError::StateInvariant);
                }
            }
            FrozenDragOrigin::Contained(origin) => {
                if drag.complete_root.as_ref().map(NodeSource::root) != Some(origin.root)
                    || root != origin.root
                    || floating != origin.floating
                    || anchor
                        != crate::intent::SurfacePointer::new(
                            origin.surface,
                            origin.initial_pointer,
                        )
                    || requested_rect != origin.source_rect
                    || minimum_size != origin.minimum_size
                    || reserve_background_root
                {
                    return Err(InteractionCounterError::StateInvariant);
                }
            }
        }
        drag.journal_presentation_reservation = Some(JournalPresentationReservation {
            surface,
            root,
            floating,
        });
        if matches!(drag.origin, FrozenDragOrigin::Workspace) {
            drag.contained_offer = Some(ContainedPresentationOffer::front(
                root,
                floating,
                anchor,
                requested_rect,
                minimum_size,
            ));
            drag.surface_background_offer =
                reserve_background_root.then_some(SurfaceBackgroundRootOffer::new(root));
        }
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
        scene: SurfaceSceneStamp,
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

    pub(crate) fn clear_drag_feedback(
        &mut self,
        session: DragSessionId,
    ) -> Result<bool, InteractionRejection> {
        let drag = self.active_drag_mut(session)?;
        let affordance_cleared = drag.affordance.take().is_some();
        let preview_cleared = drag.preview.take().is_some();
        Ok(affordance_cleared || preview_cleared)
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
        let changed = preview.acknowledge(acknowledgement)?;
        Ok((session, changed))
    }

    pub(crate) fn observe_presented_preview(
        &mut self,
        surface: SurfaceId,
        token: PreviewToken,
    ) -> bool {
        let ActiveGesture::Dragging(drag) = &mut self.active else {
            return false;
        };
        let Some(preview) = drag.preview.as_mut() else {
            return false;
        };
        if preview.public.token != token || preview.public.visual.surface() != surface {
            return false;
        }
        let changed = !preview.painted;
        preview.painted = true;
        changed
    }

    pub(crate) fn take_drag_for_release(
        &mut self,
        session: DragSessionId,
        owner: GestureOwner,
        button: PointerButton,
    ) -> Result<ActiveDrag, InteractionRejection> {
        let drag = self.active_drag(session)?;
        if drag.owner != owner {
            return Err(InteractionRejection::owner_mismatch(drag.owner, owner));
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
        start: ResizeStart,
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
            owner: start.owner,
            journal_capture_authority: start.journal_capture_authority,
            button: start.button,
            surface: start.surface,
            scene: start.scene,
            target: start.target,
            coordinate_capture: start.coordinate_capture,
            authority: start.authority,
            initial_pointer: start.initial_pointer,
            current_pointer: start.initial_pointer,
            axis_groups: start.axis_groups,
            updates: Vec::new(),
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
            ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => Err(InteractionRejection::SessionMismatch),
        }
    }

    pub(crate) fn set_resize_updates(
        &mut self,
        session: ResizeSessionId,
        current_pointer: LogicalPoint,
        updates: Vec<SplitResize>,
    ) -> Result<(), InteractionRejection> {
        match &mut self.active {
            ActiveGesture::Resizing(resize) if resize.session == session => {
                resize.current_pointer = current_pointer;
                resize.updates = updates;
                Ok(())
            }
            ActiveGesture::Idle => Err(InteractionRejection::NoActiveGesture),
            ActiveGesture::Pressed(_)
            | ActiveGesture::Armed(_)
            | ActiveGesture::Dragging(_)
            | ActiveGesture::Resizing(_)
            | ActiveGesture::ContainedTransforming(_) => Err(InteractionRejection::SessionMismatch),
        }
    }

    pub(crate) fn take_resize_for_release(
        &mut self,
        session: ResizeSessionId,
        owner: GestureOwner,
        button: PointerButton,
    ) -> Result<ActiveResize, InteractionRejection> {
        let resize = self.active_resize(session)?;
        if resize.owner != owner {
            return Err(InteractionRejection::owner_mismatch(resize.owner, owner));
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
        let continuation = start
            .continuation
            .map(|draft| draft.bind(SceneGestureSession::ContainedTransform(session)));
        let replaced = (self.status() != InteractionStatus::Idle).then_some(self.status());
        self.active = ActiveGesture::ContainedTransforming(Box::new(ActiveContainedTransform {
            session,
            owner: start.owner,
            journal_capture_authority: start.journal_capture_authority,
            button: start.button,
            surface: start.surface,
            root: start.root,
            floating: start.floating,
            source_rect: start.source_rect,
            initial_pointer: start.initial_pointer,
            current_pointer: start.initial_pointer,
            kind: start.kind,
            minimum_size: start.minimum_size,
            scene: start.scene,
            surface_bounds: start.surface_bounds,
            coordinate_capture: start.coordinate_capture,
            presentation: start.presentation,
            continuation,
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
        placement: ContainedTransformPlacement,
    ) -> Result<(ContainedTransformPreview, bool), InteractionCounterError> {
        if let Ok(transform) = self.active_contained_transform(session)
            && let Some(existing) = &transform.preview
            && existing.public.token.scene == placement.scene()
            && existing.public.rect == placement.rect()
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
                scene: placement.scene(),
                sequence,
            },
            surface: placement.surface(),
            root: transform.root,
            floating: transform.floating,
            rect: placement.rect(),
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
        let changed = preview.acknowledge(acknowledgement)?;
        Ok((session, changed))
    }

    pub(crate) fn observe_presented_contained_transform_preview(
        &mut self,
        surface: SurfaceId,
        token: ContainedTransformPreviewToken,
    ) -> bool {
        let ActiveGesture::ContainedTransforming(transform) = &mut self.active else {
            return false;
        };
        let Some(preview) = transform.preview.as_mut() else {
            return false;
        };
        if preview.public.token() != token || preview.public.surface() != surface {
            return false;
        }
        let changed = !preview.painted;
        preview.painted = true;
        changed
    }

    pub(crate) fn take_contained_transform_for_release(
        &mut self,
        session: ContainedTransformSessionId,
        owner: GestureOwner,
        button: PointerButton,
    ) -> Result<ActiveContainedTransform, InteractionRejection> {
        let transform = self.active_contained_transform(session)?;
        if transform.owner != owner {
            return Err(InteractionRejection::owner_mismatch(transform.owner, owner));
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

    pub(crate) fn cancel_click(
        &mut self,
        session: ClickSessionId,
    ) -> Result<InteractionStatus, InteractionRejection> {
        self.active_click(session)?;
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

    /// Cancels the active gesture only when it is owned by `owner`.
    ///
    /// Journal cancellation and capture-loss edges must not affect a later
    /// stream incarnation that reused the same physical pointer id.
    pub(crate) fn cancel_owner(&mut self, owner: GestureOwner) -> Option<InteractionStatus> {
        if self.active_owner() != Some(owner) {
            return None;
        }
        self.cancel_active()
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
            last_click_generation: ClickGeneration::default(),
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
    /// Click generations cannot advance without wrapping.
    #[error("click session generation is exhausted")]
    ClickGenerationExhausted,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drop_target::SceneLayerKey;
    use crate::graph::{Node, Workspace};
    use crate::ids::EngineAuthorityDomainId;
    use crate::intent::Authority;
    use crate::pointer_journal::{PointerInputLease, PointerProviderScope, PointerStreamId};
    use crate::presentation_config::PresentationConfigRevision;
    use crate::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};
    use crate::presentation_observation::{
        PresentationOutputSerial, SurfacePresentationOutputTicket,
    };
    use crate::scene::{PresentationPlan, TabBarSceneId, TabStripControlRecord};
    use crate::scene_manifest::{SurfaceRequirementRevision, SurfaceSceneRevision};
    use crate::tab_strip::{TabStripControlId, TabStripStateKey};
    use crate::viewport::CoordinateGeneration;

    fn frozen_presentation(
        surface: SurfaceId,
        epoch: WorkspaceEpoch,
    ) -> FrozenPresentationAuthority {
        let authority_domain = EngineAuthorityDomainId::new_for_test(90);
        let ticket = SurfaceMeasurementTicket::new(
            authority_domain,
            epoch,
            PresentationConfigRevision::new(1),
            crate::policy::PolicyRevision::new(1),
            SurfaceRequirementRevision::new(1),
            surface,
        );
        let output = SurfacePresentationOutputTicket::mint(
            authority_domain,
            PresentationOutputSerial::new_for_test(1),
            SurfaceSceneStamp::new(ticket, SurfaceSceneRevision::new(1)),
        );
        FrozenPresentationAuthority::new(
            PresentedSurfaceAuthority::mint_observed_for_test(output, CoordinateGeneration::new(1)),
            PopupInteractionGateRevision::default(),
        )
    }

    fn click_start_fixture() -> (ClickStart, Arc<FrozenClickAction>) {
        let epoch = WorkspaceEpoch::new(9);
        let surface = SurfaceId::new(10);
        let root = RootId::new(11);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([crate::ids::ItemId::new(12)]));
        let bar = TabBarSceneId { root, tabs };
        let bounds =
            LogicalRect::new(0.0, 0.0, 40.0, 20.0).expect("test control bounds must be valid");
        let record = TabStripControlRecord::new(
            TabStripControlId::TabListMenu(bar),
            bounds,
            true,
            SceneLayerKey::surface_base(),
        );
        let mut plan = PresentationPlan::new(surface, bounds);
        plan.push_tab_strip_control_record(record);
        let authority = EngineAuthorityDomainId::new_for_test(13);
        let ticket = SurfaceMeasurementTicket::new(
            authority,
            epoch,
            PresentationConfigRevision::new(1),
            crate::policy::PolicyRevision::new(1),
            SurfaceRequirementRevision::new(1),
            surface,
        );
        let stamp = SurfaceSceneStamp::new(ticket, SurfaceSceneRevision::new(1));
        let output = SurfacePresentationOutputTicket::mint(
            authority,
            PresentationOutputSerial::new_for_test(1),
            stamp,
        );
        let manifest = PresentationHitManifest::compile(output, &plan);
        let region = manifest
            .regions()
            .iter()
            .find(|region| {
                matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabStripControl(TabStripControlId::TabListMenu(
                        actual
                    )) if actual == bar
                )
            })
            .expect("compiled control must publish one click region")
            .id();
        let lease = PointerInputLease::new(authority, 1, PointerProviderScope::DesktopGlobal);
        let owner = GestureOwner::Stream(PointerStreamId::new(lease, PointerId::new(14), 1));
        let action = Arc::new(FrozenClickAction::TabStripControl(
            FrozenTabStripControlClick {
                key: TabStripStateKey::new(surface, bar),
                record,
            },
        ));
        (
            ClickStart {
                epoch,
                owner,
                capture_authority: Authority::Unknown(
                    crate::intent::AuthorityUnavailableReason::ProviderUnavailable,
                ),
                button: PointerButton::Primary,
                region,
                measurement: ticket,
                coordinates: SurfaceCoordinateCapture::Headless {
                    authority_generation: CoordinateGeneration::new(1),
                },
                presentation: frozen_presentation(surface, epoch),
                action: Arc::clone(&action),
            },
            action,
        )
    }

    #[test]
    fn click_generation_exhaustion_leaves_state_atomic() {
        let (start, _) = click_start_fixture();
        let mut state = InteractionState {
            last_click_generation: ClickGeneration::new(u64::MAX),
            ..InteractionState::default()
        };
        let before = state.clone();

        assert_eq!(
            state.begin_click(start),
            Err(InteractionCounterError::ClickGenerationExhausted)
        );
        assert_eq!(state, before);
    }

    #[test]
    fn active_click_clones_share_the_frozen_action() {
        let (start, action) = click_start_fixture();
        let mut state = InteractionState::default();
        state.begin_click(start).expect("test click must begin");
        assert_eq!(Arc::strong_count(&action), 2);

        let clone = state.clone();
        assert_eq!(Arc::strong_count(&action), 3);
        drop(clone);
        assert_eq!(Arc::strong_count(&action), 2);
    }
}
