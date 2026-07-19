//! Application-owned stable identity allocation for new presentations.

use dockspace::ids::{FloatingPresentationId, RootId};

/// Explicit renderer behavior when a local drag has no docking target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TearOffMode {
    /// Cancel the undocked release without allocating a new presentation.
    #[default]
    Disabled,
    /// Request a contained-floating presentation on the current logical surface.
    Contained,
}

/// Stable identities required to detach content into a new contained root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainedPresentationIds {
    /// Stable identity of the new docking root.
    pub root: RootId,
    /// Stable identity of the new contained presentation.
    pub floating: FloatingPresentationId,
}

impl ContainedPresentationIds {
    /// Creates an explicit pair of application-owned identities.
    #[must_use]
    pub const fn new(root: RootId, floating: FloatingPresentationId) -> Self {
        Self { root, floating }
    }
}

/// Supplies stable identities when a drag explicitly requests contained tear-off.
///
/// The adapter asks at most once for a new drag proposal and validates the
/// returned identities against the complete live workspace. It never scans for
/// a maximum identity, retries collisions, or infers an allocation namespace.
pub trait PresentationIdSource {
    /// Returns the next root/presentation pair, or `None` when allocation is unavailable.
    fn next_contained(&mut self) -> Option<ContainedPresentationIds>;
}

impl<F> PresentationIdSource for F
where
    F: FnMut() -> Option<ContainedPresentationIds>,
{
    fn next_contained(&mut self) -> Option<ContainedPresentationIds> {
        self()
    }
}
