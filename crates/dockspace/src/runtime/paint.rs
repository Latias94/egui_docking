//! Renderer-neutral measurement and primary read-only docking paint capabilities.

use std::collections::HashSet;
use std::fmt;

use crate::drop_guide::DropGuideClusterId;
use crate::drop_resolver::DropAffordance;
use crate::drop_target::{DropTargetId, SceneLayerKey};
use crate::engine::SurfaceContributionToken;
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::Axis;
use crate::ids::{FloatingPresentationId, HostPresentationAttemptId, ItemId, RootId, SurfaceId};
use crate::interaction::{
    ContainedTransformPreview, EscapeDelivery, InteractionPreview, PreviewVisual,
};
use crate::model::{DockspaceActionRejection, DockspaceAxis, ProductAction};
use crate::presentation_hit::{
    PresentationHitRegion, PresentationHitRegionId, PresentationHitRegionKind,
    PresentationPointerLane, PresentationPointerLanes,
};
use crate::presentation_observation::{
    HostInteractionPresentation, SurfacePresentationOutputTicket,
};
pub use crate::scene::TabGroupDragRegionKind;
use crate::scene::{
    ContainedLayoutOwner, ContainedRecord, ContainedResizeRecord, PresentationMenuAnchorHost,
    PresentationMenuAnchorRecord, PresentationPlan, ResolvedSurfaceProgramOperation,
    ResolvedSurfaceProgramRoot, ResolvedSurfaceProgramStep, SplitterJunctionDirection,
    SplitterJunctionId, SplitterJunctionRecord, SplitterRecord, SplitterSceneId, SurfaceScene,
    TabBarSceneId, TabSceneId,
};
#[cfg(test)]
use crate::scene::{SurfaceProgramRootOperation, SurfaceProgramRootPresentation};
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId, TabStripControlId};

use super::{
    DockspaceHostFrame, DockspaceInteractionError, DockspacePaneFocusObservation,
    DockspacePaneFocusRequest, DockspaceRuntimeError, PreparedPaneFocusObservation,
};

mod actions;
mod drag_decoration;
mod guides;
#[cfg(test)]
mod junction_query_tests;
mod tab_chrome;
mod tabs;
pub use actions::ContainedResizeDirection;
pub use drag_decoration::{DockspaceDragDecoration, DockspaceDragSourceKind};
pub use guides::{
    DockspaceDropDirection, DockspaceDropEligibility, DockspaceGuideScope,
    DropAffordanceClusterPaintRecord, DropAffordancePaintRecord, DropAffordanceTargetPaintRecord,
    DropGuidePaintRecord, DropGuideTargetPaintRecord,
};
pub use tab_chrome::{
    TabListMenuBackdropPaintRecord, TabListMenuPaintRecord, TabListMenuRowPaintRecord,
    TabStripControlKind, TabStripControlPaintRecord,
};
pub use tabs::{
    PanePaintRecord, TabBarPaintRecord, TabGroupDragRegionPaintRecord, TabPaintRecord,
    TabStripMemberPaintRecord, TabStripMemberVisibility,
};

/// Opaque identity for one exact semantic output supplied to a renderer.
///
/// The value proves only which core output a paint plan describes. It does not
/// prove that the renderer accepted or presented that output.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DockspaceSemanticOutput {
    pub(super) ticket: SurfacePresentationOutputTicket,
}

impl DockspaceSemanticOutput {
    /// Returns the sole logical surface named by this output.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.ticket.surface()
    }
}

impl fmt::Debug for DockspaceSemanticOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceSemanticOutput")
            .field("surface", &self.surface())
            .finish_non_exhaustive()
    }
}

/// Stable renderer identity whose structural storage remains core-private.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DockspaceVisualId(VisualIdentity);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum VisualIdentity {
    Pane(crate::scene::PaneSceneId),
    Tab(TabSceneId),
    TabBar(TabBarSceneId),
    TabGroupDragRegion {
        bar: TabBarSceneId,
        region: TabGroupDragRegionKind,
    },
    DragSource {
        root: RootId,
        node: crate::ids::NodeId,
    },
    TabStripControl(TabStripControlId),
    TabListMenu(TabListMenuSessionId),
    TabListMenuRow {
        session: TabListMenuSessionId,
        tab: TabSceneId,
    },
    TabListMenuBackdrop {
        surface: SurfaceId,
        session: TabListMenuSessionId,
        revision: PopupRoutingRevision,
    },
    PresentationMenuAnchor(RootId),
    Splitter(SplitterSceneId),
    SplitterJunction(SplitterJunctionId),
    Contained(FloatingPresentationId),
    DropGuide(DropGuideClusterId),
    DropTarget(DropTargetId),
    Receiver(PresentationHitRegionId),
}

/// Public semantic class of one opaque visual identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceVisualKind {
    /// One tabs leaf and selected pane body.
    Pane,
    /// One visible tab.
    Tab,
    /// One tab strip.
    TabBar,
    /// One exact empty tab-strip region which can drag the complete group.
    TabGroupDragRegion,
    /// One core-owned root or subtree drag source.
    DragSource,
    /// One core-owned tab-strip scroll or menu control.
    TabStripControl,
    /// One open tab-list popup frame.
    TabListMenu,
    /// One item row within an open tab-list popup.
    TabListMenuRow,
    /// One full-surface popup backdrop.
    TabListMenuBackdrop,
    /// One root-scoped presentation command menu anchor.
    PresentationMenuAnchor,
    /// One ordinary splitter.
    Splitter,
    /// One atomic multi-splitter junction.
    SplitterJunction,
    /// One contained-floating presentation.
    Contained,
    /// One complete docking-guide cluster.
    DropGuide,
    /// One exact drop target.
    DropTarget,
    /// One semantic receiver without a standalone paint record.
    Receiver,
}

/// Core-derived decision for painting one exact visual in the current program.
///
/// A source gap suppresses the source visual while a drag projection represents
/// it elsewhere. The host preserves its semantic identity and the receivers
/// retained by this program. An immediate UI can preserve widget registration;
/// a retained or self-drawn UI can retain its own nodes and hit records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfacePaintDisposition {
    /// Paint the visual normally.
    Visible,
    /// Preserve interaction identity but omit the visual's pixels.
    SourceGap,
}

impl SurfacePaintDisposition {
    /// Returns whether the current program requires a source gap for this visual.
    #[must_use]
    pub const fn is_source_gap(self) -> bool {
        matches!(self, Self::SourceGap)
    }
}

impl DockspaceVisualId {
    pub(super) const fn from_pane_scene(id: crate::scene::PaneSceneId) -> Self {
        Self(VisualIdentity::Pane(id))
    }

    pub(super) const fn from_tab_scene(id: TabSceneId) -> Self {
        Self(VisualIdentity::Tab(id))
    }

    pub(super) const fn from_tab_bar_scene(id: TabBarSceneId) -> Self {
        Self(VisualIdentity::TabBar(id))
    }

    /// Returns the stable semantic class without exposing graph storage IDs.
    #[must_use]
    pub const fn kind(self) -> DockspaceVisualKind {
        match self.0 {
            VisualIdentity::Pane(_) => DockspaceVisualKind::Pane,
            VisualIdentity::Tab(_) => DockspaceVisualKind::Tab,
            VisualIdentity::TabBar(_) => DockspaceVisualKind::TabBar,
            VisualIdentity::TabGroupDragRegion { .. } => DockspaceVisualKind::TabGroupDragRegion,
            VisualIdentity::DragSource { .. } => DockspaceVisualKind::DragSource,
            VisualIdentity::TabStripControl(_) => DockspaceVisualKind::TabStripControl,
            VisualIdentity::TabListMenu(_) => DockspaceVisualKind::TabListMenu,
            VisualIdentity::TabListMenuRow { .. } => DockspaceVisualKind::TabListMenuRow,
            VisualIdentity::TabListMenuBackdrop { .. } => DockspaceVisualKind::TabListMenuBackdrop,
            VisualIdentity::PresentationMenuAnchor(..) => {
                DockspaceVisualKind::PresentationMenuAnchor
            }
            VisualIdentity::Splitter(_) => DockspaceVisualKind::Splitter,
            VisualIdentity::SplitterJunction(_) => DockspaceVisualKind::SplitterJunction,
            VisualIdentity::Contained(_) => DockspaceVisualKind::Contained,
            VisualIdentity::DropGuide(_) => DockspaceVisualKind::DropGuide,
            VisualIdentity::DropTarget(_) => DockspaceVisualKind::DropTarget,
            VisualIdentity::Receiver(_) => DockspaceVisualKind::Receiver,
        }
    }
}

impl fmt::Debug for DockspaceVisualId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DockspaceVisualId")
            .field(&self.kind())
            .finish()
    }
}

pub(super) fn paint_disposition(
    decoration: Option<DockspaceDragDecoration<'_>>,
    visual: DockspaceVisualId,
) -> SurfacePaintDisposition {
    if decoration.is_some_and(|decoration| decoration.omits_visual(visual)) {
        SurfacePaintDisposition::SourceGap
    } else {
        SurfacePaintDisposition::Visible
    }
}

/// Core-derived paint layer. Larger values are frontmost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DockspacePaintLayer(u64);

impl DockspacePaintLayer {
    const fn from_core(layer: SceneLayerKey) -> Self {
        Self(layer.get())
    }

    /// Returns the comparable renderer representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Semantic receiver class independent of renderer widget identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceReceiverRole {
    PaneBody,
    TabBody,
    TabClose,
    TabStripControl,
    TabStripScroll,
    TabListMenuRow,
    TabListMenuScroll,
    TabListMenuBlocker,
    TabListMenuBackdrop,
    /// Dedicated whole-group grip before the first tab.
    TabGroupGrip,
    /// Unoccupied strip space after the final tab.
    TabGroupTrailingEmpty,
    Splitter,
    SplitterJunction,
    ContainedFrame,
    ContainedTitle,
    ContainedClose,
    ContainedResize,
    DropTarget,
}

/// Opaque receiver descriptor captured while painting one semantic output.
///
/// A descriptor is not input authority. It must be rebound after a concrete
/// output is finally presented before an event may name it.
#[derive(Clone, Copy, PartialEq)]
pub struct DockspaceReceiverDescriptor {
    pub(super) output: SurfacePresentationOutputTicket,
    pub(super) region: PresentationHitRegionId,
    lanes: PresentationPointerLanes,
    role: DockspaceReceiverRole,
    bounds: LogicalRect,
    center: LogicalPoint,
}

impl fmt::Debug for DockspaceReceiverDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceReceiverDescriptor")
            .field("role", &self.role)
            .field("bounds", &self.bounds)
            .field("center", &self.center)
            .finish()
    }
}

impl DockspaceReceiverDescriptor {
    pub(super) fn from_projection_region(
        output: SurfacePresentationOutputTicket,
        region: PresentationHitRegionId,
        lanes: PresentationPointerLanes,
        bounds: LogicalRect,
    ) -> Option<Self> {
        Some(Self {
            output,
            region,
            lanes,
            role: receiver_role(region.kind())?,
            bounds,
            center: crate::scene::paint_receiver_center(bounds)?,
        })
    }

    /// Returns a stable visual identity for renderer widget keys.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Receiver(self.region))
    }

    /// Returns the semantic receiver role.
    #[must_use]
    pub const fn role(self) -> DockspaceReceiverRole {
        self.role
    }

    /// Returns the base receiver rectangle supplied to the renderer.
    ///
    /// An expanded touch target can survive clipping that leaves this empty.
    /// Resolve device-specific input with [`super::DockspaceSession::host_pointer_hit_for_device`]
    /// rather than treating these bounds as the complete touch footprint.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the center of the exact half-open receiver rectangle.
    #[must_use]
    pub const fn center(self) -> LogicalPoint {
        self.center
    }

    pub(super) const fn supports_lane(self, lane: PresentationPointerLane) -> bool {
        self.lanes.contains(lane)
    }
}

