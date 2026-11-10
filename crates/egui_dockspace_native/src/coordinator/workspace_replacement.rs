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

        for &successor in report.native_bindings() {
            if !self.session.is_current_native_binding(successor) {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    successor.surface(),
                )
                .into());
            }
            if let Some((viewport, _)) = self.bridge.workspace_replacement_predecessor(successor)
                && self
                    .bridge
                    .with_viewports(|viewports| viewports.window_binding(viewport))
                    != Some(successor.window())
            {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    successor.surface(),
                )
                .into());
            }
        }

        let mut rebound = false;
        for (index, &successor) in report.native_bindings().iter().enumerate() {
            if report.native_bindings()[..index]
                .iter()
                .any(|previous| previous.window() == successor.window())
            {
                continue;
            }
            let Some((viewport, predecessor)) =
                self.bridge.workspace_replacement_predecessor(successor)
            else {
                continue;
            };
            self.bridge.clear_create_ack(predecessor);
            if let Some(binding) = self
                .focus_control
                .pending_binding()
                .filter(|binding| binding.window() == predecessor.window())
            {
                self.terminalize_focus(binding);
            }
            // The resource is physical, while already dispatched effects keep
            // their exact placement correlation until their terminal is owned.
            for binding in report
                .native_root_continuations()
                .iter()
                .map(|continuation| continuation.predecessor())
                .filter(|binding| binding.window() == predecessor.window())
                .chain(std::iter::once(predecessor))
            {
                self.bridge
                    .with_close_control(|control| control.retire_binding(binding));
                self.retain_retired_input_results(binding);
            }
            self.pointer_translator.retire_binding(predecessor.window());
            self.receivers.retire_window(predecessor.window());
            self.bridge
                .finish_workspace_replacement(viewport, predecessor);
            rebound = true;
        }
        Ok(rebound)
    }
}
