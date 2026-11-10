//! Explicit drag and resize session state.

use std::sync::Arc;

mod timed;
pub use timed::PointerContinuationToken;
pub(crate) use timed::{
    DragTimedContinuation, HoverSelectionState, PointerContinuationRequest,
    PointerContinuationSession, TabStripEdgeDirection, TabStripEdgeScrollState, TouchPressState,
};

use thiserror::Error;

use crate::close_plan::ContentCloseScope;
use crate::close_plan::{CloseItemRequirement, ClosePlan, ClosePlanTarget};
use crate::command::{
    CommandOutcome, ContainedRosterSource, MovePayload, NodeSource, SplitResize, WorkspaceCommand,
};
use crate::drop_resolver::DropAffordance;
use crate::drop_target::DropTargetId;
use crate::event::{PendingReductionCause, ReductionCause};
use crate::frame::NativeCreateRequest;
use crate::geometry::{
    Constraints, LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use crate::graph::{Axis, SplitWeight};
use crate::ids::{
    FloatingPresentationId, InputSequence, ItemId, RootId, SurfaceId, WorkspaceEpoch,
};
use crate::intent::{
    Authority, CloseSceneTarget, ContainedGestureKind, ContainedPresentationOffer,
    ContainedTearOffProposal, ContainedTransformKind, NativePresentationOffer, PointerButton,
    PointerId, SurfaceBackgroundRootOffer, SurfacePointer, TabGestureSource,
};
use crate::model::{DockspaceActionOutcome, DockspaceActionRejection};
use crate::pointer_journal::{
    DesktopRouteValidation, HostLocalPointerRoute, PointerCaptureOwner, PointerEventDeliveryOwner,
    PointerInputLease, PointerProviderScope, PointerStreamId, ScrollCancelReason, ScrollPhase,
    ScrollSequenceToken,
};
use crate::policy::PolicyRevision;
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_hit::PresentationHitRegionId;
use crate::presentation_observation::PresentedSurfaceAuthority;
use crate::runtime::direct_manipulation::{
    TargetAcquisition, TargetLatchDecision, TargetObservation, reconcile_target,
};
use crate::scene::{
    PresentationLayoutFacts, SplitterResizeTarget, SurfaceCoordinateCapture, SurfaceSceneStamp,
    TabBarSceneId, TabListMenuBackdropRecord, TabListMenuRecord, TabListMenuRowRecord, TabSceneId,
    TabStripControlRecord,
};
use crate::scene_manifest::{RequirementRevision, SurfaceMeasurementTicket};
use crate::tab_strip::{
    PopupRoutingRevision, TabListMenuSessionId, TabStripControlId, TabStripStateKey,
};
use crate::transition::WorkspaceVersion;
use crate::viewport::{CoordinateGeneration, ViewportBinding};

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
    pub(crate) prepared: ContentCloseScope,
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
    Touch,
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
    pub(crate) popup_revision: PopupRoutingRevision,
}

