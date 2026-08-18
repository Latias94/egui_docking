//! Renderer-neutral measurement and primary read-only docking paint capabilities.

use std::fmt;

use crate::drop_guide::DropGuideClusterId;
use crate::drop_resolver::DropAffordance;
use crate::drop_target::{DropTargetId, SceneLayerKey};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::Axis;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::interaction::{ContainedTransformPreview, InteractionPreview, PreviewVisual};
use crate::model::{DockspaceActionRejection, DockspaceAxis, ProductAction};
use crate::presentation_hit::{
    PresentationHitRegionId, PresentationHitRegionKind, PresentationPointerLane,
    PresentationPointerLanes,
};
use crate::presentation_observation::SurfacePresentationOutputTicket;
pub use crate::scene::TabGroupDragRegionKind;
use crate::scene::{
    ContainedRecord, ContainedResizeRecord, PresentationPlan, SplitterGapPresentation,
    SplitterGapRecord, SplitterJunctionDirection, SplitterJunctionId, SplitterJunctionRecord,
    SplitterRecord, SplitterSceneId, SurfaceScene, TabBarSceneId, TabSceneId,
};
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId, TabStripControlId};

use super::{
    DockspaceHostFrame, DockspaceInteractionError, DockspacePaneFocusObservation,
    DockspacePaneFocusRequest, DockspaceRuntimeError, PreparedPaneFocusObservation,
};

mod actions;
mod drag_decoration;
mod guides;
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
    Splitter(SplitterSceneId),
    SplitterGap(SplitterSceneId),
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
    /// One ordinary splitter.
    Splitter,
    /// One structural splitter gap, including collapsed gaps.
    SplitterGap,
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
            VisualIdentity::Splitter(_) => DockspaceVisualKind::Splitter,
            VisualIdentity::SplitterGap(_) => DockspaceVisualKind::SplitterGap,
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
            center: LogicalPoint::new(
                bounds.x() + bounds.width() * 0.5,
                bounds.y() + bounds.height() * 0.5,
            )
            .ok()?,
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

    /// Returns the exact receiver rectangle supplied to the renderer.
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
}

impl DockspaceDragPreview<'_> {
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
}

impl fmt::Debug for DockspaceContainedTransformPreview<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceContainedTransformPreview")
            .field("surface", &self.surface())
            .field("floating", &self.floating())
            .field("rect", &self.rect())
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
}

/// Product-level visibility of one structural splitter gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SplitterGapVisibility {
    /// The gap has a positive-area splitter presentation.
    Rendered,
    /// Compression removed the positive-area splitter presentation.
    Collapsed,
}

/// Read-only structural splitter-gap status.
#[derive(Debug, Clone, Copy)]
pub struct StructuralSplitterGapStatus {
    record: SplitterGapRecord,
}

impl StructuralSplitterGapStatus {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::SplitterGap(self.record.id()))
    }

    #[must_use]
    pub const fn visibility(self) -> SplitterGapVisibility {
        match self.record.presentation() {
            SplitterGapPresentation::Rendered => SplitterGapVisibility::Rendered,
            SplitterGapPresentation::Collapsed => SplitterGapVisibility::Collapsed,
        }
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
#[derive(Debug, Clone, Copy)]
pub struct ContainedPaintRecord<'plan> {
    record: &'plan ContainedRecord,
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

    pub fn resize(self) -> impl ExactSizeIterator<Item = ContainedResizePaintRecord> + 'plan {
        let operable = self.record.transform_operable();
        self.record
            .resize()
            .iter()
            .copied()
            .map(move |record| ContainedResizePaintRecord { record, operable })
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
}

/// Why one exact presentation command is disabled in the current paint candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspacePresentationCommandUnavailable {
    /// The root is already presented as contained floating content.
    AlreadyContained,
    /// The root is already the main presentation of a managed native child.
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

