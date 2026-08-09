//! Construction of the egui docking facade.

use std::{fmt::Debug, hash::Hash};

#[cfg(any(feature = "backend", test))]
use dockspace::backend::graph::Workspace;
use dockspace::model::DockspaceLayout;
use dockspace::policy::DockPolicy;
use egui::Id;

use crate::error::DockspaceError;
use crate::facade::Dockspace;
use crate::style::DockStyle;

/// Builder for one authoritative egui docking workspace.
pub struct DockspaceBuilder {
    id: Id,
    source: DockspaceBuilderSource,
    policy: DockPolicy,
    style: DockStyle,
}

enum DockspaceBuilderSource {
    Layout(DockspaceLayout),
    #[cfg(any(feature = "backend", test))]
    BackendWorkspace(Workspace),
}

impl DockspaceBuilder {
    /// Starts a builder with a stable egui identity and product layout.
    pub fn new(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> Self {
        Self {
            id: Id::new(("egui_dockspace", id_salt)),
            source: DockspaceBuilderSource::Layout(layout),
            policy: DockPolicy::default(),
            style: DockStyle::default(),
        }
    }

    #[cfg(any(feature = "backend", test))]
    pub(crate) fn from_backend_workspace(id_salt: impl Hash + Debug, workspace: Workspace) -> Self {
        Self {
            id: Id::new(("egui_dockspace", id_salt)),
            source: DockspaceBuilderSource::BackendWorkspace(workspace),
            policy: DockPolicy::default(),
            style: DockStyle::default(),
        }
    }

    /// Replaces the complete application docking policy.
    #[must_use]
    pub fn policy(mut self, policy: DockPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Replaces fixed renderer geometry and colors.
    #[must_use]
    pub fn style(mut self, style: DockStyle) -> Self {
        self.style = style;
        self
    }

    /// Validates all construction inputs and creates the facade.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError`] when style, layout, or backend workspace validation fails.
    pub fn build(self) -> Result<Dockspace, DockspaceError> {
        self.style.validate().map_err(DockspaceError::from_detail)?;
        match self.source {
            DockspaceBuilderSource::Layout(layout) => {
                Dockspace::from_layout_parts(self.id, layout, self.policy, self.style)
            }
            #[cfg(any(feature = "backend", test))]
            DockspaceBuilderSource::BackendWorkspace(workspace) => {
                Dockspace::from_parts(self.id, workspace, self.policy, self.style)
            }
        }
    }
}
