//! Application-owned pane rendering contract.

use dockspace::ids::ItemId;
use egui::{Ui, Vec2, WidgetText};

/// The application's decision for a requested pane close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneCloseResponse {
    /// The adapter may submit the close command to the docking engine.
    Allow,
    /// The pane remains open and no close command is submitted.
    Veto,
}

/// Resolves and renders application-owned panes by stable item identity.
///
/// The adapter borrows this object only while painting a frame. Implementors
/// retain ownership of pane state; the docking workspace stores only
/// [`ItemId`] values.
pub trait PaneView {
    /// Returns the current title for `item`.
    ///
    /// Returning `None` reports that the application registry has no entry for
    /// the item. The adapter can then render a recoverable missing-pane state.
    fn title(&self, item: ItemId) -> Option<WidgetText>;

    /// Renders the application content associated with `item`.
    ///
    /// The adapter does not call this method during an observation-only pass
    /// whose prior egui hit graph no longer matches the current projection.
    /// This fail-closed boundary prevents stale pointer or accessibility input
    /// from reaching application widgets. A later current pass renders the
    /// pane normally.
    fn ui(&mut self, item: ItemId, ui: &mut Ui);

    /// Returns whether the pane exposes a close action.
    fn closeable(&self, _item: ItemId) -> bool {
        true
    }

    /// Handles a close request before the adapter submits a workspace command.
    ///
    /// By default, closeable panes allow the request and non-closeable panes
    /// veto it. Treat this callback as a decision only: pane ownership must be
    /// released after the resulting engine transition confirms the close.
    ///
    /// A complete-root close calls this method exactly once for every pane in
    /// stable tree order, even when another pane vetoes the same atomic close.
    /// Implementors may override it for application-specific save prompts or
    /// lifecycle policy.
    fn close(&mut self, item: ItemId) -> PaneCloseResponse {
        if self.closeable(item) {
            PaneCloseResponse::Allow
        } else {
            PaneCloseResponse::Veto
        }
    }

    /// Returns the pane-specific minimum content size in egui points.
    ///
    /// The default delegates the minimum floor entirely to
    /// [`DockStyle`](crate::style::DockStyle).
    fn minimum_size(&self, _item: ItemId) -> Vec2 {
        Vec2::ZERO
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

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}

        fn closeable(&self, item: ItemId) -> bool {
            item != self.locked
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
    fn default_close_respects_closeability() {
        let locked = ItemId::new(7);
        let mut panes = TestPanes { locked };

        assert_eq!(panes.close(locked), PaneCloseResponse::Veto);
        assert_eq!(panes.close(ItemId::new(8)), PaneCloseResponse::Allow);
    }

    #[test]
    fn default_minimum_size_defers_to_adapter_style() {
        let panes = TestPanes {
            locked: ItemId::new(7),
        };

        assert_eq!(panes.minimum_size(ItemId::new(8)), Vec2::ZERO);
    }
}
