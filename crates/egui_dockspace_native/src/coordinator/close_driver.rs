//! Synchronous native-close product policy over ordinary core host frames.

use dockspace::close::{CloseDecision, CloseItemDecisionState};
use dockspace::runtime::{
    DockspaceClosePlan, HostFrameReport, HostInputOutcome, NativeReceiverAnswer,
    SurfaceUnavailableReason,
};

use super::*;
use crate::close_control::NativeWindowClosePolicy;

impl NativeCoordinator {
    pub(crate) fn drive_close_policy(
        &mut self,
        policy: NativeWindowClosePolicy,
        external_root: SurfaceId,
    ) -> Result<bool, NativeRuntimeError> {
        let requests = self.close_control.unresolved_requests().collect::<Vec<_>>();
        if requests.is_empty() {
            return Ok(false);
        }

        for close in requests {
            match policy.request(close.surface() == external_root) {
                Some(request) => self.session.request_native_surface_close(close, request)?,
                None => self.session.cancel_native_surface_close(close)?,
            }
        }

        let first = self.commit_close_control_frame(&[])?;
        let plans = first
            .inputs()
            .iter()
            .filter_map(|input| match input {
                HostInputOutcome::NativeSurfaceCloseRequested { plan, .. } => Some(plan.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        self.finish_close_control_report(first)?;

        if plans.iter().any(plan_has_pending_decisions) {
            let second = self.commit_close_control_frame(&plans)?;
            self.finish_close_control_report(second)?;
        }
        Ok(true)
    }

    fn commit_close_control_frame(
        &mut self,
        plans: &[DockspaceClosePlan],
    ) -> Result<HostFrameReport, NativeRuntimeError> {
        let mut frame = self.begin_host_frame(|_| NativeReceiverAnswer::Unknown)?;
        for plan in plans {
            for item in plan.items() {
                if item.state() == CloseItemDecisionState::Pending {
                    frame.frame.resolve_close(
                        plan.request(),
                        item.token(),
                        CloseDecision::Allow,
                    )?;
                }
            }
        }
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        frame.commit()
    }

    fn finish_close_control_report(
        &mut self,
        mut report: HostFrameReport,
    ) -> Result<(), NativeRuntimeError> {
        self.retain_internal_presentation_transitions(&report);
        self.settle_host_frame_inputs(report.inputs());
        self.settle_close_control_inputs(report.inputs())?;
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
        self.accept_native_effects(report.take_native_effects())
    }
}

fn plan_has_pending_decisions(plan: &DockspaceClosePlan) -> bool {
    plan.items()
        .iter()
        .any(|item| item.state() == CloseItemDecisionState::Pending)
}
