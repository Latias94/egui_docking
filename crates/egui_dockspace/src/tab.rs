//! Optional tab rendering and fixed or scrolling slots for application chrome.

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

/// Requested application slots in a tab bar, in egui points.
///
/// Core reserves the fixed leading/trailing widths before allocating tabs and
/// overflow controls. In a narrow group, leading has priority and fixed slots
/// may shrink or disappear to preserve the minimum operable tab width. Draw into
/// the actual Ui supplied to [`crate::PaneView::tab_bar_ui`].
/// The after-tabs slot scrolls with tabs and keeps its requested width; only
/// its visible intersection is painted and interactive.
/// Include widget frame padding in reserved widths and fit controls to the
/// supplied Ui's height; egui widgets retain their ordinary sizing rules.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TabBarLayout {
    /// Slot after the group grip and before the first tab or scroll arrow.
    pub leading_width: f32,
    /// Slot at the right edge, after the built-in presentation controls.
    pub trailing_width: f32,
    /// Scrolling slot immediately after the final tab, such as an add button.
    pub after_tabs_width: f32,
}

/// The application tab-bar slot currently being rendered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TabBarSlot {
    /// Fixed slot between the group grip and the tabs or scroll controls.
    Leading,
    /// Fixed slot after the built-in presentation controls at the bar's right edge.
    Trailing,
    /// Slot after the last tab, scrolling and clipping with the tab strip.
    AfterTabs,
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

    /// Returns the stable docking root which owns this group.
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
    /// Tab background, before focus cues and the close icon are painted.
    pub fill: Color32,
    /// Tab outline, painted inside its allocation; `Stroke::NONE` hides it.
    pub stroke: Stroke,
    /// Rounding shared by the tab background and its focus outline.
    pub corner_radius: CornerRadius,
    /// Fallback used by title spans without an explicit color.
    pub text_color: Color32,
    /// When present, replaces every title span's color.
    pub text_color_override: Option<Color32>,
    /// Explicit close-icon color; `None` retains egui's own interaction colors.
    /// Use this when a custom background needs a contrasting icon in every state.
    pub close_color: Option<Color32>,
    /// Keyboard-focus outline. Its corners follow `corner_radius`.
    pub focus_stroke: Stroke,
    /// Content-focus underline; stroke width is its vertical thickness.
    pub pane_focus_stroke: Stroke,
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
    /// Stable application identity of this tab's pane.
    pub item: ItemId,
    /// Group identity shared with the tab-bar layout and rendering callbacks.
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
    /// Adapter-owned tab interaction, before application content widgets are added.
    pub response: &'a Response,
    /// Whether this tab is selected in its group, independently of pane focus.
    pub selected: bool,
    /// The selected pane owns observed content focus in the active window.
    pub pane_focused: bool,
    /// Includes both the egui enablement and docking interaction policy.
    pub enabled: bool,
    /// Resolved appearance; return any changes from [`crate::PaneView::tab_ui`].
    pub visuals: TabVisuals,
}

impl Tab<'_> {
    /// Paints the default background and title, returning their final appearance.
    /// Close and focus cues are painted separately from this appearance.
    pub fn paint(&self, ui: &Ui) -> TabVisuals {
        self.paint_background(ui.painter());
        self.paint_title(ui, self.content_rect);
        self.visuals
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
        paint_title(
            ui,
            &self.title,
            rect,
            self.content_clip_rect,
            self.visuals.text_color,
            self.visuals.text_color_override,
        );
    }
}

pub(crate) fn paint_title(
    ui: &Ui,
    title: &Arc<Galley>,
    rect: Rect,
    clip: Rect,
    color: Color32,
    color_override: Option<Color32>,
) {
    if !rect.is_positive() {
        return;
    }
    let galley = galley_for_width(ui, title, rect.width());
    let painter = ui.painter().with_clip_rect(clip.intersect(rect));
    let position = egui::pos2(rect.min.x, rect.center().y - galley.size().y * 0.5);
    if let Some(color) = color_override {
        painter.galley_with_override_text_color(position, galley, color);
    } else {
        painter.galley(position, galley, color);
    }
}

/// Paint-only customization of an adapter-owned tab close button.
///
/// The response, hit target, keyboard action and close protocol belong to the
/// adapter. Change the stroke or paint another icon inside the supplied clip.
#[derive(Clone, Copy, Debug)]
pub struct TabClose<'a> {
    /// Stable application identity of the pane this control would close.
    pub item: ItemId,
    /// Core-owned close-control allocation.
    pub rect: Rect,
    /// Centered icon allocation, independent of the hit target.
    pub icon_rect: Rect,
    /// Adapter-owned close interaction; custom painting must not consume it again.
    pub response: &'a Response,
    /// Whether the tab is selected, independently of hover and keyboard focus.
    pub selected: bool,
    /// Whether the adapter currently allows this control to accept input.
    pub enabled: bool,
    /// Resolved icon color and line width, before the supplied Ui's opacity.
    pub stroke: Stroke,
}

impl TabClose<'_> {
    /// Paints the default cross in the supplied icon allocation.
    pub fn paint(&self, ui: &Ui) {
        let painter = ui
            .painter()
            .with_clip_rect(ui.clip_rect().intersect(self.rect));
        painter.line_segment(
            [self.icon_rect.left_top(), self.icon_rect.right_bottom()],
            self.stroke,
        );
        painter.line_segment(
            [self.icon_rect.right_top(), self.icon_rect.left_bottom()],
            self.stroke,
        );
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
