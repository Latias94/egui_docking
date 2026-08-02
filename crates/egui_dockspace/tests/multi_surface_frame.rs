use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::presentation_observation::HostPresentationObservationOutcome;
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::transition::SurfaceContributionOutcome;
use egui::{Context, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceError, DockspaceSurfaceStatus, EguiFrameScheduleKey, EguiOuterFrameCommit,
    EguiOuterSurfaceOutput, EguiPresentationResult, HostFrameResponse, PaneView,
    SurfaceFrameDisposition,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const CHILD_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT_ROOT: RootId = RootId::new(10);
const CHILD_ROOT: RootId = RootId::new(11);
const ROOT_ITEM: ItemId = ItemId::new(100);
const CHILD_ITEM: ItemId = ItemId::new(200);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

#[derive(Default)]
struct CountingPane {
    ui_calls: usize,
    disabled_ui_calls: usize,
}

impl PaneView for CountingPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        self.ui_calls += 1;
        self.disabled_ui_calls += usize::from(!ui.is_enabled());
    }
}

#[derive(Default)]
struct DiscardingPane {
    ui_calls: usize,
}

impl PaneView for DiscardingPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        self.ui_calls += 1;
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx().request_discard("exercise final-pass replacement");
        }
    }
}

fn single_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ROOT_ITEM]));
    builder.set_root(ROOT_ROOT, RootRecord::new(tabs));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::with_main(ROOT_ROOT));
    builder.build().expect("single-surface fixture is valid")
}

fn multi_surface_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let root_tabs = builder.insert_node(Node::tabs([ROOT_ITEM]));
    let child_tabs = builder.insert_node(Node::tabs([CHILD_ITEM]));
    builder.set_root(ROOT_ROOT, RootRecord::new(root_tabs));
    builder.set_root(CHILD_ROOT, RootRecord::new(child_tabs));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::with_main(ROOT_ROOT));
    builder.set_surface(CHILD_SURFACE, SurfacePresentation::with_main(CHILD_ROOT));
    builder.build().expect("multi-surface fixture is valid")
}

fn one_pass_context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    context
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        ..RawInput::default()
    }
}

fn paint_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) -> HostFrameResponse {
    let mut host = dockspace.begin_host_frame(key).expect("host frame begins");
    let mut paint = None;
    let _ = context.run_ui(input(), |ui| {
        paint = Some(host.show_surface(ROOT_SURFACE, ui, panes));
    });
    paint
        .expect("egui invokes the surface callback")
        .expect("surface paints");
    host.end_host_frame().expect("host frame commits")
}

fn paint_crates_io_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) -> egui_dockspace::DockspaceResponse {
    let mut response = None;
    let _ = context.run_ui(input(), |ui| {
        response = Some(
            dockspace
                .show_single_surface(ROOT_SURFACE, ui, panes)
                .expect("single-surface frame paints"),
        );
    });
    response.expect("egui invokes the surface callback")
}

fn paint_outer_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) -> EguiOuterFrameCommit {
    let mut host = dockspace
        .begin_outer_frame(key)
        .expect("outer host frame begins");
    host.run_surface(ROOT_SURFACE, context, input(), panes)
        .expect("outer host owns and confirms the surface run");
    host.finish().expect("outer host frame commits")
}

fn paint_multi_surface_outer_frame(
    root_context: &Context,
    child_context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) -> EguiOuterFrameCommit {
    let mut host = dockspace
        .begin_outer_frame(key)
        .expect("outer host frame begins");
    for (surface, context) in [(ROOT_SURFACE, root_context), (CHILD_SURFACE, child_context)] {
        host.run_surface(surface, context, input(), panes)
            .expect("outer host owns and confirms each surface run");
    }
    host.finish().expect("outer host frame commits")
}

fn bootstrap_outer_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) {
    let (_, outputs) = paint_outer_frame(context, dockspace, panes, key).into_parts();
    assert_eq!(outputs.len(), 1);
    assert!(
        outputs
            .iter()
            .all(|output| !output.has_presentation_obligation()),
        "a prepared-only bootstrap paint must not create a presentation obligation",
    );
}

