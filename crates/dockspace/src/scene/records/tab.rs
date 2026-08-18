//! Tab, tab-bar, pane, and tab-list presentation records.

use crate::drop_target::SceneLayerKey;
use crate::geometry::LogicalRect;
use crate::hit_region::HitRegion;
use crate::ids::{ItemId, NodeId, RootId};
use crate::policy::TabBarInteraction;
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId, TabStripControlId};

/// Structural identity of a rendered tab-bar rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabBarSceneId {
    /// Owning root.
    pub root: RootId,
    /// Tabs node owning the bar.
    pub tabs: NodeId,
}

/// Structural identity of one rendered tab rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabSceneId {
    /// Owning root.
    pub root: RootId,
    /// Tabs node owning the item.
    pub tabs: NodeId,
    /// Stable item identity.
    pub item: ItemId,
}

/// Stable identity of one rendered tabs leaf and its pane content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneSceneId {
    /// Owning root.
    pub root: RootId,
    /// Tabs node owning the content region.
    pub tabs: NodeId,
}

/// Core-compiled tabs-leaf and selected-pane geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaneRecord {
    id: PaneSceneId,
    bounds: LogicalRect,
    content_bounds: LogicalRect,
    selected: Option<ItemId>,
    layer: SceneLayerKey,
}

impl PaneRecord {
    pub(crate) const fn new(
        id: PaneSceneId,
        bounds: LogicalRect,
        content_bounds: LogicalRect,
        selected: Option<ItemId>,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            id,
            bounds,
            content_bounds,
            selected,
            layer,
        }
    }

    /// Returns the stable tabs-leaf identity.
    #[must_use]
    pub const fn id(&self) -> PaneSceneId {
        self.id
    }

    /// Returns the complete tabs-leaf bounds, including its tab bar.
    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    /// Returns the exact pane-content bounds below the tab bar.
    #[must_use]
    pub const fn content_bounds(&self) -> LogicalRect {
        self.content_bounds
    }

    /// Returns the final core-selected item for this leaf.
    #[must_use]
    pub const fn selected(&self) -> Option<ItemId> {
        self.selected
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

/// Core-compiled geometry and state for one visible tab.
#[derive(Debug, Clone, PartialEq)]
pub struct TabRecord {
    id: TabSceneId,
    full_bounds: LogicalRect,
    visible_bounds: LogicalRect,
    text_bounds: LogicalRect,
    drag_hit: HitRegion,
    close_visual_bounds: Option<LogicalRect>,
    close_bounds: Option<LogicalRect>,
    selected: bool,
    ordinal: usize,
    layer: SceneLayerKey,
}

impl TabRecord {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        id: TabSceneId,
        full_bounds: LogicalRect,
        visible_bounds: LogicalRect,
        text_bounds: LogicalRect,
        drag_hit: HitRegion,
        close_visual_bounds: Option<LogicalRect>,
        close_bounds: Option<LogicalRect>,
        selected: bool,
        ordinal: usize,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            id,
            full_bounds,
            visible_bounds,
            text_bounds,
            drag_hit,
            close_visual_bounds,
            close_bounds,
            selected,
            ordinal,
            layer,
        }
    }

    /// Returns the stable structural tab identity.
    #[must_use]
    pub const fn id(&self) -> &TabSceneId {
        &self.id
    }

    /// Returns the unclipped allocated tab bounds.
    #[must_use]
    pub const fn full_bounds(&self) -> LogicalRect {
        self.full_bounds
    }

    /// Returns the exact clipped rectangle painted for this tab.
    #[must_use]
    pub const fn visible_bounds(&self) -> LogicalRect {
        self.visible_bounds
    }

    /// Returns the text clipping rectangle.
    #[must_use]
    pub const fn text_bounds(&self) -> LogicalRect {
        self.text_bounds
    }

    /// Returns the exact drag/select interaction region.
    #[must_use]
    pub const fn drag_hit(&self) -> HitRegion {
        self.drag_hit
    }

    /// Returns the painted close-control rectangle allowed by static policy.
    #[must_use]
    pub const fn close_visual_bounds(&self) -> Option<LogicalRect> {
        self.close_visual_bounds
    }

    /// Returns the operable close-control rectangle when tab interaction is enabled.
    #[must_use]
    pub const fn close_bounds(&self) -> Option<LogicalRect> {
        self.close_bounds
    }

    /// Returns whether this item is selected in its tabs node.
    #[must_use]
    pub const fn selected(&self) -> bool {
        self.selected
    }

    /// Returns the stable pre-removal item ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

