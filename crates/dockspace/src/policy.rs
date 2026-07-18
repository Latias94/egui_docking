//! Explicit workspace policy without capability or geometry inference.

use std::collections::BTreeSet;

use thiserror::Error;

/// Requested presentation for a root released outside a dock target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TearOffPresentation {
    /// Present the root inside an existing logical surface.
    Contained,
    /// Request a native surface through the platform protocol.
    Native,
}

/// Explicit behavior when a requested native presentation is unavailable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ContainedFallback {
    /// Cancel the operation without changing presentation mode.
    #[default]
    Disabled,
    /// Permit a contained-floating presentation instead.
    Enabled,
}

/// Independently configurable classes of workspace mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockOperation {
    /// Merge or reorder tabs.
    TabMerge,
    /// Split a target at an edge.
    EdgeSplit,
    /// Resize an existing split.
    SplitterResize,
    /// Present a root inside an existing surface.
    ContainedFloating,
    /// Present a root on a native surface.
    NativeSurface,
}

/// Workspace-level permissions for docking mutations.
///
/// Platform capability remains a separate observed fact. Enabling native presentation here does
/// not claim that a renderer can perform it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockPolicy {
    allowed_operations: BTreeSet<DockOperation>,
    contained_fallback: ContainedFallback,
}

impl DockPolicy {
    /// Creates the conservative default docking policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether center/tab-gap merges are allowed.
    pub fn allows_tab_merge(&self) -> bool {
        self.allows(DockOperation::TabMerge)
    }

    /// Enables or disables center/tab-gap merges.
    pub fn set_allow_tab_merge(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::TabMerge, allowed);
    }

    /// Returns whether edge splits are allowed.
    pub fn allows_edge_split(&self) -> bool {
        self.allows(DockOperation::EdgeSplit)
    }

    /// Enables or disables edge splits.
    pub fn set_allow_edge_split(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::EdgeSplit, allowed);
    }

    /// Returns whether splitters may be resized.
    pub fn allows_splitter_resize(&self) -> bool {
        self.allows(DockOperation::SplitterResize)
    }

    /// Enables or disables splitter resizing.
    pub fn set_allow_splitter_resize(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::SplitterResize, allowed);
    }

    /// Returns whether contained-floating presentation is allowed.
    pub fn allows_contained_floating(&self) -> bool {
        self.allows(DockOperation::ContainedFloating)
    }

    /// Enables or disables contained-floating presentation.
    pub fn set_allow_contained_floating(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::ContainedFloating, allowed);
    }

    /// Returns whether native-surface requests are allowed by application policy.
    pub fn allows_native_surfaces(&self) -> bool {
        self.allows(DockOperation::NativeSurface)
    }

    /// Enables or disables native-surface requests.
    pub fn set_allow_native_surfaces(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::NativeSurface, allowed);
    }

    /// Returns the explicit native-to-contained fallback setting.
    pub fn contained_fallback(&self) -> ContainedFallback {
        self.contained_fallback
    }

    /// Sets explicit native-to-contained fallback behavior.
    pub fn set_contained_fallback(&mut self, fallback: ContainedFallback) {
        self.contained_fallback = fallback;
    }

    /// Returns whether one operation class is enabled.
    pub fn allows(&self, operation: DockOperation) -> bool {
        self.allowed_operations.contains(&operation)
    }

    /// Enables or disables one operation class.
    pub fn set_allowed(&mut self, operation: DockOperation, allowed: bool) {
        if allowed {
            self.allowed_operations.insert(operation);
        } else {
            self.allowed_operations.remove(&operation);
        }
    }

    /// Checks a tab merge before constructing a command.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyRejection::TabMergeDisabled`] when disabled.
    pub fn check_tab_merge(&self) -> Result<(), PolicyRejection> {
        self.allows_tab_merge()
            .then_some(())
            .ok_or(PolicyRejection::TabMergeDisabled)
    }

    /// Checks an edge split before constructing a command.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyRejection::EdgeSplitDisabled`] when disabled.
    pub fn check_edge_split(&self) -> Result<(), PolicyRejection> {
        self.allows_edge_split()
            .then_some(())
            .ok_or(PolicyRejection::EdgeSplitDisabled)
    }

    /// Checks splitter resizing before constructing a command.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyRejection::SplitterResizeDisabled`] when disabled.
    pub fn check_splitter_resize(&self) -> Result<(), PolicyRejection> {
        self.allows_splitter_resize()
            .then_some(())
            .ok_or(PolicyRejection::SplitterResizeDisabled)
    }

    /// Checks one explicitly requested tear-off presentation.
    ///
    /// # Errors
    ///
    /// Returns a presentation-specific [`PolicyRejection`] when disabled.
    pub fn check_tear_off(&self, presentation: TearOffPresentation) -> Result<(), PolicyRejection> {
        match presentation {
            TearOffPresentation::Contained if self.allows_contained_floating() => Ok(()),
            TearOffPresentation::Contained => Err(PolicyRejection::ContainedFloatingDisabled),
            TearOffPresentation::Native if self.allows_native_surfaces() => Ok(()),
            TearOffPresentation::Native => Err(PolicyRejection::NativeSurfacesDisabled),
        }
    }

    /// Resolves policy-only fallback after capability rejected a native request.
    ///
    /// This function never inspects pointer position, elapsed time, focus, or window geometry.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyRejection::ContainedFallbackDisabled`] unless fallback is explicitly
    /// enabled, or [`PolicyRejection::ContainedFloatingDisabled`] when contained presentation is
    /// itself disabled.
    pub fn native_unavailable_fallback(&self) -> Result<TearOffPresentation, PolicyRejection> {
        if self.contained_fallback != ContainedFallback::Enabled {
            return Err(PolicyRejection::ContainedFallbackDisabled);
        }
        self.check_tear_off(TearOffPresentation::Contained)?;
        Ok(TearOffPresentation::Contained)
    }
}

