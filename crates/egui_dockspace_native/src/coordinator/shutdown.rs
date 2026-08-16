//! Fatal-session drain over the existing native host-frame protocol.

use dockspace::model::SurfaceId;
use dockspace::runtime::{HostInputOutcome, NativeDispatchFailure, NativeReceiverAnswer};
use eframe::NativeOutputToken;
use eframe::egui::{ViewportCommand, ViewportId};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeShutdownRegistration {
    Registered(NativeSurfaceBinding),
    Rejected(SurfaceId),
}

#[derive(Debug)]
pub(crate) struct NativeShutdownAdvance {
    pub(crate) progress: bool,
    pub(crate) registrations: Vec<NativeShutdownRegistration>,
    pub(crate) commands: Vec<(ViewportId, ViewportCommand)>,
    pub(crate) pointer_passthrough: Option<NativePointerPassthroughCommand>,
    pub(crate) abandoned_outputs: Vec<NativeOutputToken>,
}

impl NativeCoordinator {
    pub(crate) fn advance_shutdown_boundary(
        &mut self,
    ) -> Result<NativeShutdownAdvance, NativeRuntimeError> {
        let output_prefix_pending = !self.bridge.output_prefix().is_empty();
        let quiescence_recorded = self.try_report_retirement_quiescence()?;
        let reduced_callback = self.reduce_callback_head()?;
        let prepared_retirements = self.prepare_committed_retirements()?;

        let mut host_frame = self.begin_host_frame(|_| NativeReceiverAnswer::Unknown)?;
        host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let mut report = host_frame.commit()?;

        let registrations = report
            .inputs()
            .iter()
            .filter_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => {
                    Some(NativeShutdownRegistration::Registered(*binding))
                }
                HostInputOutcome::NativeSurfaceRegistrationRejected { surface } => {
                    Some(NativeShutdownRegistration::Rejected(*surface))
                }
                _ => None,
            })
            .collect();
        let had_inputs = !report.inputs().is_empty();
        let native_snapshot_applied = self.settle_host_frame_inputs(report.inputs());
        let native_close_settled = self.settle_close_control_inputs(report.inputs())?;
        let native_admission_settled = self.settle_native_admissions(report.native_admissions())?;
        let retirement_committed = prepared_retirements.is_some();
        let abandoned_outputs = prepared_retirements
            .map_or_else(Vec::new, |prepared| self.commit_retirements(prepared));
        let repaint_requested = !report.repaint_surfaces().is_empty();
        let effects = report.take_native_effects();
        let effects_emitted = !effects.is_empty();
        self.fail_shutdown_effects(effects)?;
        let commands = self.take_viewport_commands();
        let pointer_passthrough = self.pointer_passthrough_command();
        let commands_pending = !commands.is_empty();
        let pointer_passthrough_pending = pointer_passthrough.is_some();

        Ok(NativeShutdownAdvance {
            progress: output_prefix_pending
                || quiescence_recorded
                || reduced_callback
                || had_inputs
                || native_snapshot_applied
                || native_close_settled
                || native_admission_settled
                || retirement_committed
                || !abandoned_outputs.is_empty()
                || effects_emitted
                || commands_pending
                || pointer_passthrough_pending
                || repaint_requested,
            registrations,
            commands,
            pointer_passthrough,
            abandoned_outputs,
        })
    }

    fn fail_shutdown_effects(
        &mut self,
        requests: Vec<NativeEffectRequest>,
    ) -> Result<(), NativeRuntimeError> {
        for request in requests {
            let binding = request.operation().binding();
            let result = request.dispatch_failed(NativeDispatchFailure::ProviderStopped);
            if let Err(error) = self.session.report_native_effect_result(result) {
                self.retain_or_return_effect_result(binding, error)?;
            }
        }
        Ok(())
    }
}
