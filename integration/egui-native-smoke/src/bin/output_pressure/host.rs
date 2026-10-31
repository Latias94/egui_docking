//! A bounded terminal-delivery fault injector over the real native handler.

use super::*;
use eframe::{NativeHostHandler, NativeHostWake, NativeOutputAdmission, NativeOutputResult};

pub(super) struct PressureHost {
    pub(super) inner: Arc<dyn NativeHostHandler>,
    pub(super) runtime: Runtime,
    pub(super) shared: Shared,
}

impl PressureHost {
    fn flush(&self) {
        {
            let runtime = lock(&self.runtime);
            assert_eq!(runtime.error_kind(), None);
            runtime.with_errors(|primary, cleanup| {
                assert!(primary.is_none() && cleanup.is_empty());
            });
            let backlog = runtime.output_backlog();
            assert_eq!(backlog.ordinary(), ORDINARY_OUTPUTS);
            assert_eq!(backlog.control(), 2);
            assert_eq!(backlog.protected_staging(), 1);
            let resident = runtime.input_storage_capacity_bytes().unwrap();
            let reserved = runtime.input_storage_reserved_bytes().unwrap();
            assert!(resident <= reserved && reserved <= BYTE_LIMIT);
            let mut state = lock(&self.shared);
            state.peak_resident = state.peak_resident.max(resident);
            state.peak_reserved = state.peak_reserved.max(reserved);
        }
        let scenario = {
            let mut state = lock(&self.shared);
            assert_eq!(state.peak_ordinary, ORDINARY_OUTPUTS);
            assert!(state.root_control_seen && state.protected_child_seen);
            assert!(state.root_terminal_seen && state.protected_terminal_seen);
            assert_eq!(state.buffered.len(), BUFFERED_TERMINALS);
            state.phase = Phase::Draining;
            state.scenario
        };
        eprintln!("output-pressure: O256 plus root/protected child reached; root deferred");
        // Preserve the actual completion order. The negative scenario supplies
        // the same rejected fact again after its predecessor, only for cleanup;
        // the runtime's protocol error must remain fatal.
        if scenario == Scenario::Reordered {
            let second = lock(&self.shared).buffered[1];
            self.inner.on_output(second);
        }
        loop {
            let result = {
                let mut state = lock(&self.shared);
                if scenario == Scenario::LateStaging
                    && state
                        .buffered
                        .front()
                        .is_some_and(|result| Some(result.token()) == state.protected_child)
                {
                    // The root control terminal must already have released its
                    // prepaid owner so a real UI pass can enter fatal quarantine.
                    assert!(
                        state
                            .buffered
                            .iter()
                            .all(|result| { Some(result.token()) != state.root_control })
                    );
                    eprintln!(
                        "output-pressure: withholding staging terminal until fatal quarantine"
                    );
                    break;
                }
                let Some(result) = state.buffered.pop_front() else {
                    break;
                };
                result
            };
            self.inner.on_output(result);
            lock(&self.shared).forwarded += 1;
            if matches!(scenario, Scenario::Duplicate | Scenario::LateStaging)
                && lock(&self.shared).forwarded == 1
            {
                self.inner.on_output(result);
            }
        }
    }

    fn flush_late_output_after_fatal(&self) {
        let (scenario, withheld_first_live) = {
            let state = lock(&self.shared);
            if !matches!(
                state.scenario,
                Scenario::LateStaging | Scenario::LateFirstLive
            ) || state.phase != Phase::Draining
                || state.buffered.is_empty()
            {
                return;
            }
            if state.scenario == Scenario::LateFirstLive {
                assert!(state.withheld_first_live.is_some());
                assert_eq!(state.buffered.len(), 1);
                assert_eq!(state.buffered.front().copied(), state.withheld_first_live);
            }
            (state.scenario, state.withheld_first_live)
        };
        let before = {
            let runtime = lock(&self.runtime);
            runtime.with_errors(|primary, cleanup| {
                assert!(primary.is_some_and(|error| error.is_output_order_violation()));
                assert!(cleanup.is_empty());
            });
            assert_eq!(runtime.output_backlog().protected_staging(), 0);
            assert!(!runtime.is_shutdown_drained());
            lock(&self.shared)
                .check_source_ownership(&runtime)
                .expect("fatal drain must preserve source ownership before the late terminal");
            runtime.output_backlog()
        };
        // These are the original real terminals, delivered after quarantine
        // abandoned their affine paint and before the current later terminal.
        loop {
            let Some(result) = lock(&self.shared).buffered.pop_front() else {
                break;
            };
            if scenario == Scenario::LateFirstLive {
                assert_eq!(Some(result), withheld_first_live);
            }
            self.inner.on_output(result);
            let mut state = lock(&self.shared);
            state.forwarded += 1;
            if scenario == Scenario::LateFirstLive {
                assert!(state.settled_first_live.replace(result).is_none());
            }
        }
        {
            let runtime = lock(&self.runtime);
            lock(&self.shared)
                .check_source_ownership(&runtime)
                .expect("a late terminal must only settle its output owner, without admission");
            if scenario == Scenario::LateFirstLive {
                let after = runtime.output_backlog();
                assert_eq!(
                    before.ordinary().checked_sub(1),
                    Some(after.ordinary()),
                    "the late first-live terminal must release its exact ordinary owner"
                );
                assert_eq!(after.control(), before.control());
            }
        }
        lock(&self.shared).late_terminal_after_fatal = true;
        eprintln!("output-pressure: original deferred terminal delivered after fatal quarantine");
    }
}

