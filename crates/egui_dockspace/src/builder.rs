//! Construction of the egui docking facade.

use std::{fmt::Debug, hash::Hash};

use dockspace::model::DockspaceLayout;
use dockspace::policy::DockPolicy;
#[cfg(feature = "serde")]
use dockspace::runtime::DockspaceDocumentBootstrap;
use egui::Id;

use crate::DockInputConfig;
use crate::error::DockspaceError;
use crate::facade::Dockspace;
use crate::style::DockStyle;

/// Builder for one authoritative egui docking workspace.
#[must_use = "call build() to validate the configuration and create the docking workspace"]
pub struct DockspaceBuilder {
    id: Id,
    layout: DockspaceLayout,
    policy: DockPolicy,
    style: DockStyle,
    input: DockInputConfig,
    #[cfg(feature = "serde")]
    document: Option<DockspaceDocumentBootstrap>,
}

impl DockspaceBuilder {
    /// Starts a builder with a stable egui identity and product layout.
    pub fn new(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> Self {
        Self {
            id: Id::unique(("egui_dockspace", id_salt)),
            layout,
            policy: DockPolicy::default(),
            style: DockStyle::default(),
            input: DockInputConfig::default(),
            #[cfg(feature = "serde")]
            document: None,
        }
    }

    /// Replaces the complete application docking policy.
    #[must_use]
    pub fn policy(mut self, policy: DockPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Replaces fixed renderer geometry and egui-relative visual overrides.
    #[must_use]
    pub fn style(mut self, style: DockStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces docking activation and gesture-cancellation bindings.
    #[must_use]
    pub fn input(mut self, input: DockInputConfig) -> Self {
        self.input = input;
        self
    }

    /// Binds the product layout to one session-owned document lineage.
    ///
    /// Every item in the layout must have been allocated through `bootstrap`.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn persistence(mut self, bootstrap: DockspaceDocumentBootstrap) -> Self {
        self.document = Some(bootstrap);
        self
    }

    /// Validates all construction inputs and creates the facade.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceError`] when style or layout validation fails.
    pub fn build(self) -> Result<Dockspace, DockspaceError> {
        #[cfg(feature = "serde")]
        if let Some(document) = self.document {
            let mut dockspace = Dockspace::from_persistent_layout_parts(
                self.id,
                self.layout,
                self.policy,
                self.style,
                document,
            )?;
            dockspace.set_input(self.input);
            return Ok(dockspace);
        }
        let mut dockspace =
            Dockspace::from_layout_parts(self.id, self.layout, self.policy, self.style)?;
        dockspace.set_input(self.input);
        Ok(dockspace)
    }
}