impl FrozenPresentationAuthority {
    pub(crate) const fn new(
        presented: PresentedSurfaceAuthority,
        popup_revision: PopupRoutingRevision,
    ) -> Self {
        Self {
            presented,
            popup_revision,
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
    pub(crate) fn bind(self, session: SceneGestureSession) -> SceneGestureContinuation {
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
    pub(crate) touch: Option<TouchPressState>,
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
    current_pointer: Option<LogicalPoint>,
    origin: &'state FrozenDragOrigin,
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

    /// Returns the provider-owned pointer identity for this resize.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.owner.pointer()
    }

    /// Returns the exact journal stream which owns this resize.
    #[must_use]
    pub const fn journal_stream(self) -> PointerStreamId {
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

    /// Returns the provider-owned pointer identity for this drag.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.owner.pointer()
    }

    /// Returns the exact journal stream which owns this drag.
    #[must_use]
    pub const fn journal_stream(self) -> PointerStreamId {
        self.owner.stream()
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

    pub(crate) fn contained_projection_rect(self) -> Option<LogicalRect> {
        let FrozenDragOrigin::Contained(origin) = self.origin else {
            return None;
        };
        let current = self.current_pointer?;
        LogicalRect::new(
            origin.source_rect.x() + current.x() - origin.initial_pointer.x(),
            origin.source_rect.y() + current.y() - origin.initial_pointer.y(),
            origin.source_rect.width(),
            origin.source_rect.height(),
        )
        .ok()
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

    /// Returns the provider-owned pointer identity for this transform.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.owner.pointer()
    }

    /// Returns the exact journal stream which owns this transform.
    #[must_use]
    pub const fn journal_stream(self) -> PointerStreamId {
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
    /// The host cancelled live input after exhausting its ordinary input capacity.
    InputOverload,
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
    /// The edge cannot prove the coordinate lane frozen by the current gesture.
    CoordinateAuthorityUnavailable,
    /// The platform explicitly terminated the owning pointer stream.
    PointerStreamCancelled,
    /// The provider reported the normal terminal release of an ephemeral stream.
    PointerStreamEnded,
    /// Button state became non-authoritative.
    UnknownButtonState,
    /// Hovered-target authority became unavailable.
    UnknownTargetAuthority,
    /// A foreign native window authoritatively blocked the pointer route.
    OpaquePointerBlocker,
    /// The source window could not enter the pass-through state required by a physical route.
    PointerRoutingUnavailable,
    /// The primary release did not deliver to the exact pressed Click-lane receiver.
    ClickReceiverMismatch,
    /// Physical native-window drag capability became non-authoritative.
    NativeCapabilityUnknown,
    /// Physical native-window drag capability became authoritatively unavailable.
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
    /// A presentation ownership transfer already owns the shared preview lane.
    PresentationTransitionPending,
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
    /// Split weights were rejected during candidate validation.
    ResizeRejected(crate::error::CommandError),
    /// The pressed point lies outside the claimed painted surface.
    SplitterGesturePointerOutsideSurface { surface: SurfaceId },
    /// No splitter target owns the pressed point on the authoritative layer.
    SplitterGestureHitUnavailable { surface: SurfaceId },
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
    /// Release re-resolution no longer matches the exact painted transform proof.
    ContainedTransformChanged,
}

impl InteractionRejection {
    fn owner_mismatch(expected: GestureOwner, actual: GestureOwner) -> Self {
        Self::PointerStreamMismatch {
            expected: expected.stream(),
            actual: actual.stream(),
        }
    }
}

/// Public semantic category of a terminal manipulation rejection.
///
/// Categories deliberately omit scene proofs, graph nodes, and error details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ManipulationRejection {
    /// The required presentation is unavailable or no longer current.
    PresentationUnavailable,
    /// The source content or presentation is no longer available.
    SourceUnavailable,
    /// The release cannot deliver to the requested target.
    TargetUnavailable,
    /// Input delivery, capture, or coordinate authority is unavailable.
    InputAuthorityUnavailable,
    /// Another gesture or presentation transition owns the operation.
    GestureConflict,
    /// Current application policy denies the operation.
    PolicyDenied,
    /// A checked workspace command conflicts with current state.
    Conflict,
    /// The requested control or action is unavailable.
    ActionUnavailable,
    /// The engine could not complete the checked command.
    Internal,
}

impl ManipulationRejection {
    pub(crate) fn from_interaction(reason: &InteractionRejection) -> Self {
        match reason {
            InteractionRejection::CommandRejected(error)
            | InteractionRejection::ResizeRejected(error)
            | InteractionRejection::CloseSourceUnavailable(error) => match error {
                crate::error::CommandError::Policy(_) => Self::PolicyDenied,
                error if error.is_expected_rejection() => Self::Conflict,
                _ => Self::Internal,
            },
            InteractionRejection::SemanticPresentationUnavailable { .. }
            | InteractionRejection::SemanticOutputUnavailable { .. }
            | InteractionRejection::SemanticEmissionUnavailable { .. }
            | InteractionRejection::PreviewNotPainted
            | InteractionRejection::StaleScene => Self::PresentationUnavailable,
            InteractionRejection::TabGestureSourceUnavailable { .. }
            | InteractionRejection::ContainedTransformPresentationUnavailable { .. }
            | InteractionRejection::TabStripSourceUnavailable { .. } => Self::SourceUnavailable,
            InteractionRejection::TargetChanged
            | InteractionRejection::PreviewMissing
            | InteractionRejection::ContainedTransformChanged
            | InteractionRejection::ContainedPlacementUnavailable(_) => Self::TargetUnavailable,
            InteractionRejection::TargetAuthorityInvalid
            | InteractionRejection::CaptureOwnerMismatch { .. }
            | InteractionRejection::DeliveryAuthorityUnavailable
            | InteractionRejection::DeliveryOwnerMismatch { .. }
            | InteractionRejection::SemanticDeliveryMismatch { .. }
            | InteractionRejection::CloseCoordinateAuthorityUnavailable { .. }
            | InteractionRejection::ContainedGestureCoordinateAuthorityUnavailable { .. }
            | InteractionRejection::TabGestureCoordinateAuthorityUnavailable { .. }
            | InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable { .. } => {
                Self::InputAuthorityUnavailable
            }
            InteractionRejection::GestureBusy { .. }
            | InteractionRejection::PresentationTransitionPending => Self::GestureConflict,
            _ => Self::ActionUnavailable,
        }
    }

    pub(crate) const fn from_product(reason: DockspaceActionRejection) -> Self {
        match reason {
            DockspaceActionRejection::PolicyDenied => Self::PolicyDenied,
            DockspaceActionRejection::Conflict => Self::Conflict,
            DockspaceActionRejection::IdentityExhausted => Self::Internal,
            DockspaceActionRejection::PresentationUnavailable { .. } => {
                Self::PresentationUnavailable
            }
            _ => Self::ActionUnavailable,
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
    /// The platform explicitly cannot support the required native interaction.
    NativeCapabilityUnavailable,
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
    /// The active workspace popup blocks scrolling on another surface.
    PopupBlocked,
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
    /// The host cancelled live input after exhausting its ordinary input capacity.
    InputOverload,
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
    /// A local touch completed its finite long-press qualification.
    TouchArmed {
        pointer: PointerId,
        surface: SurfaceId,
    },
    /// A local touch activated chrome on early release.
    TouchActivated {
        pointer: PointerId,
        surface: SurfaceId,
    },
    /// Pre-arm motion yielded without changing selection or layout.
    TouchYieldedToContent {
        pointer: PointerId,
        surface: SurfaceId,
    },
    /// A stationary armed release requests the host's semantic context menu.
    ContextMenuRequested {
        pointer: PointerId,
        surface: SurfaceId,
        root: RootId,
        item: Option<ItemId>,
        position: LogicalPoint,
    },
    /// One pointer-journal scroll reduction or lifecycle terminal transition.
    Scroll(ScrollReductionOutcome),
    /// One exact close plan was opened without mutating workspace topology.
    CloseRequested {
        plan: ClosePlan,
        /// True when repeated activation reused the same unresolved request.
        reused: bool,
    },
    /// One scene-bound presentation action committed or produced a valid no-op.
    ProductActionApplied(DockspaceActionOutcome),
    /// One scene-bound presentation action was rejected without mutation.
    ProductActionRejected(DockspaceActionRejection),
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
    /// A consumed drag release awaits exact final presentation.
    ReleasePending { session: DragSessionId },
    /// One consumed release or otherwise unowned press was rejected once.
    Rejected {
        /// Gesture identity retained from before the terminal edge, or `Idle` for a press.
        status: InteractionStatus,
        /// Public semantic category without private validation details.
        reason: ManipulationRejection,
    },
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
    PresentationRehome {
        command: WorkspaceCommand,
    },
}

fn preview_acquisition_matches(
    scene: SurfaceSceneStamp,
    proof: &PreviewProof,
    acquisition: Option<TargetAcquisition>,
) -> bool {
    match (proof, acquisition) {
        (PreviewProof::Dock { target, .. }, Some(acquisition)) => {
            acquisition.scene() == scene && acquisition.target() == *target
        }
        (
            PreviewProof::Contained { .. }
            | PreviewProof::Native { .. }
            | PreviewProof::PresentationRehome { .. },
            None,
        ) => true,
        (PreviewProof::Dock { .. }, None)
        | (
            PreviewProof::Contained { .. }
            | PreviewProof::Native { .. }
            | PreviewProof::PresentationRehome { .. },
            Some(_),
        ) => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ManipulationPositionKey {
    JournalSurface {
        scope: PointerProviderScope,
        position: Authority<LogicalPoint>,
    },
    JournalHost {
        manipulation: Option<HostLocalPointerRoute>,
        hover: Option<HostLocalPointerRoute>,
    },
    JournalNativeLocal {
        manipulation: Option<crate::pointer_journal::NativeSurfacePointerRoute>,
        hover: Option<crate::pointer_journal::NativeSurfacePointerRoute>,
    },
    JournalDesktop(DesktopRouteValidation),
}

impl ManipulationPositionKey {
    pub(crate) const fn surface(self) -> Option<SurfaceId> {
        match self {
            Self::JournalHost { hover, .. } => match hover {
                Some(route) => Some(route.surface()),
                None => None,
            },
            Self::JournalNativeLocal { hover, .. } => match hover {
                Some(route) => Some(route.binding().surface()),
                None => None,
            },
            Self::JournalSurface {
                scope: PointerProviderScope::SurfaceLocal(scope),
                ..
            } => Some(scope.surface()),
            Self::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::Dock(route),
            )) => Some(route.binding().surface()),
            Self::JournalSurface { .. } | Self::JournalDesktop(_) => None,
        }
    }

    pub(crate) const fn hover_probe(self) -> Option<SurfacePointer> {
        match self {
            Self::JournalHost { hover, .. } => match hover {
                Some(route) => Some(SurfacePointer::new(route.surface(), route.position())),
                None => None,
            },
            Self::JournalNativeLocal { hover, .. } => match hover {
                Some(route) => Some(SurfacePointer::new(
                    route.binding().surface(),
                    route.surface_position(),
                )),
                None => None,
            },
            Self::JournalSurface {
                scope: PointerProviderScope::SurfaceLocal(scope),
                position: Authority::Known(point),
            } => Some(SurfacePointer::new(scope.surface(), point)),
            Self::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::Dock(route),
            )) => Some(SurfacePointer::new(
                route.binding().surface(),
                route.surface_position(),
            )),
            Self::JournalSurface { .. } | Self::JournalDesktop(_) => None,
        }
    }

    /// Returns whether this edge can ask the adapter for a fresh hover fact.
    ///
    /// A surface-local provider still has a hover question when its coordinate
    /// is unknown; the adapter must answer that question with a typed unknown
    /// result instead of making the edge look not-applicable. A validated
    /// outside-all desktop route is the only active-drag position with no
    /// surface receiver to probe.
    pub(crate) const fn hover_probe_requested(self) -> bool {
        !matches!(
            self,
            Self::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::NoWindow { .. },
            ))
        )
    }

    /// Returns the surface the adapter should associate with a hover probe,
    /// even when the event-time logical point itself is unavailable.
    pub(crate) const fn hover_surface(self) -> Option<SurfaceId> {
        match self {
            Self::JournalHost { hover, .. } => match hover {
                Some(route) => Some(route.surface()),
                None => None,
            },
            Self::JournalNativeLocal { hover, .. } => match hover {
                Some(route) => Some(route.binding().surface()),
                None => None,
            },
            Self::JournalSurface {
                scope: PointerProviderScope::SurfaceLocal(scope),
                ..
            } => Some(scope.surface()),
            Self::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::Dock(route),
            )) => Some(route.binding().surface()),
            Self::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::NoWindow { .. }
                | crate::pointer_journal::ValidatedDesktopRoute::Foreign { .. },
            ))
            | Self::JournalDesktop(DesktopRouteValidation::Unavailable(_))
            | Self::JournalSurface { .. } => None,
        }
    }

    pub(crate) const fn logical_point(self) -> Option<LogicalPoint> {
        match self.hover_probe() {
            Some(pointer) => Some(pointer.position()),
            None => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PublishedPreview {
    public: InteractionPreview,
    proof: PreviewProof,
    painted: bool,
    target_layout_basis: Option<SurfaceSceneStamp>,
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

    pub(crate) const fn target_layout_basis(&self) -> Option<SurfaceSceneStamp> {
        self.target_layout_basis
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DragPreviewState {
    Unresolved {
        status: PreviewResolutionStatus,
    },
    Published {
        preview: PublishedPreview,
        acquisition: Option<TargetAcquisition>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum MagneticSourceGate {
    Open,
    ExplicitTargetsOnly,
}

impl MagneticSourceGate {
    const fn from_origin(origin: &FrozenDragOrigin) -> Self {
        match origin {
            FrozenDragOrigin::Workspace => Self::Open,
            FrozenDragOrigin::Contained(origin) => match origin.intent {
                ContainedDragIntent::MoveWindow => Self::ExplicitTargetsOnly,
                ContainedDragIntent::DockContent => Self::Open,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DragTargetState {
    last_advanced: Option<ManipulationPositionKey>,
    /// 最近一次可用于重新取证的逻辑位置；authority key 失效时仍保留它。
    last_logical_point: Option<LogicalPoint>,
    magnetic_source_gate: MagneticSourceGate,
    affordance: Option<DropAffordance>,
    preview: DragPreviewState,
}

impl DragTargetState {
    const fn new(origin: &FrozenDragOrigin) -> Self {
        Self {
            last_advanced: None,
            last_logical_point: None,
            magnetic_source_gate: MagneticSourceGate::from_origin(origin),
            affordance: None,
            preview: DragPreviewState::Unresolved {
                status: PreviewResolutionStatus::KnownNone,
            },
        }
    }

    pub(crate) const fn affordance(&self) -> Option<&DropAffordance> {
        self.affordance.as_ref()
    }

    pub(crate) const fn last_advanced(&self) -> Option<ManipulationPositionKey> {
        self.last_advanced
    }

    pub(crate) const fn current_pointer(
        &self,
        initial_pointer: Option<LogicalPoint>,
    ) -> Option<LogicalPoint> {
        if let Some(position) = self.last_advanced
            && let Some(point) = position.logical_point()
        {
            return Some(point);
        }
        if let Some(point) = self.last_logical_point {
            return Some(point);
        }
        initial_pointer
    }

    pub(crate) const fn acquisition(&self) -> Option<TargetAcquisition> {
        match &self.preview {
            DragPreviewState::Published { acquisition, .. } => *acquisition,
            DragPreviewState::Unresolved { .. } => None,
        }
    }

    pub(crate) const fn preview(&self) -> Option<&PublishedPreview> {
        match &self.preview {
            DragPreviewState::Published { preview, .. } => Some(preview),
            DragPreviewState::Unresolved { .. } => None,
        }
    }

    fn preview_mut(&mut self) -> Option<&mut PublishedPreview> {
        match &mut self.preview {
            DragPreviewState::Published { preview, .. } => Some(preview),
            DragPreviewState::Unresolved { .. } => None,
        }
    }

    pub(crate) const fn status(&self) -> PreviewResolutionStatus {
        match &self.preview {
            DragPreviewState::Published { .. } => PreviewResolutionStatus::Resolved,
            DragPreviewState::Unresolved { status, .. } => *status,
        }
    }

    fn publish(&mut self, preview: PublishedPreview, acquisition: Option<TargetAcquisition>) {
        self.preview = DragPreviewState::Published {
            preview,
            acquisition,
        };
    }

    fn clear_preview(&mut self, status: PreviewResolutionStatus) -> bool {
        let changed = matches!(self.preview, DragPreviewState::Published { .. });
        self.preview = DragPreviewState::Unresolved { status };
        changed
    }

    fn set_acquisition(&mut self, acquisition: Option<TargetAcquisition>) {
        match &mut self.preview {
            DragPreviewState::Published {
                acquisition: current,
                ..
            } => *current = acquisition,
            DragPreviewState::Unresolved { .. } => {}
        }
    }

    fn clear_feedback(&mut self, status: PreviewResolutionStatus) -> bool {
        self.last_advanced = None;
        let affordance_cleared = self.affordance.take().is_some();
        let preview_cleared = self.clear_preview(status);
        affordance_cleared || preview_cleared
    }

    fn reset_coordinate_lane(&mut self) {
        // Preserve source intent while discarding coordinate-dependent feedback.
        self.clear_feedback(PreviewResolutionStatus::Unavailable);
        self.last_logical_point = None;
    }

    fn reconcile_acquisition(
        &self,
        observation: TargetObservation,
        point: Option<LogicalPoint>,
        release_hysteresis: f64,
    ) -> TargetLatchDecision {
        reconcile_target(self.acquisition(), observation, point, release_hysteresis)
    }

    fn set_affordance(&mut self, affordance: Option<DropAffordance>) {
        self.affordance = affordance;
    }

    fn set_last_advanced(&mut self, position: ManipulationPositionKey) {
        if let Some(point) = position.logical_point() {
            self.last_logical_point = Some(point);
        }
        self.last_advanced = Some(position);
    }

    fn invalidate_last_advanced(&mut self) {
        self.last_advanced = None;
    }
}

/// Coordinate lane frozen by a validated press for one transient gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum GestureLane {
    Desktop,
    NativeLocal {
        binding: crate::viewport::WindowBinding,
        coordinate_generation: CoordinateGeneration,
    },
    HostLocal,
    SurfaceLocal,
}

/// Exact journal stream and coordinate lane owning one transient gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GestureOwner {
    stream: PointerStreamId,
    lane: GestureLane,
}

impl GestureOwner {
    pub(crate) const fn new(stream: PointerStreamId, lane: GestureLane) -> Self {
        Self { stream, lane }
    }

    pub(crate) const fn stream(self) -> PointerStreamId {
        self.stream
    }

    pub(crate) const fn lane(self) -> GestureLane {
        self.lane
    }

    pub(crate) const fn pointer(self) -> PointerId {
        self.stream.pointer()
    }

    pub(crate) const fn lease(self) -> PointerInputLease {
        self.stream.lease()
    }

    pub(crate) const fn is_desktop(self) -> bool {
        matches!(self.lane, GestureLane::Desktop)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ArmedDrag {
    pub(crate) session: DragSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) journal_threshold_origin: JournalDragThresholdOrigin,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: FrozenPresentationAuthority,
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
    HostLocal {
        surface: SurfaceId,
        position: LogicalPoint,
    },
    NativeLocal {
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
        position: LogicalPoint,
    },
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
    pub(crate) intent: ContainedDragIntent,
    pub(crate) source_rect: LogicalRect,
    pub(crate) source_roster: ContainedRosterSource,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) minimum_size: LogicalSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainedDragIntent {
    MoveWindow,
    DockContent,
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
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) journal_threshold_origin: JournalDragThresholdOrigin,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: FrozenPresentationAuthority,
    pub(crate) source_layout_facts: Option<Arc<PresentationLayoutFacts>>,
    pub(crate) journal_source_geometry: Option<JournalDragSourceGeometry>,
    pub(crate) continuation: Option<SceneGestureContinuationDraft>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveDrag {
    pub(crate) session: DragSessionId,
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) payload: MovePayload,
    pub(crate) source_surface: SurfaceId,
    pub(crate) initial_pointer: Option<LogicalPoint>,
    pub(crate) complete_root: Option<NodeSource>,
    pub(crate) partial_detachable: bool,
    pub(crate) origin: FrozenDragOrigin,
    pub(crate) source_validated_at: WorkspaceVersion,
    pub(crate) presentation: FrozenPresentationAuthority,
    pub(crate) source_layout_facts: Option<Arc<PresentationLayoutFacts>>,
    pub(crate) journal_source_geometry: Option<JournalDragSourceGeometry>,
    pub(crate) continuation: Option<SceneGestureContinuation>,
    pub(crate) timed: Option<DragTimedContinuation>,
    pub(crate) capture_acquired_at: Option<std::time::Duration>,
    pub(crate) journal_presentation_reservation: Option<JournalPresentationReservation>,
    pub(crate) contained_offer: Option<ContainedPresentationOffer>,
    pub(crate) surface_background_offer: Option<SurfaceBackgroundRootOffer>,
    pub(crate) native_offer: Option<NativePresentationOffer>,
    pub(crate) target: DragTargetState,
}

impl ActiveDrag {
    pub(crate) const fn preview(&self) -> Option<&PublishedPreview> {
        self.target.preview()
    }

    pub(crate) const fn preview_status(&self) -> PreviewResolutionStatus {
        self.target.status()
    }

    pub(crate) const fn drop_affordance(&self) -> Option<&DropAffordance> {
        self.target.affordance()
    }

    pub(crate) const fn last_advanced_position(&self) -> Option<ManipulationPositionKey> {
        self.target.last_advanced()
    }

    pub(crate) const fn target_acquisition(&self) -> Option<TargetAcquisition> {
        self.target.acquisition()
    }

    pub(crate) fn magnetic_targeting_allowed(&self) -> bool {
        matches!(self.target.magnetic_source_gate, MagneticSourceGate::Open)
    }

    pub(crate) const fn current_pointer(&self) -> Option<LogicalPoint> {
        self.target.current_pointer(self.initial_pointer)
    }
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
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) target: SplitterResizeTarget,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) authority: FrozenPresentationAuthority,
    pub(crate) initial_pointer: LogicalPoint,
    pub(crate) current_pointer: LogicalPoint,
    pub(crate) axis_groups: Vec<FrozenResizeAxisGroup>,
    pub(crate) updates: Vec<SplitResize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenResizeSplit {
    pub(crate) source: NodeSource,
    pub(crate) axis: Axis,
    pub(crate) bounds: LogicalRect,
    pub(crate) constraints: Constraints,
    pub(crate) weights: Vec<SplitWeight>,
    pub(crate) child_extents: Vec<f64>,
    pub(crate) child_minimum_extents: Vec<f64>,
    pub(crate) child_maximum_extents: Vec<f64>,
    /// Extent changes per unit displacement along the containing group's axis.
    pub(crate) child_coefficients: Vec<f64>,
    pub(crate) central_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FrozenResizeAxisGroup {
    pub(crate) axis: Axis,
    pub(crate) splits: Vec<FrozenResizeSplit>,
    pub(crate) common_delta: ResizeDeltaInterval,
}

impl FrozenResizeAxisGroup {
    pub(crate) fn sources(&self) -> impl Iterator<Item = &NodeSource> {
        self.splits.iter().map(|split| &split.source)
    }

    pub(crate) fn resized_sources(&self) -> impl Iterator<Item = &NodeSource> {
        self.splits
            .iter()
            .filter(|split| split.axis == self.axis)
            .map(|split| &split.source)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResizeDeltaInterval {
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResizeStart {
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
    pub(crate) button: PointerButton,
    pub(crate) surface: SurfaceId,
    pub(crate) scene: SurfaceSceneStamp,
    pub(crate) target: SplitterResizeTarget,
    pub(crate) coordinate_capture: SurfaceCoordinateCapture,
    pub(crate) authority: FrozenPresentationAuthority,
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
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
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
    pub(crate) presentation: FrozenPresentationAuthority,
    pub(crate) continuation: Option<SceneGestureContinuation>,
    pub(crate) preview: Option<PublishedContainedTransformPreview>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainedTransformStart {
    pub(crate) owner: GestureOwner,
    pub(crate) journal_capture_authority: Authority<PointerCaptureOwner>,
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
    pub(crate) presentation: FrozenPresentationAuthority,
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
    magnetic_targeting_enabled: bool,
    last_click_generation: ClickGeneration,
    last_drag_generation: DragGeneration,
    last_resize_generation: ResizeGeneration,
    last_contained_transform_generation: ContainedTransformGeneration,
    last_preview_sequence: PreviewSequence,
    last_continuation_generation: u64,
    last_consumed_drag: Option<DragSessionId>,
    last_consumed_contained_transform: Option<ContainedTransformSessionId>,
}

impl InteractionState {
    pub(crate) const fn magnetic_targeting_enabled(&self) -> bool {
        self.magnetic_targeting_enabled
    }

    pub(crate) fn set_magnetic_targeting(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.magnetic_targeting_enabled, enabled) != enabled
    }

    pub(crate) fn prepare_presentation_rehome_preview(
        &mut self,
        epoch: WorkspaceEpoch,
        scene: SurfaceSceneStamp,
        visual: PreviewVisual,
        proof: PreviewProof,
    ) -> Result<PublishedPreview, InteractionCounterError> {
        let generation = self
            .last_drag_generation
            .checked_next()
            .ok_or(InteractionCounterError::DragGenerationExhausted)?;
        let sequence = self
            .last_preview_sequence
            .checked_next()
            .ok_or(InteractionCounterError::PreviewSequenceExhausted)?;
        self.last_drag_generation = generation;
        self.last_preview_sequence = sequence;
        Ok(PublishedPreview {
            public: InteractionPreview {
                token: PreviewToken {
                    session: DragSessionId::new(epoch, generation),
                    scene,
                    sequence,
                },
                visual,
            },
            proof,
            painted: false,
            target_layout_basis: None,
        })
    }

    pub(crate) const fn active_surface(&self) -> Option<SurfaceId> {
        match &self.active {
            ActiveGesture::Idle => None,
            ActiveGesture::Pressed(click) => Some(click.presentation.surface()),
            ActiveGesture::Armed(drag) => Some(drag.source_surface),
            ActiveGesture::Dragging(drag) => Some(drag.source_surface),
            ActiveGesture::Resizing(resize) => Some(resize.surface),
            ActiveGesture::ContainedTransforming(transform) => Some(transform.surface),
        }
    }

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

    /// Returns the current gesture's journal stream.
    pub(crate) const fn active_stream(&self) -> Option<PointerStreamId> {
        match self.active_owner() {
            Some(owner) => Some(owner.stream()),
            None => None,
        }
    }

    pub(crate) const fn active_journal_capture_authority(
        &self,
    ) -> Option<Authority<PointerCaptureOwner>> {
        match &self.active {
            ActiveGesture::Pressed(click) => Some(click.capture_authority),
            ActiveGesture::Armed(drag) => Some(drag.journal_capture_authority),
            ActiveGesture::Dragging(drag) => Some(drag.journal_capture_authority),
            ActiveGesture::Resizing(resize) => Some(resize.journal_capture_authority),
            ActiveGesture::ContainedTransforming(transform) => {
                Some(transform.journal_capture_authority)
            }
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
            ActiveGesture::Armed(drag) => drag.journal_capture_authority = capture,
            ActiveGesture::Dragging(drag) => drag.journal_capture_authority = capture,
            ActiveGesture::Resizing(resize) => resize.journal_capture_authority = capture,
            ActiveGesture::ContainedTransforming(transform) => {
                transform.journal_capture_authority = capture;
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
            ActiveGesture::Armed(drag) => Some(drag.presentation),
            ActiveGesture::Dragging(drag) => Some(drag.presentation),
            ActiveGesture::Resizing(resize) => Some(resize.authority),
            ActiveGesture::ContainedTransforming(transform) => Some(transform.presentation),
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
        if start.button != PointerButton::Primary {
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
            touch: None,
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

    pub(crate) fn active_click_mut(
        &mut self,
        session: ClickSessionId,
    ) -> Result<&mut ActiveClick, InteractionRejection> {
        match &mut self.active {
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
                current_pointer: drag.initial_pointer,
                origin: &drag.origin,
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
                current_pointer: drag.current_pointer(),
                origin: &drag.origin,
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
            ActiveGesture::Dragging(drag) => drag.preview().map(PublishedPreview::public),
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
            ActiveGesture::Dragging(drag) => drag.drop_affordance(),
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
        let target = DragTargetState::new(&armed.origin);
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
            timed: None,
            capture_acquired_at: None,
            journal_presentation_reservation: None,
            contained_offer: None,
            surface_background_offer: None,
            native_offer: None,
            target,
        }));
        Ok(())
    }

    /// Rebinds an active native-local drag after the caller proves desktop continuity.
    ///
    /// All fallible identity checks precede mutation so the source continuation
    /// and drag owner change together without retaining target facts from the old lane.
    pub(crate) fn upgrade_drag_to_desktop(
        &mut self,
        session: DragSessionId,
        expected_owner: GestureOwner,
    ) -> Result<(), InteractionRejection> {
        let ActiveGesture::Dragging(drag) = &mut self.active else {
            return Err(InteractionRejection::SessionMismatch);
        };
        if drag.session != session
            || drag.owner != expected_owner
            || !matches!(expected_owner.lane(), GestureLane::NativeLocal { .. })
            || drag.continuation.as_ref().is_some_and(|continuation| {
                continuation.session != SceneGestureSession::Drag(session)
                    || continuation.draft.owner != expected_owner
            })
        {
            return Err(InteractionRejection::SessionMismatch);
        }

        let owner = GestureOwner::new(expected_owner.stream(), GestureLane::Desktop);
        drag.owner = owner;
        if let Some(continuation) = &mut drag.continuation {
            continuation.draft.owner = owner;
        }
        drag.timed = None;
        drag.target.reset_coordinate_lane();
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
        acquisition: Option<TargetAcquisition>,
    ) -> Result<(InteractionPreview, bool), InteractionCounterError> {
        if !preview_acquisition_matches(scene, &proof, acquisition) {
            return Err(InteractionCounterError::StateInvariant);
        }
        if let Ok(drag) = self.active_drag(session)
            && let Some(existing) = drag.preview()
            && existing.public.token.scene == scene
            && existing.public.visual == visual
            && existing.proof == proof
        {
            let existing = existing.public.clone();
            self.active_drag_mut(session)
                .map_err(|_| InteractionCounterError::StateInvariant)?
                .target
                .set_acquisition(acquisition);
            return Ok((existing, false));
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
        drag.target.publish(
            PublishedPreview {
                public: preview.clone(),
                proof,
                painted: false,
                target_layout_basis: None,
            },
            acquisition,
        );
        Ok((preview, true))
    }

    pub(crate) fn bind_preview_layout_basis(
        &mut self,
        session: DragSessionId,
        basis: Option<SurfaceSceneStamp>,
    ) -> Result<(), InteractionRejection> {
        let drag = self.active_drag_mut(session)?;
        if let DragPreviewState::Published { preview, .. } = &mut drag.target.preview {
            preview.target_layout_basis = basis;
        }
        Ok(())
    }

    pub(crate) fn clear_preview(
        &mut self,
        session: DragSessionId,
        status: PreviewResolutionStatus,
    ) -> Result<bool, InteractionRejection> {
        Ok(self.active_drag_mut(session)?.target.clear_preview(status))
    }

    pub(crate) fn set_drag_position(
        &mut self,
        session: DragSessionId,
        position: ManipulationPositionKey,
    ) -> Result<(), InteractionRejection> {
        self.active_drag_mut(session)?
            .target
            .set_last_advanced(position);
        Ok(())
    }

    pub(crate) fn reconcile_drag_target(
        &self,
        session: DragSessionId,
        observation: TargetObservation,
        point: Option<LogicalPoint>,
        release_hysteresis: f64,
    ) -> Result<TargetLatchDecision, InteractionRejection> {
        Ok(self.active_drag(session)?.target.reconcile_acquisition(
            observation,
            point,
            release_hysteresis,
        ))
    }

    pub(crate) fn invalidate_drag_position(
        &mut self,
        session: DragSessionId,
    ) -> Result<(), InteractionRejection> {
        self.active_drag_mut(session)?
            .target
            .invalidate_last_advanced();
        Ok(())
    }

    pub(crate) fn clear_drag_feedback(
        &mut self,
        session: DragSessionId,
    ) -> Result<bool, InteractionRejection> {
        let drag = self.active_drag_mut(session)?;
        Ok(drag
            .target
            .clear_feedback(PreviewResolutionStatus::Unavailable))
    }

    pub(crate) fn set_drop_affordance(
        &mut self,
        session: DragSessionId,
        affordance: Option<DropAffordance>,
    ) -> Result<(), InteractionRejection> {
        self.active_drag_mut(session)?
            .target
            .set_affordance(affordance);
        Ok(())
    }

    pub(crate) fn observe_presented_preview(
        &mut self,
        surface: SurfaceId,
        token: PreviewToken,
    ) -> bool {
        let ActiveGesture::Dragging(drag) = &mut self.active else {
            return false;
        };
        let Some(preview) = drag.target.preview_mut() else {
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
            magnetic_targeting_enabled: true,
            last_click_generation: ClickGeneration::default(),
            last_drag_generation: DragGeneration::default(),
            last_resize_generation: ResizeGeneration::default(),
            last_contained_transform_generation: ContainedTransformGeneration::default(),
            last_preview_sequence: PreviewSequence::default(),
            last_continuation_generation: 0,
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
    /// Timed continuation identities cannot advance without wrapping.
    #[error("pointer continuation generation is exhausted")]
    ContinuationGenerationExhausted,
    /// Internal code attempted to publish a preview outside its live drag.
    #[error("interaction state invariant failed while publishing a preview")]
    StateInvariant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::CoordinateSnapshot;
    use crate::drop_guide::{DropGuideClusterRecord, DropGuideTargetRecord};
    use crate::drop_resolver::{DropResolution, resolve_presented_drop};
    use crate::drop_target::{
        DropTargetAvailability, DropTargetLocation, DropTargetRecord, DropVisual, SceneLayerKey,
    };
    use crate::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
    use crate::hit_region::HitRegion;
    use crate::ids::{EngineAuthorityDomainId, ReducerTickId, WorkspaceRevision};
    use crate::intent::Authority;
    use crate::platform::WindowCoordinateObservation;
    use crate::pointer_journal::{PointerInputLease, PointerProviderScope, PointerStreamId};
    use crate::policy::DockPolicySnapshot;
    use crate::presentation_config::PresentationConfigRevision;
    use crate::presentation_hit::{PresentationHitManifest, PresentationHitRegionKind};
    use crate::presentation_observation::{
        PresentationLedger, PresentationOutputSerial, SurfacePresentationOutputTicket,
    };
    use crate::scene::{
        PresentationPlan, SurfaceProgramRootOperation, SurfaceProgramRootPresentation,
        TabBarSceneId, TabStripControlRecord,
    };
    use crate::scene_manifest::{SurfaceRequirementRevision, SurfaceSceneRevision};
    use crate::tab_strip::{TabStripControlId, TabStripStateKey};
    use crate::viewport::{
        CoordinateGeneration, CoordinateObservationGeneration, WindowIncarnation, WindowToken,
    };
    use crate::workspace::WorkspaceIndex;

    fn frozen_scene(surface: SurfaceId, epoch: WorkspaceEpoch) -> SurfaceSceneStamp {
        SurfaceSceneStamp::new(
            SurfaceMeasurementTicket::new(
                EngineAuthorityDomainId::new_for_test(90),
                epoch,
                PresentationConfigRevision::new(1),
                PolicyRevision::new(1),
                SurfaceRequirementRevision::new(1),
                surface,
            ),
            SurfaceSceneRevision::new(1),
        )
    }

    fn frozen_presentation(
        surface: SurfaceId,
        epoch: WorkspaceEpoch,
    ) -> FrozenPresentationAuthority {
        let output = SurfacePresentationOutputTicket::mint(
            EngineAuthorityDomainId::new_for_test(90),
            PresentationOutputSerial::new_for_test(1),
            frozen_scene(surface, epoch),
        );
        FrozenPresentationAuthority::new(
            PresentedSurfaceAuthority::mint_observed_for_test(output, CoordinateGeneration::new(1)),
            PopupRoutingRevision::default(),
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
        plan.begin_surface_program_root(root, SurfaceProgramRootPresentation::Main)
            .expect("test control root opens");
        plan.push_surface_program_root_operation(SurfaceProgramRootOperation::TabStripControl(
            record.id(),
        ))
        .expect("test control enters the surface program");
        plan.end_surface_program_root(root)
            .expect("test control root closes");
        plan.finish_surface_program()
            .expect("test control program finishes");
        plan.link_surface_program()
            .expect("test control program links its actual record row");
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
        let manifest = PresentationHitManifest::compile(output, &plan)
            .expect("test plan should resolve through its surface program");
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
        let owner = GestureOwner::new(
            PointerStreamId::new(lease, PointerId::new(14), 1),
            GestureLane::Desktop,
        );
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

    fn native_local_drag_start_fixture(
        contained_intent: Option<ContainedDragIntent>,
    ) -> (DragArmStart, Workspace) {
        let epoch = WorkspaceEpoch::new(23);
        let surface = SurfaceId::new(21);
        let root = RootId::new(22);
        let target_root = RootId::new(24);
        let target_surface = SurfaceId::new(25);
        let floating = FloatingPresentationId::new(26);
        let source_rect =
            LogicalRect::new(0.0, 0.0, 480.0, 360.0).expect("test source bounds must be valid");
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        let target_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_root(target_root, RootRecord::new(target_tabs));
        if contained_intent.is_some() {
            builder.set_surface(surface, SurfacePresentation::rootless());
            builder.set_contained_floating(floating, ContainedFloating::new(root, source_rect));
            builder
                .attach_contained(surface, floating)
                .expect("test contained source surface must exist");
        } else {
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        builder.set_surface(target_surface, SurfacePresentation::with_main(target_root));
        let workspace = builder.build().expect("test drag workspace must be valid");
        let source = workspace
            .capture_node_source(root, tabs)
            .expect("test drag source must be current");
        let presentation = frozen_presentation(surface, epoch);
        let domain = EngineAuthorityDomainId::new_for_test(90);
        let host = PresentationLedger::new(domain)
            .create_host()
            .expect("test joined host must mint");
        let lease = PointerInputLease::new(domain, 1, PointerProviderScope::NativeJoined(host));
        let binding = ViewportBinding::new(
            domain,
            epoch,
            surface,
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let generation = CoordinateGeneration::new(1);
        let owner = GestureOwner::new(
            PointerStreamId::new(lease, PointerId::new(1), 1),
            GestureLane::NativeLocal {
                binding: binding.window(),
                coordinate_generation: generation,
            },
        );
        let coordinates = SurfaceCoordinateCapture::NativeReady {
            coordinates: CoordinateSnapshot::from_observation(
                binding,
                generation,
                WindowCoordinateObservation::new(
                    binding,
                    CoordinateObservationGeneration::new(1),
                    Authority::Known(
                        PhysicalRect::new(400.0, -300.0, 1200.0, 900.0)
                            .expect("test content bounds must be valid"),
                    ),
                    Authority::Unknown(crate::intent::AuthorityUnavailableReason::NotReported),
                    Authority::Known(ScaleFactor::new(2.0).expect("test native scale is valid")),
                    Authority::Known(
                        ScaleFactor::new(2.5).expect("test presentation scale is valid"),
                    ),
                ),
            )
            .expect("test source coordinates must be complete"),
            authority_generation: generation,
        };
        let payload = MovePayload::Tabs(source.clone());
        let initial_pointer = LogicalPoint::new(4.0, 3.0).expect("test pointer is finite");
        let version = WorkspaceVersion::new(epoch, WorkspaceRevision::new(7));
        let minimum_size =
            LogicalSize::new(120.0, 90.0).expect("test source minimum must be valid");
        let origin = match contained_intent {
            Some(intent) => FrozenDragOrigin::Contained(FrozenContainedDragOrigin {
                surface,
                root,
                floating,
                intent,
                source_rect,
                source_roster: workspace
                    .capture_contained_roster(surface)
                    .expect("test contained roster must be current"),
                initial_pointer,
                minimum_size,
            }),
            None => FrozenDragOrigin::Workspace,
        };
        let continuation = SceneGestureContinuationDraft {
            activation: ReductionCause::SurfaceContribution {
                tick: ReducerTickId::new(9),
                surface,
                base: frozen_scene(surface, epoch),
            },
            owner,
            origin: presentation,
            workspace: version,
            policy: PolicyRevision::new(1),
            config: PresentationConfigRevision::new(1),
            requirements: RequirementRevision::new(4),
            popup_routing: PopupRoutingRevision::new_for_test(5),
            source: SceneGestureContinuationSource::Drag {
                payload: payload.clone(),
                source_surface: surface,
                complete_root: Some(source.clone()),
                origin: origin.clone(),
                coordinates,
            },
        };
        (
            DragArmStart {
                epoch,
                owner,
                journal_capture_authority: Authority::Known(PointerCaptureOwner::Native(
                    binding.window(),
                )),
                button: PointerButton::Primary,
                payload,
                source_surface: surface,
                initial_pointer: Some(initial_pointer),
                journal_threshold_origin: JournalDragThresholdOrigin::NativeLocal {
                    binding,
                    coordinate_generation: generation,
                    position: initial_pointer,
                },
                complete_root: Some(source),
                partial_detachable: false,
                origin,
                source_validated_at: version,
                presentation,
                source_layout_facts: None,
                journal_source_geometry: Some(JournalDragSourceGeometry {
                    source_rect,
                    minimum_size,
                }),
                continuation: Some(continuation),
            },
            workspace,
        )
    }

    fn active_native_local_drag_fixture(
        contained_intent: Option<ContainedDragIntent>,
    ) -> (InteractionState, DragSessionId, Workspace) {
        let (start, workspace) = native_local_drag_start_fixture(contained_intent);
        let owner = start.owner;
        let mut state = InteractionState::default();
        let (session, replaced) = state.arm_drag(start).expect("test drag must arm");
        assert_eq!(replaced, None);
        state
            .begin_drag(session, owner, PointerButton::Primary)
            .expect("test drag must begin");
        (state, session, workspace)
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

    #[test]
    fn a_gesture_freezes_its_lane_but_the_next_gesture_can_reuse_the_stream() {
        let (mut start, _) = click_start_fixture();
        let domain = start.owner.lease().authority_domain();
        let host = PresentationLedger::new(domain)
            .create_host()
            .expect("test presentation host must be available");
        let lease = PointerInputLease::new(domain, 1, PointerProviderScope::NativeJoined(host));
        let stream = PointerStreamId::new(lease, start.owner.pointer(), 1);
        let binding = ViewportBinding::new(
            domain,
            start.epoch,
            start.presentation.surface(),
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let owner = GestureOwner::new(
            stream,
            GestureLane::NativeLocal {
                binding: binding.window(),
                coordinate_generation: CoordinateGeneration::new(1),
            },
        );
        start.owner = owner;
        let mut state = InteractionState::default();
        let (session, _) = state
            .begin_click(start.clone())
            .expect("test native-local click must begin");
        let frozen = state.clone();

        for incompatible in [
            GestureOwner::new(stream, GestureLane::Desktop),
            GestureOwner::new(
                stream,
                GestureLane::NativeLocal {
                    binding: binding.window(),
                    coordinate_generation: CoordinateGeneration::new(2),
                },
            ),
        ] {
            assert_eq!(state.cancel_owner(incompatible), None);
            assert_eq!(
                state.take_click_for_release(session, incompatible, PointerButton::Primary),
                Err(InteractionRejection::PointerStreamMismatch {
                    expected: stream,
                    actual: stream,
                })
            );
            assert_eq!(state, frozen);
        }
        assert_eq!(state.active_owner(), Some(owner));
        assert_eq!(state.active_stream(), Some(stream));
        assert_eq!(
            state
                .take_click_for_release(session, owner, PointerButton::Primary)
                .expect("the exact owner can release")
                .owner,
            owner
        );
        assert_eq!(state.active_stream(), None);

        let desktop_owner = GestureOwner::new(stream, GestureLane::Desktop);
        start.owner = desktop_owner;
        let (next_session, _) = state
            .begin_click(start)
            .expect("the next gesture can choose the desktop lane");
        assert_ne!(next_session, session);
        assert_eq!(state.active_stream(), Some(stream));
        assert_eq!(state.active_owner(), Some(desktop_owner));
        assert_eq!(state.cancel_owner(owner), None);
        assert_eq!(state.active_owner(), Some(desktop_owner));
        state
            .take_click_for_release(next_session, desktop_owner, PointerButton::Primary)
            .expect("the new exact owner can release");
        assert_eq!(state.status(), InteractionStatus::Idle);
    }

    #[test]
    fn desktop_handoff_rebinds_the_source_and_discards_old_target_facts_atomically() {
        let (mut state, session, workspace) = active_native_local_drag_fixture(None);
        let drag = state.active_drag(session).expect("test drag is active");
        let owner = drag.owner;
        let source = drag
            .complete_root
            .as_ref()
            .expect("test source is complete");
        let geometry = drag
            .journal_source_geometry
            .expect("test source has geometry");
        let source_surface = drag.source_surface;
        let source_root = source.root();
        let anchor = SurfacePointer::new(
            source_surface,
            drag.initial_pointer.expect("test source has a pointer"),
        );
        state
            .freeze_journal_presentation_reservation(
                session,
                owner,
                source_surface,
                source_root,
                FloatingPresentationId::new(27),
                anchor,
                geometry.source_rect,
                geometry.minimum_size,
                false,
            )
            .expect("test source reservation must freeze");

        let target_surface = SurfaceId::new(25);
        let target_root = RootId::new(24);
        let target_tabs = workspace
            .root(target_root)
            .expect("test target root exists")
            .node;
        let presentation = frozen_presentation(target_surface, session.epoch());
        let scene = frozen_scene(target_surface, session.epoch());
        let bounds =
            LogicalRect::new(0.0, 0.0, 300.0, 200.0).expect("test target bounds must be valid");
        let point = LogicalPoint::new(100.0, 100.0).expect("test target point is finite");
        let target = Arc::new(DropTargetRecord::new(
            DropTargetLocation::Center {
                surface: target_surface,
                root: target_root,
                tabs: target_tabs,
            },
            DropTargetAvailability::Available,
            HitRegion::new(bounds),
            SceneLayerKey::surface_base(),
            DropVisual::new(bounds),
        ));
        let mut plan = PresentationPlan::new(target_surface, bounds);
        plan.bind_command_basis(
            target_root,
            workspace
                .root_fingerprint(target_root)
                .expect("test target root is current"),
        );
        plan.push_drop_target(Arc::clone(&target));
        plan.push_drop_guide_cluster(DropGuideClusterRecord::inner_center(
            target_surface,
            target_root,
            target_tabs,
            HitRegion::new(bounds),
            SceneLayerKey::surface_base(),
            DropGuideTargetRecord::new(target, bounds),
        ));
        let drag = state.active_drag(session).expect("test drag is active");
        let index = WorkspaceIndex::build(&workspace, drag.source_validated_at)
            .expect("test workspace index must build");
        let (resolution, affordance, observation) = resolve_presented_drop(
            scene,
            &plan,
            None,
            &workspace,
            drag.source_validated_at,
            &index,
            &DockPolicySnapshot::default(),
            session,
            drag.payload.clone(),
            None,
            point,
        )
        .expect("test target must resolve")
        .into_evaluation_parts();
        let DropResolution::Resolved(resolved) = resolution else {
            panic!("test center target must accept the source");
        };
        let acquisition = observation.acquisition().expect("test target is acquired");
        let (preview, _) = state
            .publish_preview(
                session,
                scene,
                PreviewVisual::Dock {
                    surface: target_surface,
                    target: resolved.target_id(),
                    rect: resolved.visual().rect(),
                },
                PreviewProof::Dock {
                    target: resolved.target_id(),
                    command: resolved.command().clone(),
                },
                Some(acquisition),
            )
            .expect("test preview must publish");
        state
            .bind_preview_layout_basis(session, Some(scene))
            .expect("test preview must retain its layout basis");
        assert!(state.observe_presented_preview(target_surface, preview.token()));
        state
            .set_drop_affordance(session, Some(affordance.expect("test guides are visible")))
            .expect("test guide affordance must install");
        let GestureLane::NativeLocal {
            binding,
            coordinate_generation,
        } = owner.lane()
        else {
            panic!("test owner must be native-local");
        };
        let local_point = LogicalPoint::new(42.0, 18.0).expect("test local point is finite");
        let route = crate::pointer_journal::NativeSurfacePointerRoute::new(
            ViewportBinding::from_window(binding, state.active_surface().unwrap()),
            coordinate_generation,
            PhysicalPoint::new(105.0, 45.0).expect("test client point is finite"),
            local_point,
        );
        state
            .set_drag_position(
                session,
                ManipulationPositionKey::JournalNativeLocal {
                    manipulation: Some(route),
                    hover: Some(route),
                },
            )
            .expect("test local position must advance");
        let token = state
            .mint_continuation(
                owner,
                PointerContinuationSession::Drag(session),
                std::time::Duration::from_millis(100),
            )
            .expect("test timed continuation must mint");
        let drag = state.active_drag_mut(session).expect("test drag is active");
        drag.timed = Some(DragTimedContinuation::HoverSelection(HoverSelectionState {
            request: PointerContinuationRequest {
                token,
                surface: target_surface,
                point,
            },
            target: TabSceneId {
                root: target_root,
                tabs: target_tabs,
                item: ItemId::new(3),
            },
            authority: presentation.presented,
            workspace: drag.source_validated_at,
            started_at: std::time::Duration::ZERO,
        }));
        assert_eq!(drag.current_pointer(), Some(local_point));
        let no_candidate = TargetObservation::surface(scene, SceneLayerKey::surface_base(), None);
        assert_eq!(
            state.reconcile_drag_target(session, no_candidate, Some(point), 8.0),
            Ok(TargetLatchDecision::RetainCurrent)
        );

        let desktop_owner = GestureOwner::new(owner.stream(), GestureLane::Desktop);
        let mut expected = state.clone();
        let drag = expected
            .active_drag_mut(session)
            .expect("test drag is active");
        drag.owner = desktop_owner;
        drag.continuation
            .as_mut()
            .expect("test source has a continuation")
            .draft
            .owner = desktop_owner;
        drag.timed = None;
        drag.target.last_advanced = None;
        drag.target.last_logical_point = None;
        drag.target.affordance = None;
        drag.target.preview = DragPreviewState::Unresolved {
            status: PreviewResolutionStatus::Unavailable,
        };

        assert_eq!(state.upgrade_drag_to_desktop(session, owner), Ok(()));
        assert_eq!(state, expected);
        assert_eq!(state.active_stream(), Some(owner.stream()));
        let drag = state
            .active_drag(session)
            .expect("same drag remains active");
        assert_eq!(drag.owner.lease(), owner.lease());
        assert_eq!(drag.owner.pointer(), owner.pointer());
        assert_eq!(drag.target_acquisition(), None);
        assert_eq!(drag.target.current_pointer(None), None);
        assert_eq!(drag.current_pointer(), drag.initial_pointer);
        assert!(!state.observe_presented_preview(target_surface, preview.token()));
        assert_eq!(
            state.reconcile_drag_target(session, no_candidate, Some(point), 8.0),
            Ok(TargetLatchDecision::AcceptCandidate)
        );
        let released = state
            .take_drag_for_release(session, desktop_owner, PointerButton::Primary)
            .expect("upgraded owner must release the same session");
        assert_eq!(released.session, session);
        assert_eq!(released.owner.stream(), owner.stream());
        assert_eq!(state.status(), InteractionStatus::Idle);
    }

    #[test]
    fn desktop_handoff_also_accepts_a_drag_without_a_source_continuation() {
        let (mut start, _) = native_local_drag_start_fixture(None);
        start.continuation = None;
        let owner = start.owner;
        let mut state = InteractionState::default();
        let (session, _) = state.arm_drag(start).expect("test drag must arm");
        state
            .begin_drag(session, owner, PointerButton::Primary)
            .expect("test drag must begin");
        assert_eq!(state.upgrade_drag_to_desktop(session, owner), Ok(()));
        assert_eq!(state.active_scene_gesture_continuation(), None);
        assert_eq!(state.active_stream(), Some(owner.stream()));
        assert_eq!(
            state.active_owner(),
            Some(GestureOwner::new(owner.stream(), GestureLane::Desktop))
        );
        let before_retry = state.clone();
        assert_eq!(
            state.upgrade_drag_to_desktop(session, owner),
            Err(InteractionRejection::SessionMismatch)
        );
        assert_eq!(state, before_retry);
    }

    #[test]
    fn desktop_handoff_rejects_wrong_session_owner_or_lane_without_mutation() {
        let (state, session, _) = active_native_local_drag_fixture(None);
        let owner = state.active_owner().expect("test drag has an owner");
        let GestureLane::NativeLocal {
            binding,
            coordinate_generation,
        } = owner.lane()
        else {
            panic!("test owner must be native-local");
        };
        let wrong_session = DragSessionId::new(session.epoch(), DragGeneration::new(2));
        let wrong_stream = PointerStreamId::new(owner.lease(), PointerId::new(2), 1);
        let wrong_provider =
            PointerInputLease::new(owner.lease().authority_domain(), 2, owner.lease().scope());
        for (requested_session, requested_owner) in [
            (wrong_session, owner),
            (
                session,
                GestureOwner::new(owner.stream(), GestureLane::Desktop),
            ),
            (
                session,
                GestureOwner::new(
                    owner.stream(),
                    GestureLane::NativeLocal {
                        binding,
                        coordinate_generation: coordinate_generation
                            .checked_next()
                            .expect("test coordinate generation must advance"),
                    },
                ),
            ),
            (session, GestureOwner::new(wrong_stream, owner.lane())),
            (
                session,
                GestureOwner::new(
                    PointerStreamId::new(wrong_provider, owner.pointer(), 1),
                    owner.lane(),
                ),
            ),
        ] {
            let mut candidate = state.clone();
            assert_eq!(
                candidate.upgrade_drag_to_desktop(requested_session, requested_owner),
                Err(InteractionRejection::SessionMismatch)
            );
            assert_eq!(candidate, state);
        }
        for lane in [
            GestureLane::Desktop,
            GestureLane::HostLocal,
            GestureLane::SurfaceLocal,
        ] {
            let mut candidate = state.clone();
            let nonlocal_owner = GestureOwner::new(owner.stream(), lane);
            let drag = candidate
                .active_drag_mut(session)
                .expect("test drag is active");
            drag.owner = nonlocal_owner;
            drag.continuation
                .as_mut()
                .expect("test source has a continuation")
                .draft
                .owner = nonlocal_owner;
            let before = candidate.clone();
            assert_eq!(
                candidate.upgrade_drag_to_desktop(session, nonlocal_owner),
                Err(InteractionRejection::SessionMismatch)
            );
            assert_eq!(candidate, before);
        }
    }

    #[test]
    fn desktop_handoff_rejects_a_mismatched_source_continuation_without_mutation() {
        let (state, session, _) = active_native_local_drag_fixture(None);
        let owner = state.active_owner().expect("test drag has an owner");
        for mismatch in 0..3 {
            let mut candidate = state.clone();
            let continuation = candidate
                .active_drag_mut(session)
                .expect("test drag is active")
                .continuation
                .as_mut()
                .expect("test source has a continuation");
            match mismatch {
                0 => {
                    continuation.session = SceneGestureSession::Drag(DragSessionId::new(
                        session.epoch(),
                        DragGeneration::new(2),
                    ));
                }
                1 => {
                    continuation.session =
                        SceneGestureSession::ContainedTransform(ContainedTransformSessionId::new(
                            session.epoch(),
                            ContainedTransformGeneration::new(1),
                        ));
                }
                _ => {
                    continuation.draft.owner =
                        GestureOwner::new(owner.stream(), GestureLane::Desktop);
                }
            }
            let before = candidate.clone();
            assert_eq!(
                candidate.upgrade_drag_to_desktop(session, owner),
                Err(InteractionRejection::SessionMismatch)
            );
            assert_eq!(candidate, before);
        }
    }

    #[test]
    fn desktop_handoff_requires_an_already_active_drag() {
        let (start, _) = native_local_drag_start_fixture(None);
        let owner = start.owner;
        let mut armed = InteractionState::default();
        let (session, _) = armed.arm_drag(start).expect("test drag must arm");
        let (click, _) = click_start_fixture();
        let mut pressed = InteractionState::default();
        pressed.begin_click(click).expect("test click must begin");
        for mut candidate in [InteractionState::default(), armed, pressed] {
            let before = candidate.clone();
            assert_eq!(
                candidate.upgrade_drag_to_desktop(session, owner),
                Err(InteractionRejection::SessionMismatch)
            );
            assert_eq!(candidate, before);
        }
    }

    #[test]
    fn desktop_handoff_preserves_contained_drag_intent_inside_and_outside_source() {
        for intent in [
            ContainedDragIntent::DockContent,
            ContainedDragIntent::MoveWindow,
        ] {
            for point in [
                LogicalPoint::new(42.0, 18.0).expect("test source point is finite"),
                LogicalPoint::new(500.0, 400.0).expect("test outside point is finite"),
            ] {
                let (mut state, session, _) = active_native_local_drag_fixture(Some(intent));
                let owner = state.active_owner().expect("test drag has an owner");
                let GestureLane::NativeLocal {
                    binding,
                    coordinate_generation,
                } = owner.lane()
                else {
                    panic!("test owner must be native-local");
                };
                let route = crate::pointer_journal::NativeSurfacePointerRoute::new(
                    ViewportBinding::from_window(binding, state.active_surface().unwrap()),
                    coordinate_generation,
                    PhysicalPoint::new(point.x() * 2.5, point.y() * 2.5)
                        .expect("test client point is finite"),
                    point,
                );
                let position = ManipulationPositionKey::JournalNativeLocal {
                    manipulation: Some(route),
                    hover: Some(route),
                };
                let allows_magnetic = intent == ContainedDragIntent::DockContent;
                assert_eq!(
                    state
                        .active_drag(session)
                        .expect("test drag is active")
                        .magnetic_targeting_allowed(),
                    allows_magnetic,
                    "source intent determines magnetism before any source-rectangle exit",
                );
                state
                    .set_drag_position(session, position)
                    .expect("test drag must observe the local point");
                let before_source = state
                    .active_scene_gesture_continuation()
                    .expect("test source has a continuation")
                    .draft
                    .source
                    .clone();
                assert_eq!(state.upgrade_drag_to_desktop(session, owner), Ok(()));
                let drag = state
                    .active_drag(session)
                    .expect("same contained drag remains active");
                assert_eq!(drag.target.last_advanced, None);
                assert_eq!(drag.target.last_logical_point, None);
                assert_eq!(
                    drag.continuation
                        .as_ref()
                        .expect("source continuation survives")
                        .draft
                        .source,
                    before_source
                );
                assert_eq!(drag.magnetic_targeting_allowed(), allows_magnetic);
            }
        }
    }

    #[test]
    fn native_local_hover_never_falls_back_to_manipulation_or_outside_all() {
        let surface = SurfaceId::new(21);
        let binding = ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(90),
            WorkspaceEpoch::new(23),
            surface,
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let route = |x, y| {
            crate::pointer_journal::NativeSurfacePointerRoute::new(
                binding,
                CoordinateGeneration::new(1),
                PhysicalPoint::new(x, y).expect("test client position must be finite"),
                LogicalPoint::new(x, y).expect("test surface position must be finite"),
            )
        };
        let manipulation = route(100.0, 20.0);
        let hover = route(20.0, 10.0);
        let position = ManipulationPositionKey::JournalNativeLocal {
            manipulation: Some(manipulation),
            hover: Some(hover),
        };
        assert_eq!(position.surface(), Some(surface));
        assert_eq!(position.hover_surface(), Some(surface));
        assert_eq!(
            position.hover_probe(),
            Some(SurfacePointer::new(surface, hover.surface_position()))
        );
        assert_eq!(position.logical_point(), Some(hover.surface_position()));

        let unavailable_hover = ManipulationPositionKey::JournalNativeLocal {
            manipulation: Some(manipulation),
            hover: None,
        };
        assert_eq!(unavailable_hover.surface(), None);
        assert_eq!(unavailable_hover.hover_surface(), None);
        assert_eq!(unavailable_hover.hover_probe(), None);
        assert_eq!(unavailable_hover.logical_point(), None);
        assert!(unavailable_hover.hover_probe_requested());

        let swapped_lanes = ManipulationPositionKey::JournalNativeLocal {
            manipulation: Some(hover),
            hover: Some(manipulation),
        };
        assert_eq!(swapped_lanes.surface(), Some(surface));
        assert_eq!(swapped_lanes.hover_surface(), Some(surface));
        assert_eq!(
            swapped_lanes.hover_probe(),
            Some(SurfacePointer::new(
                surface,
                manipulation.surface_position()
            ))
        );
        assert_eq!(
            swapped_lanes.logical_point(),
            Some(manipulation.surface_position())
        );
    }

    #[test]
    fn invalidating_position_preserves_latest_logical_point_for_revalidation() {
        let surface = SurfaceId::new(21);
        let presentation = frozen_presentation(surface, WorkspaceEpoch::new(23));
        let latest = LogicalPoint::new(42.0, 18.0).expect("test point must be finite");
        let initial = LogicalPoint::new(4.0, 3.0).expect("test point must be finite");
        let mut target = DragTargetState::new(&FrozenDragOrigin::Workspace);

        let route = HostLocalPointerRoute::new(presentation.presented, latest);
        target.set_last_advanced(ManipulationPositionKey::JournalHost {
            manipulation: Some(route),
            hover: Some(route),
        });
        target.invalidate_last_advanced();

        assert_eq!(target.last_advanced(), None);
        assert_eq!(target.current_pointer(Some(initial)), Some(latest));
    }
}
