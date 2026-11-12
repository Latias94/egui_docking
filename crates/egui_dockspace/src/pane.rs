//! Application-owned pane rendering contract.

use dockspace::model::ItemId;
use egui::{Context, Id, Response, Ui, Vec2, WidgetText};

use crate::DragPreview;
use crate::tab::{Tab, TabBar, TabBarId, TabBarLayout, TabBarSlot, TabClose, TabVisuals};

/// Ordinary application-content registrations produced by one pane callback.
///
/// Return this value directly from [`PaneView::ui`]; no per-item ID cache or
/// follow-up callback is required. An empty response declares no content, so
/// unknown widgets continue to block docking hover.
///
/// ```
/// # use egui_dockspace::PaneResponse;
/// # fn editor(ui: &mut egui::Ui, text: &mut String) -> PaneResponse {
/// let response = ui.text_edit_multiline(text);
/// PaneResponse::from(&response)
/// # }
/// ```
#[derive(Debug, Default)]
pub struct PaneResponse {
    first: Option<Id>,
    additional: Vec<Id>,
}

impl PaneResponse {
    /// Declares an ordinary content response created during this callback.
    ///
    /// Do not declare popup, modal, or foreign-overlay controls. The adapter
    /// still checks current-pass registration, layer, enablement and bounds;
    /// a declaration never changes a widget's ordinary egui input handling.
    #[must_use]
    pub fn with_content(self, response: &Response) -> Self {
        self.with_content_ids([response.id])
    }

    /// Declares the exact IDs of additional ordinary content registrations.
    ///
    /// Use this for composite controls whose child responses are created inside
    /// nested egui callbacks. Include only registrations from this pane's
    /// current callback, including any scrolling controls and hover-only content.
    #[must_use]
    pub fn with_content_ids(mut self, ids: impl IntoIterator<Item = Id>) -> Self {
        let mut ids = ids.into_iter();
        if self.first.is_none() {
            self.first = ids.next();
        }
        self.additional.extend(ids);
        self
    }

    /// Returns the ordinary content IDs declared by this callback.
    #[must_use]
    pub fn content_widgets(&self) -> impl Iterator<Item = Id> + '_ {
        self.first
            .into_iter()
            .chain(self.additional.iter().copied())
    }
}

impl From<&Response> for PaneResponse {
    fn from(response: &Response) -> Self {
        Self::default().with_content(response)
    }
}

/// Exact adapter observation of one application's pane focus target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneFocusState {
    /// The application cannot currently prove whether this pane owns focus.
    Unknown,
    /// The pane's registered focus target currently owns focus.
    Focused,
    /// The application authoritatively reports that this pane does not own focus.
    Unfocused,
}

/// Resolves and renders application-owned panes by stable item identity.
///
/// The adapter borrows this object only while painting a frame. Implementors
/// retain ownership of pane state; the docking workspace stores only
/// [`ItemId`] values.
///
/// Callbacks run synchronously within the current egui pass. Start background
/// work in application-owned tasks, apply completed results on the UI thread,
/// and request a repaint. Do not wait for I/O or retain a [`Ui`] across tasks.
/// Boxed providers, including borrowed and non-`Send` trait objects, delegate
/// every callback to their inner provider. Native hosts may add their own
/// threading and lifetime requirements.
pub trait PaneView {
    /// Returns the current title for `item`.
    ///
    /// Explicit rich-text and layout-job colors are preserved in tabs, overflow
    /// rows and drag previews unless the corresponding docking text color or
    /// the local egui `override_text_color` is set. Docking overrides take
    /// precedence over the egui override. Uncolored text follows the current
    /// interaction state; disabled UI still applies egui's standard tint.
    ///
    /// Returning `None` reports that the application registry has no entry for
    /// the item. The adapter can then render a recoverable missing-pane state.
    /// The single-surface owners retain each measured title and its text layout
    /// for painting in that same pass. A later pass reads the title again.
    fn title(&self, item: ItemId) -> Option<WidgetText>;

    /// Measures tab content before docking padding and the built-in close button.
    ///
    /// The default is the title width. Reserve extra width here when `tab_ui`
    /// adds icons, badges or buttons. Return a finite, non-negative width in
    /// egui points; the adapter measures once per item in a UI pass. This query
    /// must not depend on painting the tab or pane first. Title metadata still
    /// supplies hover text, accessibility, overflow rows and drag previews.
    fn tab_content_width(&self, _item: ItemId, _ui: &Ui, title: &egui::Galley) -> f32 {
        title.size().x
    }

