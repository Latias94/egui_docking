//! A bounded real-window check, separate from GPUI's deterministic CPU tests.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::Duration,
};

use open_gpui::{App, Window};

use crate::{HostState, ItemId, SharedHost, input};

const TIMEOUT: Duration = Duration::from_secs(12);
const VISIBLE_PANES: [ItemId; 3] = [ItemId::new(1), ItemId::new(3), ItemId::new(4)];

#[derive(Clone, Default)]
pub(super) struct Smoke {
    complete: Rc<Cell<bool>>,
}

impl Smoke {
    pub(super) fn install(&self, host: &SharedHost, window: &Window, cx: &App) {
        if cx.is_test() {
            self.fail("--smoke requires the real platform, not GPUI's CPU test platform");
        }
        let on_quit = self.clone();
        cx.on_app_quit(move |_| {
            on_quit.require_complete("application quit before exact presentation completed");
            std::future::ready(())
        })
        .detach();

        let watchdog = self.clone();
        let weak = Rc::downgrade(host);
        cx.spawn(async move |cx| {
            cx.background_executor().timer(TIMEOUT).await;
            if !watchdog.complete.get() {
                let detail = weak.upgrade().map(|host| {
                    let host = host.borrow();
                    format!(
                        "serial={}, pending={}, core_presented={}, adapter_error={:?}",
                        host.serial,
                        host.pending.is_some(),
                        input::is_presented(&host, host.serial),
                        host.error,
                    )
                });
                watchdog.fail(format!(
                    "presentation timed out after 12 seconds ({detail:?})"
                ));
            }
        })
        .detach();
        self.observe(Rc::downgrade(host), window);
    }

    fn observe(&self, weak: Weak<RefCell<HostState>>, window: &Window) {
        let smoke = self.clone();
        window.on_next_frame(move |window, cx| {
            if smoke.complete.get() {
                return;
            }
            let Some(shared) = weak.upgrade() else {
                smoke.fail("workbench was released before exact presentation completed");
            };
            let host = shared.borrow();
            if let Some(error) = &host.error {
                smoke.fail(format!("adapter failed: {error}"));
            }
            let facts = window.presentation_facts();
            let actual_panes = host.packet.as_ref().is_some_and(|packet| {
                VISIBLE_PANES.iter().all(|item| {
                    host.panels.contains_key(item) && packet.contains_pane(*item)
                })
            });
            // `is_presented` matches the current packet to core's acknowledged artifact.
            // That artifact is set only after its exact ticket reaches Submitted.
            let ready = facts.native_created
                && facts.native_visible
                && facts.non_empty_presented_generation.is_some()
                && host.pending.is_none()
                && input::is_presented(&host, host.serial)
                && actual_panes;
            if ready {
                let serial = host.serial;
                drop(host);
                smoke.complete.set(true);
                println!(
                    "SMOKE OK: native window visible; exact submission Submitted; core artifact accepted; Editor, Inspector and Tools AnyViews composed (packet {serial})"
                );
                cx.quit();
            } else {
                drop(host);
                smoke.observe(weak, window);
            }
        });
    }

    pub(super) fn require_complete(&self, reason: &str) {
        if !self.complete.get() {
            self.fail(reason);
        }
    }

    pub(super) fn fail(&self, reason: impl std::fmt::Display) -> ! {
        eprintln!("SMOKE FAILED: {reason}");
        // macOS Platform::quit uses NSApplication::terminate and exits successfully.
        // A failed executable probe must retain its failure status on that path too.
        std::process::exit(1);
    }
}
