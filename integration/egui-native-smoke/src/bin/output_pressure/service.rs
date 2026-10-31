//! Bounded fixture measurement around real output admission and finalization.

use super::*;

struct PendingService {
    token: eframe::NativeOutputToken,
    started: Instant,
    phase: Phase,
    last_finalization: Option<Duration>,
    passes: usize,
}

#[derive(Default)]
struct ServiceTotals {
    count: usize,
    passes: usize,
    elapsed: Duration,
    minimum: Option<Duration>,
    maximum: Duration,
    maximum_phase: Option<Phase>,
    over_two_ms: usize,
    repeated_outputs: usize,
}

impl ServiceTotals {
    fn observe(&mut self, phase: Phase, elapsed: Duration, passes: usize) {
        self.count += 1;
        self.passes += passes;
        self.elapsed += elapsed;
        self.minimum = Some(
            self.minimum
                .map_or(elapsed, |previous| previous.min(elapsed)),
        );
        self.repeated_outputs += usize::from(passes > 1);
        if elapsed >= self.maximum {
            self.maximum = elapsed;
            self.maximum_phase = Some(phase);
        }
        self.over_two_ms += usize::from(elapsed >= Duration::from_millis(2));
    }

    fn report(&self, kind: &str) {
        eprintln!(
            "native-service: {kind}: samples={} passes={} repeated_outputs={} total={:?} min={:?} max={:?} max_phase={:?} over_2ms={}",
            self.count,
            self.passes,
            self.repeated_outputs,
            self.elapsed,
            self.minimum,
            self.maximum,
            self.maximum_phase,
            self.over_two_ms,
        );
    }
}

pub(super) struct ServiceMeasurements {
    pending: Vec<PendingService>,
    root: ServiceTotals,
    child: ServiceTotals,
    draining: ServiceTotals,
    deferred: ServiceTotals,
    host_events: ServiceTotals,
    not_finalized: usize,
}

impl ServiceMeasurements {
    pub(super) fn new() -> Self {
        Self {
            pending: Vec::with_capacity(BUFFERED_TERMINALS),
            root: ServiceTotals::default(),
            child: ServiceTotals::default(),
            draining: ServiceTotals::default(),
            deferred: ServiceTotals::default(),
            host_events: ServiceTotals::default(),
            not_finalized: 0,
        }
    }

    pub(super) fn admitted(
        &mut self,
        token: eframe::NativeOutputToken,
        started: Instant,
        phase: Phase,
        admission: eframe::NativeOutputAdmission,
    ) {
        if admission == eframe::NativeOutputAdmission::Defer {
            self.deferred.observe(phase, started.elapsed(), 0);
            return;
        }
        assert!(!self.pending.iter().any(|entry| entry.token == token));
        assert!(self.pending.len() < BUFFERED_TERMINALS);
        self.pending.push(PendingService {
            token,
            started,
            phase,
            last_finalization: None,
            passes: 0,
        });
    }

    pub(super) fn finalized(&mut self, token: eframe::NativeOutputToken, ended: Instant) {
        let pending = self
            .pending
            .iter_mut()
            .find(|entry| entry.token == token)
            .expect("a real finalized output must have its original admission sample");
        pending.last_finalization = Some(ended.duration_since(pending.started));
        pending.passes += 1;
    }

    pub(super) fn terminal(&mut self, token: eframe::NativeOutputToken) {
        let index = self
            .pending
            .iter()
            .position(|entry| entry.token == token)
            .expect("a real output terminal must close its exact admitted measurement");
        let pending = self.pending.swap_remove(index);
        let Some(elapsed) = pending.last_finalization else {
            self.not_finalized += 1;
            return;
        };
        let totals = if token.viewport_id() == ViewportId::ROOT {
            &mut self.root
        } else {
            &mut self.child
        };
        totals.observe(pending.phase, elapsed, pending.passes);
        if pending.phase == Phase::Draining {
            self.draining
                .observe(pending.phase, elapsed, pending.passes);
        }
    }

    pub(super) fn report(&self) {
        assert!(
            self.pending.is_empty(),
            "service measurements must retire with real output terminals"
        );
        assert!(self.root.count > 0);
        eprintln!(
            "native-service: scope=before-output-admission..after-last-host-finalization renderer_and_terminal_delay_excluded=true hard_realtime=false fixture_backing={} not_finalized={}",
            self.pending.capacity() * size_of::<PendingService>(),
            self.not_finalized,
        );
        self.root.report("root");
        self.child.report("child");
        self.draining.report("draining");
        self.deferred.report("deferred-admission");
        self.host_events.report("standalone-host-event");
    }

    pub(super) fn host_event(&mut self, phase: Phase, elapsed: Duration) {
        self.host_events.observe(phase, elapsed, 0);
    }

    pub(super) fn has_repeated_output(&self) -> bool {
        self.root.repeated_outputs > 0 || self.child.repeated_outputs > 0
    }
}