impl Default for DockPolicy {
    fn default() -> Self {
        Self {
            allowed_operations: BTreeSet::from([
                DockOperation::TabMerge,
                DockOperation::EdgeSplit,
                DockOperation::SplitterResize,
                DockOperation::ContainedFloating,
            ]),
            contained_fallback: ContainedFallback::Disabled,
        }
    }
}

/// A deterministic policy rejection, independent of renderer capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PolicyRejection {
    /// Center/tab-gap merge is disabled.
    #[error("tab merging is disabled by workspace policy")]
    TabMergeDisabled,
    /// Edge splitting is disabled.
    #[error("edge splitting is disabled by workspace policy")]
    EdgeSplitDisabled,
    /// Splitter resizing is disabled.
    #[error("splitter resizing is disabled by workspace policy")]
    SplitterResizeDisabled,
    /// Contained-floating presentation is disabled.
    #[error("contained-floating presentation is disabled by workspace policy")]
    ContainedFloatingDisabled,
    /// Native surfaces are disabled by application policy.
    #[error("native surfaces are disabled by workspace policy")]
    NativeSurfacesDisabled,
    /// Native-to-contained fallback was not explicitly enabled.
    #[error("contained fallback for unavailable native presentation is disabled")]
    ContainedFallbackDisabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_contained_policies_are_independent() {
        let mut policy = DockPolicy::default();

        assert_eq!(
            policy.check_tear_off(TearOffPresentation::Native),
            Err(PolicyRejection::NativeSurfacesDisabled)
        );
        assert!(
            policy
                .check_tear_off(TearOffPresentation::Contained)
                .is_ok()
        );

        policy.set_allow_native_surfaces(true);
        policy.set_allow_contained_floating(false);
        assert!(policy.check_tear_off(TearOffPresentation::Native).is_ok());
        assert_eq!(
            policy.check_tear_off(TearOffPresentation::Contained),
            Err(PolicyRejection::ContainedFloatingDisabled)
        );
    }

    #[test]
    fn contained_fallback_requires_two_explicit_permissions() {
        let mut policy = DockPolicy::default();
        assert_eq!(
            policy.native_unavailable_fallback(),
            Err(PolicyRejection::ContainedFallbackDisabled)
        );

        policy.set_contained_fallback(ContainedFallback::Enabled);
        assert_eq!(
            policy.native_unavailable_fallback(),
            Ok(TearOffPresentation::Contained)
        );

        policy.set_allow_contained_floating(false);
        assert_eq!(
            policy.native_unavailable_fallback(),
            Err(PolicyRejection::ContainedFloatingDisabled)
        );
    }
}
