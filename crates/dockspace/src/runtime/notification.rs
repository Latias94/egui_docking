//! Application notifications from one successfully published core frame.

use crate::transition::{EngineTransition, InputOutcome, WorkspaceVersion};

use super::DockspaceEvent;

/// Durable docking-layout changes committed together in one host frame.
///
/// This includes split ratios, selection/MRU, contained bounds, membership and
/// layout replacement, including changes without a detailed lifecycle event.
/// Rendering, focus observations, scroll position and pending native transfers
/// do not independently change the durable docking layout. OS window geometry
/// remains a separate host observation and is not represented by this version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockspaceLayoutChange {
    before: WorkspaceVersion,
    after: WorkspaceVersion,
}

impl DockspaceLayoutChange {
    /// Returns the published layout version before this complete frame.
    #[must_use]
    pub const fn before(self) -> WorkspaceVersion {
        self.before
    }

    /// Returns the published layout version after this complete frame.
    #[must_use]
    pub const fn after(self) -> WorkspaceVersion {
        self.after
    }
}

/// Read-only application notification from one committed core publication.
///
/// Layout and native-window placement summaries accompany detailed lifecycle events.
/// Empty notifications need no application work. Applications own persistence,
/// debouncing and delivery; reading or cloning this value does not replay input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DockspaceNotification {
    layout_change: Option<DockspaceLayoutChange>,
    native_window_placement_changed: bool,
    events: Vec<DockspaceEvent>,
}

impl DockspaceNotification {
    pub(super) fn from_transition(transition: &EngineTransition, native_admission: bool) -> Self {
        Self {
            layout_change: transition.changed().then(|| DockspaceLayoutChange {
                before: transition.before(),
                after: transition.after(),
            }),
            native_window_placement_changed: native_admission
                || transition.reduced_inputs().iter().any(|input| {
                    matches!(
                        input.outcome(),
                        InputOutcome::PlatformSnapshotPublished { transition, .. }
                            if transition.native_window_placement_changed()
                    )
                }),
            events: super::events::committed_events(transition),
        }
    }

    /// Returns all durable docking-layout changes in this publication as one summary.
    #[must_use]
    pub const fn layout_change(&self) -> Option<DockspaceLayoutChange> {
        self.layout_change
    }

    /// Returns whether committed native-window placement can affect document capture.
    ///
    /// This covers known outer bounds, content size, native DPI scale, first-live
    /// admission and placement becoming authoritative after an observation gap or
    /// hidden interval. It does not advance the docking-layout version. Renderer
    /// zoom, clipping, focus and unavailable geometry do not independently trigger it.
    #[must_use]
    pub const fn native_window_placement_changed(&self) -> bool {
        self.native_window_placement_changed
    }

    /// Returns whether docking layout or native placement may need saving.
    ///
    /// Applications can debounce this signal before capturing a document. The
    /// notification does not save data or compare against an earlier saved file.
    #[must_use]
    pub const fn document_changed(&self) -> bool {
        self.layout_change.is_some() || self.native_window_placement_changed
    }

    /// Returns the publication's detailed product changes in core event order.
    #[must_use]
    pub fn events(&self) -> &[DockspaceEvent] {
        &self.events
    }

    /// Returns whether this publication requires no application notification.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.document_changed() && self.events.is_empty()
    }
}