fn bootstrap_multi_surface_outer_frame(
    root_context: &Context,
    child_context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) {
    let (_, outputs) =
        paint_multi_surface_outer_frame(root_context, child_context, dockspace, panes, key)
            .into_parts();
    assert_eq!(outputs.len(), 2);
    assert!(
        outputs
            .iter()
            .all(|output| !output.has_presentation_obligation()),
        "prepared-only bootstrap paints must not create presentation obligations",
    );
}

fn one_presentation(mut presentations: Vec<EguiOuterSurfaceOutput>) -> EguiOuterSurfaceOutput {
    assert_eq!(presentations.len(), 1, "one surface emits one output token");
    presentations.pop().expect("one output token exists")
}

fn complete_presentations(
    presentations: Vec<EguiOuterSurfaceOutput>,
    result: EguiPresentationResult,
) {
    for presentation in presentations {
        presentation.settle_with(|_, _| result);
    }
}

#[test]
fn bootstrap_paints_the_application_pane_in_a_disabled_scope() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("bootstrap-pane", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = CountingPane::default();

    let response = paint_crates_io_frame(&context, &mut dockspace, &mut panes);

    assert_eq!(response.surface_status(), DockspaceSurfaceStatus::Bootstrap);
    assert!(!response.interactions_current());
    assert_eq!(panes.ui_calls, 1);
    assert_eq!(panes.disabled_ui_calls, 1);
}

#[test]
fn crates_io_facade_never_synthesizes_presentation_authority() {
    // This integration target links `egui_dockspace` without `cfg(test)`, so no
    // test-only terminal-presentation provider can authorize these frames.
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("crates-io-paint-only", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = CountingPane::default();

    for frame in 0..8 {
        let response = paint_crates_io_frame(&context, &mut dockspace, &mut panes);
        let transition = &response.transitions()[0];

        assert!(transition.presentation_emissions().is_empty());
        assert!(transition.presentation_observations().is_empty());
        assert!(!response.interactions_current());
        assert_eq!(panes.ui_calls, frame + 1);
        if frame == 0 {
            assert_eq!(response.surface_status(), DockspaceSurfaceStatus::Bootstrap);
            assert_eq!(panes.disabled_ui_calls, 1);
        } else {
            assert_eq!(response.surface_status(), DockspaceSurfaceStatus::Ready);
            assert_eq!(
                panes.disabled_ui_calls, 1,
                "a current paint-only pane must remain enabled without docking authority"
            );
        }
    }

    let diagnostics = dockspace.engine().presentation_ledger_diagnostics();
    assert_eq!(diagnostics.active_streams(), 0);
    assert_eq!(diagnostics.pending_streams(), 0);
    assert_eq!(diagnostics.pending_outputs(), 0);
}

#[test]
fn single_surface_host_frame_commits_through_the_core_capability() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("core-host-frame", single_workspace())
        .build()
        .expect("fixture builds");
    let before_tick = dockspace.engine().last_reducer_tick();
    let mut panes = TestPanes;

    let response = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    assert_eq!(response.transition().tick().get(), before_tick.get() + 1);
    assert_eq!(response.surfaces().len(), 1);
    assert!(matches!(
        response
            .surface(ROOT_SURFACE)
            .expect("single response exists")
            .disposition(),
        SurfaceFrameDisposition::Contribution(SurfaceContributionOutcome::Ready { .. })
    ));
}

#[test]
fn outer_host_frame_commits_the_complete_multi_surface_roster_once() {
    let root_context = one_pass_context();
    let child_context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-complete-roster", multi_surface_workspace())
        .build()
        .expect("fixture builds");
    let before_tick = dockspace.engine().last_reducer_tick();
    let mut panes = TestPanes;
    let (response, presentations) = paint_multi_surface_outer_frame(
        &root_context,
        &child_context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    )
    .into_parts();
    assert_eq!(presentations.len(), 2);
    assert_eq!(
        presentations
            .iter()
            .map(EguiOuterSurfaceOutput::surface)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([ROOT_SURFACE, CHILD_SURFACE])
    );
    assert!(
        presentations
            .iter()
            .all(|output| !output.has_presentation_obligation())
    );
    assert_eq!(response.transition().tick().get(), before_tick.get() + 1);
    assert_eq!(response.surfaces().len(), 2);
    assert_eq!(response.transition().surface_contributions().len(), 2);
    assert!(response.surface(ROOT_SURFACE).is_some());
    assert!(response.surface(CHILD_SURFACE).is_some());
}