/// Borrowed non-visual drop target in one exact surface paint program.
///
/// Validation resolves its complete receiver identity and finite center before
/// a visitor receives it. Materialize an owned descriptor only when the host
/// needs to retain or register this receiver.
#[derive(Clone, Copy)]
pub struct DropTargetPaintRecord<'frame> {
    output: SurfacePresentationOutputTicket,
    region: &'frame PresentationHitRegion,
    center: LogicalPoint,
}

impl fmt::Debug for DropTargetPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.receiver().fmt(formatter)
    }
}

impl DropTargetPaintRecord<'_> {
    /// Returns the complete owned descriptor for this exact output.
    #[must_use]
    pub fn receiver(self) -> DockspaceReceiverDescriptor {
        DockspaceReceiverDescriptor {
            output: self.output,
            region: self.region.id(),
            lanes: self.region.lanes(),
            role: DockspaceReceiverRole::DropTarget,
            bounds: self.region.hit().rect(),
            center: self.center,
        }
    }
}

/// Stable renderer-facing preview geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockspacePreviewVisual {
    /// Highlight one exact docking rectangle.
    Dock {
        surface: SurfaceId,
        rect: LogicalRect,
    },
    /// Paint one contained-floating placement.
    Contained {
        surface: SurfaceId,
        rect: LogicalRect,
        fallback: bool,
    },
    /// Paint a source-hosted cue for a future native surface.
    Native {
        host_surface: SurfaceId,
        target_surface: SurfaceId,
        placement: PhysicalRect,
    },
}

/// Borrowed preview which belongs to one exact paint plan.
#[derive(Debug, Clone, Copy)]
pub struct DockspaceDragPreview<'plan> {
    preview: &'plan InteractionPreview,
    layout_projected: bool,
}

impl DockspaceDragPreview<'_> {
    /// Returns whether this output already lays out the complete root at this placement.
    ///
    /// The preview remains part of the exact output obligation; adapters must
    /// not paint a duplicate shell over the projected content.
    #[must_use]
    pub const fn is_layout_projected(self) -> bool {
        self.layout_projected
    }

    /// Returns the renderer-neutral geometry which must be painted.
    #[must_use]
    pub fn visual(self) -> DockspacePreviewVisual {
        match *self.preview.visual() {
            PreviewVisual::Dock { surface, rect, .. } => {
                DockspacePreviewVisual::Dock { surface, rect }
            }
            PreviewVisual::Contained {
                surface,
                rect,
                fallback,
            } => DockspacePreviewVisual::Contained {
                surface,
                rect,
                fallback,
            },
            PreviewVisual::Native {
                host_surface,
                target_surface,
                placement,
            } => DockspacePreviewVisual::Native {
                host_surface,
                target_surface,
                placement,
            },
        }
    }
}

/// Borrowed contained-floating transform preview owned by one exact paint plan.
#[derive(Clone, Copy)]
pub struct DockspaceContainedTransformPreview<'plan> {
    preview: &'plan ContainedTransformPreview,
    layout_projected: bool,
}

impl fmt::Debug for DockspaceContainedTransformPreview<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceContainedTransformPreview")
            .field("surface", &self.surface())
            .field("floating", &self.floating())
            .field("rect", &self.rect())
            .field("layout_projected", &self.is_layout_projected())
            .finish()
    }
}

impl DockspaceContainedTransformPreview<'_> {
    /// Returns the logical surface which must paint the preview.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.preview.surface()
    }

    /// Returns the contained presentation being resized.
    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.preview.floating()
    }

    /// Returns the exact rectangle which must be painted.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.preview.rect()
    }

    /// Returns whether this output already lays out the complete root at this rectangle.
    ///
    /// The preview remains part of the exact output obligation; adapters must
    /// not paint a duplicate shell over the projected content.
    #[must_use]
    pub const fn is_layout_projected(self) -> bool {
        self.layout_projected
    }
}

/// Read-only splitter geometry and resize constraints.
#[derive(Debug, Clone, Copy)]
pub struct SplitterPaintRecord<'plan> {
    record: &'plan SplitterRecord,
}

impl SplitterPaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Splitter(*self.record.id()))
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    #[must_use]
    pub const fn draw_bounds(self) -> LogicalRect {
        self.record.draw_bounds()
    }

    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.hit().rect()
    }

    #[must_use]
    pub const fn axis(self) -> DockspaceAxis {
        match self.record.axis() {
            Axis::Horizontal => DockspaceAxis::Horizontal,
            Axis::Vertical => DockspaceAxis::Vertical,
        }
    }

    #[must_use]
    pub const fn before_bounds(self) -> LogicalRect {
        self.record.before_bounds()
    }

    #[must_use]
    pub const fn after_bounds(self) -> LogicalRect {
        self.record.after_bounds()
    }

    #[must_use]
    pub const fn minimum_extents(self) -> (f64, f64) {
        (
            self.record.before_minimum_extent(),
            self.record.after_minimum_extent(),
        )
    }

    #[must_use]
    pub const fn maximum_extents(self) -> (f64, f64) {
        (
            self.record.before_maximum_extent(),
            self.record.after_maximum_extent(),
        )
    }

    #[must_use]
    pub const fn operable(self) -> bool {
        self.record.operable()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only atomic splitter-junction geometry.
#[derive(Clone, Copy)]
pub struct SplitterJunctionPaintRecord<'plan> {
    record: &'plan SplitterJunctionRecord,
}

impl fmt::Debug for SplitterJunctionPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SplitterJunctionPaintRecord")
            .field("root", &self.root())
            .field("hit_bounds", &self.hit_bounds())
            .field("arm_count", &self.arm_count())
            .field("layer", &self.layer())
            .finish_non_exhaustive()
    }
}

impl SplitterJunctionPaintRecord<'_> {
    /// Returns the stable root containing every incident splitter.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root()
    }

    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::SplitterJunction(self.record.id()))
    }

    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.hit().rect()
    }

    #[must_use]
    pub fn arm_count(self) -> usize {
        self.record.id().arm_count()
    }

    /// Returns whether this junction contains at least one incident splitter
    /// which moves along `axis`.
    #[must_use]
    pub const fn has_axis(self, axis: DockspaceAxis) -> bool {
        let id = self.record.id();
        match axis {
            DockspaceAxis::Horizontal => {
                id.arm(SplitterJunctionDirection::North).is_some()
                    || id.arm(SplitterJunctionDirection::South).is_some()
            }
            DockspaceAxis::Vertical => {
                id.arm(SplitterJunctionDirection::East).is_some()
                    || id.arm(SplitterJunctionDirection::West).is_some()
            }
        }
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only contained-floating resize geometry.
#[derive(Debug, Clone, Copy)]
pub struct ContainedResizePaintRecord {
    record: ContainedResizeRecord,
    operable: bool,
}

/// Product-facing chrome host of one root presentation command menu anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspacePresentationMenuAnchorKind {
    /// The sole core-selected tab bar of a main or native-main root.
    TabBar,
    /// The title chrome of a contained-floating root.
    ContainedTitle,
}

/// Opaque read-only root presentation command menu anchor.
#[derive(Clone, Copy)]
pub struct PresentationMenuAnchorPaintRecord<'plan> {
    record: &'plan PresentationMenuAnchorRecord,
    bounds: LogicalRect,
    operable: bool,
}

impl fmt::Debug for PresentationMenuAnchorPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PresentationMenuAnchorPaintRecord")
            .field("visual_id", &self.visual_id())
            .field("kind", &self.kind())
            .field("bounds", &self.bounds())
            .field("layer", &self.layer())
            .field("operable", &self.operable())
            .finish_non_exhaustive()
    }
}

impl PresentationMenuAnchorPaintRecord<'_> {
    /// Returns the stable visual identity of this root-scoped control.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::PresentationMenuAnchor(self.record.root()))
    }

    /// Returns the stable root whose commands this anchor opens.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.root()
    }

    /// Returns whether this anchor belongs to tab or contained-title chrome.
    #[must_use]
    pub const fn kind(self) -> DockspacePresentationMenuAnchorKind {
        match self.record.host() {
            PresentationMenuAnchorHost::TabBar(_) => DockspacePresentationMenuAnchorKind::TabBar,
            PresentationMenuAnchorHost::ContainedTitle(_) => {
                DockspacePresentationMenuAnchorKind::ContainedTitle
            }
        }
    }

    /// Returns the opaque visual identity of the chrome hosting this anchor.
    #[must_use]
    pub const fn host_visual_id(self) -> DockspaceVisualId {
        match self.record.host() {
            PresentationMenuAnchorHost::TabBar(bar) => {
                DockspaceVisualId(VisualIdentity::TabBar(bar))
            }
            PresentationMenuAnchorHost::ContainedTitle(floating) => {
                DockspaceVisualId(VisualIdentity::Contained(floating))
            }
        }
    }

    /// Returns the exact core-reserved draw and response bounds.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the roster-derived paint layer.
    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }

    /// Returns whether any authoritative area of this exact control remains exposed.
    #[must_use]
    pub const fn operable(self) -> bool {
        self.operable
    }
}

impl ContainedResizePaintRecord {
    #[must_use]
    pub const fn direction(self) -> ContainedResizeDirection {
        ContainedResizeDirection::from_core(self.record.direction())
    }

    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.hit().rect()
    }

    /// Returns whether the exact contained root, its items, and its owning
    /// surface permit a transform under the policy frozen into this plan.
    #[must_use]
    pub const fn operable(self) -> bool {
        self.operable
    }
}

/// Read-only contained-floating chrome and transform geometry.
#[derive(Clone, Copy)]
pub struct ContainedPaintRecord<'plan> {
    record: &'plan ContainedRecord,
    plan: &'plan PresentationPlan,
}

impl fmt::Debug for ContainedPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContainedPaintRecord")
            .field("visual_id", &self.visual_id())
            .field("outer_bounds", &self.outer_bounds())
            .field("title_operable", &self.title_operable())
            .field("close_operable", &self.close_operable())
            .finish_non_exhaustive()
    }
}

impl<'plan> ContainedPaintRecord<'plan> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Contained(self.record.floating()))
    }

    #[must_use]
    pub const fn floating(self) -> FloatingPresentationId {
        self.record.floating()
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.root()
    }

    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.record.ordinal()
    }

    /// Returns whether this presentation is frontmost in the complete core
    /// roster, including contained presentations clipped from this plan.
    #[must_use]
    pub const fn is_frontmost(self) -> bool {
        self.record.is_frontmost()
    }

    #[must_use]
    pub const fn outer_bounds(self) -> LogicalRect {
        self.record.outer_bounds()
    }

    #[must_use]
    pub const fn inner_bounds(self) -> LogicalRect {
        self.record.inner_bounds()
    }

    #[must_use]
    pub const fn title_bounds(self) -> LogicalRect {
        self.record.title_bounds()
    }

    #[must_use]
    pub const fn title_drag_bounds(self) -> LogicalRect {
        self.record.title_drag_hit().rect()
    }

    #[must_use]
    pub const fn content_bounds(self) -> LogicalRect {
        self.record.content_bounds()
    }

    #[must_use]
    pub const fn close_bounds(self) -> Option<LogicalRect> {
        self.record.close_bounds()
    }

    /// Returns whether any authoritative area of the title-drag control remains exposed.
    #[must_use]
    pub fn title_operable(self) -> bool {
        self.plan
            .region_is_operable(self.record.title_drag_hit().rect(), self.record.layer())
    }

    /// Returns whether the exact close control exists and remains exposed.
    #[must_use]
    pub fn close_operable(self) -> bool {
        self.record
            .close_bounds()
            .is_some_and(|bounds| self.plan.region_is_operable(bounds, self.record.layer()))
    }

    pub fn resize(self) -> impl ExactSizeIterator<Item = ContainedResizePaintRecord> + 'plan {
        let plan = self.plan;
        let layer = self.record.layer();
        let transform_operable = self.record.transform_operable();
        self.record
            .resize()
            .iter()
            .copied()
            .map(move |record| ContainedResizePaintRecord {
                operable: transform_operable && plan.region_is_operable(record.hit().rect(), layer),
                record,
            })
    }

    #[must_use]
    pub const fn minimum_size(self) -> LogicalSize {
        self.record.minimum_size()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Stable root-level presentation command shown by renderer-owned menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspacePresentationCommandKind {
    /// Present the root as contained floating content in its current logical surface.
    Float,
    /// Promote the root into a managed native child window.
    MoveToNewWindow,
    /// Recover the root into its current core-derived docking destination.
    DockBack,
    /// Dock the root at the left edge of its core-derived recovery target.
    DockLeft,
    /// Dock the root at the right edge of its core-derived recovery target.
    DockRight,
    /// Dock the root at the top edge of its core-derived recovery target.
    DockTop,
    /// Dock the root at the bottom edge of its core-derived recovery target.
    DockBottom,
    /// Merge the root into the center of its core-derived recovery target.
    DockCenter,
}

/// Why one exact presentation command is disabled in the current paint candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspacePresentationCommandUnavailable {
    /// The root is already presented as contained floating content.
    AlreadyContained,
    /// The root is already the main presentation of a managed native child and cannot be
    /// promoted into another child window.
    AlreadyNative,
    /// Core rejected the product action for its existing stable reason.
    Action(DockspaceActionRejection),
}

