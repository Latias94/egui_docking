//! Optional tab rendering and fixed tab-bar slots for application chrome.

use std::sync::Arc;

use dockspace::runtime::{DockspaceVisualId, TabBarPaintRecord};
use egui::{Color32, CornerRadius, Galley, Painter, Rect, Response, Stroke, StrokeKind, Ui};

use crate::{ItemId, RootId};

/// Stable identity of a tab group within one docking workspace.
///
/// Use this as an application map key or an egui ID salt. It survives selection
/// and scrolling; replacing the layout may replace the group. Scope shared
/// application maps by workspace when using multiple docking workspaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabBarId(pub(crate) DockspaceVisualId);

/// Requested fixed widths outside the scrollable tab strip, in egui points.
///
/// Core reserves these widths before allocating tabs and overflow controls.
/// In a narrow group, the leading slot has priority and both slots may shrink
/// or disappear to preserve the minimum operable tab width. Always draw into
/// the actual Ui supplied to [`crate::PaneView::tab_bar_ui`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabBarLayout {
    /// Slot after the group grip and before the first tab or scroll arrow.
    pub leading_width: f32,
    /// Slot at the right edge, after the built-in presentation controls.
    pub trailing_width: f32,
}

/// The fixed tab-bar slot currently being rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TabBarSlot {
    Leading,
    Trailing,
}

/// Current group identity and core-allocated geometry for a tab-bar callback.
#[derive(Clone, Copy, Debug)]
pub struct TabBar<'a> {
    pub(crate) record: TabBarPaintRecord<'a>,
}

impl TabBar<'_> {
    /// Returns the same identity used by the layout callback and every member tab.
    #[must_use]
    pub fn id(self) -> TabBarId {
        TabBarId(self.record.visual_id())
    }

    #[must_use]
    pub fn root(self) -> RootId {
        self.record.root()
    }

    /// Iterates every open item in group order, including scrolled-out tabs.
    pub fn items(self) -> impl ExactSizeIterator<Item = ItemId> {
        self.record.members().map(|member| member.item())
    }
}

/// Resolved defaults which a tab callback can override for one item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabVisuals {
    pub fill: Color32,
    pub stroke: Stroke,
    pub corner_radius: CornerRadius,
    /// Fallback used by title spans without an explicit color.
    pub text_color: Color32,
    /// When present, replaces every title span's color.
    pub text_color_override: Option<Color32>,
}

/// One tab's allocated geometry, response, title, and resolved appearance.
///
/// The adapter owns selection, dragging, close controls, focus cues and
/// accessibility. Custom callbacks can paint the entire tab and place ordinary
/// widgets inside [`Self::content_rect`]. Those widgets keep their egui input
/// and block docking gestures over themselves. Respect the supplied clip and
/// leave the close-control area outside `content_rect` available to the adapter.
#[derive(Clone, Debug)]
pub struct Tab<'a> {
    pub item: ItemId,
    pub bar: TabBarId,
    /// Full tab allocation, before horizontal scrolling clips it.
    pub rect: Rect,
    /// Visible intersection of the tab and its scrollable viewport.
    pub visible_rect: Rect,
    /// Content allocation after padding and the built-in close control.
    pub content_rect: Rect,
    /// Visible content intersection, excluding the close control.
    pub content_clip_rect: Rect,
    /// Original shaped title. Use [`Self::paint_title`] to elide it for a subrect.
    pub title: Arc<Galley>,
    pub response: &'a Response,
    pub selected: bool,
    /// Includes both the egui enablement and docking interaction policy.
    pub enabled: bool,
    pub visuals: TabVisuals,
}

impl Tab<'_> {
    /// Paints the default background and title. Close and focus cues are separate.
    pub fn paint(&self, ui: &Ui) {
        self.paint_background(ui.painter());
        self.paint_title(ui, self.content_rect);
    }

    /// Paints the configured background and outline without the title.
    pub fn paint_background(&self, painter: &Painter) {
        painter.with_clip_rect(self.visible_rect).rect(
            self.rect,
            self.visuals.corner_radius,
            self.visuals.fill,
            self.visuals.stroke,
            StrokeKind::Inside,
        );
    }

    /// Paints a vertically centered title, elided to the provided content subrect.
    ///
    /// Use this after reserving space for an icon, badge or application button.
    pub fn paint_title(&self, ui: &Ui, rect: Rect) {
        let rect = rect.intersect(self.content_rect);
        if !rect.is_positive() {
            return;
        }
        let galley = galley_for_width(ui, &self.title, rect.width());
        let painter = ui
            .painter()
            .with_clip_rect(self.content_clip_rect.intersect(rect));
        let position = egui::pos2(rect.min.x, rect.center().y - galley.size().y * 0.5);
        if let Some(color) = self.visuals.text_color_override {
            painter.galley_with_override_text_color(position, galley, color);
        } else {
            painter.galley(position, galley, self.visuals.text_color);
        }
    }
}

pub(crate) fn galley_for_width(ui: &Ui, galley: &Arc<Galley>, width: f32) -> Arc<Galley> {
    if !galley.elided && galley.size().x <= width {
        return galley.clone();
    }
    let mut job = (*galley.job).clone();
    job.wrap.max_width = width.max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