#[test]
fn multi_surface_renderer_results_settle_independently() {
    let root_context = one_pass_context();
    let child_context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-independent-results", multi_surface_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_multi_surface_outer_frame(
        &root_context,
        &child_context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, first) = paint_multi_surface_outer_frame(
        &root_context,
        &child_context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(EguiOuterSurfaceOutput::has_presentation_obligation)
    );
    for presentation in first {
        let result = if presentation.surface() == ROOT_SURFACE {
            EguiPresentationResult::Presented
        } else {
            EguiPresentationResult::Dropped
        };
        presentation.settle_with(|_, _| result);
    }

    let (second, pending) = paint_multi_surface_outer_frame(
        &root_context,
        &child_context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    let retired = second
        .transition()
        .presentation_observations()
        .iter()
        .filter_map(|outcome| match outcome {
            HostPresentationObservationOutcome::Retired { presented, .. } => Some(presented),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(retired.len(), 2);
    assert_eq!(
        retired
            .iter()
            .filter(|presented| matches!(presented, dockspace::intent::Authority::Known(Some(_))))
            .count(),
        1
    );
    assert_eq!(
        retired
            .iter()
            .filter(|presented| matches!(presented, dockspace::intent::Authority::Known(None)))
            .count(),
        1
    );
    complete_presentations(pending, EguiPresentationResult::Dropped);
}

#[test]
fn outer_host_frame_replaces_an_earlier_egui_pass_before_reduction() {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 2.try_into().expect("two is non-zero");
    });
    let mut dockspace = Dockspace::builder("outer-final-pass", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = DiscardingPane::default();
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer host frame begins");
    host.run_surface(ROOT_SURFACE, &context, input(), &mut panes)
        .expect("outer host owns the complete multipass run");

    assert_eq!(panes.ui_calls, 2);
    let (response, outputs) = host.finish().expect("final pass commits").into_parts();
    assert_eq!(
        one_presentation(outputs)
            .full_output()
            .platform_output
            .num_completed_passes,
        2
    );
    assert_eq!(response.surfaces().len(), 1);
    assert_eq!(response.transition().surface_contributions().len(), 1);
}

#[test]
fn outer_host_frame_rejects_a_duplicate_callback_in_the_same_pass() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-duplicate-pass", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer host frame begins");
    let mut duplicate = None;

    let _ = context.run_ui(input(), |ui| {
        host.show_surface(ROOT_SURFACE, ui, &mut panes)
            .expect("first callback paints");
        duplicate = Some(host.show_surface(ROOT_SURFACE, ui, &mut panes));
    });

    assert!(matches!(
        duplicate.expect("duplicate callback runs"),
        Err(DockspaceError::HostFrameSurfacePassNotIncreasing {
            surface: ROOT_SURFACE,
            ..
        })
    ));
}

#[test]
fn outer_host_frame_rejects_an_unconfirmed_final_output() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-unconfirmed-output", single_workspace())
        .build()
        .expect("fixture builds");
    let before_tick = dockspace.engine().last_reducer_tick();
    let mut panes = TestPanes;
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer host frame begins");
    let _ = context.run_ui(input(), |ui| {
        host.show_surface(ROOT_SURFACE, ui, &mut panes)
            .expect("surface paints");
    });

    assert!(matches!(
        host.finish(),
        Err(DockspaceError::OuterHostSurfaceOutputUnconfirmed {
            surface: ROOT_SURFACE
        })
    ));
    assert_eq!(dockspace.engine().last_reducer_tick(), before_tick);
}

