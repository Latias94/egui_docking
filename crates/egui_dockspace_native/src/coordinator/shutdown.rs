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
    pub(crate) errors: Vec<NativeRuntimeError>,
}

impl NativeShutdownAdvance {
    fn partial(progress: bool, errors: Vec<NativeRuntimeError>) -> Self {
        Self {
            progress,
            registrations: Vec::new(),
            commands: Vec::new(),
            pointer_passthrough: None,
            abandoned_outputs: Vec::new(),
            errors,
        }
    }
}

impl NativeCoordinator {
    pub(crate) fn advance_shutdown_boundary(&mut self) -> NativeShutdownAdvance {
        let input_enable_cancelled = self.cancel_quarantined_input_results();
        let (pending_effect_reported, pending_effect_error) =
            self.report_pending_effect_results_until_blocked();
        let mut errors = pending_effect_error.into_iter().collect::<Vec<_>>();
        let output_prefix_pending = !self.bridge.output_prefix().is_empty();
        let (quiescence_recorded, quiescence_errors) = self.try_report_retirement_quiescence_all();
        errors.extend(quiescence_errors);
        let mut progress = pending_effect_reported
            || input_enable_cancelled
            || output_prefix_pending
            || quiescence_recorded;
        let reduced_callback = match self.reduce_callback_head() {
            Ok(reduced) => reduced,
            Err(error) => {
                errors.extend(self.take_callback_errors());
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, errors);
            }
        };
        progress |= reduced_callback;
        errors.extend(self.take_callback_errors());
        let prepared_retirements = match self.prepare_committed_retirements() {
            Ok(prepared) => prepared,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, errors);
            }
        };

        let mut host_frame = match self.begin_host_frame(|_| NativeReceiverAnswer::Unknown) {
            Ok(frame) => frame,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, errors);
            }
        };
        if let Err(error) =
            host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        {
            errors.push(error);
            return NativeShutdownAdvance::partial(progress, errors);
        }
        let mut report = match host_frame.commit() {
            Ok(report) => report,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, errors);
            }
        };

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
        let (native_close_settled, close_errors) =
            self.settle_close_control_inputs_all(report.inputs());
        errors.extend(close_errors);
        let (native_admission_settled, admission_errors) =
            self.settle_native_admissions_all(report.native_admissions());
        errors.extend(admission_errors);
        let retirement_committed = prepared_retirements.is_some();
        let abandoned_outputs = prepared_retirements
            .map_or_else(Vec::new, |prepared| self.commit_retirements(prepared));
        let repaint_requested = !report.repaint_surfaces().is_empty();
        let effects = report.take_native_effects();
        let effects_emitted = !effects.is_empty();
        self.fail_shutdown_effects(effects, &mut errors);
        let commands = self.take_viewport_commands();
        let input_enable_cancelled_after_commit = self.cancel_quarantined_input_results();
        let pointer_passthrough = self.pointer_passthrough_command();
        let commands_pending = !commands.is_empty();
        let pointer_passthrough_pending = pointer_passthrough.is_some();

        NativeShutdownAdvance {
            progress: progress
                || had_inputs
                || native_snapshot_applied
                || native_close_settled
                || native_admission_settled
                || retirement_committed
                || !abandoned_outputs.is_empty()
                || effects_emitted
                || input_enable_cancelled_after_commit
                || commands_pending
                || pointer_passthrough_pending
                || repaint_requested,
            registrations,
            commands,
            pointer_passthrough,
            abandoned_outputs,
            errors,
        }
    }

    fn cancel_quarantined_input_results(&mut self) -> bool {
        let mut cancelled = false;
        while let Some(result) = self.input_control.cancel_quarantined_front() {
            self.queue_pending_effect_result(result, None);
            cancelled = true;
        }
        cancelled
    }

    fn fail_shutdown_effects(
        &mut self,
        requests: Vec<NativeEffectRequest>,
        errors: &mut Vec<NativeRuntimeError>,
    ) {
        for request in requests {
            let binding = request.operation().binding();
            let result = request.dispatch_failed(NativeDispatchFailure::ProviderStopped);
            if let Err(error) = self.session.report_native_effect_result(result)
                && let Err(error) = self.retain_or_return_effect_result(binding, error)
            {
                errors.push(error);
            }
        }
    }
}
