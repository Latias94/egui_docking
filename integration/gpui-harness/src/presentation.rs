//! Exact GPUI frame acceptance and renderer-present results for affine core outputs.

use std::rc::Rc;

use dockspace::runtime::{
    DockspaceUpdate, PaintedSurfaceOutput, SurfacePresentationResult, SurfaceUnavailableReason,
};
use open_gpui::{
    AcceptedFrameFence, App, Window, WindowFrameSubmissionOutcome, WindowFrameSubmissionTicket,
};

use crate::{
    HostState, SharedHost,
    continuation::{self, InputFrame},
    element::Packet,
    input::QueuedPointer,
};

pub(super) struct PendingSubmission {
    pub(super) serial: u64,
    pub(super) ticket: WindowFrameSubmissionTicket,
    pub(super) packet: Rc<Packet>,
    pub(super) update: Option<DockspaceUpdate>,
    pub(super) inputs: Vec<QueuedPointer>,
    output: Option<PaintedSurfaceOutput>,
    accepted_generation: Option<u64>,
}

impl PendingSubmission {
    pub(super) fn new(
        serial: u64,
        ticket: WindowFrameSubmissionTicket,
        packet: Rc<Packet>,
        update: DockspaceUpdate,
        inputs: Vec<QueuedPointer>,
    ) -> Self {
        Self {
            serial,
            ticket,
            packet,
            update: Some(update),
            inputs,
            output: None,
            accepted_generation: None,
        }
    }
}

/// Retries one unaccepted framework candidate without consuming its input prefix.
pub(super) fn abandon_unaccepted(host: &mut HostState, window: &mut Window) {
    if !host
        .pending
        .as_ref()
        .is_some_and(|pending| pending.update.is_some())
    {
        return;
    }
    withdraw_pending(host, window);
    host.dirty = true;
}

pub(super) fn observe(
    shared: &SharedHost,
    ticket: &WindowFrameSubmissionTicket,
    serial: u64,
    window: &mut Window,
) {
    let weak = Rc::downgrade(shared);
    window.on_exact_frame_submission_presented(ticket, move |snapshot, window, cx| {
        let Some(shared) = weak.upgrade() else {
            return;
        };
        let mut host = shared.borrow_mut();
        if !host
            .pending
            .as_ref()
            .is_some_and(|pending| pending.serial == serial)
        {
            return;
        }
        let Some(mut pending) = host.pending.take() else {
            return;
        };
        let exact = pending.accepted_generation.is_some()
            && snapshot.accepted_frame_generation() == pending.accepted_generation
            && snapshot.submitted_frame_generation() == pending.accepted_generation;
        let result = if snapshot.outcome() == WindowFrameSubmissionOutcome::Submitted && exact {
            SurfacePresentationResult::Presented
        } else {
            SurfacePresentationResult::Dropped
        };
        window.retire_exact_frame_submission(&pending.ticket);
        if let Some(output) = pending.output.take() {
            let artifact = output.presentation_artifact();
            if let Err(error) = host.session.report_surface_presentation(output, result) {
                host.fail(error);
            }
            if let Err(error) = settle(&mut host) {
                host.fail(error);
            }
            if result == SurfacePresentationResult::Presented {
                host.presented = Some((serial, artifact));
            }
        }
        if result == SurfacePresentationResult::Dropped {
            host.dirty = true;
        }
        if host.dirty || !host.inputs.is_empty() {
            window.refresh();
        }
        drop(host);
        continuation::schedule(&shared, window, cx);
    });
}

pub(super) fn accept(
    shared: &SharedHost,
    serial: u64,
    input_frame: InputFrame,
    fence: AcceptedFrameFence,
    window: &mut Window,
    cx: &App,
) {
    let mut host = shared.borrow_mut();
    if serial != host.serial {
        return;
    }
    host.input_frame = Some(input_frame);
    if !host
        .pending
        .as_ref()
        .is_some_and(|pending| pending.serial == serial)
    {
        drop(host);
        continuation::schedule(shared, window, cx);
        return;
    }
    let Some(mut pending) = host.pending.take() else {
        return;
    };
    if let Some(update) = pending.update.take() {
        let result = host
            .session
            .resume_update(update)
            .and_then(|frame| frame.commit());
        match result {
            Ok(mut report) => {
                host.dirty |= report.repaint_surfaces().contains(&crate::SURFACE);
                pending.output = report.take_painted_outputs().into_iter().next();
                pending.inputs.clear();
                host.packet = Some(pending.packet.clone());
            }
            Err(error) => {
                pending.inputs.append(&mut host.inputs);
                host.inputs = pending.inputs;
                host.dirty = true;
                host.fail(error);
                window.retire_exact_frame_submission(&pending.ticket);
                window.refresh();
                return;
            }
        }
    }
    pending.accepted_generation = Some(fence.generation());
    host.pending = Some(pending);
    drop(host);
    continuation::schedule(shared, window, cx);
}

pub(super) fn accept_focus_stable(
    shared: &SharedHost,
    serial: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let host = shared.borrow();
    let Some(pending) = host
        .pending
        .as_ref()
        .filter(|pending| pending.serial == serial)
    else {
        return;
    };
    if let Some(generation) = pending.accepted_generation {
        if let Err(error) =
            window.accept_exact_frame_submission_frame(&pending.ticket, generation, cx)
        {
            eprintln!("GPUI docking exact submission: {error:#}");
            window.refresh();
        }
    }
}

/// Retracts this publication when its subtree disappears from an accepted GPUI frame.
pub(super) fn discard(shared: &SharedHost, window: &mut Window) {
    let mut host = shared.borrow_mut();
    // The callback may come from the previous accepted packet after a newer
    // candidate was rolled back. Publication loss retracts the entire host.
    withdraw_pending(&mut host, window);
    continuation::cancel(&mut host);
    host.input_frame = None;
    host.packet = None;
    host.presented = None;
    host.dirty = true;
}

fn withdraw_pending(host: &mut HostState, window: &mut Window) {
    continuation::cancel(host);
    let Some(mut pending) = host.pending.take() else {
        return;
    };
    window.retire_exact_frame_submission(&pending.ticket);
    drop(pending.update.take());
    if let Some(output) = pending.output.take() {
        if let Err(error) = host
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        {
            host.fail(error);
        }
    } else {
        // New edges, including platform cancellation, follow the unpublished prefix.
        pending.inputs.append(&mut host.inputs);
        host.inputs = pending.inputs;
    }
}

fn settle(host: &mut HostState) -> anyhow::Result<()> {
    let mut frame = host.session.begin_host_frame()?;
    frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
    let report = frame.commit()?;
    host.dirty |= !report.repaint_surfaces().is_empty();
    Ok(())
}

pub(super) fn close(host: &mut HostState) {
    // Affine output drop retires an unsubmitted or terminal-window artifact as Dropped.
    host.pending = None;
    host.packet = None;
    host.presented = None;
    host.inputs.clear();
    continuation::cancel(host);
    host.input_frame = None;
    let _ = host.session.disable_host_pointer();
}
