//! Construction of the egui docking facade.

use std::{fmt::Debug, hash::Hash};

use dockspace::model::DockspaceLayout;
use dockspace::policy::DockPolicy;
#[cfg(feature = "serde")]
use dockspace::runtime::DockspaceDocumentBootstrap;
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
    #[cfg(feature = "serde")]
    document: Option<DockspaceDocumentBootstrap>,
}

enum DockspaceBuilderSource {
    Layout(DockspaceLayout),
}

impl DockspaceBuilder {
    /// Starts a builder with a stable egui identity and product layout.
    pub fn new(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> Self {
        Self {
            id: Id::new(("egui_dockspace", id_salt)),
            source: DockspaceBuilderSource::Layout(layout),
            policy: DockPolicy::default(),
            style: DockStyle::default(),
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
        self.style.validate().map_err(DockspaceError::from_detail)?;
        match self.source {
            DockspaceBuilderSource::Layout(layout) => {
                #[cfg(feature = "serde")]
                if let Some(document) = self.document {
                    return Dockspace::from_persistent_layout_parts(
                        self.id,
                        layout,
                        self.policy,
                        self.style,
                        document,
                    );
                }
                Dockspace::from_layout_parts(self.id, layout, self.policy, self.style)
            }
        }
    }
}
