//! Structured output from one egui docking frame.

use dockspace::error::CommandError;
use dockspace::ids::ItemId;
use dockspace::scene::SceneStamp;
use dockspace::transition::EngineTransition;
use dockspace::transition::WorkspaceVersion;

/// Truthful availability of one adapter operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCapability {
    /// Every application and platform prerequisite is currently available.
    Supported,
    /// The operation is disabled for an explicit, queryable reason.
    Unavailable(DockspaceUnavailableReason),
}

/// Lifecycle state of the logical surface requested by one `show` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceSurfaceStatus {
    /// The surface exists and was projected into a sealed, ready scene.
    Ready,
    /// The surface is not owned by the current workspace.
    ///
    /// This is an ordinary result after closing the final non-central pane of
    /// a root. The adapter deliberately paints nothing instead of treating the
    /// completed lifecycle transition as a projection failure.
    Absent,
}

/// Why semantic actions captured by a prior paint were not reduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceInputRejection {
    /// Application state advanced before the captured UI actions reached a boundary.
    StaleWorkspace {
        /// Workspace version painted by the renderer.
        expected: WorkspaceVersion,
        /// Workspace version current after application inputs were reduced.
        current: WorkspaceVersion,
        /// Number of semantic actions discarded as one atomic batch.
        dropped_actions: usize,
    },
    /// A different sealed geometry generation replaced the one that captured the actions.
    StaleScene {
        /// Scene generation painted by the renderer.
        expected: SceneStamp,
        /// Currently published scene, or `None` when publication was invalidated.
        current: Option<SceneStamp>,
        /// Number of semantic actions discarded as one atomic batch.
        dropped_actions: usize,
    },
}

/// Why an egui adapter operation is unavailable in the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceUnavailableReason {
    /// Local undocked releases are explicitly configured to cancel.
    TearOffModeDisabled,
    /// Application policy disables contained-floating presentation.
    ContainedPolicyDisabled,
    /// No application-owned stable presentation identity source was installed.
    PresentationIdSourceMissing,
    /// The identity source returned no identity for this request.
    PresentationIdsExhausted,
    /// The identity source returned an identity already owned by the workspace.
    PresentationIdentityCollision,
    /// No ready sealed scene can authorize placement on this surface.
    SurfaceBoundsUnavailable,
    /// The requested logical surface is not owned by the current workspace.
    SurfaceAbsent,
}

/// Published engine boundaries and renderer diagnostics from one `show` call.
#[derive(Debug)]
pub struct DockspaceResponse {
    pub(crate) transitions: Vec<EngineTransition>,
    pub(crate) missing_panes: Vec<ItemId>,
    pub(crate) capture_errors: Vec<CommandError>,
    pub(crate) input_rejections: Vec<DockspaceInputRejection>,
    pub(crate) interactions_current: bool,
    pub(crate) surface_status: DockspaceSurfaceStatus,
    pub(crate) contained_capability: DockspaceCapability,
}

impl DockspaceResponse {
    /// Returns atomic engine publications performed before this paint.
    #[must_use]
    pub fn transitions(&self) -> &[EngineTransition] {
        &self.transitions
    }

    /// Returns stable item identities missing from the application pane registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        &self.missing_panes
    }

    /// Returns exact source/target capture errors observed while registering widgets.
    #[must_use]
    pub fn capture_errors(&self) -> &[CommandError] {
        &self.capture_errors
    }

    /// Returns stale renderer batches rejected before application callbacks ran.
    #[must_use]
    pub fn input_rejections(&self) -> &[DockspaceInputRejection] {
        &self.input_rejections
    }

    /// Returns whether egui's prior-pass hit geometry exactly matched this projection.
    ///
    /// A false result is an observation-only pass: docking geometry input is
    /// rejected, [`PaneView::ui`](crate::PaneView::ui) is not called, and the
    /// adapter requests an egui discard. The host may decline that request when
    /// its configured pass budget is exhausted.
    #[must_use]
    pub const fn interactions_current(&self) -> bool {
        self.interactions_current
    }

    /// Returns whether the requested surface was projected or is now absent.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.surface_status
    }

    /// Returns current contained-floating tear-off availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.contained_capability
    }
}