/// Read-only plan supplied before one exact renderer paint.
#[derive(Clone, Copy)]
pub struct SurfacePaintPlan<'frame> {
    pub(super) surface: SurfaceId,
    pub(super) authority_domain: crate::ids::EngineAuthorityDomainId,
    pub(super) version: crate::model::WorkspaceVersion,
    pub(super) scene: crate::scene::SurfaceSceneStamp,
    pub(super) output: SurfacePresentationOutputTicket,
    pub(super) plan: &'frame PresentationPlan,
    pub(super) hit_manifest: &'frame crate::presentation_hit::PresentationHitManifest,
    pub(super) splitter_keyboard_step: f64,
    pub(super) escape_available: bool,
    pub(super) drop_affordance: Option<&'frame DropAffordance>,
    pub(super) drag_preview: Option<&'frame InteractionPreview>,
    pub(super) drag_decoration: Option<DockspaceDragDecoration<'frame>>,
    pub(super) contained_transform_preview: Option<&'frame ContainedTransformPreview>,
    pub(super) pane_focus_request: Option<DockspacePaneFocusRequest>,
    pub(super) candidate: crate::engine::HostFrameView<'frame>,
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

impl DockspaceHostFrame<'_> {
    /// Freezes pointer input and returns the current candidate plan which the
    /// renderer may paint.
    ///
    /// # Errors
    ///
    /// Returns an error when the pointer-input phase cannot be completed.
    pub fn paint_plan(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<SurfacePaintPlan<'_>>, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let view = self.frame.view();
        let Some(scene) = view.scene().surface(surface).and_then(SurfaceScene::ready) else {
            return Ok(None);
        };
        let candidate = scene.candidate();
        let drop_affordance = view.interaction().drop_affordance().filter(|affordance| {
            affordance.surface() == surface && affordance.scene() == candidate.stamp()
        });
        let drag_decoration = view
            .interaction()
            .active_drag_view()
            .and_then(|drag| drag_decoration::resolve(view.scene(), drag));
        let version = view.version();
        let pane_focus_request = view
            .published_pane_focus_intent()
            .filter(|intent| intent.surface() == surface)
            .and_then(|intent| {
                DockspacePaneFocusRequest::from_intent(
                    self.session.engine.authority_domain(),
                    version.epoch(),
                    intent,
                )
            });
        Ok(Some(SurfacePaintPlan {
            surface,
            authority_domain: self.session.engine.authority_domain(),
            version,
            scene: candidate.stamp(),
            output: candidate.output_ticket(),
            plan: candidate.plan(),
            hit_manifest: candidate.hit_manifest(),
            splitter_keyboard_step: view.presentation_config().splitter_keyboard_step(),
            escape_available: view.interaction().local_response_gesture_surface() == Some(surface),
            drop_affordance,
            drag_preview: view.presentation_drag_preview(surface),
            drag_decoration,
            contained_transform_preview: view.presentation_contained_transform_preview(surface),
            pane_focus_request,
            candidate: view,
        }))
    }

    /// Records that the renderer painted the complete current plan, including
    /// every transient preview exposed by [`Self::paint_plan`].
    ///
    /// # Errors
    ///
    /// Returns an error when the surface was already answered or has no
    /// current paintable plan.
    pub fn confirm_surface_painted(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.surface_answered(surface) {
            return Err(DockspaceInteractionError::SurfaceAlreadyAnswered { surface }.into());
        }
        if self.paint_plan(surface)?.is_none() {
            return Err(DockspaceInteractionError::SurfaceNotPaintable { surface }.into());
        }
        self.painted_surfaces.insert(surface);
        Ok(())
    }
}

impl<'frame> SurfacePaintPlan<'frame> {
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact semantic output represented by this paint plan.
    #[must_use]
    pub const fn semantic_output(self) -> DockspaceSemanticOutput {
        DockspaceSemanticOutput {
            ticket: self.output,
        }
    }

