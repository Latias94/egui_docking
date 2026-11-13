//! Immutable, non-interactive future target tabs belonging to one exact preview.

use crate::geometry::LogicalRect;
use crate::ids::ItemId;
use crate::scene_compiler::{FutureTab, FutureTabStrip};

use super::DockspaceDragPreview;

/// Future target-strip geometry computed by the unique docking preflight.
///
/// This record carries paint facts only. It never exposes receivers or actions,
/// and is absent for split previews, hidden bars and unmeasured future chrome.
#[derive(Debug, Clone, Copy)]
pub struct DockspacePreviewTabBarPaintRecord<'plan> {
    record: &'plan FutureTabStrip,
}

impl DockspacePreviewTabBarPaintRecord<'_> {
    /// Returns the future tab-bar background bounds.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.record.bounds
    }

    /// Returns the exact viewport after reserved chrome and overflow controls.
    #[must_use]
    pub const fn viewport(self) -> LogicalRect {
        self.record.viewport
    }

    /// Returns the scroll offset applied to every future member.
    #[must_use]
    pub const fn scroll_offset(self) -> f64 {
        self.record.scroll_offset
    }

    /// Returns the future strip's maximum scroll offset.
    #[must_use]
    pub const fn maximum_scroll_offset(self) -> f64 {
        self.record.maximum_scroll_offset
    }
}

/// One member of the future target strip, in candidate tab order.
///
/// Members outside the viewport remain enumerable for title/resource lookup and
/// payload order. Paint only members with [`Self::visible_bounds`]. The clipping
/// and text allocations use the same rules as ordinary target tabs.
/// This preview never grants selection, close, drag or other input authority.
#[derive(Debug, Clone, Copy)]
pub struct DockspacePreviewTabPaintRecord<'plan> {
    record: &'plan FutureTab,
}

impl DockspacePreviewTabPaintRecord<'_> {
    /// Returns the application-owned item whose title should be painted.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.record.item
    }

    /// Returns the member's index in the complete future target strip.
    #[must_use]
    pub const fn ordinal(self) -> usize {
        self.record.ordinal
    }

    /// Returns whether this member will be selected after the drop.
    #[must_use]
    pub const fn selected(self) -> bool {
        self.record.selected
    }

    /// Returns whether this member belongs to the complete dragged payload.
    #[must_use]
    pub const fn is_payload(self) -> bool {
        self.record.payload
    }

    /// Returns the full tab allocation before viewport clipping.
    #[must_use]
    pub const fn full_bounds(self) -> LogicalRect {
        self.record.full_bounds
    }

    /// Returns the exact target viewport which clips this member.
    #[must_use]
    pub const fn clip_bounds(self) -> LogicalRect {
        self.record.clip_bounds
    }

    /// Returns the paintable tab bounds, or `None` for a clipped-out member.
    ///
    /// A sliver too small to contain the standard tab chrome is also omitted.
    #[must_use]
    pub const fn visible_bounds(self) -> Option<LogicalRect> {
        self.record.visible_bounds
    }

    /// Returns the full title allocation before clipping, when the tab is paintable.
    #[must_use]
    pub const fn text_layout_bounds(self) -> Option<LogicalRect> {
        self.record.text_layout_bounds
    }

    /// Returns the title's visible clip, retaining standard close-button spacing.
    #[must_use]
    pub const fn text_bounds(self) -> Option<LogicalRect> {
        self.record.text_bounds
    }

    /// Returns reserved close-button geometry for matching the future tab shape.
    ///
    /// This is a paint allocation, never a close receiver or action.
    #[must_use]
    pub const fn close_visual_bounds(self) -> Option<LogicalRect> {
        self.record.close_visual_bounds
    }
}

impl<'plan> DockspaceDragPreview<'plan> {
    /// Returns the projected target bar for a center merge or tab-gap insertion.
    ///
    /// The immutable geometry belongs to this exact preview token. No candidate
    /// transaction or tab layout is executed while painting. Hidden bars, split
    /// targets and unavailable measurements produce no invented tab bar.
    #[must_use]
    pub fn tab_bar(self) -> Option<DockspacePreviewTabBarPaintRecord<'plan>> {
        self.preview
            .tab_strip()
            .map(|record| DockspacePreviewTabBarPaintRecord { record })
    }

    /// Enumerates every future target member in candidate order, including clipped tabs.
    ///
    /// Use [`DockspacePreviewTabPaintRecord::is_payload`] to identify all dragged
    /// titles. Bounds and selection are derived from the prevalidated candidate,
    /// rather than from the source ghost or the target's current tab bar.
    pub fn tabs(self) -> impl ExactSizeIterator<Item = DockspacePreviewTabPaintRecord<'plan>> {
        self.preview
            .tab_strip()
            .map_or(&[][..], |strip| strip.tabs.as_ref())
            .iter()
            .map(|record| DockspacePreviewTabPaintRecord { record })
    }
}
