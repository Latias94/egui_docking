use std::time::Instant;

/// Retains the earliest requested repaint deadline for one host window.
///
/// Capture an absolute deadline when a repaint is requested, before sending it
/// through an event-loop proxy. Applying the original delay after event delivery
/// would postpone repainting by the queue's latency.
#[derive(Clone, Debug, Default)]
pub struct RepaintScheduler {
    deadline: Option<Instant>,
}

impl RepaintScheduler {
    /// Requests a repaint no later than this absolute deadline.
    pub fn request(&mut self, deadline: Instant) {
        self.deadline = Some(
            self.deadline
                .map_or(deadline, |current| current.min(deadline)),
        );
    }

    /// Returns the earliest outstanding deadline, suitable for `ControlFlow::WaitUntil`.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Consumes an expired deadline once, returning whether a repaint is due.
    #[must_use]
    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|deadline| deadline <= now) {
            self.deadline = None;
            true
        } else {
            false
        }
    }

    /// Clears requests satisfied by this frame, preserving future deadlines.
    ///
    /// Call this before running the UI so requests emitted by that run remain pending.
    pub fn frame_started(&mut self, now: Instant) {
        let _ = self.take_due(now);
    }
}