/// One opaque root command frozen from an exact current paint candidate.
#[derive(Clone, Copy, PartialEq)]
pub struct PresentationCommandPaintRecord {
    authority_domain: crate::ids::EngineAuthorityDomainId,
    version: crate::model::WorkspaceVersion,
    scene: crate::scene::SurfaceSceneStamp,
    surface: SurfaceId,
    root: RootId,
    kind: DockspacePresentationCommandKind,
    action: Option<ProductAction>,
    unavailable: Option<DockspacePresentationCommandUnavailable>,
}

impl PresentationCommandPaintRecord {
    fn ready(
        authority_domain: crate::ids::EngineAuthorityDomainId,
        version: crate::model::WorkspaceVersion,
        scene: crate::scene::SurfaceSceneStamp,
        root: RootId,
        kind: DockspacePresentationCommandKind,
        action: ProductAction,
    ) -> Self {
        Self {
            authority_domain,
            version,
            scene,
            surface: scene.surface(),
            root,
            kind,
            action: Some(action),
            unavailable: None,
        }
    }

    fn unavailable(
        authority_domain: crate::ids::EngineAuthorityDomainId,
        version: crate::model::WorkspaceVersion,
        scene: crate::scene::SurfaceSceneStamp,
        root: RootId,
        kind: DockspacePresentationCommandKind,
        reason: DockspacePresentationCommandUnavailable,
    ) -> Self {
        Self {
            authority_domain,
            version,
            scene,
            surface: scene.surface(),
            root,
            kind,
            action: None,
            unavailable: Some(reason),
        }
    }

    /// Returns the stable root addressed by this command.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns this command's stable semantic kind.
    #[must_use]
    pub const fn kind(self) -> DockspacePresentationCommandKind {
        self.kind
    }

    /// Returns whether core admitted this command for the exact paint candidate.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.action.is_some()
    }

    /// Returns the stable reason this command is disabled, if any.
    #[must_use]
    pub const fn unavailable_reason(self) -> Option<DockspacePresentationCommandUnavailable> {
        self.unavailable
    }
}

impl fmt::Debug for PresentationCommandPaintRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PresentationCommandPaintRecord")
            .field("surface", &self.surface)
            .field("root", &self.root)
            .field("kind", &self.kind)
            .field("ready", &self.is_ready())
            .field("unavailable", &self.unavailable)
            .finish_non_exhaustive()
    }
}

/// Borrowed semantic program for one exact logical-surface output.
///
/// A host may consume its operations immediately or compile them into an owned
/// display list or retained-node update. Records borrow this frame: copy the
/// required geometry, identities, and content-slot information before committing
/// it. Content resources and renderer scheduling remain host-owned.
///
/// Preview-only acknowledgements cannot replace complete output settlement:
///
/// ```compile_fail
/// fn preview_only(plan: dockspace::runtime::SurfacePaintPlan<'_>) {
///     plan.prepare_drag_preview_painted();
/// }
/// ```
///
/// ```compile_fail
/// fn transform_only(plan: dockspace::runtime::SurfacePaintPlan<'_>) {
///     plan.prepare_contained_transform_preview_painted();
/// }
/// ```
#[derive(Clone, Copy)]
pub struct SurfacePaintPlan<'frame> {
    encoding_started: &'frame std::cell::Cell<bool>,
    encoding_pass: u64,
    pub(super) surface: SurfaceId,
    pub(super) attempt: HostPresentationAttemptId,
    pub(super) authority_domain: crate::ids::EngineAuthorityDomainId,
    pub(super) version: crate::model::WorkspaceVersion,
    pub(super) scene: crate::scene::SurfaceSceneStamp,
    pub(super) output: SurfacePresentationOutputTicket,
    pub(super) contribution: SurfaceContributionToken,
    pub(super) interaction: HostInteractionPresentation,
    pub(super) plan: &'frame PresentationPlan,
    pub(super) hit_manifest: &'frame crate::presentation_hit::PresentationHitManifest,
    pub(super) splitter_keyboard_step: f64,
    pub(super) escape_delivery: Option<EscapeDelivery>,
    pub(super) drop_affordance: Option<&'frame DropAffordance>,
    pub(super) drag_preview: Option<&'frame InteractionPreview>,
    pub(super) drag_decoration: Option<DockspaceDragDecoration<'frame>>,
    pub(super) contained_transform_preview: Option<&'frame ContainedTransformPreview>,
    pub(super) pane_focus_request: Option<DockspacePaneFocusRequest>,
    pub(super) candidate: crate::engine::HostFrameView<'frame>,
    pub(super) program_valid: bool,
    pub(super) program_link: Option<&'frame crate::scene::SurfacePaintProgramLink>,
}

/// Opaque receipt that a host accepted one exact program from beginning to end.
///
/// The receipt is bound to the host-frame attempt, semantic scene, coordinate
/// capture, and transient interaction visuals from which the program was
/// produced. It is not cloneable and must be consumed by
/// [`DockspaceHostFrame::confirm_surface_painted`]. It proves complete emission
/// to the sink, not GPU submission or physical presentation. The host reports
/// the later output disposition through [`super::DockspaceSession::report_surface_presentation`].
#[must_use = "a completed surface program must be confirmed or explicitly discarded"]
pub struct SurfaceProgramExecutionReceipt {
    encoding_pass: u64,
    surface: SurfaceId,
    attempt: HostPresentationAttemptId,
    output: SurfacePresentationOutputTicket,
    contribution: SurfaceContributionToken,
    interaction: HostInteractionPresentation,
}

impl SurfaceProgramExecutionReceipt {
    /// Returns the sole logical surface executed by this receipt.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub(super) const fn into_presentation_parts(
        self,
    ) -> (SurfaceContributionToken, HostInteractionPresentation) {
        (self.contribution, self.interaction)
    }
}

impl fmt::Debug for SurfaceProgramExecutionReceipt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurfaceProgramExecutionReceipt")
            .field("surface", &self.surface)
            .finish_non_exhaustive()
    }
}

/// Root presentation kind in the core-owned surface paint program.
#[derive(Debug, Clone, Copy)]
pub enum SurfacePaintProgramRootKind<'frame> {
    /// The surface's ordinary main root.
    Main,
    /// A contained-floating root painted in roster order.
    Contained(ContainedPaintRecord<'frame>),
}

/// One operation in the core-owned surface paint program.
#[derive(Debug, Clone, Copy)]
pub enum SurfacePaintProgramOperation<'frame> {
    /// Paint one pane background and content host.
    Pane(PanePaintRecord<'frame>),
    /// Paint one tab strip.
    TabBar(TabBarPaintRecord<'frame>),
    /// Paint one visible tab.
    Tab(TabPaintRecord<'frame>),
    /// Paint one splitter.
    Splitter(SplitterPaintRecord<'frame>),
    /// Paint one atomic splitter junction.
    SplitterJunction(SplitterJunctionPaintRecord<'frame>),
    /// Paint one tab-strip control.
    TabStripControl(TabStripControlPaintRecord),
    /// Paint the contained-window title and resize controls.
    ContainedControls(ContainedPaintRecord<'frame>),
    /// Paint one root-scoped presentation-menu anchor.
    PresentationMenuAnchor(PresentationMenuAnchorPaintRecord<'frame>),
}

impl SurfacePaintProgramOperation<'_> {
    /// Returns the exact stable visual identity executed by this operation.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        match self {
            Self::Pane(record) => record.visual_id(),
            Self::TabBar(record) => record.visual_id(),
            Self::Tab(record) => record.visual_id(),
            Self::Splitter(record) => record.visual_id(),
            Self::SplitterJunction(record) => record.visual_id(),
            Self::TabStripControl(record) => record.visual_id(),
            Self::ContainedControls(record) => record.visual_id(),
            Self::PresentationMenuAnchor(record) => record.visual_id(),
        }
    }
}

/// One root header in the core-owned paint program.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePaintProgramRoot<'frame> {
    root: RootId,
    kind: SurfacePaintProgramRootKind<'frame>,
    title_item: Option<ItemId>,
}

impl SurfacePaintProgramRoot<'_> {
    /// Returns the stable root identity.
    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns whether the root is main or contained-floating.
    #[must_use]
    pub const fn kind(&self) -> SurfacePaintProgramRootKind<'_> {
        self.kind
    }

    /// Returns the contained frame when this is a contained root.
    #[must_use]
    pub const fn contained(&self) -> Option<ContainedPaintRecord<'_>> {
        match self.kind {
            SurfacePaintProgramRootKind::Main => None,
            SurfacePaintProgramRootKind::Contained(record) => Some(record),
        }
    }

    /// Returns the first selected item in core traversal order.
    #[must_use]
    pub const fn title_item(&self) -> Option<ItemId> {
        self.title_item
    }
}