/// Stable semantic identity of one whole-tab-stack drag region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabGroupDragRegionKind {
    /// Dedicated grip before the first tab.
    LeadingGrip,
    /// Unoccupied strip space after the final tab.
    TrailingEmpty,
}

/// Exact draw and hit geometry for one whole-tab-stack drag region.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabGroupDragRegionRecord {
    kind: TabGroupDragRegionKind,
    bounds: LogicalRect,
    hit: HitRegion,
}

impl TabGroupDragRegionRecord {
    const fn new(kind: TabGroupDragRegionKind, bounds: LogicalRect) -> Self {
        Self {
            kind,
            bounds,
            hit: HitRegion::new(bounds),
        }
    }

    /// Returns the stable semantic role of this region.
    #[must_use]
    pub const fn kind(self) -> TabGroupDragRegionKind {
        self.kind
    }

    /// Returns the exact rectangle which adapters may decorate.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the exact half-open group gesture region.
    #[must_use]
    pub const fn hit(self) -> HitRegion {
        self.hit
    }
}

/// Exact hit geometry for the whole-tab-stack gesture of one tab bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabGroupDragRecord {
    leading_grip: TabGroupDragRegionRecord,
    trailing_empty: Option<TabGroupDragRegionRecord>,
}

impl TabGroupDragRecord {
    pub(crate) const fn new(
        leading_grip: LogicalRect,
        trailing_empty: Option<LogicalRect>,
    ) -> Self {
        Self {
            leading_grip: TabGroupDragRegionRecord::new(
                TabGroupDragRegionKind::LeadingGrip,
                leading_grip,
            ),
            trailing_empty: match trailing_empty {
                Some(bounds) => Some(TabGroupDragRegionRecord::new(
                    TabGroupDragRegionKind::TrailingEmpty,
                    bounds,
                )),
                None => None,
            },
        }
    }

    /// Returns the dedicated region before the first tab.
    #[must_use]
    pub const fn leading_grip(self) -> TabGroupDragRegionRecord {
        self.leading_grip
    }

    /// Returns unoccupied strip space after the final tab, when present.
    #[must_use]
    pub const fn trailing_empty(self) -> Option<TabGroupDragRegionRecord> {
        self.trailing_empty
    }

    /// Returns one exact region by semantic role.
    #[must_use]
    pub const fn region(self, kind: TabGroupDragRegionKind) -> Option<TabGroupDragRegionRecord> {
        match kind {
            TabGroupDragRegionKind::LeadingGrip => Some(self.leading_grip),
            TabGroupDragRegionKind::TrailingEmpty => self.trailing_empty,
        }
    }

    /// Iterates exact group-drag regions in stable visual order.
    pub fn regions(self) -> impl Iterator<Item = TabGroupDragRegionRecord> {
        [Some(self.leading_grip), self.trailing_empty]
            .into_iter()
            .flatten()
    }

    /// Returns whether either exact group-drag region contains the point.
    #[must_use]
    pub fn contains(self, point: crate::geometry::LogicalPoint) -> bool {
        self.regions().any(|region| region.hit().contains(point))
    }

    /// Returns the dedicated leading grip bounds.
    ///
    /// This compatibility accessor is equivalent to
    /// [`Self::leading_grip`]'s bounds.
    #[must_use]
    pub const fn grip_bounds(self) -> LogicalRect {
        self.leading_grip.bounds()
    }

    /// Returns the dedicated leading grip hit region.
    ///
    /// This compatibility accessor does not include trailing empty space.
    #[must_use]
    pub const fn hit(self) -> HitRegion {
        self.leading_grip.hit()
    }
}

/// Core-compiled geometry and transient state for one tab bar.
#[derive(Debug, Clone, PartialEq)]
pub struct TabBarRecord {
    id: TabBarSceneId,
    bounds: LogicalRect,
    viewport: LogicalRect,
    scroll_offset: f64,
    maximum_scroll_offset: f64,
    hidden_items: Vec<ItemId>,
    members: Vec<TabStripMemberRecord>,
    group_grip_bounds: Option<LogicalRect>,
    group_drag: Option<TabGroupDragRecord>,
    interaction: TabBarInteraction,
    menu_geometry: TabListMenuGeometryAvailability,
    layer: SceneLayerKey,
}

