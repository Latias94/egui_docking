//! Temporary diagnostic spans; never part of a shipping build.

use std::time::Instant;

pub(crate) struct Span {
    label: &'static str,
    start: Instant,
    last: Instant,
}

impl Span {
    pub(crate) fn new(label: &'static str) -> Self {
        let start = Instant::now();
        Self { label, start, last: start }
    }

    pub(crate) fn mark(&mut self, phase: &'static str) {
        let ns = self.last.elapsed().as_nanos();
        eprintln!("[DEBUG-select-cost] part {} {} {ns}", self.label, phase);
        self.last = Instant::now();
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        eprintln!("[DEBUG-select-cost] span {} {}", self.label, self.start.elapsed().as_nanos());
    }
}
