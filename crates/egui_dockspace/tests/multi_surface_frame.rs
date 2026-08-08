use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::scene_manifest::MeasurementUnavailableReason;
use egui::{Context, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::backend::{
    EguiFrameScheduleKey, EguiOuterFrameCommit, EguiOuterSurfaceOutput, EguiPresentationResult,
    HostFrameResponse,
};
use egui_dockspace::{
    Dockspace, DockspaceErrorKind, DockspaceSurfaceCommitStatus, DockspaceSurfaceStatus, PaneView,
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
    let _ = crate::test_support::run_ui_without_renderer(&context, input(), |ui| {
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
    let _ = crate::test_support::run_ui_without_renderer(&context, input(), |ui| {
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
        presentation.settle_with(|_, full_output| {
            full_output.drop_without_applying_deltas();
            result
        });
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

        assert!(!response.mutation().workspace_changed());
        assert!(matches!(
            response.surface_commit_status(),
            DockspaceSurfaceCommitStatus::Ready | DockspaceSurfaceCommitStatus::Retained
        ));
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
}

#[test]
fn single_surface_host_frame_commits_through_the_core_capability() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("core-host-frame", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;

    let response = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    assert!(!response.mutation().workspace_changed());
    assert!(response.mutation().published_state_changed());
    assert_eq!(response.surfaces().len(), 1);
    assert_eq!(
        response
            .surface(ROOT_SURFACE)
            .expect("single response exists")
            .status(),
        DockspaceSurfaceCommitStatus::Ready,
    );
}

#[test]
fn outer_host_frame_commits_the_complete_multi_surface_roster_once() {
    let root_context = one_pass_context();
    let child_context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-complete-roster", multi_surface_workspace())
        .build()
        .expect("fixture builds");
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
    assert!(!response.mutation().workspace_changed());
    assert!(response.mutation().published_state_changed());
    assert_eq!(response.surfaces().len(), 2);
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
        presentation.settle_with(|_, full_output| {
            full_output.drop_without_applying_deltas();
            result
        });
    }

    let (second, pending) = paint_multi_surface_outer_frame(
        &root_context,
        &child_context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    let summary = second.presentation_summary();
    assert_eq!(summary.observed(), 2);
    assert_eq!(summary.retired_presented_eligible(), 1);
    assert_eq!(summary.retired_dropped(), 1);
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
    assert_eq!(
        response
            .surface(ROOT_SURFACE)
            .expect("single response exists")
            .status(),
        DockspaceSurfaceCommitStatus::Ready,
    );
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

    let _ = crate::test_support::run_ui_without_renderer(&context, input(), |ui| {
        host.show_surface(ROOT_SURFACE, ui, &mut panes)
            .expect("first callback paints");
        duplicate = Some(host.show_surface(ROOT_SURFACE, ui, &mut panes));
    });

    assert_eq!(
        duplicate
            .expect("duplicate callback runs")
            .expect_err("a non-increasing pass must be rejected")
            .kind(),
        DockspaceErrorKind::HostProtocol,
    );
}

#[test]
fn outer_host_frame_rejects_an_unconfirmed_final_output() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("outer-unconfirmed-output", single_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;
    let mut host = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer host frame begins");
    let _ = crate::test_support::run_ui_without_renderer(&context, input(), |ui| {
        host.show_surface(ROOT_SURFACE, ui, &mut panes)
            .expect("surface paints");
    });

    assert_eq!(
        host.finish()
            .expect_err("an unconfirmed final output must be rejected")
            .kind(),
        DockspaceErrorKind::HostProtocol,
    );
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
        full_output.drop_without_applying_deltas();
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
    let _ = crate::test_support::run_ui_without_renderer(&second_context, input(), |_| {});

    assert_eq!(
        host.run_surface(ROOT_SURFACE, &second_context, input(), &mut panes)
            .expect_err("a different context cannot replace the authoritative pass")
            .kind(),
        DockspaceErrorKind::HostProtocol,
    );
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
        one_presentation(pending).settle_with(|_, full_output| {
            full_output.drop_without_applying_deltas();
            EguiPresentationResult::Presented
        });
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
    let pending = one_presentation(pending);
    assert!(pending.has_presentation_obligation());
    let (surface, full_output, settlement) = pending.into_parts();

    assert_eq!(surface, ROOT_SURFACE);
    assert!(
        full_output
            .viewport_output
            .contains_key(&context.viewport_id())
    );
    assert_eq!(settlement.surface(), ROOT_SURFACE);
    assert!(settlement.is_required());
    assert_eq!(settlement.native_route(), None);
    full_output.drop_without_applying_deltas();

    let (before_result, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert_eq!(before_result.presentation_summary().retired(), 0);
    let no_op_output = one_presentation(outputs);
    assert!(!no_op_output.has_presentation_obligation());
    let (_, full_output, no_op) = no_op_output.into_parts();
    full_output.drop_without_applying_deltas();
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
            .presentation_summary()
            .retired_presented_eligible(),
        1,
        "the late result must complete the exact obligation once",
    );
    let stable = one_presentation(outputs);
    assert!(stable.has_presentation_obligation());
    stable.settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });
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
    let pending = one_presentation(pending);
    assert!(pending.has_presentation_obligation());
    let (_, full_output, settlement) = pending.into_parts();
    full_output.drop_without_applying_deltas();
    assert!(settlement.is_required());

    drop(settlement);

    let (response, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert_eq!(response.presentation_summary().retired_dropped(), 1);
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
    let (surface, full_output, settlement) = one_presentation(outputs).into_parts();
    full_output.drop_without_applying_deltas();

    assert_eq!(surface, ROOT_SURFACE);
    assert_eq!(settlement.surface(), ROOT_SURFACE);
    assert!(!settlement.is_required());
    settlement.settle(EguiPresentationResult::Presented);
    let (next, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    )
    .into_parts();
    assert_eq!(next.presentation_summary().observed(), 0);
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
    second.settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Presented
    });

    let (third, third_pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(4, 0),
    )
    .into_parts();
    assert_eq!(third.presentation_summary().retired(), 0);
    assert!(
        !third
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("third surface paints")
            .interactions_current()
    );

    first.settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Presented
    });
    let (fourth, fourth_pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(5, 0),
    )
    .into_parts();
    assert_eq!(
        fourth.presentation_summary().retired_presented_eligible(),
        1,
    );

    for pending in [third_pending, fourth_pending].into_iter().flatten() {
        pending.settle_with(|_, full_output| {
            full_output.drop_without_applying_deltas();
            EguiPresentationResult::Dropped
        });
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
    }

    held.settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });
    let (settled, outputs) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(260, 0),
    )
    .into_parts();
    assert_eq!(settled.presentation_summary().retired_dropped(), 1);
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
    one_presentation(pending).settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });

    let (response, pending) = paint_outer_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    )
    .into_parts();
    assert_eq!(response.presentation_summary().retired_dropped(), 1);
    assert!(
        !response
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("second surface paints")
            .interactions_current()
    );
    one_presentation(pending).settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });
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
    assert_eq!(response.presentation_summary().retired_dropped(), 1);
    one_presentation(pending).settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });
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
    one_presentation(pending).settle_with(|_, full_output| {
        full_output.drop_without_applying_deltas();
        EguiPresentationResult::Dropped
    });

    let response = paint_crates_io_frame(&context, &mut dockspace, &mut panes);
    assert!(!response.mutation().workspace_changed());
    assert!(matches!(
        response.surface_commit_status(),
        DockspaceSurfaceCommitStatus::Ready | DockspaceSurfaceCommitStatus::Retained
    ));
    assert!(!response.interactions_current());
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
    assert_eq!(
        first
            .surface(ROOT_SURFACE)
            .expect("first surface result exists")
            .status(),
        DockspaceSurfaceCommitStatus::Ready,
    );
    assert_eq!(first.presentation_summary().observed(), 0);

    let second = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    );
    assert_eq!(second.presentation_summary().observed(), 0);
    assert_eq!(
        second
            .surface(ROOT_SURFACE)
            .expect("second surface result exists")
            .status(),
        DockspaceSurfaceCommitStatus::Retained,
    );
    assert!(
        !second
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("second surface paints")
            .interactions_current()
    );

    let third = paint_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    );
    assert_eq!(third.presentation_summary().observed(), 0);
    assert_eq!(
        third
            .surface(ROOT_SURFACE)
            .expect("third surface result exists")
            .status(),
        DockspaceSurfaceCommitStatus::Retained,
    );
    assert!(
        !third
            .surface(ROOT_SURFACE)
            .and_then(|surface| surface.paint())
            .expect("third surface paints")
            .interactions_current()
    );
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
    assert_eq!(slot.status(), DockspaceSurfaceCommitStatus::Unavailable);
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
    assert_eq!(
        response
            .surface(ROOT_SURFACE)
            .expect("frozen surface has one terminal result")
            .status(),
        DockspaceSurfaceCommitStatus::Unavailable,
    );
}

#[test]
fn crates_io_facade_rejects_multi_surface_workspace_before_paint() {
    let context = one_pass_context();
    let mut dockspace = Dockspace::builder("strict-single", multi_surface_workspace())
        .build()
        .expect("fixture builds");
    let mut panes = TestPanes;
    let mut result = None;
    let _ = crate::test_support::run_ui_without_renderer(&context, input(), |ui| {
        result = Some(dockspace.show_single_surface(ROOT_SURFACE, ui, &mut panes));
    });
    assert_eq!(
        result
            .expect("egui invokes the surface callback")
            .expect_err("the convenience facade accepts one surface only")
            .kind(),
        DockspaceErrorKind::Unsupported,
    );
    assert_eq!(
        dockspace
            .begin_host_frame(EguiFrameScheduleKey::new(1, 0))
            .err()
            .expect("the convenience host frame accepts one surface only")
            .kind(),
        DockspaceErrorKind::Unsupported,
    );
}
