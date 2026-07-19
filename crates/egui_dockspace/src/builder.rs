//! Construction of the egui docking facade.

use std::{fmt::Debug, hash::Hash};

use dockspace::graph::Workspace;
use dockspace::policy::DockPolicy;
use egui::Id;

use crate::error::DockspaceError;
use crate::facade::Dockspace;
use crate::presentation::{PresentationIdSource, TearOffMode};
use crate::style::DockStyle;

/// Builder for one authoritative egui docking workspace.
pub struct DockspaceBuilder {
    id: Id,
    workspace: Workspace,
    policy: DockPolicy,
    style: DockStyle,
    tear_off_mode: TearOffMode,
    presentation_ids: Option<Box<dyn PresentationIdSource>>,
}

impl DockspaceBuilder {
    /// Starts a builder with a stable egui identity and validated workspace candidate.
    pub fn new(id_salt: impl Hash + Debug, workspace: Workspace) -> Self {
        Self {
            id: Id::new(("egui_dockspace", id_salt)),
            workspace,
            policy: DockPolicy::default(),
            style: DockStyle::default(),
            tear_off_mode: TearOffMode::default(),
            presentation_ids: None,
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

    /// Selects explicit renderer behavior for a drag released outside docking targets.
    #[must_use]
    pub fn tear_off_mode(mut self, mode: TearOffMode) -> Self {
        self.tear_off_mode = mode;
        self
    }

    /// Installs the explicit stable identity source used by contained tear-off.
    #[must_use]
    pub fn presentation_ids(mut self, source: impl PresentationIdSource + 'static) -> Self {
        self.presentation_ids = Some(Box::new(source));
        self
    }

    /// Validates all construction inputs and creates the facade.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError`] when style or workspace validation fails.
    pub fn build(self) -> Result<Dockspace, DockspaceError> {
        self.style.validate()?;
        Dockspace::from_parts(
            self.id,
            self.workspace,
            self.policy,
            self.style,
            self.tear_off_mode,
            self.presentation_ids,
        )
    }
}
