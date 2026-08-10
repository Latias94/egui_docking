//! Pane, tab, and tab-bar paint records.

use crate::geometry::LogicalRect;
use crate::ids::{ItemId, RootId};
use crate::policy::TabBarInteraction;
use crate::scene::{
    PaneRecord, TabBarRecord, TabBarSceneId, TabRecord, TabStripMemberRecord,
    TabStripMemberVisibility as CoreTabStripMemberVisibility,
};

use super::{DockspacePaintLayer, DockspaceVisualId, VisualIdentity};

/// Read-only tabs-leaf and pane-content geometry.
#[derive(Debug, Clone, Copy)]
pub struct PanePaintRecord<'plan> {
    pub(super) record: &'plan PaneRecord,
}

impl PanePaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Pane(self.record.id()))
    }

    /// Returns the tab strip which owns this pane slot.
    #[must_use]
    pub const fn tab_bar_visual_id(self) -> DockspaceVisualId {
        let pane = self.record.id();
        DockspaceVisualId(VisualIdentity::TabBar(TabBarSceneId {
            root: pane.root,
            tabs: pane.tabs,
        }))
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
    pub(super) record: &'plan TabRecord,
}

impl TabPaintRecord<'_> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Tab(*self.record.id()))
    }

    /// Returns the tab strip which owns this tab.
    #[must_use]
    pub const fn tab_bar_visual_id(self) -> DockspaceVisualId {
        let tab = self.record.id();
        DockspaceVisualId(VisualIdentity::TabBar(TabBarSceneId {
            root: tab.root,
            tabs: tab.tabs,
        }))
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

/// Product-facing classification of one tab relative to its clipped strip viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabStripMemberVisibility {
    /// The complete allocated tab rectangle is visible.
    Visible,
    /// A positive but incomplete portion intersects the viewport.
    PartiallyVisible,
    /// No positive portion intersects the viewport.
    Hidden,
}

/// Read-only member of one tab strip, including clipped visibility.
#[derive(Debug, Clone, Copy)]
pub struct TabStripMemberPaintRecord {
    pub(super) record: TabStripMemberRecord,
}

impl TabStripMemberPaintRecord {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Tab(self.record.tab()))
    }

    #[must_use]
    pub const fn tab_bar_visual_id(self) -> DockspaceVisualId {
        let tab = self.record.tab();
        DockspaceVisualId(VisualIdentity::TabBar(TabBarSceneId {
            root: tab.root,
            tabs: tab.tabs,
        }))
    }

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
        match self.record.visibility() {
            CoreTabStripMemberVisibility::Visible => TabStripMemberVisibility::Visible,
            CoreTabStripMemberVisibility::PartiallyVisible => {
                TabStripMemberVisibility::PartiallyVisible
            }
            CoreTabStripMemberVisibility::Hidden => TabStripMemberVisibility::Hidden,
        }
    }
}

/// Read-only tab-bar geometry and overflow state.
#[derive(Debug, Clone, Copy)]
pub struct TabBarPaintRecord<'plan> {
    pub(super) record: &'plan TabBarRecord,
}

impl<'plan> TabBarPaintRecord<'plan> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabBar(*self.record.id()))
    }

    /// Returns the pane slot owned by this tab strip.
    #[must_use]
    pub const fn pane_visual_id(self) -> DockspaceVisualId {
        let bar = self.record.id();
        DockspaceVisualId(VisualIdentity::Pane(crate::scene::PaneSceneId {
            root: bar.root,
            tabs: bar.tabs,
        }))
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