    /// Paints one tab, optionally adding application widgets to its content area.
    ///
    /// Call [`Tab::paint`] for defaults, or use its background and title helpers
    /// separately. The Ui starts at `tab.content_rect`, with a clip covering
    /// only the visible tab. The adapter retains the tab response,
    /// close control, keyboard handling, focus cues and accessibility.
    ///
    /// Render synchronously and respect the supplied clip. This method is
    /// skipped for missing panes and a dragged source gap.
    /// Application widgets remain ordinary egui widgets; no [`PaneResponse`]
    /// registration is needed or accepted for tab chrome.
    /// Return the final appearance so built-in focus and close cues follow it.
    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) -> TabVisuals {
        tab.paint(ui)
    }

    /// Paints a visible close icon without taking ownership of its interaction.
    fn tab_close_ui(&mut self, ui: &Ui, close: &TabClose<'_>) {
        close.paint(ui);
    }

    /// Paints a docking drag preview without adding interactive widgets.
    fn drag_preview_ui(&mut self, ui: &Ui, preview: &DragPreview) {
        preview.paint(ui);
    }

    /// Measures fixed and scrolling application slots in one group's tab bar.
    ///
    /// Return finite, non-negative widths. Defaults reserve no space and do
    /// not invoke `tab_bar_ui`. Use egui font/spacing measurements here rather
    /// than a previous paint pass; core may shrink fixed slot allocations.
    fn tab_bar_layout(&self, _bar: TabBarId, _ui: &Ui) -> TabBarLayout {
        TabBarLayout::default()
    }

    /// Renders one visible, core-allocated application tab-bar slot.
    ///
    /// All slots use a left-to-right, vertically centered Ui. It is clipped to
    /// the actual allocation and disabled when tab-bar interaction is disabled.
    /// Widgets own their input and cannot initiate a docking gesture. This uses
    /// the same synchronous rendering contract as [`Self::tab_ui`]. Queue
    /// requested workspace edits in application state and apply them after the
    /// docking call returns.
    fn tab_bar_ui(&mut self, _ui: &mut Ui, _bar: TabBar<'_>, _slot: TabBarSlot) {}

    /// Renders the application content associated with `item`.
    ///
    /// The adapter calls this method only for the pane selected by the current
    /// semantic workspace and proven to occupy the retained semantic content
    /// slot. Bootstrap, stale, and projection-changing content may use retained
    /// geometry and a disabled child UI, but [`Ui::is_enabled`] only describes
    /// egui widget input; it does not isolate arbitrary application mutations in
    /// this callback. A superseded, relocated, or replacement-epoch pane is
    /// therefore never invoked through unrelated retained chrome. Once the pane
    /// projection is current, the child UI remains enabled even when docking
    /// presentation authority is unavailable.
    ///
    /// Caller-owned presentation settlement currently admits synchronous,
    /// append-only painting on this Ui's layer. Drawing into another layer or
    /// changing earlier shapes makes the complete surface output unavailable
    /// for settlement. Respect the supplied opacity and clip, including when a
    /// source gap omits the pane's pixels during manipulation. Do not defer
    /// drawing beyond this callback. Application paint callbacks remain subject
    /// to the host renderer contract; shape comparison cannot inspect them.
    ///
    /// Return ordinary content registrations from this callback through
    /// [`PaneResponse`]. Accepted registrations allow docking hover to pass
    /// through and permit containing-window activation while retaining their
    /// ordinary egui input. The adapter reads the response while the child UI
    /// is still alive; application state need not retain these IDs.
    ///
    /// The adapter accepts enabled responses on the child UI's layer, including
    /// explicitly declared hover-only labels and ordinary content containers.
    /// Their already-clipped interaction rectangles must be finite, positive,
    /// and fully contained within the pane's valid content bounds. A hover-only
    /// declaration preserves its non-interactive sense; it cannot clear an
    /// overlapping unknown interactive widget. Never declare popup, modal, or
    /// foreign-overlay widgets, even if their geometry fits.
    ///
    /// These are explicit host-application ownership facts, not an automatic
    /// widget-tree resolver or a general inference of platform hit eligibility.
    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse;

    /// Returns the stable egui focus identity registered by `item`'s pane UI.
    ///
    /// The adapter uses this identity to request or surrender focus, and to keep
    /// an already focused selected pane alive while its UI is skipped. Merely
    /// selecting the item's tab, rendering the pane, or issuing a request never
    /// acknowledges a core pane-focus intent. In a caller-owned non-root
    /// viewport, terminal confirmation requires this identity to appear in the
    /// viewport's final AccessKit tree; otherwise the adapter reports the
    /// binding as unavailable rather than consulting root-viewport memory.
    fn focus_target(&self, _item: ItemId) -> Option<Id> {
        None
    }

    /// Reports the pane's actual focus state after its UI has rendered.
    ///
    /// The default observes the exact [`Self::focus_target`] in egui memory.
    /// Implementors with a composite focus scope may override this method, but
    /// must return [`PaneFocusState::Unknown`] whenever that scope cannot be
    /// observed authoritatively.
    fn focus_state(&self, item: ItemId, context: &Context) -> PaneFocusState {
        let Some(target) = self.focus_target(item) else {
            return PaneFocusState::Unknown;
        };
        if context.input(|input| input.focused) && context.memory(|memory| memory.has_focus(target))
        {
            PaneFocusState::Focused
        } else {
            PaneFocusState::Unfocused
        }
    }

    /// Returns the pane-specific minimum content size in egui points.
    ///
    /// This metadata is queried for every open tab, including inactive or
    /// unloaded content, so it must not depend on creating the pane UI. Core
    /// needs exact potential-layout minimums before a tab is dragged out.
    ///
    /// The default delegates the minimum floor entirely to
    /// [`DockStyle`](crate::style::DockStyle).
    fn minimum_size(&self, _item: ItemId) -> Vec2 {
        Vec2::ZERO
    }
}