#[test]
fn outer_host_surface_run_returns_only_its_owned_full_output() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-owned-output", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(2, 0))
        .expect("outer host frame begins");
    let paint = host
        .run_surface(ROOT_SURFACE, &context, input(), &mut panes)
        .expect("outer host owns the complete surface run");
    assert_eq!(paint.surface(), ROOT_SURFACE);
    let (_, mut outputs) = host.finish().expect("confirmed frame commits").into_parts();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().expect("one bound output exists");
    assert!(output.has_presentation_obligation());
    assert!(
        output
            .full_output()
            .viewport_output
            .contains_key(&context.viewport_id())
    );
    output.settle_with(|surface, full_output| {
        assert_eq!(surface, ROOT_SURFACE);
        assert!(
            full_output
                .viewport_output
                .contains_key(&context.viewport_id())
        );
        EguiPresentationResult::Dropped
    });
}

#[test]
fn outer_host_rejects_replacing_a_surface_run_with_another_context() {
    let first_context = one_pass_context();
    let second_context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-context-replacement", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer host frame begins");
    host.run_surface(ROOT_SURFACE, &first_context, input(), &mut panes)
        .expect("first context owns the initial surface run");
    let _ = second_context.run_ui(input(), |_| {});

    assert!(matches!(
        host.run_surface(ROOT_SURFACE, &second_context, input(), &mut panes),
        Err(DockspaceError::OuterHostSurfaceContextMismatch {
            surface: ROOT_SURFACE
        })
    ));
}

#[test]
fn presentation_token_can_settle_on_the_renderer_thread() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-token-renderer-thread", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();

    assert_eq!(pending.len(), 1);
    assert!(pending[0].has_presentation_obligation());

    std::thread::spawn(move || {
        one_presentation(pending).settle_with(|_, _| EguiPresentationResult::Presented);
    })
    .join()
    .expect("renderer thread does not panic");
}

#[test]
fn split_presentation_stays_pending_until_a_late_renderer_result() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-split-late-result", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    let (surface, full_output, settlement) = one_presentation(pending).into_parts();

    assert_eq!(surface, ROOT_SURFACE);
    assert!(
        full_output
            .viewport_output
            .contains_key(&context.viewport_id())
    );
    assert_eq!(settlement.surface(), ROOT_SURFACE);
    assert!(settlement.is_required());
    assert!(
        settlement.presentation_output().is_some(),
        "a required renderer settlement must retain its exact core output identity",
    );
    assert_eq!(settlement.native_route(), None);
    assert_eq!(
        dockspace
            .engine()
            .presentation_ledger_diagnostics()
            .pending_outputs(),
        1,
        "splitting an output must not settle its presentation obligation"
    );

    let (before_result, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert!(
        before_result
            .transition()
            .presentation_observations()
            .iter()
            .all(|outcome| !matches!(outcome, HostPresentationObservationOutcome::Retired { .. }))
    );
    let (_, _, no_op) = one_presentation(outputs).into_parts();
    assert!(!no_op.is_required());
    no_op.settle(EguiPresentationResult::Presented);

    settlement.settle(EguiPresentationResult::Presented);

    let (after_result, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(4, 0),
    )
    .into_parts();
    assert_eq!(
        after_result
            .transition()
            .presentation_observations()
            .iter()
            .filter(|outcome| matches!(
                outcome,
                HostPresentationObservationOutcome::Retired {
                    presented: dockspace::intent::Authority::Known(Some(_)),
                    ..
                }
            ))
            .count(),
        1,
        "the late result must complete the exact obligation once"
    );
    complete_presentations(outputs, EguiPresentationResult::Dropped);
}

#[test]
fn dropping_a_split_settlement_terminally_drops_the_output() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-split-drop", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    let (_, _, settlement) = one_presentation(pending).into_parts();
    assert!(settlement.is_required());

    drop(settlement);

    let (response, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert!(
        response
            .transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(
                outcome,
                HostPresentationObservationOutcome::Retired {
                    presented: dockspace::intent::Authority::Known(None),
                    ..
                }
            ))
    );
    complete_presentations(outputs, EguiPresentationResult::Dropped);
}

