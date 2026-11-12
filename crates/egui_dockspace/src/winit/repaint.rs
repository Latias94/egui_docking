//! Repaint delivery and surface-recovery pacing for the single-window runner.

use std::time::Instant;

use dockspace_winit::RepaintScheduler;
use egui::{Context, RequestRepaintInfo, ViewportId};

pub(super) struct RepaintRequest {
    pub(super) deadline: Instant,
    pass: u64,
}

impl RepaintRequest {
    pub(super) fn new(info: RequestRepaintInfo, requested_at: Instant) -> Option<Self> {
        if info.viewport_id != ViewportId::ROOT {
            return None;
        }
        Some(Self {
            deadline: requested_at.checked_add(info.delay)?,
            pass: info.current_cumulative_pass_nr,
        })
    }

    pub(super) fn is_current(&self, context: &Context) -> bool {
        // A request from inside a pass may arrive just after that pass finishes.
        matches!(
            context
                .cumulative_pass_nr_for(ViewportId::ROOT)
                .checked_sub(self.pass),
            Some(0 | 1)
        )
    }
}

#[derive(Default)]
pub(super) struct Repaint {
    requested: RepaintScheduler,
    not_before: Option<Instant>,
}

impl Repaint {
    pub(super) fn request(&mut self, deadline: Instant) {
        self.requested.request(deadline);
    }

    pub(super) fn defer_until(&mut self, deadline: Instant) {
        self.not_before = Some(deadline);
    }

    pub(super) fn resume(&mut self) {
        self.not_before = None;
    }

    pub(super) fn frame_started(&mut self, now: Instant) {
        self.requested.frame_started(now);
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.requested.deadline().map(|deadline| {
            self.not_before
                .map_or(deadline, |retry| retry.max(deadline))
        })
    }

    pub(super) fn take_due(&mut self, now: Instant) -> bool {
        if self.deadline().is_some_and(|deadline| deadline <= now) {
            self.requested.take_due(now)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::*;

    fn frame(context: &Context) {
        let mut output = context.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
    }

    #[test]
    fn delayed_delivery_does_not_repaint_after_a_newer_pass() {
        let context = Context::default();
        let (sender, receiver) = mpsc::channel();
        context.set_request_repaint_callback(move |info| sender.send(info).unwrap());
        frame(&context);
        let now = Instant::now();
        let request = RepaintRequest::new(receiver.try_recv().unwrap(), now).unwrap();
        assert!(request.is_current(&context));
        frame(&context);
        assert!(!request.is_current(&context));
    }

    #[test]
    fn background_request_after_a_pass_still_requests_the_next_frame() {
        let context = Context::default();
        for _ in 0..3 {
            frame(&context);
        }
        let (sender, receiver) = mpsc::channel();
        context.set_request_repaint_callback(move |info| sender.send(info).unwrap());
        context.request_repaint();
        let request = RepaintRequest::new(receiver.try_recv().unwrap(), Instant::now()).unwrap();
        assert!(request.is_current(&context));
        frame(&context);
        assert!(request.is_current(&context));
    }

    #[test]
    fn recovery_pacing_coalesces_immediate_requests_without_postponing_retry() {
        let now = Instant::now();
        let retry = now + Duration::from_millis(250);
        let mut repaint = Repaint::default();
        repaint.request(now);
        repaint.defer_until(retry);
        for millis in [0, 20, 100, 249] {
            let arrival = now + Duration::from_millis(millis);
            repaint.request(arrival);
            assert_eq!(repaint.deadline(), Some(retry));
            assert!(!repaint.take_due(arrival));
        }
        assert!(repaint.take_due(retry));
        assert!(!repaint.take_due(retry));
        assert_eq!(
            repaint.deadline(),
            None,
            "no polling without another request"
        );
    }

    #[test]
    fn native_input_can_present_before_retry_and_restore_immediate_repaints() {
        let now = Instant::now();
        let mut repaint = Repaint::default();
        repaint.defer_until(now + Duration::from_millis(250));
        repaint.frame_started(now);
        repaint.resume();
        repaint.request(now);
        assert!(repaint.take_due(now));
    }

    #[test]
    fn recovery_does_not_accelerate_a_later_application_timer() {
        let now = Instant::now();
        let later = now + Duration::from_secs(1);
        let mut repaint = Repaint::default();
        repaint.request(later);
        repaint.defer_until(now + Duration::from_millis(250));
        assert_eq!(repaint.deadline(), Some(later));
        assert!(!repaint.take_due(now + Duration::from_millis(250)));
        assert!(repaint.take_due(later));
    }
}