/// One linear step in the core-owned surface paint program.
#[derive(Debug, Clone, Copy)]
pub enum SurfacePaintProgramStep<'frame> {
    /// Starts one main or contained root segment.
    RootBegin {
        /// Core-owned root segment being opened.
        root: SurfacePaintProgramRoot<'frame>,
        /// Paint disposition for contained frame chrome; main roots are always visible.
        paint: SurfacePaintDisposition,
    },
    /// Executes one root-scoped operation.
    Operation {
        /// Core-owned root segment containing the operation.
        root: SurfacePaintProgramRoot<'frame>,
        /// Exact paint operation.
        operation: SurfacePaintProgramOperation<'frame>,
        /// Core-derived paint disposition for this exact operation.
        paint: SurfacePaintDisposition,
    },
    /// Ends the active root segment.
    RootEnd(SurfacePaintProgramRoot<'frame>),
    /// Paints one workspace-global popup backdrop.
    PopupBackdrop(TabListMenuBackdropPaintRecord),
    /// Paints one workspace-global tab-list menu.
    PopupMenu {
        /// Exact popup operation.
        menu: TabListMenuPaintRecord<'frame>,
        /// Core-derived disposition shared by its frame, rows, and scrollbar.
        paint: SurfacePaintDisposition,
    },
    /// Paints the current docking or rehome preview.
    DragPreview(DockspaceDragPreview<'frame>),
    /// Publishes one non-visual drop target through the ordered program.
    DropTarget(DropTargetPaintRecord<'frame>),
    /// Paints one guide cluster in core order.
    DropGuide {
        /// Structural guide identity and geometry compiled into the scene.
        structural: DropGuidePaintRecord<'frame>,
        /// Payload-specific visible guide state for the current drag point.
        affordance: Option<DropAffordanceClusterPaintRecord<'frame>>,
    },
    /// Paints the current contained transform preview.
    ContainedTransformPreview(DockspaceContainedTransformPreview<'frame>),
    /// Paints the current drag ghost before popup projection.
    DragDecoration(DockspaceDragDecoration<'frame>),
    /// Resolves typed interaction feedback after popup input has run.
    InteractionFeedback,
    /// Seals the ordered traversal for this exact semantic output.
    SurfaceEnd {
        /// Candidate output represented by every preceding operation.
        semantic_output: DockspaceSemanticOutput,
    },
}

/// Failure while resolving or executing one compiled paint program.
#[derive(Debug)]
pub enum SurfacePaintProgramVisitError<E> {
    /// The sealed program no longer matches its canonical record roster.
    InvalidProgram {
        /// Surface whose program failed its internal invariant.
        surface: SurfaceId,
    },
    /// The renderer rejected one emitted step.
    Sink(E),
}

impl<E: Into<DockspaceRuntimeError>> SurfacePaintProgramVisitError<E> {
    /// Converts either failure lane into the renderer-neutral facade error.
    #[doc(hidden)]
    pub fn into_runtime_error(self) -> DockspaceRuntimeError {
        match self {
            Self::InvalidProgram { surface } => {
                DockspaceRuntimeError::surface_program_invariant(surface)
            }
            Self::Sink(error) => error.into(),
        }
    }
}

impl fmt::Debug for SurfacePaintPlan<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SurfacePaintPlan")
            .field("surface", &self.surface)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// One update's normal result before any pane content is encoded.
pub enum SurfaceWork<'frame> {
    Ready(SurfacePaintPlan<'frame>),
    Deferred(SurfaceWorkDeferred),
}

/// A surface can be deferred while semantic and terminal progress still publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceWorkDeferred {
    Unmeasured,
    OutputCapacity,
}

impl<'frame> SurfaceWork<'frame> {
    pub fn ready(self) -> Option<SurfacePaintPlan<'frame>> {
        match self {
            Self::Ready(plan) => Some(plan),
            Self::Deferred(_) => None,
        }
    }
}

impl DockspaceHostFrame<'_> {
    /// Seals semantic input and retains only the derived state needed for actual
    /// renderer multipass retries. No pane callback is invoked by this method.
    pub fn begin_encoding_pass(&mut self) -> Result<(), DockspaceRuntimeError> {
        if self.encoding_checkpoint.is_none() {
            self.complete_pointer_input()?;
            self.encoding_checkpoint = Some(self.frame.capture_encoding_checkpoint()?);
        }
        self.encoding_started.set(true);
        Ok(())
    }

    /// Discards the previous pass's private encoding and starts its replacement.
    /// Accepted input remains in the same candidate and cannot be replayed.
    pub fn restart_encoding_pass(&mut self) -> Result<(), DockspaceRuntimeError> {
        let checkpoint = self
            .encoding_checkpoint
            .as_ref()
            .ok_or(DockspaceInteractionError::EncodingPassUnavailable)?;
        let next = self
            .encoding_pass
            .checked_add(1)
            .ok_or(DockspaceInteractionError::EncodingPassExhausted)?;
        self.frame.restart_encoding_from(checkpoint)?;
        self.painted_surfaces.clear();
        self.painted_native_staging.clear();
        self.encoding_surfaces.clear();
        self.encoding_pass = next;
        self.encoding_started.set(true);
        Ok(())
    }

    /// Admits an output before encoding its pane content. Backpressure is a
    /// normal deferred result and never prevents a zero-output publication.
    pub fn surface_work(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceWork<'_>, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        if self
            .frame
            .view()
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .is_none()
        {
            return Ok(SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured));
        }
        if !self.encoding_surfaces.contains(&surface) {
            let (observations, stream) = self.frame.output_admission_facts(surface);
            if !self.session.presentation.reserve_output(
                &self.submitted_presentation,
                observations,
                self.encoding_surfaces.len(),
                stream,
                self.encoding_surfaces
                    .iter()
                    .map(|surface| self.frame.output_admission_facts(*surface).1),
            )? {
                return Ok(SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity));
            }
            self.encoding_surfaces.insert(surface);
        }
        Ok(match self.prepared_paint_plan(surface)? {
            Some(plan) => SurfaceWork::Ready(plan),
            None => SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured),
        })
    }

    /// Returns the admitted paint plan, if its scene and output capacity are available.
    pub fn paint_plan(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<SurfacePaintPlan<'_>>, DockspaceRuntimeError> {
        Ok(self.surface_work(surface)?.ready())
    }

    /// Freezes pointer input and returns the current candidate plan which the
    /// renderer may paint.
    ///
    /// # Errors
    ///
    /// Returns an error when the pointer-input phase cannot be completed.
    fn prepared_paint_plan(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<SurfacePaintPlan<'_>>, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let view = self.frame.view();
        let Some(scene) = view.scene().surface(surface).and_then(SurfaceScene::ready) else {
            return Ok(None);
        };
        let candidate = scene.candidate();
        let drop_affordance = view.presentation_drop_affordance(surface);
        let drag_decoration = view
            .interaction()
            .active_drag_view()
            .and_then(|drag| drag_decoration::resolve(view.scene(), drag));
        let version = view.version();
        let pane_focus_request = view
            .published_pane_focus_intent()
            .filter(|intent| intent.surface() == surface)
            .map(|intent| {
                DockspacePaneFocusRequest::from_intent(
                    self.session.engine.authority_domain(),
                    version.epoch(),
                    intent,
                )
            });
        let escape_delivery =
            if view.interaction().status() == crate::interaction::InteractionStatus::Idle {
                None
            } else if let Some(record) = view.viewport().viewport(surface) {
                Some(EscapeDelivery::NativeBinding(record.binding()))
            } else {
                (view.interaction().active_surface() == Some(surface))
                    .then_some(EscapeDelivery::Surface(surface))
            };
        let plan = SurfacePaintPlan {
            encoding_started: &self.encoding_started,
            encoding_pass: self.encoding_pass,
            surface,
            attempt: self.frame.presentation_attempt(),
            authority_domain: self.session.engine.authority_domain(),
            version,
            scene: candidate.stamp(),
            output: candidate.output_ticket(),
            contribution: view.begin_surface_contribution(surface)?,
            interaction: view
                .presentation_interaction(surface)
                .ok_or_else(|| DockspaceRuntimeError::surface_program_invariant(surface))?,
            plan: candidate.plan(),
            hit_manifest: candidate.hit_manifest(),
            splitter_keyboard_step: view.presentation_config().splitter_keyboard_step(),
            escape_delivery,
            drop_affordance,
            drag_preview: view.presentation_drag_preview(surface),
            drag_decoration,
            contained_transform_preview: view.presentation_contained_transform_preview(surface),
            pane_focus_request,
            candidate: view,
            program_valid: candidate.paint_program().is_some(),
            program_link: candidate.paint_program(),
        };
        Ok(Some(plan))
    }

    /// Records complete emission of the current plan to the host's accepting sink,
    /// including every transient preview exposed by [`Self::paint_plan`].
    ///
    /// The sink may have drawn immediately or recorded owned work for later
    /// execution. This does not report GPU submission or presentation. A successful
    /// frame commit emits a [`super::PaintedSurfaceOutput`] which the output owner
    /// must later report as presented or dropped.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface was already answered or has no
    /// current paintable plan.
    pub fn confirm_surface_painted(
        &mut self,
        receipt: SurfaceProgramExecutionReceipt,
    ) -> Result<(), DockspaceRuntimeError> {
        let surface = receipt.surface;
        if self.painted_surfaces.contains_key(&surface)
            || self
                .frame
                .surface_contributions()
                .any(|entry| entry.surface() == surface)
        {
            return Err(DockspaceInteractionError::SurfaceAlreadyAnswered { surface }.into());
        }
        let view = self.frame.view();
        let Some(candidate) = view
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .map(|scene| scene.candidate())
        else {
            return Err(DockspaceInteractionError::SurfaceNotPaintable { surface }.into());
        };
        let expected_contribution = view.begin_surface_contribution(surface)?;
        let expected_interaction = view
            .presentation_interaction(surface)
            .ok_or_else(|| DockspaceRuntimeError::surface_program_invariant(surface))?;
        if receipt.encoding_pass != self.encoding_pass
            || receipt.attempt != self.frame.presentation_attempt()
            || receipt.output != candidate.output_ticket()
            || receipt.contribution != expected_contribution
            || receipt.interaction != expected_interaction
        {
            return Err(
                DockspaceInteractionError::SurfaceProgramExecutionMismatch { surface }.into(),
            );
        }
        self.painted_surfaces.insert(surface, receipt);
        Ok(())
    }
}

impl<'frame> SurfacePaintPlan<'frame> {
    /// Visits tab resource requests in their original surface-program order.
    ///
    /// Requests include visible tabs, popup rows and the active drag ghost,
    /// including repeated items. This reads the sealed program without building
    /// interaction descriptors or granting a surface execution receipt.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error when the sealed program is invalid.
    /// Invalid programs are rejected before the visitor receives any request.
    pub fn visit_tab_resources(
        &self,
        mut visitor: impl FnMut(DockspaceVisualId, ItemId),
    ) -> Result<(), DockspaceRuntimeError> {
        if !self.program_valid {
            return Err(DockspaceRuntimeError::surface_program_invariant(
                self.surface,
            ));
        }
        self.plan
            .visit_surface_program(|step| {
                match step {
                    ResolvedSurfaceProgramStep::Operation {
                        operation: ResolvedSurfaceProgramOperation::Tab(record),
                        ..
                    } => visitor(
                        DockspaceVisualId(VisualIdentity::Tab(*record.id())),
                        record.id().item,
                    ),
                    ResolvedSurfaceProgramStep::PopupMenu(menu) => {
                        for row in menu.rows() {
                            visitor(
                                DockspaceVisualId(VisualIdentity::Tab(row.tab())),
                                row.tab().item,
                            );
                        }
                    }
                    ResolvedSurfaceProgramStep::DragDecoration => {
                        if let Some(decoration) = self.drag_decoration()
                            && let Some(item) = decoration.ghost_item()
                        {
                            visitor(decoration.source_visual(), item);
                        }
                    }
                    ResolvedSurfaceProgramStep::RootBegin(_)
                    | ResolvedSurfaceProgramStep::Operation { .. }
                    | ResolvedSurfaceProgramStep::RootEnd(_)
                    | ResolvedSurfaceProgramStep::PopupBackdrop(_)
                    | ResolvedSurfaceProgramStep::DragPreview
                    | ResolvedSurfaceProgramStep::DropTarget(_)
                    | ResolvedSurfaceProgramStep::DropGuide(_)
                    | ResolvedSurfaceProgramStep::ContainedTransformPreview
                    | ResolvedSurfaceProgramStep::InteractionFeedback
                    | ResolvedSurfaceProgramStep::SurfaceEnd => {}
                }
                Ok::<(), std::convert::Infallible>(())
            })
            .map_err(|_| DockspaceRuntimeError::surface_program_invariant(self.surface))
    }

