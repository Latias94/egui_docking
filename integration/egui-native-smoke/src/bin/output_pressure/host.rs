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
            let Some(result) = lock(&self.shared).buffered.pop_front() else {
                break;
            };
            self.inner.on_output(result);
            lock(&self.shared).forwarded += 1;
            if scenario == Scenario::Duplicate && lock(&self.shared).forwarded == 1 {
                self.inner.on_output(result);
            }
        }
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
            if token.viewport_id() != ViewportId::ROOT && state.phase == Phase::AwaitStaging {
                state.child_paused = true;
                return NativeOutputAdmission::Defer;
            }
            if token.viewport_id() != ViewportId::ROOT
                && state.phase == Phase::Filling
                && (!state.root_control_seen || before.ordinary() != ORDINARY_OUTPUTS)
            {
                // Keep the admitted create/staging obligation while root output
                // owners fill. The framework retains this candidate's input.
                return NativeOutputAdmission::Defer;
            }
        }
        // Isolate output-owner pressure from the independent two-roster backing
        // limit. These borrowed inventory observations have not been admitted;
        // the next fresh complete roster resumes after terminal delivery. Live
        // window callbacks and each output's exact snapshot still pass through.
        let root_roster = if lock(&self.shared).phase == Phase::Filling {
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
        let should_flush = {
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
            flush
        };
        if should_flush {
            // Calling the inner handler only returns already owned terminal
            // facts. It does not re-enter eframe or run another viewport pass.
            self.flush();
        }
        admission
    }

    fn on_output_pass_finalized(
        &self,
        finalization: eframe::NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        self.inner.on_output_pass_finalized(finalization)
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        {
            let mut state = lock(&self.shared);
            if state.phase == Phase::AwaitChild && result.token().viewport_id() != ViewportId::ROOT
            {
                state.child_viewport = Some(result.token().viewport_id());
                state.phase = Phase::AwaitStaging;
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
            if state.scenario != Scenario::Ordered {
                assert!(runtime.is_shutdown_drained());
            }
            runtime.with_errors(|primary, cleanup| {
                assert!(cleanup.is_empty());
                if state.scenario == Scenario::Ordered {
                    assert!(primary.is_none());
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