impl NativeHostHandler for PressureHost {
    fn install_output_pass_finalizer(
        &self,
        finalizer: Arc<dyn eframe::NativeOutputPassFinalizer>,
    ) -> bool {
        self.inner.install_output_pass_finalizer(finalizer)
    }

    fn try_attach(&self, attachment: eframe::NativeHostAttachment) -> bool {
        self.inner.try_attach(attachment)
    }

    fn detach(&self, attachment: eframe::NativeHostAttachment) {
        self.inner.detach(attachment);
    }

    fn begin_deferred_viewport_create(
        &self,
        attempt: eframe::NativeViewportCreateAttempt,
    ) -> eframe::NativeViewportCreateAdmission {
        self.inner.begin_deferred_viewport_create(attempt)
    }

    fn render_hidden_deferred_viewport(&self, viewport: ViewportId) -> bool {
        self.inner.render_hidden_deferred_viewport(viewport)
    }

    fn on_window_event(
        &self,
        event: eframe::NativeWindowEvent<'_>,
    ) -> eframe::NativeWindowEventDisposition {
        if let Some(combined) = &mut lock(&self.shared).combined {
            combined.observe_destroyed(event);
        }
        self.inner.on_window_event(event)
    }

    fn on_device_removed(&self, removal: eframe::NativeDeviceRemoval) {
        self.inner.on_device_removed(removal);
    }

    fn on_global_focus(&self, observation: eframe::NativeGlobalFocusObservation) -> NativeHostWake {
        self.inner.on_global_focus(observation)
    }

    fn on_viewport_focus(&self, result: eframe::NativeViewportFocusResult) -> NativeHostWake {
        self.inner.on_viewport_focus(result)
    }

    fn on_viewport_pointer_passthrough(
        &self,
        result: eframe::NativeViewportPointerPassthroughResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_pointer_passthrough(result)
    }

    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> NativeHostWake {
        self.inner.on_viewport_property_command_retired(retirement)
    }

