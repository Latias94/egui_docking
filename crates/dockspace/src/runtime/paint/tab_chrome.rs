//! Tab-strip controls and popup paint records.

use crate::geometry::LogicalRect;
use crate::ids::{ItemId, SurfaceId};
use crate::scene::{
    TabListMenuBackdropRecord, TabListMenuRecord, TabListMenuRowRecord, TabStripControlRecord,
};
use crate::tab_strip::{TabListMenuSessionId, TabStripControlId};

use super::{DockspacePaintLayer, DockspaceVisualId, VisualIdentity};

/// Product-facing class of one core-owned tab-strip control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabStripControlKind {
    /// Scroll toward the beginning of the stable tab order.
    ScrollBackward,
    /// Scroll toward the end of the stable tab order.
    ScrollForward,
    /// Open or close the complete tab-list menu.
    TabListMenu,
}

/// Read-only geometry and state for one tab-strip control.
#[derive(Debug, Clone, Copy)]
pub struct TabStripControlPaintRecord {
    pub(super) record: TabStripControlRecord,
}

impl TabStripControlPaintRecord {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabStripControl(self.record.id()))
    }

    #[must_use]
    pub const fn tab_bar_visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabBar(self.record.id().bar()))
    }

    #[must_use]
    pub const fn kind(self) -> TabStripControlKind {
        match self.record.id() {
            TabStripControlId::ScrollBackward(_) => TabStripControlKind::ScrollBackward,
            TabStripControlId::ScrollForward(_) => TabStripControlKind::ScrollForward,
            TabStripControlId::TabListMenu(_) => TabStripControlKind::TabListMenu,
        }
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds()
    }

    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.hit().rect()
    }

    #[must_use]
    pub const fn enabled(self) -> bool {
        self.record.enabled()
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only row within one core-owned tab-list popup.
#[derive(Debug, Clone, Copy)]
pub struct TabListMenuRowPaintRecord {
    pub(super) session: TabListMenuSessionId,
    pub(super) record: TabListMenuRowRecord,
}

impl TabListMenuRowPaintRecord {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabListMenuRow {
            session: self.session,
            tab: self.record.tab(),
        })
    }

    #[must_use]
    pub const fn tab_visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::Tab(self.record.tab()))
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
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds()
    }

    #[must_use]
    pub const fn interaction_bounds(self) -> Option<LogicalRect> {
        match self.record.hit() {
            Some(hit) => Some(hit.rect()),
            None => None,
        }
    }

    #[must_use]
    pub const fn selected(self) -> bool {
        self.record.selected()
    }

    #[must_use]
    pub const fn focused(self) -> bool {
        self.record.focused()
    }
}

/// Read-only geometry for one open tab-list popup.
#[derive(Debug, Clone, Copy)]
pub struct TabListMenuPaintRecord<'plan> {
    pub(super) record: &'plan TabListMenuRecord,
}

impl<'plan> TabListMenuPaintRecord<'plan> {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabListMenu(self.record.session()))
    }

    #[must_use]
    pub const fn tab_bar_visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabBar(self.record.bar()))
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

    pub fn rows(self) -> impl ExactSizeIterator<Item = TabListMenuRowPaintRecord> + 'plan {
        let session = self.record.session();
        self.record
            .rows()
            .iter()
            .copied()
            .map(move |record| TabListMenuRowPaintRecord { session, record })
    }

    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }
}

/// Read-only full-surface popup backdrop geometry.
#[derive(Debug, Clone, Copy)]
pub struct TabListMenuBackdropPaintRecord {
    pub(super) surface: SurfaceId,
    pub(super) record: TabListMenuBackdropRecord,
}

impl TabListMenuBackdropPaintRecord {
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabListMenuBackdrop {
            surface: self.surface,
            session: self.record.session(),
            revision: self.record.revision(),
        })
    }

    #[must_use]
    pub const fn menu_visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::TabListMenu(self.record.session()))
    }

    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds()
    }
}