#[test]
fn split_output_without_an_obligation_has_an_explicit_no_op_settlement() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-split-no-op", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    let (_, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    )
    .into_parts();
    let (surface, _, settlement) = one_presentation(outputs).into_parts();

    assert_eq!(surface, ROOT_SURFACE);
    assert_eq!(settlement.surface(), ROOT_SURFACE);
    assert!(!settlement.is_required());
    settlement.settle(EguiPresentationResult::Presented);
    assert_eq!(
        dockspace
            .engine()
            .presentation_ledger_diagnostics()
            .pending_outputs(),
        0,
        "a no-op settlement must not manufacture a presentation output"
    );

    let (next, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    assert!(next.transition().presentation_observations().is_empty());
    complete_presentations(outputs, EguiPresentationResult::Dropped);
}

#[test]
fn outer_presentation_completion_cannot_jump_an_earlier_pending_output() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-causal-barrier", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, first) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    let (_, second) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    let first = one_presentation(first);
    let second = one_presentation(second);
    assert!(first.has_presentation_obligation());
    assert!(!second.has_presentation_obligation());
    second.settle_with(|_, _| EguiPresentationResult::Presented);

    let (third, third_pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(4, 0),
    )
    .into_parts();
    assert!(
        third
            .transition()
            .presentation_observations()
            .iter()
            .all(|outcome| !matches!(outcome, HostPresentationObservationOutcome::Retired { .. }))
    );
    assert!(
        !third
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("third surface paints")
            .interactions_current()
    );

    first.settle_with(|_, _| EguiPresentationResult::Presented);
    let (fourth, fourth_pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(5, 0),
    )
    .into_parts();
    assert!(
        fourth
            .transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(outcome, HostPresentationObservationOutcome::Retired { .. }))
    );

    for pending in [third_pending, fourth_pending].into_iter().flatten() {
        pending.settle_with(|_, _| EguiPresentationResult::Dropped);
    }
}

#[test]
fn held_outer_output_applies_per_surface_backpressure() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-held-output-backpressure", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, first) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    let held = one_presentation(first);
    assert!(held.has_presentation_obligation());

    for sequence in 3..=259 {
        let (_, outputs) = paint_outer_frame(
            &context,
            &mut dockspace,
            &mut panes,
            EguiFrameScheduleKey::new(sequence, 0),
        )
        .into_parts();
        let output = one_presentation(outputs);
        assert!(
            !output.has_presentation_obligation(),
            "a held prefix must suppress later presentation emissions"
        );
        drop(output);
        assert_eq!(
            dockspace
                .engine()
                .presentation_ledger_diagnostics()
                .pending_outputs(),
            1
        );
    }

    held.settle_with(|_, _| EguiPresentationResult::Dropped);
    let (settled, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(260, 0),
    )
    .into_parts();
    assert!(
        settled
            .transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(outcome, HostPresentationObservationOutcome::Retired { .. }))
    );
    assert!(one_presentation(outputs).has_presentation_obligation());
}

#[test]
fn dropped_outer_output_retires_without_granting_interaction_authority() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-dropped-output", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    one_presentation(pending).settle_with(|_, _| EguiPresentationResult::Dropped);

    let (response, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert!(
        response
            .transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(
                outcome,
                HostPresentationObservationOutcome::Retired {
                    presented: dockspace::intent::Authority::Known(None),
                    ..
                }
            ))
    );
    assert!(
        !response
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("second surface paints")
            .interactions_current()
    );
    one_presentation(pending).settle_with(|_, _| EguiPresentationResult::Dropped);
}

#[test]
fn abandoned_outer_output_terminally_drops_without_blocking_the_stream() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-abandoned-output", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    drop(one_presentation(pending));

    let (response, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert!(
        response
            .transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(
                outcome,
                HostPresentationObservationOutcome::Retired {
                    presented: dockspace::intent::Authority::Known(None),
                    ..
                }
            ))
    );
    one_presentation(pending).settle_with(|_, _| EguiPresentationResult::Dropped);
}

