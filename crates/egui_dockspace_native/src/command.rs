//! Application intents prepared by the native runtime at admission.

use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use dockspace::model::{DockPlacement, ItemId, NativeWindowPlacement, RootId, SurfaceId};

/// One application intent for [`NativeDockspaceApp::request_command`](crate::NativeDockspaceApp::request_command).
///
/// Commands are ordinary copyable values that menus and command palettes may
/// retain. Admission prepares them against the current session under its lock;
/// a stored command is not a prepared action or presentation authority.
/// Consume [`NativeDockspaceApp::take_action_status`](crate::NativeDockspaceApp::take_action_status)
/// before submitting another command. Pane state always stays in the application.
///
/// ```
/// use dockspace::model::ItemId;
/// use egui_dockspace_native::{NativeActionRequestError, NativeCommand, NativeDockspaceApp};
/// # fn raise<P: egui_dockspace::PaneView + Send + 'static>(app: &NativeDockspaceApp<P>) -> Result<(), NativeActionRequestError> {
/// let command = NativeCommand::RaiseContained { item: ItemId::new(1) };
/// app.request_command(command)?;
/// // Consume take_action_status on a later application update before submitting more.
/// # Ok(())
/// # }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeCommand {
    /// Select an open pane and request its focus through the core.
    ///
    /// An eligible native window is explicitly activated, including when its pane
    /// is already selected. Native and pane focus still require host observations.
    SelectItem {
        /// Pane to select.
        item: ItemId,
    },
    /// Open a closed pane; an already-open pane keeps its location and selection.
    OpenItem {
        /// Application-owned pane identity.
        item: ItemId,
        /// Placement for newly opened content.
        placement: DockPlacement,
    },
    /// Move one open pane to an explicit docking placement.
    DockItem {
        /// Pane to move.
        item: ItemId,
        /// Destination in the current workspace.
        placement: DockPlacement,
    },
    /// Dock an entire root as one group.
    DockRoot {
        /// Complete group to move.
        root: RootId,
        /// Destination in the current workspace.
        placement: DockPlacement,
    },
    /// Recover a complete contained or native root to its core-derived destination.
    DockRootBack {
        /// Group whose recovery target the core revalidates.
        root: RootId,
    },
    /// Float one pane inside an existing logical surface, using current geometry.
    ///
    /// This creates contained content, not an OS window. A complete singleton
    /// preserves its root identity; part of a larger group receives a new root.
    FloatItem {
        /// Pane to float.
        item: ItemId,
        /// Existing surface that will contain the floating window.
        surface: SurfaceId,
    },
    /// Raise the contained window owning this pane without selecting the pane.
    RaiseContained {
        /// Any pane in the contained group.
        item: ItemId,
    },
    /// Clamp a contained window into current visible bounds without raising it.
    ///
    /// This explicit command changes durable geometry. Passive resizing does not
    /// perform this recovery; current surface measurement must be available.
    BringContainedIntoView {
        /// Any pane in the contained group.
        item: ItemId,
    },
    /// Begin moving a complete root to a newly managed OS window.
    ///
    /// Source ownership remains until the child's exact first-live presentation.
    MoveRootToNewWindow {
        /// Complete group to promote.
        root: RootId,
        /// Requested desktop-physical outer bounds.
        placement: NativeWindowPlacement,
    },
    /// Request a pane close without approving the application's decision.
    CloseItem {
        /// Pane for the core-owned close plan.
        item: ItemId,
    },
    /// Request an atomic close plan for every pane in a complete root.
    CloseRoot {
        /// Complete group for the close plan.
        root: RootId,
    },
    /// Answer an exact initial token from a core-owned close plan.
    DecideClose {
        /// Core-issued close request.
        request: CloseRequestId,
        /// Exact pending item decision.
        token: CloseDecisionToken,
        /// Explicit application decision.
        decision: CloseDecision,
    },
    /// Continue an exact deferred close decision.
    ContinueClose {
        /// Core-issued close request.
        request: CloseRequestId,
        /// Exact deferred continuation.
        token: DeferredCloseToken,
        /// Explicit continuation decision.
        decision: DeferredCloseDecision,
    },
    /// Change arrangement locking while preserving selection and content input.
    SetArrangementLocked {
        /// Requested lock state.
        locked: bool,
    },
}
