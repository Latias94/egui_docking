//! Default egui facade backed by one renderer-neutral [`DockspaceSession`].

use std::collections::BTreeSet;
use std::{fmt::Debug, hash::Hash};

use dockspace::model::{DockspaceLayout, DockspaceView, ItemId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, SurfaceUnavailableReason, WorkspaceVersion,
};
use egui::Id;

use crate::error_detail::DockspaceErrorSource;
use crate::pass_settlement::ProductPassSettlement;
#[cfg(test)]
use crate::render;
use crate::response::{
    DockspaceCapability, DockspaceMutation, DockspaceResponse, DockspaceSurfaceStatus,
};
use crate::{DockInputConfig, DockStyle, DockspaceBuilder, DockspaceError};

mod actions;
mod callback;
mod owned_run;
#[cfg(feature = "serde")]
mod persistence;

#[cfg(feature = "motion")]
mod motion;

#[cfg(test)]
mod chrome_tests;

/// Stateful egui adapter whose headless session is the sole docking authority.
pub struct Dockspace {
    id: Id,
    session: DockspaceSession,
    style: DockStyle,
    pass_settlement: ProductPassSettlement,
    pending_focus: crate::focus::PendingFocusObservations,
    pointer: crate::pointer::ProductPointer,
    ui_actions: owned_run::UiActionQueue,
    #[cfg(feature = "motion")]
    motion_settings: dockspace::motion::MotionSettings,
    #[cfg(feature = "motion")]
    motion_preference: dockspace::motion::MotionPreference,
    #[cfg(test)]
    drop_target_observations: Vec<render::RegisteredDropTargetObservation>,
}

impl Dockspace {
    /// Starts a builder for a stable egui instance and product docking layout.
    pub fn builder(id_salt: impl Hash + Debug, layout: DockspaceLayout) -> DockspaceBuilder {
        DockspaceBuilder::new(id_salt, layout)
    }

    fn from_session(id: Id, session: DockspaceSession, style: DockStyle) -> Self {
        Self {
            id,
            session,
            style,
            pass_settlement: ProductPassSettlement::default(),
            pending_focus: crate::focus::PendingFocusObservations::default(),
            pointer: crate::pointer::ProductPointer::default(),
            ui_actions: owned_run::UiActionQueue::default(),
            #[cfg(feature = "motion")]
            motion_settings: dockspace::motion::MotionSettings::default(),
            #[cfg(feature = "motion")]
            motion_preference: dockspace::motion::MotionPreference::Unknown,
            #[cfg(test)]
            drop_target_observations: Vec::new(),
        }
    }

    pub(crate) fn from_layout_parts(
        id: Id,
        layout: DockspaceLayout,
        policy: DockPolicy,
        style: DockStyle,
    ) -> Result<Self, DockspaceError> {
        let presentation = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let session =
            DockspaceSession::from_layout_with_presentation_config(layout, policy, presentation)
                .map_err(DockspaceError::from_detail)?;
        Ok(Self::from_session(id, session, style))
    }

