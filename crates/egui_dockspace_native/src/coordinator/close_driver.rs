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
        let mut scratch = std::mem::take(&mut self.close_decision_scratch);
        let result = self.drive_close_policy_with_scratch(
            context,
            policy,
            external_root,
            budget,
            &mut scratch,
        );
        scratch.clear();
        self.close_decision_scratch = scratch;
        result
    }

    fn drive_close_policy_with_scratch(
        &mut self,
        context: &eframe::egui::Context,
        policy: NativeWindowClosePolicy,
        external_root: SurfaceId,
        budget: &mut NativeInputBudget,
        decision_requests: &mut Vec<dockspace::close::CloseRequestId>,
    ) -> Result<bool, NativeRuntimeError> {
        self.bridge.with_close_control(|control| {
            let count = control
                .decision_requests()
                .take(budget.remaining_records())
                .count();
            if let Some(limit) = self.owner_storage {
                if count > limit.close_slots || decision_requests.capacity() < count {
                    return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
                }
            } else {
                decision_requests
                    .try_reserve_exact(count)
                    .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            }
            decision_requests.extend(control.decision_requests().take(count));
            Ok(())
        })?;
        let mut recorded = false;
        // Appending policy input does not consume the reduction budget until
        // poll. Freeze this admission count before selecting requests one by one.
        for _ in 0..budget.remaining_records() {
            if budget.is_exhausted() {
                break;
            }
            let Some(close) = self
                .bridge
                .with_close_control(|control| control.unsubmitted_requests().next())
            else {
                break;
            };
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
        let mut frame = crate::host_frame::NativeHostFrame::new_with_input(
            frame,
            self.input,
            self.bridge.clone(),
            &mut self.receivers,
            None,
        );
        let mut completed = 0;
        if !frame.frame.native_input_pending() {
            for index in 0..decision_requests.len() {
                if budget.is_exhausted() {
                    break;
                }
                let request = decision_requests[index];
                loop {
                    if budget.is_exhausted() {
                        break;
                    }
                    let token = frame
                        .frame
                        .pending_close_decisions(request)
                        .next()
                        .map(|item| item.token());
                    let Some(token) = token else {
                        break;
                    };
                    frame
                        .frame
                        .resolve_close(request, token, CloseDecision::Allow)?;
                    budget.complete_unsequenced_records(1);
                    if frame
                        .frame
                        .pending_close_decisions(request)
                        .next()
                        .is_some_and(|item| item.token() == token)
                    {
                        return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
                    }
                }
                if frame
                    .frame
                    .pending_close_decisions(request)
                    .next()
                    .is_none()
                {
                    decision_requests[completed] = request;
                    completed += 1;
                }
            }
        }
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let report = frame.commit()?;
        self.bridge.with_close_control(|control| {
            control.settle_decision_requests(&decision_requests[..completed])
        });
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