/// Whether this exact tab-bar projection can allocate a tab-list menu.
///
/// This is a compiled presentation capability, not a policy decision. It stays
/// explicit so an absent optional measurement cannot be mistaken for an
/// enabled menu command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabListMenuGeometryAvailability {
    /// The adapter supplied complete metrics and the bar has a menu anchor.
    Available,
    /// This exact projection cannot allocate menu geometry.
    Unavailable,
}

impl TabListMenuGeometryAvailability {
    /// Returns whether this exact projection can allocate menu geometry.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

impl TabBarRecord {
    pub(crate) fn new(
        id: TabBarSceneId,
        bounds: LogicalRect,
        viewport: LogicalRect,
        scroll_offset: f64,
        maximum_scroll_offset: f64,
        hidden_items: Vec<ItemId>,
        members: Vec<TabStripMemberRecord>,
        group_grip_bounds: Option<LogicalRect>,
        group_drag: Option<TabGroupDragRecord>,
        interaction: TabBarInteraction,
        menu_geometry: TabListMenuGeometryAvailability,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            id,
            bounds,
            viewport,
            scroll_offset,
            maximum_scroll_offset,
            hidden_items,
            members,
            group_grip_bounds,
            group_drag,
            interaction,
            menu_geometry,
            layer,
        }
    }

    /// Returns the stable tabs-node identity.
    #[must_use]
    pub const fn id(&self) -> &TabBarSceneId {
        &self.id
    }

    /// Returns the complete tab-bar rectangle.
    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    /// Returns the exact clipped viewport available to tab records.
    #[must_use]
    pub const fn viewport(&self) -> LogicalRect {
        self.viewport
    }

    /// Returns the core-clamped transient scroll offset.
    #[must_use]
    pub const fn scroll_offset(&self) -> f64 {
        self.scroll_offset
    }

    /// Returns the largest valid transient scroll offset.
    #[must_use]
    pub const fn maximum_scroll_offset(&self) -> f64 {
        self.maximum_scroll_offset
    }

    /// Returns hidden items in their stable tabs-node order.
    #[must_use]
    pub fn hidden_items(&self) -> &[ItemId] {
        &self.hidden_items
    }

    /// Returns every tab in stable workspace order with explicit visibility.
    #[must_use]
    pub fn members(&self) -> &[TabStripMemberRecord] {
        &self.members
    }

    /// Returns paint geometry for the whole-stack grip, when one is visible.
    #[must_use]
    pub const fn group_grip_bounds(&self) -> Option<LogicalRect> {
        self.group_grip_bounds
    }

    /// Returns the exact whole-stack group gesture geometry, when available.
    #[must_use]
    pub const fn group_drag(&self) -> Option<&TabGroupDragRecord> {
        self.group_drag.as_ref()
    }

    /// Returns whether this visible bar may publish semantic interactions.
    #[must_use]
    pub const fn interaction(&self) -> TabBarInteraction {
        self.interaction
    }

    /// Returns the exact compiled menu-geometry capability.
    #[must_use]
    pub const fn menu_geometry_availability(&self) -> TabListMenuGeometryAvailability {
        self.menu_geometry
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

/// Core classification of one tab relative to the clipped strip viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabStripMemberVisibility {
    /// The complete allocated tab rectangle is visible.
    Visible,
    /// A positive but incomplete portion intersects the viewport.
    PartiallyVisible,
    /// No positive portion intersects the viewport.
    Hidden,
}

/// Stable ordered membership of one tab strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripMemberRecord {
    tab: TabSceneId,
    ordinal: usize,
    full_bounds: LogicalRect,
    visibility: TabStripMemberVisibility,
}

impl TabStripMemberRecord {
    pub(crate) const fn new(
        tab: TabSceneId,
        ordinal: usize,
        full_bounds: LogicalRect,
        visibility: TabStripMemberVisibility,
    ) -> Self {
        Self {
            tab,
            ordinal,
            full_bounds,
            visibility,
        }
    }

    #[must_use]
    pub const fn tab(self) -> TabSceneId {
        self.tab
    }

    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.ordinal
    }

    /// Returns the exact unclipped rectangle allocated to this member.
    #[must_use]
    pub const fn full_bounds(self) -> LogicalRect {
        self.full_bounds
    }

    #[must_use]
    pub const fn visibility(self) -> TabStripMemberVisibility {
        self.visibility
    }
}