    /// Returns whether this exact program includes a manipulation preview.
    ///
    /// Hosts preserve the current candidate until its release presentation
    /// obligation settles, including after the physical pointer was released.
    #[must_use]
    pub fn has_manipulation_preview(&self) -> bool {
        self.drag_preview.is_some()
            || self.contained_transform_preview.is_some()
            || self.plan.contained_projection().is_some()
    }

    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Inspects the single ordered program for this exact surface candidate.
    ///
    /// The visitor receives the core-owned order directly. Official adapters
    /// execute that order without rebuilding a second surface-wide schedule;
    /// test helpers and measurement code may collect a trace without gaining
    /// authority to confirm that a renderer executed the program. Any stale
    /// record identity fails the whole traversal before the visitor sees any
    /// step.
    ///
    /// # Errors
    ///
    /// Returns [`SurfacePaintProgramVisitError::InvalidProgram`] when the
    /// sealed tape no longer matches its canonical record roster, or
    /// [`SurfacePaintProgramVisitError::Sink`] when the renderer rejects a
    /// step.
    pub fn visit_surface_program<E>(
        self,
        visitor: impl FnMut(SurfacePaintProgramStep<'frame>) -> Result<(), E>,
    ) -> Result<(), SurfacePaintProgramVisitError<E>> {
        self.visit_surface_program_inner(visitor)
    }

    /// Executes the single ordered program for this exact surface candidate.
    ///
    /// Unlike [`Self::visit_surface_program`], successful execution returns an
    /// affine receipt which is the only capability accepted by
    /// [`DockspaceHostFrame::confirm_surface_painted`]. Renderer adapters must
    /// use this path only when their sink has accepted every program step.
    /// Accepting owned records for deferred rendering satisfies this contract;
    /// later presentation is a separate host report. Hosts preserve composition
    /// order and clipping when compiling or batching these records.
    ///
    /// # Errors
    ///
    /// Returns [`SurfacePaintProgramVisitError::InvalidProgram`] when the
    /// sealed tape no longer matches its canonical record roster, or
    /// [`SurfacePaintProgramVisitError::Sink`] when the renderer rejects a
    /// step. Neither failure mints a receipt.
    pub fn execute_surface_program<E>(
        self,
        visitor: impl FnMut(SurfacePaintProgramStep<'frame>) -> Result<(), E>,
    ) -> Result<SurfaceProgramExecutionReceipt, SurfacePaintProgramVisitError<E>> {
        self.encoding_started.set(true);
        self.visit_surface_program_inner(visitor)?;
        Ok(SurfaceProgramExecutionReceipt {
            encoding_pass: self.encoding_pass,
            surface: self.surface,
            attempt: self.attempt,
            output: self.output,
            contribution: self.contribution,
            interaction: self.interaction,
        })
    }

    fn visit_surface_program_inner<E>(
        &self,
        mut visitor: impl FnMut(SurfacePaintProgramStep<'frame>) -> Result<(), E>,
    ) -> Result<(), SurfacePaintProgramVisitError<E>> {
        if !self.program_valid {
            return Err(SurfacePaintProgramVisitError::InvalidProgram {
                surface: self.surface,
            });
        }

        let menu_open_bars = self.program_menu_open_bars();
        let mut targets = self.program_link.map(|link| link.targets());
        let mut sink_error = None;
        let traversal = self.plan.visit_surface_program(|step| {
            let target = if matches!(step, ResolvedSurfaceProgramStep::DropTarget(_)) {
                match targets.as_mut() {
                    Some(targets) => Some(targets.next().ok_or(())?),
                    None => None,
                }
            } else {
                None
            };
            let resolved = match self.resolve_surface_program_step(step, &menu_open_bars, target) {
                Ok(Some(step)) => step,
                Ok(None) => return Ok(()),
                Err(()) => return Err(()),
            };
            match visitor(resolved) {
                Ok(()) => Ok(()),
                Err(error) => {
                    sink_error = Some(error);
                    Err(())
                }
            }
        });
        if let Some(error) = sink_error {
            return Err(SurfacePaintProgramVisitError::Sink(error));
        }
        if traversal.is_err() || targets.is_some_and(|mut targets| targets.next().is_some()) {
            return Err(SurfacePaintProgramVisitError::InvalidProgram {
                surface: self.surface,
            });
        }
        Ok(())
    }

    #[cfg(test)]
    fn validate_surface_program(&self) -> bool {
        crate::scene::validate_surface_paint_program(self.plan, self.hit_manifest)
    }

    fn resolve_surface_program_step(
        &self,
        step: ResolvedSurfaceProgramStep<'frame>,
        menu_open_bars: &HashSet<TabBarSceneId>,
        linked_target: Option<(usize, LogicalPoint)>,
    ) -> Result<Option<SurfacePaintProgramStep<'frame>>, ()> {
        Ok(match step {
            ResolvedSurfaceProgramStep::RootBegin(root) => {
                let root = self.resolve_surface_program_root(root);
                let paint = root
                    .contained()
                    .map_or(SurfacePaintDisposition::Visible, |record| {
                        paint_disposition(self.drag_decoration, record.visual_id())
                    });
                Some(SurfacePaintProgramStep::RootBegin { root, paint })
            }
            ResolvedSurfaceProgramStep::Operation { root, operation } => {
                let root = self.resolve_surface_program_root(root);
                let operation = self
                    .resolve_surface_program_operation_record(operation, menu_open_bars)
                    .ok_or(())?;
                let paint = paint_disposition(self.drag_decoration, operation.visual_id());
                Some(SurfacePaintProgramStep::Operation {
                    root,
                    operation,
                    paint,
                })
            }
            ResolvedSurfaceProgramStep::RootEnd(root) => Some(SurfacePaintProgramStep::RootEnd(
                self.resolve_surface_program_root(root),
            )),
            ResolvedSurfaceProgramStep::PopupBackdrop(record) => Some(
                SurfacePaintProgramStep::PopupBackdrop(TabListMenuBackdropPaintRecord {
                    surface: self.surface,
                    record: *record,
                }),
            ),
            ResolvedSurfaceProgramStep::PopupMenu(record) => {
                let backdrop = *self
                    .plan
                    .tab_list_menu_backdrop_records()
                    .iter()
                    .find(|backdrop| backdrop.session() == record.session())
                    .ok_or(())?;
                let menu = TabListMenuPaintRecord {
                    record,
                    surface: self.surface,
                    backdrop,
                };
                let paint = paint_disposition(self.drag_decoration, menu.visual_id());
                Some(SurfacePaintProgramStep::PopupMenu { menu, paint })
            }
            ResolvedSurfaceProgramStep::DragPreview => self
                .drag_preview()
                .map(SurfacePaintProgramStep::DragPreview),
            ResolvedSurfaceProgramStep::DropTarget(record) => {
                let (region, center) = if let Some((row, center)) = linked_target {
                    (self.hit_manifest.regions().get(row).ok_or(())?, center)
                } else {
                    let region = self
                        .hit_manifest
                        .region_for_kind(PresentationHitRegionKind::DropTarget(record.id()))
                        .ok_or(())?;
                    if region.is_passive()
                        || receiver_role(region.id().kind())
                            != Some(DockspaceReceiverRole::DropTarget)
                    {
                        return Err(());
                    }
                    let center =
                        crate::scene::paint_receiver_center(region.hit().rect()).ok_or(())?;
                    (region, center)
                };
                Some(SurfacePaintProgramStep::DropTarget(DropTargetPaintRecord {
                    output: self.output,
                    region,
                    center,
                }))
            }
            ResolvedSurfaceProgramStep::DropGuide(record) => {
                let structural = DropGuidePaintRecord { record };
                let affordance = self.drop_affordance().and_then(|affordance| {
                    affordance
                        .clusters()
                        .find(|cluster| cluster.visual_id() == structural.visual_id())
                });
                Some(SurfacePaintProgramStep::DropGuide {
                    structural,
                    affordance,
                })
            }
            ResolvedSurfaceProgramStep::ContainedTransformPreview => self
                .contained_transform_preview()
                .map(SurfacePaintProgramStep::ContainedTransformPreview),
            ResolvedSurfaceProgramStep::DragDecoration => self
                .drag_decoration()
                .map(SurfacePaintProgramStep::DragDecoration),
            ResolvedSurfaceProgramStep::InteractionFeedback => {
                Some(SurfacePaintProgramStep::InteractionFeedback)
            }
            ResolvedSurfaceProgramStep::SurfaceEnd => Some(SurfacePaintProgramStep::SurfaceEnd {
                semantic_output: self.semantic_output(),
            }),
        })
    }

    fn resolve_surface_program_root(
        &self,
        root: ResolvedSurfaceProgramRoot<'frame>,
    ) -> SurfacePaintProgramRoot<'frame> {
        let kind = match root.contained_record() {
            None => SurfacePaintProgramRootKind::Main,
            Some(record) => SurfacePaintProgramRootKind::Contained(ContainedPaintRecord {
                record,
                plan: self.plan,
            }),
        };
        SurfacePaintProgramRoot {
            root: root.root(),
            kind,
            title_item: root.title_item(),
        }
    }

    fn resolve_surface_program_operation_record(
        &self,
        operation: ResolvedSurfaceProgramOperation<'frame>,
        menu_open_bars: &HashSet<TabBarSceneId>,
    ) -> Option<SurfacePaintProgramOperation<'frame>> {
        Some(match operation {
            ResolvedSurfaceProgramOperation::Pane(record) => {
                SurfacePaintProgramOperation::Pane(PanePaintRecord {
                    record,
                    selected: self.plan.pane_selected(record),
                })
            }
            ResolvedSurfaceProgramOperation::TabBar(record) => {
                SurfacePaintProgramOperation::TabBar(TabBarPaintRecord { record })
            }
            ResolvedSurfaceProgramOperation::Tab(record) => {
                SurfacePaintProgramOperation::Tab(TabPaintRecord {
                    record,
                    selected: self.plan.tab_selected(record),
                    operable: self.tab_is_operable(record),
                })
            }
            ResolvedSurfaceProgramOperation::Splitter(record) => {
                SurfacePaintProgramOperation::Splitter(SplitterPaintRecord { record })
            }
            ResolvedSurfaceProgramOperation::SplitterJunction(record) => {
                SurfacePaintProgramOperation::SplitterJunction(SplitterJunctionPaintRecord {
                    record,
                })
            }
            ResolvedSurfaceProgramOperation::TabStripControl(record) => {
                SurfacePaintProgramOperation::TabStripControl(TabStripControlPaintRecord {
                    record: *record,
                    menu_open: menu_open_bars.contains(&record.id().bar()),
                })
            }
            ResolvedSurfaceProgramOperation::ContainedControls(record) => {
                SurfacePaintProgramOperation::ContainedControls(ContainedPaintRecord {
                    record,
                    plan: self.plan,
                })
            }
            ResolvedSurfaceProgramOperation::PresentationMenuAnchor(record) => {
                let bounds = record.ready_bounds()?;
                SurfacePaintProgramOperation::PresentationMenuAnchor(
                    PresentationMenuAnchorPaintRecord {
                        record,
                        bounds,
                        operable: self.plan.region_is_operable(bounds, record.layer()),
                    },
                )
            }
        })
    }

    /// Returns the exact semantic output represented by this paint plan.
    #[must_use]
    pub const fn semantic_output(&self) -> DockspaceSemanticOutput {
        DockspaceSemanticOutput {
            ticket: self.output,
        }
    }

    /// Returns the exact pane-focus request admitted for this surface.
    #[must_use]
    pub const fn pane_focus_request(&self) -> Option<DockspacePaneFocusRequest> {
        self.pane_focus_request
    }

    /// Prepares one exact final-pass observation for this plan's focus request.
    #[must_use]
    pub fn prepare_pane_focus_observation(
        self,
        request: DockspacePaneFocusRequest,
        observation: DockspacePaneFocusObservation,
    ) -> Option<PreparedPaneFocusObservation> {
        (self.pane_focus_request == Some(request))
            .then(|| PreparedPaneFocusObservation::new(request, observation))
    }

