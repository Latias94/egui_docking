//! Budgeted native-close policy using the existing exact close owner.

use dockspace::close::CloseDecision;
use dockspace::runtime::{
    HostFrameReport, NativeInputBudget, NativeReceiverAnswer, SurfaceUnavailableReason,
};

use super::*;
use crate::close_control::NativeWindowClosePolicy;

impl NativeCoordinator {
    fn rearm_close_continuation(
        &self,
        context: &eframe::egui::Context,
    ) -> Result<(), NativeRuntimeError> {
        context.request_repaint_once_of(ViewportId::ROOT);
        if let Some(token) = eframe::current_native_output_token()
            && !self.bridge.require_output_wake_after_current(token)
        {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        Ok(())
    }

    fn rearm_deferred_close_policy(
        &self,
        context: &eframe::egui::Context,
    ) -> Result<(), NativeRuntimeError> {
        if self
            .bridge
            .with_close_control(|control| control.has_runnable_policy_work())
        {
            self.rearm_close_continuation(context)?;
        }
        Ok(())
    }

    pub(crate) fn drive_close_policy(
        &mut self,
        context: &eframe::egui::Context,
        policy: NativeWindowClosePolicy,
        external_root: SurfaceId,
        budget: &mut NativeInputBudget,
    ) -> Result<bool, NativeRuntimeError> {
        self.bridge.admission_dispatch();
        if !self.bridge.permits_unscheduled_input() {
            return Ok(false);
        }
        if budget.is_exhausted() {
            self.rearm_deferred_close_policy(context)?;
            return Ok(false);
        }
        if self.bridge.callback_boundary_pending() || self.bridge.has_unsubmitted_output_prefix() {
            return Ok(false);
        }
        let requests = self.bridge.with_close_control(|control| {
            control
                .unsubmitted_requests()
                .take(budget.remaining_records())
                .collect::<Vec<_>>()
        });
        let decision_requests = self.bridge.with_close_control(|control| {
            control
                .decision_requests()
                .take(budget.remaining_records())
                .collect::<Vec<_>>()
        });
        let mut recorded = false;
        for close in requests {
            if budget.is_exhausted() {
                break;
            }
            match policy.request(close.surface() == external_root) {
                Some(action) => self.session.request_native_surface_close(close, action)?,
                None => self.session.cancel_native_surface_close(close)?,
            }
            self.bridge
                .with_close_control(|control| control.mark_policy_recorded(close));
            self.bridge.hold_native_boundary();
            recorded = true;
        }
        if !recorded {
            if budget.is_exhausted() {
                self.rearm_deferred_close_policy(context)?;
                return Ok(false);
            }
            if decision_requests.is_empty() {
                return Ok(false);
            }
        }

        // Later raw callbacks must not overtake a policy already retained by
        // the joined recorder. A general prefix poll can resume it next token;
        // report settlement stores only the decision request on its exact close owner.
        let Some(frame) = self
            .session
            .poll_native_host_frame(budget, |_| NativeReceiverAnswer::Unknown)?
        else {
            self.bridge.hold_native_boundary();
            self.rearm_close_continuation(context)?;
            return Ok(recorded);
        };
        let mut frame = crate::host_frame::NativeHostFrame::new(
            frame,
            self.bridge.clone(),
            &mut self.receivers,
            None,
        );
        let mut completed_requests = Vec::new();
        if !frame.frame.native_input_pending() {
            for request in decision_requests {
                if budget.is_exhausted() {
                    break;
                }
                let decisions = frame
                    .frame
                    .pending_close_decisions(request)
                    .take(budget.remaining_records())
                    .collect::<Vec<_>>();
                for item in decisions {
                    if budget.is_exhausted() {
                        break;
                    }
                    frame
                        .frame
                        .resolve_close(request, item.token(), CloseDecision::Allow)?;
                    budget.complete_unsequenced_records(1);
                }
                if frame
                    .frame
                    .pending_close_decisions(request)
                    .next()
                    .is_none()
                {
                    completed_requests.push(request);
                }
            }
        }
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let report = frame.commit()?;
        self.bridge
            .with_close_control(|control| control.settle_decision_requests(&completed_requests));
        self.finish_internal_report(context, report)?;
        self.rearm_deferred_close_policy(context)?;
        Ok(true)
    }

    pub(super) fn finish_internal_report(
        &mut self,
        context: &eframe::egui::Context,
        mut report: HostFrameReport,
    ) -> Result<(), NativeRuntimeError> {
        self.retain_internal_presentation_transitions(&report);
        self.settle_host_frame_report(&report)?;
        self.settle_close_control_report(&report)?;
        self.settle_native_admissions(report.native_admissions())?;
        let outputs = report.take_painted_outputs();
        if !outputs.is_empty() {
            let actual = outputs.len();
            drop(outputs);
            return Err(NativeHostProtocolError::PaintedOutputCountMismatch {
                expected: 0,
                actual,
            }
            .into());
        }
        self.accept_native_effects(context, report.take_native_effects())
    }
}
