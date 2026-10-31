//! Continue retained physical root windows across a committed workspace replacement.

use super::*;

impl NativeCoordinator {
    /// Captures the committed document without exposing mutable session access.
    #[cfg(feature = "serde")]
    pub(crate) fn save_document_json(
        &mut self,
    ) -> Result<Vec<u8>, dockspace::runtime::DockspacePersistenceError> {
        self.session.save_document_json()
    }

    /// Retires predecessor sidecars after their native input prefix has settled.
    ///
    /// The bridge prepays and installs exact root successors under its publication
    /// lock. The complete committed roster validates that installation here.
    /// Retired children retain their existing effect and destruction protocol;
    /// old outputs keep their original binding and renderer terminal obligation.
    pub(crate) fn settle_workspace_replacement(
        &mut self,
        report: &HostFrameReport,
    ) -> Result<bool, NativeRuntimeError> {
        let replacement = report.inputs().iter().any(|input| {
            matches!(
                input,
                HostInputOutcome::LayoutReset { .. } | HostInputOutcome::DocumentRestored { .. }
            )
        });
        if !replacement
            && !report.native_bindings().iter().any(|&binding| {
                self.bridge
                    .workspace_replacement_predecessor(binding)
                    .is_some()
            })
        {
            return Ok(false);
        }
        if report.native_input_pending() {
            return Ok(false);
        }
        if self.queued_snapshot.is_some() {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }

        let mut rebound = false;
        for &successor in report.native_bindings() {
            let route_matches = self.bridge.with_viewports(|viewports| {
                let viewport = viewports.viewport(successor.surface())?;
                Some(viewports.binding(viewport) == Some(successor))
            });
            if route_matches.is_none() {
                continue;
            }
            if route_matches == Some(false) || !self.session.is_current_native_binding(successor) {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    successor.surface(),
                )
                .into());
            }
            let Some((viewport, predecessor)) =
                self.bridge.workspace_replacement_predecessor(successor)
            else {
                continue;
            };

            self.bridge.clear_create_ack(predecessor);
            self.bridge.clear_presentation_ack(predecessor);
            self.effects.remove_show(predecessor);
            self.terminalize_focus(predecessor);
            self.bridge
                .with_close_control(|control| control.retire_binding(predecessor));
            self.retain_retired_input_results(predecessor);
            self.pointer_translator.retire_binding(predecessor);
            self.receivers.retire_binding(predecessor);
            self.bridge
                .finish_workspace_replacement(viewport, predecessor);
            rebound = true;
        }
        Ok(rebound)
    }
}
