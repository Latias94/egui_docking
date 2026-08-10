//! Renderer-neutral measurement and primary read-only docking paint capabilities.

use crate::drop_guide::{DropGuideClusterId, DropGuideScope, DropGuideSlot};
use crate::drop_target::{DropTargetAvailability, DropTargetId, DropTargetKind, SceneLayerKey};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::graph::Axis;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::interaction::{InteractionPreview, PreviewVisual};
use crate::model::DockspaceAxis;
use crate::policy::TabBarInteraction;
use crate::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use crate::presentation_observation::SurfacePresentationOutputTicket;
use crate::scene::{
    ContainedRecord, ContainedResizeDirection, ContainedResizeRecord, PaneRecord, PresentationPlan,
    SplitterJunctionId, SplitterJunctionRecord, SplitterRecord, SplitterSceneId, TabBarRecord,
    TabBarSceneId, TabRecord, TabSceneId, TabStripMemberVisibility,
};
/// Stable renderer identity whose structural storage remains core-private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DockspaceVisualId(VisualIdentity);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum VisualIdentity {
    Pane(crate::scene::PaneSceneId),
    Tab(TabSceneId),
    TabBar(TabBarSceneId),
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
            VisualIdentity::Splitter(_) => DockspaceVisualKind::Splitter,
            VisualIdentity::SplitterJunction(_) => DockspaceVisualKind::SplitterJunction,
            VisualIdentity::Contained(_) => DockspaceVisualKind::Contained,
            VisualIdentity::DropGuide(_) => DockspaceVisualKind::DropGuide,
            VisualIdentity::DropTarget(_) => DockspaceVisualKind::DropTarget,
            VisualIdentity::Receiver(_) => DockspaceVisualKind::Receiver,
        }
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

/// Read-only tabs-leaf and pane-content geometry.
#[derive(Debug, Clone, Copy)]
pub struct PanePaintRecord<'plan> {
    record: &'plan PaneRecord,
}

impl PanePaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Pane(self.record.id()))
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds()
    }

    #[must_use]
    pub const fn content_bounds(self) -> LogicalRect {
        self.record.content_bounds()
    }

    #[must_use]
    pub const fn selected(self) -> Option<ItemId> {
        self.record.selected()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only visible-tab geometry and state.
#[derive(Debug, Clone, Copy)]
pub struct TabPaintRecord<'plan> {
    record: &'plan TabRecord,
}

impl TabPaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Tab(*self.record.id()))
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    #[must_use]
    pub const fn item(self) -> ItemId {
        self.record.id().item
    }

    #[must_use]
    pub const fn full_bounds(self) -> LogicalRect {
        self.record.full_bounds()
    }

    #[must_use]
    pub const fn visible_bounds(self) -> LogicalRect {
        self.record.visible_bounds()
    }

    #[must_use]
    pub const fn text_bounds(self) -> LogicalRect {
        self.record.text_bounds()
    }

    #[must_use]
    pub const fn drag_bounds(self) -> LogicalRect {
        self.record.drag_hit().rect()
    }

    #[must_use]
    pub const fn close_visual_bounds(self) -> Option<LogicalRect> {
        self.record.close_visual_bounds()
    }

    #[must_use]
    pub const fn close_bounds(self) -> Option<LogicalRect> {
        self.record.close_bounds()
    }

    #[must_use]
    pub const fn selected(self) -> bool {
        self.record.selected()
    }

    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.record.ordinal()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only member of one tab strip, including clipped visibility.
#[derive(Debug, Clone, Copy)]
pub struct TabStripMemberPaintRecord {
    record: crate::scene::TabStripMemberRecord,
}

impl TabStripMemberPaintRecord {
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.record.tab().item
    }

    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.record.ordinal()
    }

    #[must_use]
    pub const fn full_bounds(self) -> LogicalRect {
        self.record.full_bounds()
    }

    #[must_use]
    pub const fn visibility(self) -> TabStripMemberVisibility {
        self.record.visibility()
    }
}

/// Read-only tab-bar geometry and overflow state.
#[derive(Debug, Clone, Copy)]
pub struct TabBarPaintRecord<'plan> {
    record: &'plan TabBarRecord,
}

impl<'plan> TabBarPaintRecord<'plan> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabBar(*self.record.id()))
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds()
    }

    #[must_use]
    pub const fn viewport(self) -> LogicalRect {
        self.record.viewport()
    }

    #[must_use]
    pub const fn scroll_offset(self) -> f64 {
        self.record.scroll_offset()
    }

    #[must_use]
    pub const fn maximum_scroll_offset(self) -> f64 {
        self.record.maximum_scroll_offset()
    }

    #[must_use]
    pub fn hidden_items(self) -> &'plan [ItemId] {
        self.record.hidden_items()
    }

    pub fn members(self) -> impl ExactSizeIterator<Item = TabStripMemberPaintRecord> + 'plan {
        self.record
            .members()
            .iter()
            .copied()
            .map(|record| TabStripMemberPaintRecord { record })
    }

    #[must_use]
    pub const fn group_grip_bounds(self) -> Option<LogicalRect> {
        self.record.group_grip_bounds()
    }

    #[must_use]
    pub const fn interaction(self) -> TabBarInteraction {
        self.record.interaction()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
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
        self.record.direction()
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

/// Public guide-cluster class without the internal target-node identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceGuideScope {
    Inner,
    Outer,
}

/// Read-only guide target geometry and availability.
#[derive(Debug, Clone, Copy)]
pub struct DropGuideTargetPaintRecord<'plan> {
    slot: DropGuideSlot,
    record: &'plan crate::drop_guide::DropGuideTargetRecord,
}

impl DropGuideTargetPaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropTarget(self.record.id()))
    }

    #[must_use]
    pub const fn slot(self) -> DropGuideSlot {
        self.slot
    }

    #[must_use]
    pub const fn kind(self) -> DropTargetKind {
        self.record.id().kind()
    }

    #[must_use]
    pub const fn draw_bounds(self) -> LogicalRect {
        self.record.draw()
    }

    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.target().region().rect()
    }

    #[must_use]
    pub const fn preview_bounds(self) -> LogicalRect {
        self.record.target().visual().rect()
    }

    #[must_use]
    pub const fn availability(self) -> DropTargetAvailability {
        self.record.target().availability()
    }
}

/// Read-only complete docking-guide cluster.
#[derive(Debug, Clone, Copy)]
pub struct DropGuidePaintRecord<'plan> {
    record: &'plan crate::drop_guide::DropGuideClusterRecord,
}

impl<'plan> DropGuidePaintRecord<'plan> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropGuide(self.record.id()))
    }

    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    #[must_use]
    pub const fn scope(self) -> DockspaceGuideScope {
        match self.record.id().scope {
            DropGuideScope::Inner(_) => DockspaceGuideScope::Inner,
            DropGuideScope::Outer => DockspaceGuideScope::Outer,
        }
    }

    #[must_use]
    pub const fn activation_bounds(self) -> LogicalRect {
        self.record.activation().rect()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }

    pub fn targets(
        self,
    ) -> impl DoubleEndedIterator<Item = DropGuideTargetPaintRecord<'plan>> + 'plan {
        self.record
            .targets()
            .map(|(slot, record)| DropGuideTargetPaintRecord { slot, record })
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
    pub(super) drag_preview: Option<&'frame InteractionPreview>,
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