#[test]
fn ordinary_frame_settles_completed_outer_output_without_mode_switch_back() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-to-ordinary", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    bootstrap_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );

    let (_, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    one_presentation(pending).settle_with(|_, _| EguiPresentationResult::Dropped);

    let response = paint_crates_io_frame(&context, &mut dockspace, &mut panes);
    assert!(response.transitions().iter().any(|transition| {
        transition
            .presentation_observations()
            .iter()
            .any(|outcome| {
                matches!(
                    outcome,
                    HostPresentationObservationOutcome::Retired {
                        presented: dockspace::intent::Authority::Known(None),
                        ..
                    }
                )
            })
    }));
}

#[test]
fn explicit_host_frames_remain_fail_closed_without_automatic_presentation_facts() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("retained-surface-slot", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    let first = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let ticket = match first.transition().surface_contributions() {
        [SurfaceContributionOutcome::Ready { ticket, .. }] => *ticket,
        outcomes => panic!("first frame must install one ready contribution: {outcomes:?}"),
    };

    let second = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    );
    assert!(second.transition().presentation_observations().is_empty());
    assert!(second.transition().presentation_emissions().is_empty());
    assert!(matches!(
        second.transition().surface_contributions(),
        [SurfaceContributionOutcome::Retained { ticket: actual, .. }] if *actual == ticket
    ));

    let third = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    );
    assert!(third.transition().presentation_observations().is_empty());
    assert!(third.transition().presentation_emissions().is_empty());
    assert!(matches!(
        third.transition().surface_contributions(),
        [SurfaceContributionOutcome::Retained { ticket: actual, .. }] if *actual == ticket
    ));
}

#[test]
fn omitted_single_surface_callback_becomes_an_explicit_deferred_contribution() {
    let mut dockspace = Dockspace::builder("deferred-surface-slot", single_workspace())
        .build()
        .expect("fixture builds");

    let response = dockspace
        .begin_host_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("host frame begins")
        .end_host_frame()
        .expect("missing callback is represented explicitly before core reduction");

    let slot = response
        .surface(ROOT_SURFACE)
        .expect("frozen surface has one terminal result");
    assert!(slot.paint().is_none());
    assert!(matches!(
        slot.disposition(),
        SurfaceFrameDisposition::Contribution(SurfaceContributionOutcome::Unavailable { .. })
    ));
    assert!(matches!(
        response.transition().surface_contributions(),
        [SurfaceContributionOutcome::Unavailable { surface, .. }] if *surface == ROOT_SURFACE
    ));
}

#[test]
fn explicit_unavailable_slot_uses_the_core_unavailable_preparation_path() {
    let mut dockspace = Dockspace::builder("explicit-unavailable-slot", single_workspace())
        .build()
        .expect("fixture builds");
    let mut host = dockspace
        .begin_host_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("host frame begins");
    host.mark_surface_unavailable(
        ROOT_SURFACE,
        MeasurementUnavailableReason::ContentUnavailable,
    )
    .expect("explicit unavailable slot prepares");

    let response = host.end_host_frame().expect("host frame commits");
    assert!(matches!(
        response
            .surface(ROOT_SURFACE)
            .expect("frozen surface has one terminal result")
            .disposition(),
        SurfaceFrameDisposition::Contribution(SurfaceContributionOutcome::Unavailable { .. })
    ));
}

#[test]
fn crates_io_facade_rejects_multi_surface_workspace_before_paint() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("strict-single", multi_surface_workspace())
        .build()
        .expect("fixture builds");
    let before_tick = dockspace.engine().last_reducer_tick();
    let before_version = dockspace.engine().version();
    let mut panes = TestPanes;
    let mut result = None;
    let _ = context.run_ui(input(), |ui| {
        result = Some(dockspace.show_single_surface(ROOT_SURFACE, ui, &mut panes));
    });
    assert!(matches!(
        result.expect("egui invokes the surface callback"),
        Err(DockspaceError::SingleSurfaceHostFrameRequiresOneSurface { surface_count: 2 })
    ));
    assert!(matches!(
        dockspace.begin_host_frame(EguiFrameScheduleKey::new(1, 0)),
        Err(DockspaceError::MultiSurfaceHostFrameUnsupported { surface_count: 2 })
    ));
    assert_eq!(dockspace.engine().last_reducer_tick(), before_tick);
    assert_eq!(dockspace.engine().version(), before_version);
}