    /// Returns one root-level presentation command frozen from this exact candidate.
    ///
    /// A root owned by another surface is not part of this plan and returns `None`.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when core command preflight violates an invariant.
    pub fn presentation_command(
        self,
        root: RootId,
        kind: DockspacePresentationCommandKind,
    ) -> Result<Option<PresentationCommandPaintRecord>, DockspaceRuntimeError> {
        let owner = match self.candidate.workspace().presentation_for_root(root) {
            Some(owner) => owner,
            None => return Ok(None),
        };
        let owner_surface = match owner {
            crate::RootPresentationOwner::Main { surface }
            | crate::RootPresentationOwner::Contained { surface, .. } => surface,
        };
        if owner_surface != self.surface {
            return Ok(None);
        }

        let unavailable = |reason| {
            PresentationCommandPaintRecord::unavailable(
                self.authority_domain,
                self.version,
                self.scene,
                root,
                kind,
                reason,
            )
        };
        if kind == DockspacePresentationCommandKind::Float
            && matches!(owner, crate::RootPresentationOwner::Contained { .. })
        {
            return Ok(Some(unavailable(
                DockspacePresentationCommandUnavailable::AlreadyContained,
            )));
        }
        if kind == DockspacePresentationCommandKind::MoveToNewWindow
            && self.candidate.root_is_bound_native_main(root)
        {
            return Ok(Some(unavailable(
                DockspacePresentationCommandUnavailable::AlreadyNative,
            )));
        }

        let action = match kind {
            DockspacePresentationCommandKind::Float => {
                let target_surface = match self.candidate.native_main_recovery_surface(root) {
                    Ok(Some(surface)) => surface,
                    Ok(None) => self.surface,
                    Err(reason) => {
                        return Ok(Some(unavailable(
                            DockspacePresentationCommandUnavailable::Action(reason),
                        )));
                    }
                };
                let rect = if target_surface == self.surface {
                    match self.candidate.default_contained_float_rect(
                        root,
                        target_surface,
                        self.plan,
                    ) {
                        Ok(rect) => Some(rect),
                        Err(reason) => {
                            return Ok(Some(unavailable(
                                DockspacePresentationCommandUnavailable::Action(reason),
                            )));
                        }
                    }
                } else {
                    None
                };
                ProductAction::FloatRoot {
                    root,
                    surface: target_surface,
                    rect,
                }
            }
            DockspacePresentationCommandKind::DockBack => ProductAction::DockBackRoot { root },
            DockspacePresentationCommandKind::DockLeft
            | DockspacePresentationCommandKind::DockRight
            | DockspacePresentationCommandKind::DockTop
            | DockspacePresentationCommandKind::DockBottom
            | DockspacePresentationCommandKind::DockCenter => {
                let edge = match kind {
                    DockspacePresentationCommandKind::DockLeft => {
                        Some(crate::model::DockEdge::Left)
                    }
                    DockspacePresentationCommandKind::DockRight => {
                        Some(crate::model::DockEdge::Right)
                    }
                    DockspacePresentationCommandKind::DockTop => Some(crate::model::DockEdge::Top),
                    DockspacePresentationCommandKind::DockBottom => {
                        Some(crate::model::DockEdge::Bottom)
                    }
                    DockspacePresentationCommandKind::DockCenter => None,
                    DockspacePresentationCommandKind::Float
                    | DockspacePresentationCommandKind::MoveToNewWindow
                    | DockspacePresentationCommandKind::DockBack => unreachable!(
                        "directional presentation commands are matched by the outer branch"
                    ),
                };
                let placement = match self.candidate.directional_dock_placement(root, edge) {
                    Ok(placement) => placement,
                    Err(reason) => {
                        return Ok(Some(unavailable(
                            DockspacePresentationCommandUnavailable::Action(reason),
                        )));
                    }
                };
                ProductAction::DockRoot { root, placement }
            }
            DockspacePresentationCommandKind::MoveToNewWindow => {
                let placement = match self
                    .candidate
                    .default_native_window_placement(root, self.plan)
                {
                    Ok(placement) => placement,
                    Err(reason) => {
                        return Ok(Some(unavailable(
                            DockspacePresentationCommandUnavailable::Action(reason),
                        )));
                    }
                };
                ProductAction::TearOffRoot { root, placement }
            }
        };
        let record = match self.candidate.prepare_product_action_if_available(action)? {
            Ok(prepared) => {
                let (authority_domain, version, action) = prepared.into_parts();
                debug_assert_eq!(authority_domain, self.authority_domain);
                debug_assert_eq!(version, self.version);
                PresentationCommandPaintRecord::ready(
                    authority_domain,
                    version,
                    self.scene,
                    root,
                    kind,
                    action,
                )
            }
            Err(reason) => unavailable(DockspacePresentationCommandUnavailable::Action(reason)),
        };
        Ok(Some(record))
    }

    /// Prepares one exact scene-bound activation for a ready presentation command.
    #[must_use]
    pub fn prepare_presentation_command(
        self,
        record: PresentationCommandPaintRecord,
    ) -> Option<super::PreparedSurfaceAction> {
        (record.authority_domain == self.authority_domain
            && record.version == self.version
            && record.scene == self.scene
            && record.surface == self.surface)
            .then_some(())?;
        Some(super::PreparedSurfaceAction::presentation_command(
            self.authority_domain,
            self.version,
            self.scene,
            self.frozen_action_witness()?,
            record.action?,
        ))
    }

    /// Prepares a complete-root close request from an operable command-menu anchor.
    ///
    /// Returns `None` when the root is occluded, absent or disallows closing.
    #[must_use]
    pub fn prepare_root_close(self, root: RootId) -> Option<super::PreparedSurfaceAction> {
        self.plan
            .presentation_menu_anchor_records()
            .iter()
            .find(|anchor| {
                anchor.root() == root
                    && anchor
                        .ready_bounds()
                        .is_some_and(|bounds| self.plan.region_is_operable(bounds, anchor.layer()))
            })?;
        self.candidate.root_close_is_available(root).then_some(())?;
        Some(super::PreparedSurfaceAction::close_root(
            self.authority_domain,
            self.version,
            self.scene,
            self.frozen_action_witness()?,
            root,
        ))
    }

    /// Prepares a current-candidate tab selection without exposing scene identity.
    ///
    /// Activation requests the selected pane's content focus. Use
    /// [`Self::prepare_tab_navigation`] to preserve framework tab focus instead.
    #[must_use]
    pub fn prepare_tab_select(self, item: ItemId) -> Option<super::PreparedSurfaceAction> {
        let tab = self.tab_record(item)?;
        self.tab_is_operable(tab).then_some(())?;
        Some(super::PreparedSurfaceAction::select_tab(
            self.authority_domain,
            self.version,
            self.scene,
            self.frozen_action_witness()?,
            *tab.id(),
            crate::engine::LocalTabFocus::Pane,
        ))
    }

    /// Prepares a close request for one visible tab close control.
    #[must_use]
    pub fn prepare_tab_close(self, item: ItemId) -> Option<super::PreparedSurfaceAction> {
        let tab = self.tab_record(item)?;
        let close_bounds = tab.close_bounds()?;
        self.plan
            .region_is_operable(close_bounds, tab.layer())
            .then_some(())?;
        Some(super::PreparedSurfaceAction::close(
            self.authority_domain,
            self.version,
            self.scene,
            self.frozen_action_witness()?,
            crate::intent::CloseSceneTarget::Tab(*tab.id()),
        ))
    }

    /// Prepares a close request for one visible contained-floating close control.
    #[must_use]
    pub fn prepare_contained_close(
        self,
        floating: FloatingPresentationId,
    ) -> Option<super::PreparedSurfaceAction> {
        let record = self
            .plan
            .contained_records()
            .iter()
            .find(|record| record.floating() == floating)?;
        let close_bounds = record.close_bounds()?;
        self.plan
            .region_is_operable(close_bounds, record.layer())
            .then_some(())?;
        self.candidate
            .root_close_is_available(record.root())
            .then_some(())?;
        Some(super::PreparedSurfaceAction::close(
            self.authority_domain,
            self.version,
            self.scene,
            self.frozen_action_witness()?,
            crate::intent::CloseSceneTarget::Contained(floating),
        ))
    }

    #[must_use]
    pub fn bounds(&self) -> LogicalRect {
        self.plan.bounds()
    }

