//! Construction of the egui docking facade.

use std::{fmt::Debug, hash::Hash};

use dockspace::graph::Workspace;
use dockspace::policy::DockPolicy;
use egui::Id;

use crate::error::DockspaceError;
use crate::facade::Dockspace;
use crate::style::DockStyle;

/// Builder for one authoritative egui docking workspace.
pub struct DockspaceBuilder {
    id: Id,
    workspace: Workspace,
    policy: DockPolicy,
    style: DockStyle,
}

impl DockspaceBuilder {
    /// Starts a builder with a stable egui identity and validated workspace candidate.
    pub fn new(id_salt: impl Hash + Debug, workspace: Workspace) -> Self {
        Self {
            id: Id::new(("egui_dockspace", id_salt)),
            workspace,
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
    /// Returns [`DockspaceError`] when style or workspace validation fails.
    pub fn build(self) -> Result<Dockspace, DockspaceError> {
        self.style.validate()?;
        Dockspace::from_parts(self.id, self.workspace, self.policy, self.style)
    }
}