    /// Returns the exact pane-focus request admitted for this surface.
    #[must_use]
    pub const fn pane_focus_request(self) -> Option<DockspacePaneFocusRequest> {
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
                let rect =
                    match self
                        .candidate
                        .default_contained_float_rect(root, self.surface, self.plan)
                    {
                        Ok(rect) => rect,
                        Err(reason) => {
                            return Ok(Some(unavailable(
                                DockspacePresentationCommandUnavailable::Action(reason),
                            )));
                        }
                    };
                ProductAction::FloatRoot {
                    root,
                    surface: self.surface,
                    rect: Some(rect),
                }
            }
            DockspacePresentationCommandKind::DockBack => ProductAction::DockBackRoot { root },
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
            record.action?,
        ))
    }

    /// Prepares a current-candidate tab selection without exposing scene identity.
    #[must_use]
    pub fn prepare_tab_select(self, item: ItemId) -> Option<super::PreparedSurfaceAction> {
        let tab = self.tab_record(item)?;
        self.tab_is_operable(tab).then_some(())?;
        Some(super::PreparedSurfaceAction::select_tab(
            self.authority_domain,
            self.version,
            self.scene,
            *tab.id(),
        ))
    }

    /// Prepares a close request for one visible tab close control.
    #[must_use]
    pub fn prepare_tab_close(self, item: ItemId) -> Option<super::PreparedSurfaceAction> {
        let tab = self.tab_record(item)?;
        tab.close_bounds()?;
        Some(super::PreparedSurfaceAction::close(
            self.authority_domain,
            self.version,
            self.scene,
            crate::intent::CloseSceneTarget::Tab(*tab.id()),
        ))
    }

    /// Prepares a close request for one visible contained-floating close control.
    #[must_use]
    pub fn prepare_contained_close(
        self,
        floating: FloatingPresentationId,
    ) -> Option<super::PreparedSurfaceAction> {
        self.plan
            .contained_records()
            .iter()
            .find(|record| record.floating() == floating && record.close_bounds().is_some())?;
        Some(super::PreparedSurfaceAction::dock_back(
            self.authority_domain,
            self.version,
            self.scene,
            floating,
        ))
    }

    /// Prepares one exact primary-press activation for a rear contained presentation.
    ///
    /// The core retains ownership of stacking and pane-focus selection. The
    /// caller supplies only the current plan's opaque contained identity and
    /// the surface-logical point observed by its framework response.
    #[must_use]
    pub fn prepare_contained_activation(
        self,
        floating: FloatingPresentationId,
        point: LogicalPoint,
    ) -> Option<super::PreparedSurfaceAction> {
        let contained = self
            .plan
            .contained_records()
            .iter()
            .find(|record| record.floating() == floating)?;
        (!contained.is_frontmost()
            && contained.outer_bounds().contains(point)
            && self
                .plan
                .point_is_on_authoritative_layer(point, contained.layer()))
        .then(|| {
            super::PreparedSurfaceAction::activate_contained(
                self.authority_domain,
                self.version,
                self.scene,
                floating,
                point,
            )
        })
    }

    #[must_use]
    pub fn bounds(self) -> LogicalRect {
        self.plan.bounds()
    }

    /// Returns source decoration for the current core-owned drag.
    ///
    /// Unlike a docking preview, this record requires no presentation
    /// acknowledgement. It exists only after the core enters its active
    /// dragging phase and disappears with the terminal or cancelled gesture.
    #[must_use]
    pub const fn drag_decoration(self) -> Option<DockspaceDragDecoration<'frame>> {
        self.drag_decoration
    }

    pub fn panes(self) -> impl ExactSizeIterator<Item = PanePaintRecord<'frame>> {
        self.plan
            .pane_records()
            .iter()
            .map(|record| PanePaintRecord { record })
    }

    pub fn tabs(self) -> impl ExactSizeIterator<Item = TabPaintRecord<'frame>> {
        self.plan
            .tab_records()
            .iter()
            .map(move |record| TabPaintRecord {
                record,
                operable: self.tab_is_operable(record),
            })
    }

    pub fn tab_bars(self) -> impl ExactSizeIterator<Item = TabBarPaintRecord<'frame>> {
        self.plan
            .tab_bar_records()
            .iter()
            .map(|record| TabBarPaintRecord { record })
    }

    pub fn tab_strip_controls(
        self,
    ) -> impl ExactSizeIterator<Item = TabStripControlPaintRecord> + 'frame {
        self.plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .map(|record| TabStripControlPaintRecord { record })
    }

    pub fn tab_list_menus(self) -> impl ExactSizeIterator<Item = TabListMenuPaintRecord<'frame>> {
        self.plan
            .tab_list_menu_records()
            .iter()
            .map(|record| TabListMenuPaintRecord { record })
    }

    pub fn tab_list_menu_backdrops(
        self,
    ) -> impl ExactSizeIterator<Item = TabListMenuBackdropPaintRecord> + 'frame {
        let surface = self.surface;
        self.plan
            .tab_list_menu_backdrop_records()
            .iter()
            .copied()
            .map(move |record| TabListMenuBackdropPaintRecord { surface, record })
    }

    pub fn splitter_gap_statuses(
        self,
    ) -> impl ExactSizeIterator<Item = StructuralSplitterGapStatus> + 'frame {
        self.plan
            .splitter_gap_records()
            .iter()
            .copied()
            .map(|record| StructuralSplitterGapStatus { record })
    }

    pub fn splitters(self) -> impl ExactSizeIterator<Item = SplitterPaintRecord<'frame>> {
        self.plan
            .splitter_records()
            .iter()
            .map(|record| SplitterPaintRecord { record })
    }

    pub fn splitter_junctions(
        self,
    ) -> impl ExactSizeIterator<Item = SplitterJunctionPaintRecord<'frame>> {
        self.plan
            .splitter_junction_records()
            .iter()
            .map(|record| SplitterJunctionPaintRecord { record })
    }

    pub fn contained(self) -> impl ExactSizeIterator<Item = ContainedPaintRecord<'frame>> {
        self.plan
            .contained_records()
            .iter()
            .map(|record| ContainedPaintRecord { record })
    }

    pub fn drop_guides(self) -> impl ExactSizeIterator<Item = DropGuidePaintRecord<'frame>> {
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
    pub fn drop_affordance(self) -> Option<DropAffordancePaintRecord<'frame>> {
        self.drop_affordance
            .map(|affordance| DropAffordancePaintRecord { affordance })
    }

    /// Returns every active semantic receiver in deterministic hit-stack order.
    pub fn receivers(self) -> impl Iterator<Item = DockspaceReceiverDescriptor> + 'frame {
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
    pub fn tab_receiver(self, item: ItemId) -> Option<DockspaceReceiverDescriptor> {
        let tab = self.tab_record(item)?;
        self.tab_is_operable(tab).then_some(())?;
        self.receiver(PresentationHitRegionKind::TabBody(*tab.id()))
    }

    /// Returns the exact receiver for one painted pane body.
    #[must_use]
    pub fn receiver_for_pane(
        self,
        pane: PanePaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let pane_id = pane.record.id();
        self.receiver(PresentationHitRegionKind::PaneBody(pane_id))
    }

    /// Returns the exact receiver for one painted tab body.
    #[must_use]
    pub fn receiver_for_tab_body(
        self,
        tab: TabPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        tab.operable().then_some(())?;
        let tab_id = *tab.record.id();
        self.receiver(PresentationHitRegionKind::TabBody(tab_id))
    }

    /// Returns the exact receiver for one painted tab close control.
    #[must_use]
    pub fn receiver_for_tab_close(
        self,
        tab: TabPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let tab_id = *tab.record.id();
        self.receiver(PresentationHitRegionKind::TabClose(tab_id))
    }

    /// Returns the exact receiver for one whole-tab-group drag grip.
    #[must_use]
    pub fn receiver_for_tab_group_grip(
        self,
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
        self,
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
        self,
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
        self,
        bar: TabBarPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let bar_id = *bar.record.id();
        self.receiver(PresentationHitRegionKind::TabStripScroll(bar_id))
    }

    /// Returns the exact receiver for one core-owned tab-strip control.
    #[must_use]
    pub fn receiver_for_tab_strip_control(
        self,
        control: TabStripControlPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let control_id = control.record.id();
        self.receiver(PresentationHitRegionKind::TabStripControl(control_id))
    }

    /// Returns the exact receiver for one visible tab-list menu row.
    #[must_use]
    pub fn receiver_for_tab_list_menu_row(
        self,
        row: TabListMenuRowPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = row.session;
        let tab = row.record.tab();
        self.receiver(PresentationHitRegionKind::TabListMenuRow { menu: session, tab })
    }

    /// Returns the blocker receiver covering one open tab-list menu frame.
    #[must_use]
    pub fn receiver_for_tab_list_menu_frame(
        self,
        menu: TabListMenuPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = menu.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuBlocker(session))
    }

    /// Returns the scroll receiver for one open tab-list menu.
    #[must_use]
    pub fn receiver_for_tab_list_menu_scroll(
        self,
        menu: TabListMenuPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = menu.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuScroll(session))
    }

    /// Returns the full-surface backdrop receiver for one open tab-list menu.
    #[must_use]
    pub fn receiver_for_tab_list_menu_backdrop(
        self,
        backdrop: TabListMenuBackdropPaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
        let session = backdrop.record.session();
        self.receiver(PresentationHitRegionKind::TabListMenuBackdrop(session))
    }

    /// Returns the exact receiver for one painted splitter handle.
    #[must_use]
    pub fn receiver_for_splitter(
        self,
        splitter: SplitterPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let splitter_id = *splitter.record.id();
        self.receiver(PresentationHitRegionKind::SplitterHandle(splitter_id))
    }

    /// Returns the exact receiver for one atomic splitter junction.
    #[must_use]
    pub fn receiver_for_splitter_junction(
        self,
        junction: SplitterJunctionPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        self.receiver(PresentationHitRegionKind::SplitterJunction(
            junction.record.id(),
        ))
    }

    /// Returns whether every incident handle accepts one atomic resize gesture.
    #[must_use]
    pub fn splitter_junction_operable(self, junction: SplitterJunctionPaintRecord<'frame>) -> bool {
        junction.record.operable()
    }

    /// Returns whether every incident splitter on one junction axis accepts a
    /// scene-bound keyboard or accessibility adjustment.
    #[must_use]
    pub fn splitter_junction_axis_operable(
        self,
        junction: SplitterJunctionPaintRecord<'frame>,
        axis: DockspaceAxis,
    ) -> bool {
        let axis: Axis = axis.into();
        let Some(record) = self
            .plan
            .splitter_junction_records()
            .iter()
            .find(|record| record.id() == junction.record.id())
        else {
            return false;
        };
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
        let mut ids = directions
            .into_iter()
            .filter_map(|direction| record.id().arm(direction))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        !ids.is_empty()
            && ids.into_iter().all(|id| {
                self.plan
                    .splitter_record(id)
                    .is_some_and(|splitter| splitter.axis() == axis && splitter.operable())
            })
    }

    pub(super) fn splitter_junction_id_operable(self, id: SplitterJunctionId) -> bool {
        self.plan
            .splitter_junction_records()
            .iter()
            .find(|record| record.id() == id)
            .is_some_and(SplitterJunctionRecord::operable)
    }

    /// Returns the exact blocker receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_frame(
        self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedFrameBlocker(floating))
    }

    /// Returns the exact title-drag receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_title(
        self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedTitle(floating))
    }

    /// Returns the exact close receiver for one contained floating surface.
    #[must_use]
    pub fn receiver_for_contained_close(
        self,
        contained: ContainedPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let floating = contained.record.floating();
        self.receiver(PresentationHitRegionKind::ContainedClose(floating))
    }

    /// Returns the exact resize receiver for one contained floating edge.
    #[must_use]
    pub fn receiver_for_contained_resize(
        self,
        contained: ContainedPaintRecord<'frame>,
        resize: ContainedResizePaintRecord,
    ) -> Option<DockspaceReceiverDescriptor> {
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
        self,
        target: DropAffordanceTargetPaintRecord<'frame>,
    ) -> Option<DockspaceReceiverDescriptor> {
        let target_id = target.target_id();
        self.receiver(PresentationHitRegionKind::DropTarget(target_id))
    }

    #[must_use]
    pub fn center_drop_receiver_for_item(
        self,
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
    pub fn drag_preview(self) -> Option<DockspaceDragPreview<'frame>> {
        self.drag_preview
            .map(|preview| DockspaceDragPreview { preview })
    }

    /// Returns the exact contained resize preview which this surface must paint.
    #[must_use]
    pub fn contained_transform_preview(self) -> Option<DockspaceContainedTransformPreview<'frame>> {
        self.contained_transform_preview
            .map(|preview| DockspaceContainedTransformPreview { preview })
    }

    fn tab_record(self, item: ItemId) -> Option<&'frame crate::scene::TabRecord> {
        self.plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)
    }

    fn tab_is_operable(self, tab: &crate::scene::TabRecord) -> bool {
        let bounds = tab.drag_hit().rect();
        bounds.width() > 0.0
            && bounds.height() > 0.0
            && self
                .plan
                .tab_bar_records()
                .iter()
                .find(|bar| bar.id().root == tab.id().root && bar.id().tabs == tab.id().tabs)
                .is_some_and(|bar| bar.interaction() == crate::policy::TabBarInteraction::Enabled)
            && self.plan.region_is_operable(bounds, tab.layer())
    }

    fn receiver(self, kind: PresentationHitRegionKind) -> Option<DockspaceReceiverDescriptor> {
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
