//! Explicit headless presentation, owned-run helpers and bounded observation waits.

use egui::{Context, Event, FullOutput, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceCapability, DockspaceCloseRequest, PaneView, SurfacePresentationResult,
    run_ui,
};

use super::{FocusPanes, Panes, SURFACE};

pub(super) fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    }
}

pub(super) fn run_headless(
    context: &Context,
    input: RawInput,
    add_contents: impl FnMut(&mut Ui),
) -> FullOutput {
    let (output, receipt) = run_ui(context, input, add_contents).into_parts();
    // This headless owner accepts the terminal frame; callers inspect its output and clear textures.
    receipt.report(SurfacePresentationResult::Presented);
    output
}

pub(super) fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> FrameOutput {
    run_pane_frame(context, dockspace, panes, events)
}

pub(super) fn run_pane_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    events: Vec<Event>,
) -> FrameOutput {
    run_pane_frame_with_mutation(context, dockspace, panes, events).0
}

pub(super) fn run_pane_frame_with_mutation(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    events: Vec<Event>,
) -> (FrameOutput, bool, bool) {
    let (output, response) = dockspace
        .run_single_surface(context, input(events), SURFACE, panes)
        .expect("the owned product run publishes once");
    let close_requests = response.close_request_events().to_vec();
    let workspace_changed = response.mutation().workspace_changed();
    let published_state_changed = response.mutation().published_state_changed();
    let (mut output, receipt) = output.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    (
        FrameOutput {
            output,
            close_requests,
        },
        workspace_changed,
        published_state_changed,
    )
}

pub(super) fn run_disabled_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> FrameOutput {
    let mut close_requests = Vec::new();
    let input = input(events);
    let mut output = run_headless(context, input, |ui| {
        ui.add_enabled_ui(false, |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("the disabled product frame advances");
            close_requests.extend(response.close_request_events().iter().cloned());
        });
    });
    output.textures_delta.clear();
    FrameOutput {
        output,
        close_requests,
    }
}

pub(super) fn run_frame_with_discard_after_dockspace(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> (FrameOutput, usize) {
    let mut close_requests = Vec::new();
    let mut passes = 0;
    let input = input(events);
    let mut output = run_headless(context, input, |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the product multipass frame advances");
        close_requests.extend(response.close_request_events().iter().cloned());
        passes += 1;
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx()
                .request_discard("exercise product response action retention");
        }
    });
    output.textures_delta.clear();
    (
        FrameOutput {
            output,
            close_requests,
        },
        passes,
    )
}

pub(super) fn run_two_discarded_dockspace_passes(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    events: Vec<Event>,
) -> usize {
    let mut passes = 0;
    let input = input(events);
    let mut output = run_headless(context, input, |ui| {
        passes += 1;
        if ui.ctx().current_pass_index() < 2 {
            dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("the discarded product pass advances");
        } else {
            ui.label("terminal pass intentionally omits the dockspace");
        }
    });
    output.textures_delta.clear();
    passes
}

pub(super) fn run_focus_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut FocusPanes,
    events: Vec<Event>,
) -> (FrameOutput, DockspaceCapability) {
    let (output, response) = dockspace
        .run_single_surface(context, input(events), SURFACE, panes)
        .expect("the pane-focus owned product run advances");
    let close_requests = response.close_request_events().to_vec();
    let pane_focus_capability = response.pane_focus_capability();
    let (mut output, receipt) = output.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    (
        FrameOutput {
            output,
            close_requests,
        },
        pane_focus_capability,
    )
}

pub(super) struct FrameOutput {
    pub(super) output: egui::FullOutput,
    pub(super) close_requests: Vec<DockspaceCloseRequest>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CausalSettlementTimeout {
    budget: usize,
    missing: Vec<&'static str>,
}

impl std::fmt::Display for CausalSettlementTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "causal settlement exceeded {} observations; missing {}",
            self.budget,
            self.missing.join(", "),
        )
    }
}

pub(super) fn settle_until<T>(
    budget: usize,
    mut observe: impl FnMut() -> (T, Vec<&'static str>),
) -> Result<T, CausalSettlementTimeout> {
    assert!(budget > 0, "a causal settlement budget must be non-zero");
    let mut last_missing = Vec::new();
    for _ in 0..budget {
        let (value, missing) = observe();
        if missing.is_empty() {
            return Ok(value);
        }
        last_missing = missing;
    }
    Err(CausalSettlementTimeout {
        budget,
        missing: last_missing,
    })
}

#[test]
pub(super) fn causal_settlement_timeout_names_the_missing_observations() {
    let timeout = settle_until(2, || {
        ((), vec!["magnetic Dock preview", "terminal drag state"])
    })
    .expect_err("the predicate never becomes causally ready");
    assert_eq!(
        timeout.to_string(),
        "causal settlement exceeded 2 observations; missing magnetic Dock preview, terminal drag state",
    );
}
