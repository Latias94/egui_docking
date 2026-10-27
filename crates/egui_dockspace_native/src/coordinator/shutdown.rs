//! Fatal-session drain over the existing native host-frame protocol.

use dockspace::model::SurfaceId;
use dockspace::runtime::{HostInputOutcome, NativeDispatchFailure, NativeReceiverAnswer};
use eframe::NativeOutputToken;
use eframe::egui::{ViewportCommand, ViewportId};

use super::*;
use crate::close_control::NativeWindowClosePolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeShutdownRegistration {
    Registered(NativeSurfaceBinding),
    Rejected(SurfaceId),
}

#[derive(Debug)]
pub(crate) struct NativeShutdownAdvance {
    pub(crate) progress: bool,
    pub(crate) transition_source_exhausted: bool,
    pub(crate) presentation_transitions: Vec<DockspacePresentationTransition>,
    pub(crate) registrations: Vec<NativeShutdownRegistration>,
    pub(crate) commands: Vec<(ViewportId, ViewportCommand)>,
    pub(crate) repaint_viewports: Vec<ViewportId>,
    pub(crate) pointer_passthrough: Option<NativePointerPassthroughCommand>,
    pub(crate) abandoned_outputs: Vec<NativeOutputToken>,
    pub(crate) errors: Vec<NativeRuntimeError>,
}

impl NativeShutdownAdvance {
    fn partial(
        progress: bool,
        presentation_transitions: Vec<DockspacePresentationTransition>,
        errors: Vec<NativeRuntimeError>,
    ) -> Self {
        Self {
            progress,
            transition_source_exhausted: false,
            presentation_transitions,
            registrations: Vec::new(),
            commands: Vec::new(),
            repaint_viewports: Vec::new(),
            pointer_passthrough: None,
            abandoned_outputs: Vec::new(),
            errors,
        }
    }

    fn partial_with_repaints(
        progress: bool,
        presentation_transitions: Vec<DockspacePresentationTransition>,
        errors: Vec<NativeRuntimeError>,
        repaint_viewports: Vec<ViewportId>,
    ) -> Self {
        Self {
            repaint_viewports,
            ..Self::partial(progress, presentation_transitions, errors)
        }
    }
}

impl NativeCoordinator {
    pub(crate) fn advance_shutdown_boundary_with_close_policy(
        &mut self,
        policy: NativeWindowClosePolicy,
        external_root: SurfaceId,
    ) -> NativeShutdownAdvance {
        let mut advance = self.advance_shutdown_boundary();
        match self.drive_close_policy(policy, external_root) {
            Ok(close_progress) => {
                let presentation_transitions = self.take_internal_presentation_transitions();
                let commands = self.take_viewport_commands();
                let progressed =
                    close_progress || !presentation_transitions.is_empty() || !commands.is_empty();
                advance.progress |= progressed;
                if progressed {
                    advance.transition_source_exhausted = false;
                }
                advance
                    .presentation_transitions
                    .extend(presentation_transitions);
                advance.commands.extend(commands);
            }
            Err(error) => {
                advance.transition_source_exhausted = false;
                advance.errors.push(error);
            }
        }
        advance
    }

    pub(crate) fn advance_shutdown_boundary(&mut self) -> NativeShutdownAdvance {
        let mut presentation_transitions = self.take_internal_presentation_transitions();
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
        let reduced_callback = match self.reduce_callback_prefix() {
            Ok(reduced) => reduced.is_some(),
            Err(error) => {
                errors.extend(self.take_callback_errors());
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, presentation_transitions, errors);
            }
        };
        progress |= reduced_callback;
        errors.extend(self.take_callback_errors());
        let prepared_retirements = match self.prepare_committed_retirements() {
            Ok(prepared) => prepared,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial(progress, presentation_transitions, errors);
            }
        };
        let ready_no_frame_invalidations = self.prepare_no_frame_invalidations();
        let (_, fallback_repaint_viewports) = split_no_frame_invalidations_for_frame(
            &[],
            ready_no_frame_invalidations.iter().copied(),
            |invalidation| invalidation.surface(),
            |invalidation| invalidation.viewport(),
        );

        let mut host_frame = match self.begin_host_frame(|_| NativeReceiverAnswer::Unknown) {
            Ok(frame) => frame,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial_with_repaints(
                    progress,
                    presentation_transitions,
                    errors,
                    fallback_repaint_viewports,
                );
            }
        };
        let frame_surfaces = host_frame.frame.surfaces();
        let (accepted_no_frame_invalidations, repaint_viewports) =
            split_no_frame_invalidations_for_frame(
                &frame_surfaces,
                ready_no_frame_invalidations,
                |invalidation| invalidation.surface(),
                |invalidation| invalidation.viewport(),
            );
        for invalidation in &accepted_no_frame_invalidations {
            if let Err(error) = host_frame.defer_surface(
                invalidation.surface(),
                SurfaceUnavailableReason::SurfaceUnavailable,
            ) {
                errors.push(error);
                return NativeShutdownAdvance::partial_with_repaints(
                    progress,
                    presentation_transitions,
                    errors,
                    repaint_viewports,
                );
            }
        }
        if let Err(error) =
            host_frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        {
            errors.push(error);
            return NativeShutdownAdvance::partial_with_repaints(
                progress,
                presentation_transitions,
                errors,
                repaint_viewports,
            );
        }
        let mut report = match host_frame.commit() {
            Ok(report) => report,
            Err(error) => {
                errors.push(error);
                return NativeShutdownAdvance::partial_with_repaints(
                    progress,
                    presentation_transitions,
                    errors,
                    repaint_viewports,
                );
            }
        };
        self.finish_no_frame_invalidations(&accepted_no_frame_invalidations);
        presentation_transitions.extend_from_slice(report.presentation_transitions());

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
        let native_snapshot_applied = match self.settle_host_frame_inputs(report.inputs()) {
            Ok(applied) => applied,
            Err(error) => {
                errors.push(error);
                false
            }
        };
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

        let transition_progress = progress
            || had_inputs
            || native_snapshot_applied
            || native_close_settled
            || native_admission_settled
            || !accepted_no_frame_invalidations.is_empty()
            || retirement_committed
            || !abandoned_outputs.is_empty()
            || effects_emitted
            || input_enable_cancelled_after_commit
            || commands_pending
            || pointer_passthrough_pending;
        let progress = transition_progress || repaint_requested;
        let transition_source_exhausted = !transition_progress
            && errors.is_empty()
            && presentation_transitions.is_empty()
            && self.is_quiescent();
        NativeShutdownAdvance {
            progress,
            transition_source_exhausted,
            presentation_transitions,
            registrations,
            commands,
            repaint_viewports,
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