    /// Returns source decoration for the current core-owned drag.
    ///
    /// Unlike a docking preview, this record requires no presentation
    /// acknowledgement. It exists only after the core enters its active
    /// dragging phase and disappears with the terminal or cancelled gesture.
    #[must_use]
    const fn drag_decoration(&self) -> Option<DockspaceDragDecoration<'frame>> {
        self.drag_decoration
    }

    #[cfg(test)]
    pub(crate) fn panes(self) -> impl ExactSizeIterator<Item = PanePaintRecord<'frame>> {
        self.plan
            .pane_records()
            .iter()
            .map(move |record| PanePaintRecord {
                record,
                selected: self.plan.pane_selected(record),
            })
    }

    #[cfg(test)]
    pub(crate) fn tabs(self) -> impl ExactSizeIterator<Item = TabPaintRecord<'frame>> {
        self.plan
            .tab_records()
            .iter()
            .map(move |record| TabPaintRecord {
                record,
                selected: self.plan.tab_selected(record),
                operable: self.tab_is_operable(record),
            })
    }

    #[cfg(test)]
    pub(crate) fn tab_bars(self) -> impl ExactSizeIterator<Item = TabBarPaintRecord<'frame>> {
        self.plan
            .tab_bar_records()
            .iter()
            .map(|record| TabBarPaintRecord { record })
    }

    /// Returns one exact core-selected presentation menu anchor per visible root.
    #[cfg(test)]
    pub(crate) fn presentation_menu_anchors(
        self,
    ) -> impl Iterator<Item = PresentationMenuAnchorPaintRecord<'frame>> {
        self.plan
            .presentation_menu_anchor_records()
            .iter()
            .filter_map(|record| {
                record
                    .ready_bounds()
                    .map(|bounds| PresentationMenuAnchorPaintRecord {
                        record,
                        bounds,
                        operable: self.plan.region_is_operable(bounds, record.layer()),
                    })
            })
    }

    #[cfg(test)]
    pub(crate) fn tab_strip_controls(
        self,
    ) -> impl ExactSizeIterator<Item = TabStripControlPaintRecord> + 'frame {
        let menu_open_bars = self.program_menu_open_bars();
        self.plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .map(move |record| TabStripControlPaintRecord {
                menu_open: menu_open_bars.contains(&record.id().bar()),
                record,
            })
    }

    pub(crate) fn tab_list_menus(self) -> impl Iterator<Item = TabListMenuPaintRecord<'frame>> {
        let surface = self.surface;
        let plan = self.plan;
        plan.tab_list_menu_records()
            .iter()
            .filter_map(move |record| {
                let backdrop = *plan
                    .tab_list_menu_backdrop_records()
                    .iter()
                    .find(|backdrop| backdrop.session() == record.session())?;
                Some(TabListMenuPaintRecord {
                    record,
                    surface,
                    backdrop,
                })
            })
    }

    #[cfg(test)]
    pub(crate) fn splitters(self) -> impl ExactSizeIterator<Item = SplitterPaintRecord<'frame>> {
        self.plan
            .splitter_records()
            .iter()
            .map(|record| SplitterPaintRecord { record })
    }

    pub(crate) fn splitter_junctions(
        self,
    ) -> impl ExactSizeIterator<Item = SplitterJunctionPaintRecord<'frame>> {
        self.plan
            .splitter_junction_records()
            .iter()
            .map(|record| SplitterJunctionPaintRecord { record })
    }

    #[cfg(test)]
    pub(crate) fn contained(self) -> impl ExactSizeIterator<Item = ContainedPaintRecord<'frame>> {
        let plan = self.plan;
        self.plan
            .contained_records()
            .iter()
            .map(move |record| ContainedPaintRecord { record, plan })
    }

    #[cfg(test)]
    pub(crate) fn drop_guides(self) -> impl ExactSizeIterator<Item = DropGuidePaintRecord<'frame>> {
        self.plan
            .drop_guide_clusters()
            .iter()
            .map(|record| DropGuidePaintRecord { record })
    }

    /// Returns the payload-specific guide set visible at the current drag point.
    ///
    /// This value is independent from [`Self::drag_preview`]: a complete cluster
    /// remains visible while the pointer is between buttons or over a rejected
    /// target. The renderer must not infer active or disabled state from geometry.
    #[must_use]
    fn drop_affordance(&self) -> Option<DropAffordancePaintRecord<'frame>> {
        self.drop_affordance
            .map(|affordance| DropAffordancePaintRecord { affordance })
    }

    /// Returns every active semantic receiver in deterministic hit-stack order.
    #[cfg(test)]
    pub(crate) fn receivers(self) -> impl Iterator<Item = DockspaceReceiverDescriptor> + 'frame {
        self.hit_manifest
            .regions()
            .iter()
            .copied()
            .filter(|region| !region.is_passive())
            .filter_map(move |region| {
                DockspaceReceiverDescriptor::from_projection_region(
                    self.output,
                    region.id(),
                    region.lanes(),
                    region.hit().rect(),
                )
            })
    }

    #[must_use]
    pub fn tab_receiver(&self, item: ItemId) -> Option<DockspaceReceiverDescriptor> {
        let tab = self.tab_record(item)?;
        self.tab_is_operable(tab).then_some(())?;
        self.receiver(PresentationHitRegionKind::TabBody(*tab.id()))
    }

    /// Returns the exact receiver for one painted pane body.
    #[must_use]
    pub fn receiver_for_pane(
        &self,
        pane: PanePaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let pane_id = pane.record.id();
        self.receiver(PresentationHitRegionKind::PaneBody(pane_id))
    }

    /// Returns the exact receiver for one painted tab body.
    #[must_use]
    pub fn receiver_for_tab_body(
        &self,
        tab: TabPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        tab.operable().then_some(())?;
        let tab_id = *tab.record.id();
        self.receiver(PresentationHitRegionKind::TabBody(tab_id))
    }

    /// Returns the exact receiver for one painted tab close control.
    #[must_use]
    pub fn receiver_for_tab_close(
        &self,
        tab: TabPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let close_bounds = tab.record.close_bounds()?;
        self.plan
            .region_is_operable(close_bounds, tab.record.layer())
            .then_some(())?;
        let tab_id = *tab.record.id();
        self.receiver(PresentationHitRegionKind::TabClose(tab_id))
    }

    /// Returns the exact receiver for one whole-tab-group drag grip.
    #[must_use]
    pub fn receiver_for_tab_group_grip(
        &self,
        bar: TabBarPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let bar_id = *bar.record.id();
        self.receiver(PresentationHitRegionKind::TabGroupDrag {
            bar: bar_id,
            region: TabGroupDragRegionKind::LeadingGrip,
        })
    }

    /// Returns the exact receiver for one core-compiled group-drag region.
    #[must_use]
    pub fn receiver_for_tab_group_drag_region(
        &self,
        region: TabGroupDragRegionPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        self.receiver(PresentationHitRegionKind::TabGroupDrag {
            bar: region.bar,
            region: region.record.kind(),
        })
    }

    /// Returns the trailing-empty group receiver, when this bar has unused space.
    #[must_use]
    pub fn receiver_for_tab_group_trailing_empty(
        &self,
        bar: TabBarPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let bar_id = *bar.record.id();
        self.receiver(PresentationHitRegionKind::TabGroupDrag {
            bar: bar_id,
            region: TabGroupDragRegionKind::TrailingEmpty,
        })
    }

    /// Returns the scroll receiver covering one overflowing tab-strip viewport.
    #[must_use]
    pub fn receiver_for_tab_strip_scroll(
        &self,
        bar: TabBarPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let bar_id = *bar.record.id();
        self.receiver(PresentationHitRegionKind::TabStripScroll(bar_id))
    }

    /// Returns the exact receiver for one core-owned tab-strip control.
    #[must_use]
    pub fn receiver_for_tab_strip_control(
        &self,
        control: TabStripControlPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let control_id = control.record.id();
        self.receiver(PresentationHitRegionKind::TabStripControl(control_id))
    }

    /// Returns the exact receiver for one visible tab-list menu row.
    #[must_use]
    pub fn receiver_for_tab_list_menu_row(
        &self,
        row: TabListMenuRowPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = row.session;
        let tab = row.record.tab();
        self.receiver(PresentationHitRegionKind::TabListMenuRow { menu: session, tab })
    }

    /// Returns the blocker receiver covering one open tab-list menu frame.
    #[must_use]
    pub fn receiver_for_tab_list_menu_frame(
        &self,
        menu: TabListMenuPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = menu.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuBlocker(session))
    }

    /// Returns the scroll receiver for one open tab-list menu.
    #[must_use]
    pub fn receiver_for_tab_list_menu_scroll(
        &self,
        menu: TabListMenuPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = menu.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuScroll(session))
    }

    /// Returns the full-surface backdrop receiver for one open tab-list menu.
    #[must_use]
    pub fn receiver_for_tab_list_menu_backdrop(
        &self,
        backdrop: TabListMenuBackdropPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = backdrop.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuBackdrop(session))
    }

    /// Returns the exact receiver for one painted splitter handle.
    #[must_use]
    pub fn receiver_for_splitter(
        &self,
        splitter: SplitterPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let splitter_id = *splitter.record.id();
        self.receiver(PresentationHitRegionKind::SplitterHandle(splitter_id))
    }

    /// Returns the exact receiver for one atomic splitter junction.
    #[must_use]
    pub fn receiver_for_splitter_junction(
        &self,
        junction: SplitterJunctionPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        self.receiver(PresentationHitRegionKind::SplitterJunction(
            junction.record.id(),
        ))
    }

    /// Returns whether every incident handle accepts one atomic resize gesture.
    #[must_use]
    pub fn splitter_junction_operable(
        &self,
        junction: SplitterJunctionPaintRecord<'frame>,
    ) -> bool {
        junction.record.operable()
    }

    /// Returns whether every incident splitter on one junction axis accepts a
    /// scene-bound keyboard or accessibility adjustment.
    #[must_use]
    pub fn splitter_junction_axis_operable(
        &self,
        junction: SplitterJunctionPaintRecord<'frame>,
        axis: DockspaceAxis,
    ) -> bool {
        let axis: Axis = axis.into();
        // Runtime plans borrow the validator's canonical, unique record arrays.
        let junctions = self.plan.splitter_junction_records();
        let Ok(index) =
            junctions.binary_search_by_key(&junction.record.id(), SplitterJunctionRecord::id)
        else {
            return false;
        };
        let record = &junctions[index];
        let directions = match axis {
            Axis::Horizontal => [
                SplitterJunctionDirection::North,
                SplitterJunctionDirection::South,
            ],
            Axis::Vertical => [
                SplitterJunctionDirection::East,
                SplitterJunctionDirection::West,
            ],
        };
        let [first, second] = directions.map(|direction| record.id().arm(direction));
        let Some(first) = first.or(second) else {
            return false;
        };
        let splitters = self.plan.splitter_records();
        let is_operable = |id| {
            splitters
                .binary_search_by_key(&id, |splitter| *splitter.id())
                .ok()
                .is_some_and(|index| {
                    let splitter = &splitters[index];
                    splitter.axis() == axis && splitter.operable()
                })
        };
        is_operable(first) && second.filter(|id| *id != first).is_none_or(is_operable)
    }

    /// Returns the exact blocker receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_frame(
        &self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedFrameBlocker(floating))
    }

    /// Returns the exact title-drag receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_title(
        &self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        contained.title_operable().then_some(())?;
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedTitle(floating))
    }

    /// Returns the exact close receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_close(
        &self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        contained.close_operable().then_some(())?;
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedClose(floating))
    }

    /// Returns the exact resize receiver for one contained floating edge.
    #[must_use]
    pub fn receiver_for_contained_resize(
        &self,
        contained: ContainedPaintRecord<'frame>,
        resize: ContainedResizePaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        resize.operable().then_some(())?;
        let floating = contained.record.floating();
        let direction = resize.record.direction();
        self.receiver(PresentationHitRegionKind::ContainedResize {
            floating,
            direction,
        })
    }

    /// Returns the exact receiver for one core-selected drop-guide target.
    #[must_use]
    pub fn receiver_for_drop_target(
        &self,
        target: DropAffordanceTargetPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let target_id = target.target_id();
        self.receiver(PresentationHitRegionKind::DropTarget(target_id))
    }

    #[must_use]
    pub fn center_drop_receiver_for_item(
        &self,
        item: ItemId,
    ) -> Option<DockspaceReceiverDescriptor> {
        let tab = self.tab_record(item)?;
        let tab = *tab.id();
        self.receiver(PresentationHitRegionKind::DropTarget(
            DropTargetId::Center {
                surface: self.output.surface(),
                root: tab.root,
                tabs: tab.tabs,
            },
        ))
    }

    #[must_use]
    fn drag_preview(&self) -> Option<DockspaceDragPreview<'frame>> {
        let preview = self.drag_preview?;
        let layout_projected = self.plan.contained_projection().is_some_and(|projection| {
            projection.owner == ContainedLayoutOwner::Drag(preview.token().session())
                && projection.surface == self.surface
                && matches!(preview.visual(), PreviewVisual::Contained { surface, rect, .. }
                    if *surface == projection.surface && *rect == projection.rect)
                && self.plan.contained_records().iter().any(|record| {
                    record.root() == projection.root && record.floating() == projection.floating
                })
        });
        Some(DockspaceDragPreview {
            preview,
            layout_projected,
        })
    }

    /// Returns the exact contained resize preview which this surface must paint.
    #[must_use]
    fn contained_transform_preview(&self) -> Option<DockspaceContainedTransformPreview<'frame>> {
        let preview = self.contained_transform_preview?;
        let layout_projected = self.plan.contained_projection().is_some_and(|projection| {
            projection.owner == ContainedLayoutOwner::Transform(preview.token().session())
                && projection.surface == self.surface
                && projection.surface == preview.surface()
                && projection.root == preview.root()
                && projection.floating == preview.floating()
                && projection.rect == preview.rect()
        }) && self.plan.contained_records().iter().any(|record| {
            record.root() == preview.root() && record.floating() == preview.floating()
        });
        Some(DockspaceContainedTransformPreview {
            preview,
            layout_projected,
        })
    }

    fn tab_record(&self, item: ItemId) -> Option<&'frame crate::scene::TabRecord> {
        self.plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)
    }

    fn tab_is_operable(&self, tab: &crate::scene::TabRecord) -> bool {
        let bounds = tab.drag_hit().rect();
        bounds.width() > 0.0
            && bounds.height() > 0.0
            && self
                .plan
                .tab_bar_records()
                .binary_search_by_key(
                    &TabBarSceneId {
                        root: tab.id().root,
                        tabs: tab.id().tabs,
                    },
                    |bar| *bar.id(),
                )
                .ok()
                .map(|index| &self.plan.tab_bar_records()[index])
                .is_some_and(|bar| bar.interaction() == crate::policy::TabBarInteraction::Enabled)
            && self.plan.region_is_operable(bounds, tab.layer())
    }

    fn receiver(&self, kind: PresentationHitRegionKind) -> Option<DockspaceReceiverDescriptor> {
        let region = *self.hit_manifest.region_for_kind(kind)?;
        if region.is_passive() {
            return None;
        }
        DockspaceReceiverDescriptor::from_projection_region(
            self.output,
            region.id(),
            region.lanes(),
            region.hit().rect(),
        )
    }

    /// Returns tab bars whose popup menu is present in the ordered program.
    fn program_menu_open_bars(&self) -> HashSet<TabBarSceneId> {
        if self.plan.tab_list_menu_records().is_empty() {
            return HashSet::new();
        }
        self.plan.surface_program_menu_open_bars().collect()
    }
}

