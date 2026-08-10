//! Renderer-neutral measurement and primary read-only docking paint capabilities.

use std::fmt;

use crate::drop_guide::DropGuideClusterId;
use crate::drop_resolver::DropAffordance;
use crate::drop_target::{DropTargetId, SceneLayerKey};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::Axis;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::interaction::{ContainedTransformPreview, InteractionPreview, PreviewVisual};
use crate::model::DockspaceAxis;
use crate::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use crate::presentation_observation::SurfacePresentationOutputTicket;
use crate::scene::{
    ContainedRecord, ContainedResizeRecord, PresentationPlan, SplitterGapPresentation,
    SplitterGapRecord, SplitterJunctionId, SplitterJunctionRecord, SplitterRecord, SplitterSceneId,
    TabBarSceneId, TabSceneId,
};
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId, TabStripControlId};

mod actions;
mod guides;
mod tab_chrome;
mod tabs;
pub use actions::ContainedResizeDirection;
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
    PanePaintRecord, TabBarPaintRecord, TabPaintRecord, TabStripMemberPaintRecord,
    TabStripMemberVisibility,
};

/// Stable renderer identity whose structural storage remains core-private.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DockspaceVisualId(VisualIdentity);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum VisualIdentity {
    Pane(crate::scene::PaneSceneId),
    Tab(TabSceneId),
    TabBar(TabBarSceneId),
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
    TabGroupGrip,
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockspaceReceiverDescriptor {
    pub(super) output: SurfacePresentationOutputTicket,
    pub(super) region: PresentationHitRegionId,
    role: DockspaceReceiverRole,
    bounds: LogicalRect,
    center: LogicalPoint,
}

impl DockspaceReceiverDescriptor {
    pub(super) fn from_projection_region(
        output: SurfacePresentationOutputTicket,
        region: PresentationHitRegionId,
        bounds: LogicalRect,
    ) -> Option<Self> {
        Some(Self {
            output,
            region,
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
#[derive(Debug, Clone, Copy)]
pub struct SplitterJunctionPaintRecord<'plan> {
    record: &'plan SplitterJunctionRecord,
}

impl SplitterJunctionPaintRecord<'_> {
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

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only contained-floating resize geometry.
#[derive(Debug, Clone, Copy)]
pub struct ContainedResizePaintRecord {
    record: ContainedResizeRecord,
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
        self.record
            .resize()
            .iter()
            .copied()
            .map(|record| ContainedResizePaintRecord { record })
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

/// Read-only plan supplied before one exact renderer paint.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePaintPlan<'frame> {
    pub(super) surface: SurfaceId,
    pub(super) authority_domain: crate::ids::EngineAuthorityDomainId,
    pub(super) version: crate::model::WorkspaceVersion,
    pub(super) scene: crate::scene::SurfaceSceneStamp,
    pub(super) output: SurfacePresentationOutputTicket,
    pub(super) plan: &'frame PresentationPlan,
    pub(super) hit_manifest: &'frame crate::presentation_hit::PresentationHitManifest,
    pub(super) drop_affordance: Option<&'frame DropAffordance>,
    pub(super) drag_preview: Option<&'frame InteractionPreview>,
    pub(super) contained_transform_preview: Option<&'frame ContainedTransformPreview>,
}

impl<'frame> SurfacePaintPlan<'frame> {
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Prepares a current-candidate tab selection without exposing scene identity.
    #[must_use]
    pub fn prepare_tab_select(self, item: ItemId) -> Option<super::PreparedSurfaceAction> {
        let tab = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)?;
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
        let tab = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item && record.close_bounds().is_some())?;
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
        Some(super::PreparedSurfaceAction::close(
            self.authority_domain,
            self.version,
            self.scene,
            crate::intent::CloseSceneTarget::Contained(floating),
        ))
    }

    #[must_use]
    pub fn bounds(self) -> LogicalRect {
        self.plan.bounds()
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
            .map(|record| TabPaintRecord { record })
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
            .filter_map(move |region| self.descriptor(region.id(), region.hit().rect()))
    }

    #[must_use]
    pub fn tab_receiver(self, item: ItemId) -> Option<DockspaceReceiverDescriptor> {
        self.receiver(
            |kind| matches!(kind, PresentationHitRegionKind::TabBody(tab) if tab.item == item),
        )
    }

    #[must_use]
    pub fn center_drop_receiver_for_item(
        self,
        item: ItemId,
    ) -> Option<DockspaceReceiverDescriptor> {
        let tabs = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)?
            .id()
            .tabs;
        self.receiver(|kind| {
            matches!(
                kind,
                PresentationHitRegionKind::DropTarget(DropTargetId::Center {
                    tabs: target,
                    ..
                }) if target == tabs
            )
        })
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

    fn receiver(
        self,
        matches: impl Fn(PresentationHitRegionKind) -> bool,
    ) -> Option<DockspaceReceiverDescriptor> {
        let region = self
            .hit_manifest
            .regions()
            .iter()
            .copied()
            .find(|region| !region.is_passive() && matches(region.id().kind()))?;
        self.descriptor(region.id(), region.hit().rect())
    }

    fn descriptor(
        self,
        region: PresentationHitRegionId,
        bounds: LogicalRect,
    ) -> Option<DockspaceReceiverDescriptor> {
        DockspaceReceiverDescriptor::from_projection_region(self.output, region, bounds)
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
        PresentationHitRegionKind::TabGroupGrip(_) => DockspaceReceiverRole::TabGroupGrip,
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
