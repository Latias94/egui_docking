//! Capture effects share the existing stationary owner and window callback receipt.

use dockspace::runtime::{NativePointerCaptureRequest, NativePointerContinuation};
use eframe::{
    NativeViewportPointerCaptureAction as Action, NativeViewportPointerCaptureStatus as Status,
};
use winit::event::{DeviceId, PointerPressToken};

use super::*;
use crate::pointer_capture::NativePointerCaptureKey;

#[cfg(all(test, target_os = "linux"))]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturePhase {
    Acquire,
    Acquired,
    Release,
}

#[derive(Debug)]
pub(super) struct NativeCaptureDuty {
    request: NativePointerCaptureRequest,
    viewport: ViewportId,
    window: WindowId,
    device: DeviceId,
    press: PointerPressToken,
    phase: CapturePhase,
    pending: Option<NativePointerCaptureKey>,
    destroyed: bool,
    not_executed: bool,
    may_own_capture: bool,
}

impl NativeCoordinator {
    pub(super) fn capture_references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.stationary
            .capture
            .as_ref()
            .is_some_and(|duty| duty.request.source_binding() == binding)
    }

    pub(super) fn capture_is_acquired_for(&self, request: NativePointerContinuation) -> bool {
        self.stationary.capture.as_ref().is_some_and(|duty| {
            duty.phase == CapturePhase::Acquired
                && duty.pending.is_none()
                && !duty.destroyed
                && duty.request.pointer() == request.pointer()
                && duty.request.source_binding() == request.source_binding()
                && self.session.native_pointer_capture_request() == Some(duty.request)
        })
    }

    pub(super) fn capture_acquisition_pending_for(
        &self,
        request: NativePointerContinuation,
    ) -> bool {
        self.stationary.capture.as_ref().is_some_and(|duty| {
            duty.phase == CapturePhase::Acquire
                && !duty.destroyed
                && duty.request.pointer() == request.pointer()
                && duty.request.source_binding() == request.source_binding()
                && self.session.native_pointer_capture_request() == Some(duty.request)
        })
    }

    pub(super) fn capture_window_destroyed(
        &mut self,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) {
        if let Some(duty) = self.stationary.capture.as_mut()
            && duty.request.source_binding() == binding
            && duty.window == window
        {
            duty.destroyed = true;
            duty.phase = CapturePhase::Release;
        }
    }

    /// Reconcile only after accepted raw/core input has committed. A missing
    /// transient projection while input remains pending cannot retire a lease.
    pub(crate) fn drive_pointer_capture(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
        allow_acquire: bool,
    ) -> Result<bool, NativeRuntimeError> {
        let coherent = !self.session.has_pending_native_input()
            && !self.has_pending_callback_records()
            && !self.bridge.callback_boundary_pending();
        let current = coherent
            .then(|| self.session.native_pointer_capture_request())
            .flatten();
        if let Some(duty) = self.stationary.capture.as_mut() {
            if !allow_acquire
                || !self.bridge.accepts_new_work()
                || (coherent && current != Some(duty.request))
            {
                duty.phase = CapturePhase::Release;
            }
            if duty.destroyed && duty.pending.is_none() {
                self.bridge
                    .release_capture_binding(duty.request.source_binding());
                self.stationary.capture = None;
            }
        }
        if self.stationary.capture.is_none()
            && coherent
            && allow_acquire
            && self.bridge.accepts_new_work()
            && let Some(request) = current
            && self.stationary.capture_blocked != Some(request)
            && let Some((device, Some(press))) = self
                .pointer_translator
                .mouse_press_for_pointer(request.pointer())
            && let Some((viewport, window)) =
                self.exact_window_route_for_binding(request.source_binding())
        {
            self.stationary.capture = Some(NativeCaptureDuty {
                request,
                viewport,
                window,
                device,
                press: press.clone(),
                phase: CapturePhase::Acquire,
                pending: None,
                destroyed: false,
                not_executed: false,
                may_own_capture: false,
            });
        }
        let Some(duty) = self.stationary.capture.as_ref() else {
            return Ok(false);
        };
        if duty.pending.is_some() || duty.phase == CapturePhase::Acquired || duty.not_executed {
            return Ok(false);
        }
        if budget.is_exhausted() {
            context.request_repaint_once_of(ViewportId::ROOT);
            return Ok(false);
        }
        let action = if duty.phase == CapturePhase::Acquire {
            Action::Acquire
        } else {
            Action::Release
        };
        let command = eframe::prepare_native_viewport_pointer_capture(
            duty.viewport,
            duty.window,
            duty.device,
            duty.press.clone(),
            action,
        );
        let key = NativePointerCaptureKey {
            token: command.token(),
            viewport: duty.viewport,
            window: duty.window,
            device: duty.device,
            action,
        };
        let binding = duty.request.source_binding();
        if !self.bridge.reserve_pointer_capture(key, binding) {
            // Ordinary admission may be frozen while cleanup keeps its prepaid
            // receipt. Retain this same duty for the next complete boundary.
            return Ok(false);
        }
        budget.complete_unsequenced_records(1);
        self.stationary
            .capture
            .as_mut()
            .expect("the exact duty remains owned")
            .pending = Some(key);
        command.enqueue(context);
        Ok(true)
    }

    pub(super) fn reduce_next_pointer_capture(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some((record, _captured_at)) = self.bridge.front_pointer_capture() else {
            return Ok(false);
        };
        let key = record.key();
        let mut acquired_request = None;
        if let Some(duty) = self.stationary.capture.as_mut()
            && duty.pending == Some(key)
            && duty.request.source_binding() == record.binding()
        {
            duty.pending = None;
            match (key.action, record.status()) {
                (Action::Acquire, Status::Acquired) => {
                    duty.may_own_capture = true;
                    if duty.phase != CapturePhase::Release {
                        duty.phase = CapturePhase::Acquired;
                        acquired_request = Some(duty.request);
                    }
                }
                (_, Status::Pending) => {
                    duty.may_own_capture = true;
                }
                (Action::Release, Status::Released | Status::Unavailable) => {
                    self.bridge
                        .release_capture_binding(duty.request.source_binding());
                    self.stationary.capture = None;
                }
                (Action::Acquire, Status::Unavailable | Status::Unsupported) => {
                    self.stationary.capture_blocked = Some(duty.request);
                    // An earlier Pending call may still have retained a native
                    // obligation. Exact release must check it before retirement.
                    duty.phase = CapturePhase::Release;
                }
                (_, Status::NotExecuted) => {
                    self.stationary.capture_blocked = Some(duty.request);
                    if duty.may_own_capture {
                        duty.phase = CapturePhase::Release;
                        duty.not_executed = true;
                    } else {
                        self.bridge
                            .release_capture_binding(duty.request.source_binding());
                        self.stationary.capture = None;
                    }
                }
                (Action::Release, Status::Unsupported) => {
                    // A backend which accepted no native acquisition has no
                    // native duty; unsupported acquire never enters Acquired.
                    if duty.may_own_capture {
                        duty.not_executed = true;
                    } else {
                        self.bridge
                            .release_capture_binding(duty.request.source_binding());
                        self.stationary.capture = None;
                    }
                }
                _ => return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into()),
            }
        }
        if let Some(request) = acquired_request {
            self.rearm_stationary_after_capture_acquisition(request);
        }
        if !self.bridge.acknowledge_pointer_capture(key) {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        }
        Ok(true)
    }
}
