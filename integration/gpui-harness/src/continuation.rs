//! One-shot scheduling for the committed core gesture's stationary work.

use std::rc::Rc;

use dockspace::runtime::DockspacePointerContinuation;
use open_gpui::{
    App, Bounds, HitboxId, Pixels, PlatformHoveredWindow, PointerCaptureHandle, Task, Window,
};

use crate::{HostState, SURFACE, SharedHost, input};

#[derive(Clone)]
pub(super) struct InputFrame {
    pub(super) serial: u64,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) hitboxes: Vec<HitboxId>,
    pub(super) capture: PointerCaptureHandle,
}

pub(super) struct Wake {
    request: DockspacePointerContinuation,
    serial: u64,
    _task: Task<()>,
}

pub(super) fn cancel(host: &mut HostState) {
    host.wake = None;
}

/// Schedule only against an accepted packet with its exact Submitted artifact.
pub(super) fn schedule(shared: &SharedHost, window: &Window, cx: &App) {
    let mut host = shared.borrow_mut();
    let ready = host.input_frame.as_ref().and_then(|frame| {
        let request = host.session.pointer_continuation()?;
        (host.pending.is_none()
            && host.inputs.is_empty()
            && frame.serial == host.serial
            && request.surface() == SURFACE
            && input::is_presented(&host, frame.serial))
        .then_some((request, frame.serial))
    });
    let Some((request, serial)) = ready else {
        cancel(&mut host);
        return;
    };
    if host
        .wake
        .as_ref()
        .is_some_and(|wake| wake.request == request && wake.serial == serial)
    {
        return;
    }
    cancel(&mut host);
    let timer = host
        .clock
        .timer(request.deadline().saturating_sub(host.observed_at()));
    let weak = Rc::downgrade(shared);
    let task = window.spawn(cx, async move |cx| {
        timer.await;
        let Some(shared) = weak.upgrade() else {
            return;
        };
        let _ = cx.update(|window, cx| wake(&shared, request, serial, window, cx));
    });
    host.wake = Some(Wake {
        request,
        serial,
        _task: task,
    });
}

fn wake(
    shared: &SharedHost,
    request: DockspacePointerContinuation,
    serial: u64,
    window: &mut Window,
    cx: &App,
) {
    let mut host = shared.borrow_mut();
    if !host
        .wake
        .as_ref()
        .is_some_and(|wake| wake.request == request && wake.serial == serial)
    {
        return;
    }
    cancel(&mut host);
    let Some(frame) = host.input_frame.clone() else {
        return;
    };
    let window_hovered = match cx.hovered_window() {
        PlatformHoveredWindow::Window(handle) => handle == window.window_handle(),
        PlatformHoveredWindow::NoWindow => false,
        PlatformHoveredWindow::Unavailable => window.is_window_hovered(),
    };
    let position = window.mouse_position();
    if host.session.pointer_continuation() != Some(request)
        || host.observed_at() < request.deadline()
        || host.pending.is_some()
        || !host.inputs.is_empty()
        || frame.serial != serial
        || serial != host.serial
        || !input::is_presented(&host, serial)
        || !window.is_window_active()
        || !window_hovered
        || !frame.capture.is_captured(window)
        || input::logical_point(position, frame.bounds) != Some(request.position())
        || !frame
            .hitboxes
            .iter()
            .any(|hitbox| hitbox.window_point_target_rank(position, window) == Some(0))
    {
        return;
    }
    // This is a new stationary observation, never a replay of the motion that armed it.
    if input::queue(
        &mut host,
        serial,
        request.event(),
        position,
        frame.bounds,
        &frame.hitboxes,
        true,
        window.modifiers().shift,
        window,
    ) {
        window.refresh();
    }
}