impl<T: PaneView + ?Sized> PaneView for Box<T> {
    fn title(&self, item: ItemId) -> Option<WidgetText> {
        (**self).title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        (**self).ui(item, ui)
    }

    fn tab_content_width(&self, item: ItemId, ui: &Ui, title: &egui::Galley) -> f32 {
        (**self).tab_content_width(item, ui, title)
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) -> TabVisuals {
        (**self).tab_ui(ui, tab)
    }

    fn tab_close_ui(&mut self, ui: &Ui, close: &TabClose<'_>) {
        (**self).tab_close_ui(ui, close);
    }

    fn drag_preview_ui(&mut self, ui: &Ui, preview: &DragPreview) {
        (**self).drag_preview_ui(ui, preview);
    }

    fn tab_bar_layout(&self, bar: TabBarId, ui: &Ui) -> TabBarLayout {
        (**self).tab_bar_layout(bar, ui)
    }

    fn tab_bar_ui(&mut self, ui: &mut Ui, bar: TabBar<'_>, slot: TabBarSlot) {
        (**self).tab_bar_ui(ui, bar, slot);
    }

    fn focus_target(&self, item: ItemId) -> Option<Id> {
        (**self).focus_target(item)
    }

    fn focus_state(&self, item: ItemId, context: &Context) -> PaneFocusState {
        (**self).focus_state(item, context)
    }

    fn minimum_size(&self, item: ItemId) -> Vec2 {
        (**self).minimum_size(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestPanes {
        locked: ItemId,
    }

    impl PaneView for TestPanes {
        fn title(&self, item: ItemId) -> Option<WidgetText> {
            (item != self.locked).then(|| "Pane".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
            PaneResponse::default()
        }
    }

    fn accepts_trait_object(_panes: &mut dyn PaneView) {}

    #[test]
    fn pane_view_is_object_safe_and_reports_missing_items() {
        let locked = ItemId::new(7);
        let mut panes = TestPanes { locked };

        accepts_trait_object(&mut panes);
        assert!(panes.title(locked).is_none());
        assert!(panes.title(ItemId::new(8)).is_some());
    }

    #[test]
    fn default_minimum_size_defers_to_adapter_style() {
        let panes = TestPanes {
            locked: ItemId::new(7),
        };

        assert_eq!(panes.minimum_size(ItemId::new(8)), Vec2::ZERO);
    }

    #[test]
    fn missing_focus_provider_stays_unknown() {
        let panes = TestPanes {
            locked: ItemId::new(7),
        };

        assert_eq!(
            panes.focus_state(ItemId::new(8), &Context::default()),
            PaneFocusState::Unknown
        );
    }

    #[test]
    fn boxed_borrowed_provider_preserves_custom_focus_and_painting() {
        use std::{cell::Cell, rc::Rc};

        struct BorrowedPanes<'a> {
            title: &'a str,
            paints: Rc<Cell<usize>>,
        }

        impl PaneView for BorrowedPanes<'_> {
            fn title(&self, _item: ItemId) -> Option<WidgetText> {
                Some(self.title.into())
            }

            fn ui(&mut self, _item: ItemId, ui: &mut Ui) -> PaneResponse {
                self.paints.set(self.paints.get() + 1);
                PaneResponse::from(&ui.label(self.title))
            }

            fn focus_state(&self, _item: ItemId, _context: &Context) -> PaneFocusState {
                PaneFocusState::Focused
            }

            fn focus_target(&self, item: ItemId) -> Option<Id> {
                Some(Id::unique(("boxed-provider", item)))
            }

            fn minimum_size(&self, _item: ItemId) -> Vec2 {
                Vec2::new(80.0, 40.0)
            }
        }

        let title = String::from("Borrowed provider");
        let paints = Rc::new(Cell::new(0));
        let mut boxed: Box<dyn PaneView + '_> = Box::new(BorrowedPanes {
            title: &title,
            paints: Rc::clone(&paints),
        });
        // Coercing the box itself exercises its implementation, not deref lookup.
        let provider: &mut dyn PaneView = &mut boxed;
        let item = ItemId::new(8);
        let context = Context::default();
        assert_eq!(provider.title(item).unwrap().text(), title);
        assert_eq!(provider.minimum_size(item), Vec2::new(80.0, 40.0));
        assert_eq!(
            provider.focus_target(item),
            Some(Id::unique(("boxed-provider", item)))
        );
        assert_eq!(
            provider.focus_state(item, &context),
            PaneFocusState::Focused
        );
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let response = provider.ui(item, ui);
            assert_eq!(response.content_widgets().count(), 1);
        });
        output.textures_delta.clear();
        assert!(paints.get() > 0);
    }
}