/// Core-compiled draw and hit geometry for one tab-strip control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripControlRecord {
    id: TabStripControlId,
    bounds: LogicalRect,
    hit: HitRegion,
    enabled: bool,
    layer: SceneLayerKey,
}

impl TabStripControlRecord {
    pub(crate) const fn new(
        id: TabStripControlId,
        bounds: LogicalRect,
        enabled: bool,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            id,
            bounds,
            hit: HitRegion::new(bounds),
            enabled,
            layer,
        }
    }

    #[must_use]
    pub const fn id(self) -> TabStripControlId {
        self.id
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    #[must_use]
    pub const fn hit(self) -> HitRegion {
        self.hit
    }

    #[must_use]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn layer(self) -> SceneLayerKey {
        self.layer
    }
}

/// One ordered tab-list menu row with clipped optional interaction geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabListMenuRowRecord {
    tab: TabSceneId,
    ordinal: usize,
    bounds: LogicalRect,
    hit: Option<HitRegion>,
    selected: bool,
    focused: bool,
}

impl TabListMenuRowRecord {
    pub(crate) const fn new(
        tab: TabSceneId,
        ordinal: usize,
        bounds: LogicalRect,
        hit: Option<HitRegion>,
        selected: bool,
        focused: bool,
    ) -> Self {
        Self {
            tab,
            ordinal,
            bounds,
            hit,
            selected,
            focused,
        }
    }

    #[must_use]
    pub const fn tab(self) -> TabSceneId {
        self.tab
    }

    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.ordinal
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    #[must_use]
    pub const fn hit(self) -> Option<HitRegion> {
        self.hit
    }

    #[must_use]
    pub const fn selected(self) -> bool {
        self.selected
    }

    #[must_use]
    pub const fn focused(self) -> bool {
        self.focused
    }
}

/// Core-compiled geometry for one open tab-list menu.
#[derive(Debug, Clone, PartialEq)]
pub struct TabListMenuRecord {
    session: TabListMenuSessionId,
    bar: TabBarSceneId,
    bounds: LogicalRect,
    viewport: LogicalRect,
    scroll_offset: f64,
    maximum_scroll_offset: f64,
    rows: Vec<TabListMenuRowRecord>,
    layer: SceneLayerKey,
}

/// Full-surface receiver coverage for one workspace-global tab-list popup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabListMenuBackdropRecord {
    session: TabListMenuSessionId,
    revision: PopupRoutingRevision,
    bounds: LogicalRect,
}

impl TabListMenuBackdropRecord {
    pub(crate) const fn new(
        session: TabListMenuSessionId,
        revision: PopupRoutingRevision,
        bounds: LogicalRect,
    ) -> Self {
        Self {
            session,
            revision,
            bounds,
        }
    }

    /// Returns the sole popup session covered by this backdrop.
    #[must_use]
    pub const fn session(self) -> TabListMenuSessionId {
        self.session
    }

    /// Returns the exact global popup-plane revision.
    #[must_use]
    pub const fn revision(self) -> PopupRoutingRevision {
        self.revision
    }

    /// Returns the full logical bounds of the covered surface plan.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }
}

impl TabListMenuRecord {
    pub(crate) fn new(
        session: TabListMenuSessionId,
        bar: TabBarSceneId,
        bounds: LogicalRect,
        viewport: LogicalRect,
        scroll_offset: f64,
        maximum_scroll_offset: f64,
        rows: Vec<TabListMenuRowRecord>,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            session,
            bar,
            bounds,
            viewport,
            scroll_offset,
            maximum_scroll_offset,
            rows,
            layer,
        }
    }

    /// Returns the exact core-owned menu instance represented by this record.
    #[must_use]
    pub const fn session(&self) -> TabListMenuSessionId {
        self.session
    }

    #[must_use]
    pub const fn bar(&self) -> TabBarSceneId {
        self.bar
    }

    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    #[must_use]
    pub const fn viewport(&self) -> LogicalRect {
        self.viewport
    }

    #[must_use]
    pub const fn scroll_offset(&self) -> f64 {
        self.scroll_offset
    }

    #[must_use]
    pub const fn maximum_scroll_offset(&self) -> f64 {
        self.maximum_scroll_offset
    }

    #[must_use]
    pub fn rows(&self) -> &[TabListMenuRowRecord] {
        &self.rows
    }

    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}