    fn on_output_begin(
        &self,
        token: eframe::NativeOutputToken,
        window: eframe::NativeWindowSnapshot,
        root_roster: Option<eframe::NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        let service_started = Instant::now();
        let before = self
            .runtime
            .try_lock()
            .expect("this smoke requires actual deferred native viewports")
            .output_backlog();
        {
            let mut state = lock(&self.shared);
            if let Some(roster) = root_roster {
                state.root_roster_has_child = roster
                    .records()
                    .iter()
                    .any(|record| Some(record.viewport_id()) == state.child_viewport);
            }
            if let Some(combined) = &mut state.combined {
                if token.viewport_id() == ViewportId::ROOT {
                    combined.observe_final_roster(token, root_roster);
                } else if combined.is_satellite(token.viewport_id())
                    && matches!(
                        state.phase,
                        Phase::AwaitChild | Phase::AwaitStaging | Phase::Filling
                    )
                {
                    let phase = state.phase;
                    state.service.admitted(
                        token,
                        service_started,
                        phase,
                        NativeOutputAdmission::Defer,
                    );
                    return NativeOutputAdmission::Defer;
                }
            }
            if token.viewport_id() != ViewportId::ROOT && state.phase == Phase::AwaitStaging {
                state.child_paused = true;
                let phase = state.phase;
                state
                    .service
                    .admitted(token, service_started, phase, NativeOutputAdmission::Defer);
                return NativeOutputAdmission::Defer;
            }
            if token.viewport_id() != ViewportId::ROOT
                && state.phase == Phase::Filling
                && (!state.root_control_seen || before.ordinary() != ORDINARY_OUTPUTS)
            {
                // Keep the admitted create/staging obligation while root output
                // owners fill. The framework retains this candidate's input.
                let phase = state.phase;
                state
                    .service
                    .admitted(token, service_started, phase, NativeOutputAdmission::Defer);
                return NativeOutputAdmission::Defer;
            }
        }
        // Legacy W2 cases isolate output pressure from roster backing. Combined
        // W16 pressure keeps every borrowed complete inventory observation.
        // Live window callbacks and exact output snapshots always pass through.
        let root_roster = if {
            let state = lock(&self.shared);
            state.phase == Phase::Filling && state.combined.is_none()
        } {
            None
        } else {
            root_roster
        };
        let admission = self.inner.on_output_begin(token, window, root_roster);
        let after = self
            .runtime
            .try_lock()
            .expect("native admission must precede the application borrow")
            .output_backlog();
        let (should_flush, service_phase) = {
            let mut state = lock(&self.shared);
            state.peak_ordinary = state.peak_ordinary.max(after.ordinary());
            state.peak_control = state.peak_control.max(after.control());
            assert!(after.ordinary() <= ORDINARY_OUTPUTS);
            if state.phase == Phase::Filling && after.ordinary() == ORDINARY_OUTPUTS {
                if token.viewport_id() == ViewportId::ROOT && after.control() > before.control() {
                    state.root_control_seen = true;
                    assert!(state.root_control.replace(token).is_none());
                    eprintln!("output-pressure: full ordinary backing; root control admitted");
                } else if token.viewport_id() != ViewportId::ROOT
                    && after.control() > before.control()
                {
                    assert_eq!(after.protected_staging(), 1);
                    state.protected_child_seen = true;
                    assert!(state.protected_child.replace(token).is_none());
                    eprintln!("output-pressure: full ordinary backing; protected child admitted");
                }
            }
            let flush = admission == NativeOutputAdmission::Defer
                && token.viewport_id() == ViewportId::ROOT
                && state.phase == Phase::Filling
                && state.root_control_seen
                && state.protected_child_seen;
            state.full_root_defer_seen |= flush;
            if flush && let Some(combined) = &mut state.combined {
                combined.check_full_roster(token, root_roster);
            }
            (flush, state.phase)
        };
        if should_flush {
            // Calling the inner handler only returns already owned terminal
            // facts. It does not re-enter eframe or run another viewport pass.
            self.flush();
        }
        lock(&self.shared)
            .service
            .admitted(token, service_started, service_phase, admission);
        admission
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        let token =
            eframe::current_native_output_token().expect("finalization has an active real output");
        let wake = self.inner.on_output_pass_finalized(finalization);
        let ended = Instant::now();
        lock(&self.shared).service.finalized(token, ended);
        wake
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        lock(&self.shared).service.terminal(result.token());
        self.flush_late_output_after_fatal();
        let first_live_pending = {
            let state = lock(&self.shared);
            state.scenario == Scenario::LateFirstLive
                && state.phase == Phase::AwaitFirstLive
                && Some(result.token().viewport_id()) == state.child_viewport
                && Some(result.token()) == state.child_content_output
                && result.status() == eframe::NativeOutputStatus::Presented
        };
        if first_live_pending {
            let previous = {
                let runtime = lock(&self.runtime);
                let mut state = lock(&self.shared);
                assert_eq!(runtime.error_kind(), None);
                state
                    .check_source_ownership(&runtime)
                    .expect("full first-live content must leave durable source ownership in place");
                let child = state
                    .child_surface
                    .expect("the requested child identity has already been returned");
                assert!(!runtime.is_surface_presented(child));
                assert!(!runtime.is_quiescent());
                let backlog = runtime.output_backlog();
                assert!(backlog.ordinary() > 0);
                assert_eq!(backlog.protected_staging(), 0);
                assert_eq!(backlog.awaiting_staging(), 0);
                let previous = state
                    .previous_terminal
                    .expect("a previously delivered real output precedes the first-live child");
                assert!(previous.token().same_context(result.token()));
                assert_ne!(previous.token(), result.token());
                assert!(previous.ordinal() < result.ordinal());
                assert!(state.buffered.is_empty());
                assert!(state.withheld_first_live.replace(result).is_none());
                state.buffered.push_back(result);
                state.phase = Phase::Draining;
                previous
            };
            self.inner.on_output(previous);
            eprintln!(
                "output-pressure: withholding real full-content first-live terminal; \
                 source ownership preserved; duplicating a previously delivered real output"
            );
            return NativeHostWake::RepaintRootAfterCurrent;
        }
        {
            let mut state = lock(&self.shared);
            if state.phase == Phase::AwaitChild
                && result.token().viewport_id() != ViewportId::ROOT
                && state
                    .combined
                    .as_ref()
                    .is_none_or(|combined| !combined.is_satellite(result.token().viewport_id()))
            {
                if let Some(combined) = &mut state.combined {
                    combined.observe_child(result.token());
                }
                state.child_viewport = Some(result.token().viewport_id());
                state.phase = if state.scenario == Scenario::LateFirstLive {
                    Phase::AwaitFirstLive
                } else {
                    Phase::AwaitStaging
                };
            }
            if state.phase == Phase::AwaitStaging
                && result.token().viewport_id() == ViewportId::ROOT
                && state.child_paused
                && state.root_roster_has_child
                && result.status() == eframe::NativeOutputStatus::Presented
                && self
                    .runtime
                    .try_lock()
                    .expect("terminal delivery follows the application borrow")
                    .output_backlog()
                    .awaiting_staging()
                    == 1
            {
                state.phase = Phase::Filling;
                eprintln!(
                    "output-pressure: child creation observed; staging candidate deferred; filling root"
                );
            }
            if state.phase == Phase::Filling {
                if state.root_control == Some(result.token()) {
                    assert_eq!(result.status(), eframe::NativeOutputStatus::NotPresented);
                    state.root_terminal_seen = true;
                }
                if state.protected_child == Some(result.token()) {
                    assert_eq!(result.status(), eframe::NativeOutputStatus::Presented);
                    let backlog = self
                        .runtime
                        .try_lock()
                        .expect("terminal delivery follows the viewport UI borrow")
                        .output_backlog();
                    assert_eq!(backlog.protected_staging(), 1);
                    state.protected_terminal_seen = true;
                }
                if let Some(previous) = state.buffered.back() {
                    assert_eq!(result.ordinal().get(), previous.ordinal().get() + 1);
                }
                assert!(state.buffered.len() < BUFFERED_TERMINALS);
                state.buffered.push_back(result);
                if state.buffered.len() % 32 == 0 {
                    eprintln!(
                        "output-pressure: buffered {} real terminals",
                        state.buffered.len()
                    );
                }
                return NativeHostWake::RepaintRootAfterCurrent;
            }
        }
        let wake = self.inner.on_output(result);
        lock(&self.shared).previous_terminal = Some(result);
        if result.token().viewport_id() == ViewportId::ROOT
            && lock(&self.shared).phase == Phase::Done
        {
            let runtime = lock(&self.runtime);
            let state = lock(&self.shared);
            assert_eq!(
                runtime.output_backlog().ordinary(),
                0,
                "final root terminal: checked_before={}, result={result:?}",
                state.final_terminal_checked,
            );
            assert_eq!(runtime.output_backlog().control(), 0);
            if matches!(
                state.scenario,
                Scenario::LateStaging | Scenario::LateFirstLive
            ) {
                state
                    .check_source_ownership(&runtime)
                    .expect("final root terminal must preserve the untransferred source");
                assert_eq!(runtime.output_backlog().protected_staging(), 0);
                assert_eq!(runtime.output_backlog().awaiting_staging(), 0);
            }
            if !state.scenario.expects_success() {
                assert!(runtime.is_shutdown_drained());
            }
            runtime.with_errors(|primary, cleanup| {
                assert!(cleanup.is_empty());
                if state.scenario.expects_success() {
                    assert!(
                        primary.is_none(),
                        "unexpected native primary error: {primary:?}"
                    );
                } else {
                    assert!(primary.is_some_and(|error| error.is_output_order_violation()));
                }
            });
            drop(state);
            lock(&self.shared).final_terminal_checked = true;
        }
        wake
    }

    fn on_viewport_create_failed(
        &self,
        failure: eframe::NativeViewportCreateFailure,
    ) -> NativeHostWake {
        self.inner.on_viewport_create_failed(failure)
    }

    fn on_viewport_visibility(
        &self,
        result: eframe::NativeViewportVisibilityResult,
    ) -> NativeHostWake {
        self.inner.on_viewport_visibility(result)
    }

    fn on_viewport_close_cancelled(
        &self,
        request: eframe::NativeViewportCloseRequest,
    ) -> NativeHostWake {
        self.inner.on_viewport_close_cancelled(request)
    }
}
