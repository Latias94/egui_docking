//! Synchronous native-close product policy over ordinary core host frames.

use dockspace::close::{CloseDecision, CloseItemDecisionState};
use dockspace::runtime::{
    DockspaceClosePlan, HostFrameReport, HostInputOutcome, NativeReceiverAnswer,
    NativeSurfaceCloseRequest, SurfaceUnavailableReason,
};

use super::*;
use crate::close_control::NativeWindowClosePolicy;

impl NativeCoordinator {
    pub(crate) fn drive_close_policy(
        &mut self,
        policy: NativeWindowClosePolicy,
        external_root: SurfaceId,
    ) -> Result<bool, NativeRuntimeError> {
        if self.close_control.unresolved_requests().next().is_none() {
            return Ok(false);
        }
        // The request is minted only after its CloseRequested record reaches
        // the FIFO head and that exact host frame commits. Every callback
        // older than the close edge is therefore already settled; callbacks
        // now in the mailbox are later and must not delay the synchronous
        // close decision for this eframe update.
        let requests = self.close_control.unresolved_requests().collect::<Vec<_>>();

        for close in &requests {
            match policy.request(close.surface() == external_root) {
                Some(request) => self.session.request_native_surface_close(*close, request)?,
                None => self.session.cancel_native_surface_close(*close)?,
            }
        }

        let first = self.commit_close_control_frame(&requests, &[])?;
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
            let second = self.commit_close_control_frame(&requests, &plans)?;
            self.finish_close_control_report(second)?;
        }
        Ok(true)
    }

    fn commit_close_control_frame(
        &mut self,
        requests: &[NativeSurfaceCloseRequest],
        plans: &[DockspaceClosePlan],
    ) -> Result<HostFrameReport, NativeRuntimeError> {
        let mut frame = self.begin_close_control_frame(requests)?;
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

    fn begin_close_control_frame(
        &mut self,
        requests: &[NativeSurfaceCloseRequest],
    ) -> Result<crate::host_frame::NativeHostFrame<'_>, NativeRuntimeError> {
        if requests.is_empty()
            || self.bridge.callback_boundary_pending()
            || !requests
                .iter()
                .all(|request| self.close_control.recognizes_request(*request))
        {
            return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
        }

        // The exact CloseRequested callback already committed before the core
        // minted these requests. Later callback records remain in the mailbox,
        // but they cannot causally overtake the synchronous close response
        // which eframe must observe in this same output.
        Ok(crate::host_frame::NativeHostFrame::new(
            self.session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)?,
            self.bridge.clone(),
            self.viewports.clone(),
            &mut self.receivers,
            None,
        ))
    }

    fn finish_close_control_report(
        &mut self,
        mut report: HostFrameReport,
    ) -> Result<(), NativeRuntimeError> {
        self.retain_internal_presentation_transitions(&report);
        self.settle_host_frame_inputs(report.inputs())?;
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