    /// Returns the stable egui identity used to scope adapter widgets.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Returns the published item/surface-centric workspace view.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        self.session.view()
    }

    /// Returns the durable workspace version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.session.version()
    }

    /// Returns whether the committed workspace owns a pointer press, drag or resize.
    ///
    /// Applications can use this read-only summary when coordinating other
    /// gestures. It grants no receiver, input-delivery or presentation authority.
    #[must_use]
    pub fn has_active_gesture(&self) -> bool {
        self.session.has_active_gesture()
    }

    /// Returns the current declarative docking policy.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        self.session.policy()
    }

    /// Returns the current renderer style.
    #[must_use]
    pub const fn style(&self) -> &DockStyle {
        &self.style
    }

    /// Replaces the complete declarative docking policy atomically.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the published policy when the host frame cannot commit.
    pub fn set_policy(&mut self, policy: DockPolicy) -> Result<DockspaceMutation, DockspaceError> {
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .replace_policy(policy)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        Ok(DockspaceMutation::from_runtime_report(&report))
    }

    /// Atomically applies an application-owned default or preset layout.
    ///
    /// This path needs no serialization or document lineage. Pane data, item-key
    /// history, configured policy and renderer style remain application/session
    /// owned. Retired presentation identities are rebased by core before commit.
    ///
    /// # Errors
    ///
    /// Returns an error without partial publication when the arrangement is
    /// locked, a document-bound item is unknown, or the replacement cannot commit.
    pub fn reset_layout(
        &mut self,
        layout: DockspaceLayout,
    ) -> Result<DockspaceMutation, DockspaceError> {
        let reset = self
            .session
            .prepare_layout_reset(layout)
            .map_err(DockspaceError::from_detail)?;
        let mut frame = self
            .session
            .begin_layout_reset_frame(&reset)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.pending_focus.clear();
        Ok(DockspaceMutation::from_runtime_report(&report))
    }

    /// Returns the adapter's docking activation and cancellation bindings.
    #[must_use]
    pub const fn input(&self) -> DockInputConfig {
        self.pointer.input()
    }

    /// Replaces gesture bindings at the next input boundary.
    ///
    /// An active pointer retains its last observed modifiers. The next run
    /// recomputes docking intent without inventing pointer motion or restoring
    /// a previously cleared target.
    pub fn set_input(&mut self, input: DockInputConfig) {
        self.pointer.set_input(input);
    }

    /// Replaces renderer geometry and visual styling atomically.
    ///
    /// Geometry changes commit through core before the adapter publishes the new style. A
    /// visual-only change remains adapter-local, so it cannot disturb an in-flight gesture or
    /// presentation settlement. Failed validation or geometry publication leaves both
    /// authorities unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the style is invalid or the host frame cannot commit.
    pub fn set_style(&mut self, style: DockStyle) -> Result<DockspaceMutation, DockspaceError> {
        let presentation = style
            .presentation_config_with(self.session.presentation_config())
            .map_err(DockspaceError::from_detail)?;
        let style_changed = self.style != style;
        let presentation_changed = self.session.presentation_config() != &presentation;
        if !presentation_changed {
            self.style = style;
            let mut mutation = DockspaceMutation::unchanged(self.session.version());
            if style_changed {
                mutation.include_adapter_presentation_change(
                    self.session.view().surfaces().map(|surface| surface.id()),
                );
            }
            return Ok(mutation);
        }
        let mut frame = self
            .session
            .begin_host_frame()
            .map_err(DockspaceError::from_detail)?;
        frame
            .replace_presentation_config(presentation)
            .map_err(DockspaceError::from_detail)?;
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .map_err(DockspaceError::from_detail)?;
        let report = frame.commit().map_err(DockspaceError::from_detail)?;
        self.style = style;
        let mut mutation = DockspaceMutation::from_runtime_report(&report);
        if style_changed {
            mutation.include_adapter_presentation_change(
                self.session.view().surfaces().map(|surface| surface.id()),
            );
        }
        Ok(mutation)
    }

    #[cfg(test)]
    fn registered_drop_target_observations(&self) -> &[render::RegisteredDropTargetObservation] {
        &self.drop_target_observations
    }
}

#[cfg(test)]
mod tests;

fn product_response(
    report: &HostFrameReport,
    surface: SurfaceId,
    missing: BTreeSet<ItemId>,
    local_actions_current: bool,
    had_plan: bool,
    presentation_settlement_capability: DockspaceCapability,
    pane_focus_capability: DockspaceCapability,
) -> Result<DockspaceResponse, DockspaceError> {
    DockspaceResponse::from_product_report(
        report,
        surface,
        missing.into_iter().collect(),
        local_actions_current,
        if had_plan {
            DockspaceSurfaceStatus::Ready
        } else {
            DockspaceSurfaceStatus::Bootstrap
        },
        presentation_settlement_capability,
        pane_focus_capability,
    )
    .ok_or(DockspaceErrorSource::ApplicationOutcomeUnavailable {
        operation: "surface contribution",
    })
    .map_err(Into::into)
}