const fn receiver_role(kind: PresentationHitRegionKind) -> Option<DockspaceReceiverRole> {
    Some(match kind {
        PresentationHitRegionKind::PaneBody(_) => DockspaceReceiverRole::PaneBody,
        PresentationHitRegionKind::TabBody(_) => DockspaceReceiverRole::TabBody,
        PresentationHitRegionKind::TabClose(_) => DockspaceReceiverRole::TabClose,
        PresentationHitRegionKind::TabStripControl(_) => DockspaceReceiverRole::TabStripControl,
        PresentationHitRegionKind::TabStripScroll(_) => DockspaceReceiverRole::TabStripScroll,
        PresentationHitRegionKind::TabListMenuRow { .. } => DockspaceReceiverRole::TabListMenuRow,
        PresentationHitRegionKind::TabListMenuScroll(_) => DockspaceReceiverRole::TabListMenuScroll,
        PresentationHitRegionKind::TabListMenuBlocker(_) => {
            DockspaceReceiverRole::TabListMenuBlocker
        }
        PresentationHitRegionKind::TabListMenuBackdrop(_) => {
            DockspaceReceiverRole::TabListMenuBackdrop
        }
        PresentationHitRegionKind::TabGroupDrag {
            region: TabGroupDragRegionKind::LeadingGrip,
            ..
        } => DockspaceReceiverRole::TabGroupGrip,
        PresentationHitRegionKind::TabGroupDrag {
            region: TabGroupDragRegionKind::TrailingEmpty,
            ..
        } => DockspaceReceiverRole::TabGroupTrailingEmpty,
        PresentationHitRegionKind::SplitterHandle(_) => DockspaceReceiverRole::Splitter,
        PresentationHitRegionKind::SplitterJunction(_) => DockspaceReceiverRole::SplitterJunction,
        PresentationHitRegionKind::ContainedFrameBlocker(_) => {
            DockspaceReceiverRole::ContainedFrame
        }
        PresentationHitRegionKind::ContainedTitle(_) => DockspaceReceiverRole::ContainedTitle,
        PresentationHitRegionKind::ContainedClose(_) => DockspaceReceiverRole::ContainedClose,
        PresentationHitRegionKind::ContainedResize { .. } => DockspaceReceiverRole::ContainedResize,
        PresentationHitRegionKind::DropGuideActivation(_) => return None,
        PresentationHitRegionKind::DropTarget(_) => DockspaceReceiverRole::DropTarget,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drop_target::{DropTargetRecord, DropVisual, SurfaceBackground};
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::hit_region::HitRegion;
    use crate::policy::DockPolicy;
    use crate::runtime::{DockspaceSession, UniformSurfaceMetrics};
    use crate::scene::PaneSceneId;

    fn measured_surface_session() -> (DockspaceSession, SurfaceId) {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let item = ItemId::new(3);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().expect("the test workspace validates"),
            DockPolicy::default(),
        )
        .expect("the runtime initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
            80.0,
        )
        .expect("test metrics validate");
        let mut measurement_frame = session
            .begin_host_frame()
            .expect("the measurement frame begins");
        measurement_frame
            .measure_surface(surface, metrics)
            .expect("the surface measurement stages");
        measurement_frame
            .commit()
            .expect("the surface measurement commits");
        (session, surface)
    }

    fn assert_preflight_matches_complete_resolution(plan: SurfacePaintPlan<'_>) {
        let menu_open_bars = plan.program_menu_open_bars();
        let fully_resolvable = plan
            .plan
            .visit_surface_program(|step| {
                plan.resolve_surface_program_step(step, &menu_open_bars, None)
                    .map(|_| ())
            })
            .is_ok();
        assert_eq!(plan.validate_surface_program(), fully_resolvable);
        assert_eq!(plan.program_valid, fully_resolvable);
        let mut expected_resources = Vec::new();
        let full_visit = plan.visit_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Tab(tab),
                    ..
                } => expected_resources.push((tab.visual_id(), tab.item())),
                SurfacePaintProgramStep::PopupMenu { menu, .. } => {
                    expected_resources
                        .extend(menu.rows().map(|row| (row.tab_visual_id(), row.item())));
                }
                SurfacePaintProgramStep::DragDecoration(decoration) => {
                    if let Some(item) = decoration.ghost_item() {
                        expected_resources.push((decoration.source_visual(), item));
                    }
                }
                SurfacePaintProgramStep::DropTarget(target) => {
                    let descriptor = target.receiver();
                    assert_eq!(Some(descriptor), plan.receiver(descriptor.region.kind()));
                }
                _ => {}
            }
            Ok::<(), ()>(())
        });
        let mut actual_resources = Vec::new();
        let resource_visit = plan.visit_tab_resources(|visual, item| {
            actual_resources.push((visual, item));
        });
        assert_eq!(full_visit.is_ok(), resource_visit.is_ok());
        assert_eq!(actual_resources, expected_resources);
    }

    #[test]
    fn linked_target_receivers_match_identity_resolution_without_paint_time_searches() {
        use crate::drop_resolver::structural_work;

        let (mut session, surface) = measured_surface_session();
        for width in [640.0, 800.0, 640.0] {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .measure_surface(
                    surface,
                    UniformSurfaceMetrics::new(
                        LogicalRect::new(0.0, 0.0, width, 480.0).unwrap(),
                        LogicalSize::new(32.0, 24.0).unwrap(),
                        80.0,
                    )
                    .unwrap(),
                )
                .unwrap();
            let plan = frame.paint_plan(surface).unwrap().unwrap();
            assert!(plan.program_link.is_some());
            let collect = |plan: SurfacePaintPlan<'_>| {
                let mut receivers = Vec::new();
                plan.visit_surface_program(|step| {
                    if let SurfacePaintProgramStep::DropTarget(target) = step {
                        receivers.push(target.receiver());
                    }
                    Ok::<(), ()>(())
                })
                .unwrap();
                receivers
            };
            structural_work::reset();
            let linked = collect(plan);
            assert!(!linked.is_empty());
            assert_eq!(
                structural_work::snapshot().presentation_hit_lookup_comparisons,
                0,
                "executing the installed link must not search target identities"
            );
            let resolved = collect(SurfacePaintPlan {
                program_link: None,
                ..plan
            });
            assert_eq!(linked, resolved);
            assert!(structural_work::snapshot().presentation_hit_lookup_comparisons > 0);
            frame.commit().unwrap();
        }
    }

    #[test]
    fn only_execution_mints_a_receipt_bound_to_its_exact_host_frame() {
        let (mut session, surface) = measured_surface_session();

        let mut inspection_frame = session
            .begin_host_frame()
            .expect("the inspection frame begins");
        let plan = inspection_frame
            .paint_plan(surface)
            .expect("the inspection plan resolves")
            .expect("the measured surface is paintable");
        assert_preflight_matches_complete_resolution(plan);
        let inspected: () = plan
            .visit_surface_program(|_| Ok::<(), ()>(()))
            .expect("inspection traverses the complete program without minting authority");
        assert_eq!(inspected, ());
        let rejected = plan.execute_surface_program(|_| Err::<(), _>("sink rejected"));
        assert!(matches!(
            rejected,
            Err(SurfacePaintProgramVisitError::Sink("sink rejected"))
        ));
        let receipt = plan
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("the renderer accepts the complete program");
        drop(inspection_frame);

        let mut successor = session
            .begin_host_frame()
            .expect("the successor frame begins");
        let error = successor
            .confirm_surface_painted(receipt)
            .expect_err("a receipt from an aborted host frame must fail closed");
        assert!(matches!(
            error.interaction_error(),
            Some(DockspaceInteractionError::SurfaceProgramExecutionMismatch {
                surface: rejected
            }) if *rejected == surface
        ));
    }

    #[test]
    fn invalid_surface_program_emits_no_paint_steps() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let item = ItemId::new(3);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().expect("the test workspace validates"),
            DockPolicy::default(),
        )
        .expect("the runtime initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
            80.0,
        )
        .expect("test metrics validate");
        let mut measurement_frame = session
            .begin_host_frame()
            .expect("the measurement frame begins");
        measurement_frame
            .measure_surface(surface, metrics)
            .expect("the surface measurement stages");
        measurement_frame
            .commit()
            .expect("the surface measurement commits");

        let mut paint_frame = session.begin_host_frame().expect("the paint frame begins");
        let valid = paint_frame
            .paint_plan(surface)
            .expect("the paint plan lookup succeeds")
            .expect("the measured surface is paintable");
        assert!(
            valid.program_valid,
            "ready plans must preflight their program"
        );
        assert_preflight_matches_complete_resolution(valid);
        let mut malformed = PresentationPlan::new(surface, valid.bounds());
        malformed
            .begin_surface_program_root(root, SurfaceProgramRootPresentation::Main)
            .expect("the malformed program root opens");
        malformed
            .push_surface_program_root_operation(SurfaceProgramRootOperation::Pane(PaneSceneId {
                root,
                tabs,
            }))
            .expect("the missing record identity enters the program");
        malformed
            .end_surface_program_root(root)
            .expect("the malformed program root closes");
        malformed
            .finish_surface_program()
            .expect("the malformed program seals structurally");
        assert!(matches!(
            malformed.link_surface_program(),
            Err(crate::scene::SceneBuildError::InvalidSurfaceProgram { .. })
        ));
        let malformed = SurfacePaintPlan {
            plan: &malformed,
            program_valid: false,
            ..valid
        };
        let malformed = SurfacePaintPlan {
            program_valid: malformed.validate_surface_program(),
            ..malformed
        };
        assert_preflight_matches_complete_resolution(malformed);

        assert!(
            !malformed.program_valid,
            "the preflight must reject a program referencing a missing pane record"
        );

        let mut emitted_steps = 0;
        let result = malformed.visit_surface_program(|_| {
            emitted_steps += 1;
            Ok::<(), ()>(())
        });

        assert!(matches!(
            result,
            Err(SurfacePaintProgramVisitError::InvalidProgram {
                surface: rejected
            }) if rejected == surface
        ));
        assert_eq!(
            emitted_steps, 0,
            "an invalid program must fail before the renderer can paint a partial output"
        );
    }

    #[test]
    fn drop_target_without_receiver_invalidates_the_whole_surface_program() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let item = ItemId::new(3);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().expect("the test workspace validates"),
            DockPolicy::default(),
        )
        .expect("the runtime initializes");
        let bounds = LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate");
        let metrics = UniformSurfaceMetrics::new(
            bounds,
            LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
            80.0,
        )
        .expect("test metrics validate");
        let mut measurement_frame = session
            .begin_host_frame()
            .expect("the measurement frame begins");
        measurement_frame
            .measure_surface(surface, metrics)
            .expect("the surface measurement stages");
        measurement_frame
            .commit()
            .expect("the surface measurement commits");

        let mut paint_frame = session.begin_host_frame().expect("the paint frame begins");
        let valid = paint_frame
            .paint_plan(surface)
            .expect("the paint plan lookup succeeds")
            .expect("the measured surface is paintable");
        assert_preflight_matches_complete_resolution(valid);
        let mut malformed = PresentationPlan::new(surface, bounds);
        malformed
            .set_surface_background(DropTargetRecord::surface_background(
                SurfaceBackground::new(surface),
                HitRegion::new(bounds),
                DropVisual::new(bounds),
            ))
            .expect("the background target stages");
        malformed
            .finish_surface_program()
            .expect("the target program seals structurally");
        malformed
            .link_surface_program()
            .expect("the target row links before testing dynamic receiver validation");
        let malformed = SurfacePaintPlan {
            plan: &malformed,
            program_valid: false,
            ..valid
        };
        let malformed = SurfacePaintPlan {
            program_valid: malformed.validate_surface_program(),
            ..malformed
        };
        assert_preflight_matches_complete_resolution(malformed);

        assert!(
            !malformed.program_valid,
            "a program drop target without the exact receiver must fail preflight"
        );
        let mut emitted_steps = 0;
        let result = malformed.visit_surface_program(|_| {
            emitted_steps += 1;
            Ok::<(), ()>(())
        });

        assert!(matches!(
            result,
            Err(SurfacePaintProgramVisitError::InvalidProgram {
                surface: rejected
            }) if rejected == surface
        ));
        assert_eq!(
            emitted_steps, 0,
            "the missing receiver must not degrade into a partially painted program"
        );
    }
}
