//! Durable source ownership before the first valid interactive child output.

use super::*;
use crate::model::WorkspaceVersion;
use crate::runtime::{
    SurfaceMeasurementAnswer, SurfaceMeasurementRequest, SurfacePaintProgramOperation,
    TabListMenuMetrics, TabStripControlKind, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
};
use crate::tab_strip::PopupPlaneRequirement;
use crate::viewport_registry::ViewportAdmission;

#[inline(never)]
fn pending_first_live_native_create()
-> (DockspaceSession, NativeSurfaceBinding, NativeSurfaceBinding) {
    let (mut session, root) = programmatic_tear_off_session();
    let placement = PhysicalRect::new(920.0, 120.0, 420.0, 320.0)
        .expect("the requested child placement validates");
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the programmatic tear-off frame begins");
    frame
        .move_root_to_new_window_current(ROOT, NativeWindowPlacement::new(placement))
        .expect("the source root tear-off stages");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the source presentation remains retained");
    let mut report = frame.commit().expect("the child create request commits");
    assert!(report.native_admissions().is_empty());
    let create = take_only_native_effect(&mut report);
    let child = create.operation().binding();
    assert!(matches!(
        create.operation(),
        NativeEffectOperation::CreateWindow { .. }
    ));
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the create request must return a create acknowledgement");
    };
    let facts = NativeWindowFacts::live()
        .with_content_bounds(placement)
        .with_outer_bounds(placement)
        .with_native_scale_factor(native_scale())
        .with_presentation_scale_factor(native_scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_create_readiness(crate::runtime::NativeCreateReadiness::Created, None)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot(
            [
                (root, root_window_facts()),
                (
                    child,
                    facts
                        .with_presentation(NativeWindowPresentationState::Hidden, None)
                        .with_create_readiness(
                            crate::runtime::NativeCreateReadiness::Created,
                            Some(created),
                        ),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact hidden child and create acknowledgement record");
    assert!(
        commit_managed_frame_report(&mut session)
            .native_admissions()
            .is_empty()
    );

    let pre_show = paint_staging(&mut session, child, NativeStagingPresentationPhase::PreShow);
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the exact pre-show placeholder presentation records");
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let show = take_only_native_effect(&mut report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
        panic!("the show request must return a presentation acknowledgement");
    };
    // Visibility must arrive after the exact show acknowledgement's boundary.
    for (state, acknowledgement) in [
        (NativeWindowPresentationState::Hidden, Some(shown)),
        (NativeWindowPresentationState::Visible, None),
    ] {
        session
            .report_managed_native_snapshot(
                [
                    (root, root_window_facts()),
                    (child, facts.with_presentation(state, acknowledgement)),
                ],
                NativeWorkAreaRoster::Exact(vec![work_area(1)]),
            )
            .expect("the exact child presentation facts record");
        assert!(
            commit_managed_frame_report(&mut session)
                .native_admissions()
                .is_empty()
        );
    }

    let before_post_show = session.version();
    let source_ownership = [ITEM, SECOND_ITEM].map(|item| {
        let view = session.view();
        let source = view.item(item).expect("the source item exists");
        (item, source.root(), source.surface())
    });
    assert_eq!(
        source_ownership,
        [
            (ITEM, ROOT, root.surface()),
            (SECOND_ITEM, ROOT, root.surface()),
        ],
        "both items must still belong to the source before PostShow"
    );

    // Staging presents a placeholder; no semantic child output is presented here.
    let post_show = paint_staging(
        &mut session,
        child,
        NativeStagingPresentationPhase::PostShow,
    );
    session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .expect("the exact post-show placeholder presentation records");
    let mut candidate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the post-show presentation candidate begins");
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the dropped placeholder candidate settles every surface");
    drop(candidate);
    assert_eq!(
        session.version(),
        before_post_show,
        "dropping the placeholder candidate must preserve the workspace version"
    );
    for (item, source_root, source_surface) in source_ownership {
        let view = session.view();
        let current = view.item(item).expect("the source item remains present");
        assert_eq!(
            (current.root(), current.surface()),
            (source_root, source_surface),
            "dropping the placeholder candidate must preserve source ownership for {item:?}"
        );
    }

    let report = commit_managed_frame_report(&mut session);
    assert!(
        report.native_admissions().is_empty(),
        "the placeholder must not admit the child before its first interactive output"
    );
    for (item, source_root, source_surface) in source_ownership {
        let view = session.view();
        let current = view.item(item).expect("the source item remains present");
        assert_eq!(
            (current.root(), current.surface()),
            (source_root, source_surface),
            "PostShow placeholder presentation must preserve source ownership for {item:?}"
        );
    }
    assert_eq!(
        session.version(),
        before_post_show,
        "PostShow placeholder presentation must not advance the workspace version"
    );
    assert_pending_first_live_source(&session, root, child, before_post_show);
    (session, root, child)
}

#[test]
fn post_show_placeholder_cannot_publish_native_item_ownership_or_workspace_revision() {
    let _ = pending_first_live_native_create();
}

fn assert_pending_first_live_source(
    session: &DockspaceSession,
    root: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    version: WorkspaceVersion,
) {
    assert_eq!(session.version(), version);
    for item in [ITEM, SECOND_ITEM] {
        let source = session
            .view()
            .item(item)
            .expect("the source item remains present");
        assert_eq!(
            (source.root(), source.surface()),
            (ROOT, root.surface()),
            "source ownership must remain published until exact first-live admission for {item:?}"
        );
    }
    assert!(
        session.presented_surface(child.surface()).is_none(),
        "an unadmitted child must not expose presented interaction authority"
    );
    assert!(
        session
            .engine
            .workspace()
            .surface(child.surface())
            .is_none()
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .expect("the exact child binding remains reserved")
            .admission(),
        ViewportAdmission::Pending
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .any(|(_, saga)| {
                saga.binding() == child.binding
                    && matches!(
                        saga.phase(),
                        crate::frame::NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                    )
            })
    );
}

fn first_live_popup_strip_metrics() -> TabStripMetrics {
    TabStripMetrics::new(0.0, 0.0)
        .expect("the popup strip reservations validate")
        .with_controls(
            TabStripControlMetrics::new(0.0)
                .expect("the menu control spacing validates")
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .expect("the source menu control extent validates"),
                ),
        )
        .with_tab_list_menu(
            TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("the source popup metrics validate"),
        )
}

#[inline(never)]
fn emit_first_live_child_output(
    session: &mut DockspaceSession,
    root: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let version = session.version();
    let source_sequence = session.committed_source_sequence;
    let popup = session.engine.presentation_requirements().popup();
    let selected = [ITEM, SECOND_ITEM]
        .into_iter()
        .find(|item| {
            session
                .view()
                .item(*item)
                .is_some_and(|source| source.is_selected())
        })
        .expect("the source pane has one selected item");
    let mut measure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the public first-live measurement frame begins");
    let surfaces = measure.surfaces();
    assert_eq!(surfaces.len(), 2);
    assert!(surfaces.contains(&root.surface()));
    assert!(
        surfaces.contains(&child.surface()),
        "the public surface roster must include the prospective child alongside its source"
    );
    for surface in surfaces {
        let (width, height) = if surface == child.surface() {
            (420.0, 320.0)
        } else {
            (640.0, 480.0)
        };
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, width, height).expect("the exact surface bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("the pane minimum validates"),
            80.0,
        )
        .expect("the surface metrics validate");
        let strip = if popup.session().is_some() {
            first_live_popup_strip_metrics()
        } else {
            metrics.tab_strip
        };
        let mut popup_bounds_requests = 0;
        measure
            .measure_surface_with(surface, |request| match request {
                SurfaceMeasurementRequest::DockBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(metrics.bounds)
                }
                SurfaceMeasurementRequest::PopupPlaneBounds { surface: requested } => {
                    assert_eq!(requested, surface);
                    popup_bounds_requests += 1;
                    SurfaceMeasurementAnswer::Bounds(metrics.bounds)
                }
                SurfaceMeasurementRequest::PaneMinimum { .. } => {
                    SurfaceMeasurementAnswer::PaneMinimum(metrics.pane_minimum)
                }
                SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                    SurfaceMeasurementAnswer::TabIntrinsic(metrics.tab_intrinsic.content_width())
                }
                SurfaceMeasurementRequest::TabStrip { .. } => {
                    SurfaceMeasurementAnswer::TabStrip(strip)
                }
            })
            .expect("the publicly enumerated surface measures");
        assert_eq!(
            popup_bounds_requests,
            usize::from(
                popup
                    .owner()
                    .is_some_and(|owner| owner.surface() == surface)
            ),
            "public measurement requests must match the current popup plane for {surface:?}"
        );
    }
    measure
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the measurement frame settles every surface");
    let mut report = measure.commit().expect("the exact measurements commit");
    assert!(report.native_admissions().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    assert_pending_first_live_source(session, root, child, version);

    let mut paint = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the public first-live paint frame begins");
    assert!(paint.surfaces().contains(&child.surface()));
    let plan = paint
        .paint_plan(child.surface())
        .expect("the public prospective child plan resolves")
        .expect("the measured prospective child is paintable");
    assert_eq!(plan.surface(), child.surface());
    assert_eq!(plan.plan.popup(), popup);
    let mut panes = Vec::new();
    let mut tabs = Vec::new();
    let mut popup_backdrops = 0;
    let mut semantic_output = None;
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::Operation {
                    root, operation, ..
                } => match operation {
                    SurfacePaintProgramOperation::Pane(pane) => {
                        assert_eq!(root.root(), ROOT);
                        panes.push((pane.root(), pane.selected()));
                    }
                    SurfacePaintProgramOperation::Tab(tab) => {
                        assert_eq!(root.root(), ROOT);
                        tabs.push((tab.root(), tab.item(), tab.selected()));
                    }
                    _ => {}
                },
                SurfacePaintProgramStep::PopupBackdrop(backdrop) => {
                    assert_eq!(backdrop.bounds(), plan.bounds());
                    popup_backdrops += 1;
                }
                SurfacePaintProgramStep::PopupMenu { .. } => {
                    panic!("the pending child's popup menu must remain on its source surface");
                }
                SurfacePaintProgramStep::SurfaceEnd {
                    semantic_output: output,
                } => assert!(semantic_output.replace(output).is_none()),
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .expect("the accepting renderer executes the complete prospective child program");
    assert_eq!(panes, [(ROOT, Some(selected))]);
    assert_eq!(tabs.len(), 2);
    for item in [ITEM, SECOND_ITEM] {
        assert!(tabs.contains(&(ROOT, item, item == selected)));
    }
    assert_eq!(popup_backdrops, 0, "the popup stays on its source owner");
    let semantic_output = semantic_output.expect("the complete program emits its semantic seal");
    paint
        .confirm_surface_painted(receipt)
        .expect("the complete child program emission confirms");
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the first-live paint frame settles every surface");
    let mut report = paint.commit().expect("the exact child output emits");
    assert!(report.native_admissions().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap_or_else(
        |outputs: Vec<PaintedSurfaceOutput>| {
            panic!("expected one exact first-live child output, got {outputs:?}")
        },
    );
    assert_eq!(output.surface(), child.surface());
    assert!(output.matches_native_binding(child));
    assert!(output.matches_semantic_output(semantic_output));
    assert_pending_first_live_source(session, root, child, version);
    assert_eq!(session.committed_source_sequence, source_sequence);
    output
}

fn assert_first_live_child_admitted(
    session: &DockspaceSession,
    child: NativeSurfaceBinding,
    previous: WorkspaceVersion,
) {
    assert_eq!(session.version().epoch(), previous.epoch());
    assert_eq!(
        session.version().revision().get(),
        previous.revision().get() + 1
    );
    for item in [ITEM, SECOND_ITEM] {
        let current = session
            .view()
            .item(item)
            .expect("the admitted item remains present");
        assert_eq!((current.root(), current.surface()), (ROOT, child.surface()));
    }
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .expect("the child binding remains live")
            .admission(),
        ViewportAdmission::Admitted
    );
}

#[test]
fn exact_first_live_presented_retries_dropped_candidate_and_publishes_ownership_once() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let output = emit_first_live_child_output(&mut session, root, child);
    let artifact = output.presentation_artifact();
    let ingress = session.engine.backend_ingress_committed_through();
    assert_pending_first_live_source(&session, root, child, version);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact first-live child presentation records");
    assert_pending_first_live_source(&session, root, child, version);

    let mut candidate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first-live admission candidate begins");
    assert_eq!(candidate.version().epoch(), version.epoch());
    assert_eq!(
        candidate.version().revision().get(),
        version.revision().get() + 1
    );
    for item in [ITEM, SECOND_ITEM] {
        let current = candidate
            .view()
            .item(item)
            .expect("the candidate owns the item");
        assert_eq!((current.root(), current.surface()), (ROOT, child.surface()));
    }
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the dropped admission candidate settles every surface");
    drop(candidate);
    assert_pending_first_live_source(&session, root, child, version);
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(session.engine.backend_ingress_committed_through(), ingress);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );

    let report = commit_managed_frame_report(&mut session);
    assert_eq!(report.native_admissions(), &[child]);
    let [presentation] = report.surface_presentation_commits() else {
        panic!("retry must accept exactly the retained first-live presentation");
    };
    assert!(presentation.presents(artifact));
    assert_first_live_child_admitted(&session, child, version);
    assert!(
        session
            .presented_surface(child.surface())
            .expect("the admitted child exposes exact presented authority")
            .matches_presentation_artifact(artifact)
    );
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        0
    );

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_first_live_child_admitted(&session, child, version);
    assert_eq!(session.committed_source_sequence, sequence);
}

#[test]
fn ordered_first_live_rebases_following_surface_action_and_retries_atomically() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let ingress = session.engine.backend_ingress_committed_through();
    let output = emit_first_live_child_output(&mut session, root, child);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the ordered first-live observation records");

    for commit in [false, true] {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the joined first-live prefix reduces");
        let current = frame.version();
        assert_eq!(current.revision().get(), version.revision().get() + 1);
        let workspace = frame.frame.view().workspace();
        let tabs = workspace.root(ROOT).expect("the child root exists").node;
        let stale = crate::command::WorkspaceCommand::Select {
            source: workspace
                .capture_item_source(ROOT, tabs, ITEM)
                .expect("the former selected item remains in the child"),
        };
        let fresh = frame
            .paint_plan(child.surface())
            .expect("the admitted child plan lookup succeeds")
            .expect("the exact first-live child plan remains current")
            .prepare_tab_select(SECOND_ITEM)
            .expect("the current child prepares a public surface action");
        frame
            .append(EngineInput::WorkspaceCommand {
                expected: version,
                command: stale,
            })
            .expect("the former source-version command is structurally valid");
        frame
            .submit_surface_action(fresh)
            .expect("the current surface action follows the ordered lifecycle prefix");
        assert!(
            frame
                .view()
                .item(SECOND_ITEM)
                .expect("the fresh command's item exists")
                .is_selected(),
            "the current post-first-live version must authorize the fresh command"
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("every surface receives an explicit disposition");
        if commit {
            let report = frame
                .commit()
                .expect("the identical prefix and commands commit");
            assert_eq!(report.native_admissions(), &[child]);
            assert!(matches!(
                report.inputs(),
                [crate::runtime::HostInputOutcome::StaleRejected { expected, accepted }]
                    if *expected == version && *accepted == current
            ));
        } else {
            drop(frame);
            assert_pending_first_live_source(&session, root, child, version);
            assert_eq!(session.committed_source_sequence, sequence);
            assert_eq!(session.engine.backend_ingress_committed_through(), ingress);
        }
    }
    assert_eq!(session.committed_source_sequence, sequence + 2);
    assert_eq!(
        session.version().revision().get(),
        version.revision().get() + 2
    );
    assert!(session.view().item(SECOND_ITEM).unwrap().is_selected());
}

#[inline(never)]
fn open_first_live_source_popup(
    session: &mut DockspaceSession,
    root: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
) -> PopupPlaneRequirement {
    let version = session.version();
    let mut configure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the source overflow configuration frame begins");
    configure
        .replace_presentation_config(
            crate::presentation_config::DockPresentationConfig::builder()
                .tab_min_width(180.0)
                .build()
                .expect("the contained source's two tabs require real overflow"),
        )
        .expect("the exact source overflow geometry configures");
    configure
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configure
        .commit()
        .expect("the overflow configuration commits");
    let mut measure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the source menu measurement frame begins");
    let mut metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("the source bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("the source pane minimum validates"),
        80.0,
    )
    .expect("the source menu metrics validate");
    metrics.tab_strip = first_live_popup_strip_metrics();
    measure
        .measure_surface(root.surface(), metrics)
        .expect("the public source menu measurements stage");
    measure
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source menu measurement retains the pending child");
    assert!(
        measure
            .commit()
            .expect("the source menu measurements commit")
            .native_admissions()
            .is_empty()
    );
    assert_pending_first_live_source(session, root, child, version);

    let mut prepare = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the source menu control preparation frame begins");
    let plan = prepare
        .paint_plan(root.surface())
        .expect("the public source plan resolves")
        .expect("the measured source menu control is paintable");
    let control = plan
        .tab_strip_controls()
        .find(|control| {
            control.root() == ROOT && control.kind() == TabStripControlKind::TabListMenu
        })
        .expect("the actual source plan contains its tab-list menu control");
    let open = plan
        .prepare_tab_strip_control_activation(control)
        .expect("the exact source menu control prepares its public action");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source menu preparation retains both surfaces");
    assert!(
        prepare
            .commit()
            .expect("the exact source menu action preparation commits")
            .native_admissions()
            .is_empty()
    );
    let mut activate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the source menu activation frame begins");
    activate
        .submit_surface_action(open)
        .expect("the public source menu action submits");
    activate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the active popup defers fresh measurements");
    assert!(
        activate
            .commit()
            .expect("the source menu opens through the original state store")
            .native_admissions()
            .is_empty()
    );
    assert_pending_first_live_source(session, root, child, version);
    let popup = session.engine.presentation_requirements().popup();
    let source_bar = session
        .engine
        .presentation_requirements()
        .surface(root.surface())
        .expect("the source remains in the public requirements")
        .tab_bars()
        .find(|bar| bar.id().root == ROOT)
        .expect("the current requirements identify the source tab bar")
        .id();
    assert!(matches!(
        popup,
        PopupPlaneRequirement::Active { owner, .. }
            if owner.surface() == root.surface() && owner.bar() == source_bar
    ));
    popup
}

#[inline(never)]
fn emit_inactive_first_live_source_output(
    session: &mut DockspaceSession,
    root: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let mut measure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the surviving source measurement frame begins");
    assert!(measure.surfaces().contains(&root.surface()));
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("the surviving source bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("the surviving source pane minimum validates"),
        80.0,
    )
    .expect("the surviving source metrics validate");
    measure
        .measure_surface(root.surface(), metrics)
        .expect("the surviving source publicly measures after transfer");
    measure
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source measurement retains the exact child output");
    assert!(
        measure
            .commit()
            .expect("the surviving source measurements commit")
            .native_admissions()
            .is_empty()
    );

    let mut paint = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the surviving source paint frame begins");
    let plan = paint
        .paint_plan(root.surface())
        .expect("the public surviving source plan resolves")
        .expect("the measured surviving source is paintable");
    assert!(matches!(
        plan.plan.popup(),
        PopupPlaneRequirement::Inactive { .. }
    ));
    let mut panes = 0;
    let mut semantic_output = None;
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Pane(pane),
                    ..
                } => {
                    assert_ne!(pane.root(), ROOT);
                    panes += 1;
                }
                SurfacePaintProgramStep::PopupBackdrop(_)
                | SurfacePaintProgramStep::PopupMenu { .. } => {
                    panic!("the surviving source must paint the inactive popup successor");
                }
                SurfacePaintProgramStep::SurfaceEnd {
                    semantic_output: output,
                } => assert!(semantic_output.replace(output).is_none()),
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .expect("the surviving source executes its complete inactive program");
    assert!(panes > 0, "the original source still owns a live main pane");
    paint
        .confirm_surface_painted(receipt)
        .expect("the complete surviving source program confirms");
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source painting retains the exact child output");
    let mut report = paint
        .commit()
        .expect("the inactive surviving source output emits");
    assert!(report.native_admissions().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap_or_else(
        |outputs: Vec<PaintedSurfaceOutput>| {
            panic!("expected one exact inactive source output, got {outputs:?}")
        },
    );
    assert!(output.matches_native_binding(root));
    assert!(output.matches_semantic_output(
        semantic_output.expect("the surviving source emits its semantic seal")
    ));
    output
}

#[test]
fn active_popup_first_live_presented_dismisses_before_fresh_inactive_output_admits_once() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let popup = open_first_live_source_popup(&mut session, root, child);
    let sequence = session.committed_source_sequence;
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();

    // A complete child program still belongs to the active workspace popup plane.
    let output = emit_first_live_child_output(&mut session, root, child);
    let active_artifact = output.presentation_artifact();
    let active_ticket = session
        .engine
        .presentation_requirements()
        .surface(child.surface())
        .expect("the pending child has active popup requirements")
        .ticket();
    assert!(
        session
            .engine
            .presentation_requirements()
            .surface(child.surface())
            .unwrap()
            .popup_plane_bounds()
            .is_none()
    );
    let ingress = session.engine.backend_ingress_committed_through();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact complete child output records with the active popup");
    assert_pending_first_live_source(&session, root, child, version);
    assert_eq!(session.engine.presentation_requirements().popup(), popup);

    // Dismissing inside a discarded candidate must not close the published menu.
    let mut candidate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the rollbackable popup dismissal candidate begins");
    assert_eq!(candidate.version(), version);
    for item in [ITEM, SECOND_ITEM] {
        let source = candidate
            .view()
            .item(item)
            .expect("the source item remains");
        assert_eq!((source.root(), source.surface()), (ROOT, root.surface()));
    }
    let inactive = candidate.frame.view().presentation_requirements().popup();
    assert!(matches!(inactive, PopupPlaneRequirement::Inactive { .. }));
    assert_ne!(inactive.revision(), popup.revision());
    assert!(
        candidate
            .frame
            .view()
            .presentation_requirements()
            .surface(child.surface())
            .expect("popup dismissal retains the pending child requirements")
            .popup_plane_bounds()
            .is_none()
    );
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the discarded popup dismissal settles both surfaces");
    drop(candidate);
    assert_pending_first_live_source(&session, root, child, version);
    assert_eq!(session.engine.presentation_requirements().popup(), popup);
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(session.engine.backend_ingress_committed_through(), ingress);

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the active-popup output must settle exactly once without admission");
    };
    assert!(presentation.retires(active_artifact));
    assert_pending_first_live_source(&session, root, child, version);
    assert_eq!(session.engine.presentation_requirements().popup(), inactive);
    let inactive_requirements = session
        .engine
        .presentation_requirements()
        .surface(child.surface())
        .expect("the unadmitted child remains publicly measurable");
    assert_ne!(inactive_requirements.ticket(), active_ticket);
    assert!(
        inactive_requirements.popup_plane_bounds().is_none(),
        "rebuilding an inactive popup must clear the child's former Some bounds key"
    );
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_pending_first_live_source(&session, root, child, version);

    // Only fresh public measurements and a new complete inactive program may transfer.
    let replacement = emit_first_live_child_output(&mut session, root, child);
    let inactive_artifact = replacement.presentation_artifact();
    assert_ne!(inactive_artifact, active_artifact);
    session
        .report_surface_presentation(replacement, SurfacePresentationResult::Presented)
        .expect("the fresh inactive child output records its exact presentation");
    assert_pending_first_live_source(&session, root, child, version);
    let report = commit_managed_frame_report(&mut session);
    assert_eq!(report.native_admissions(), &[child]);
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the fresh inactive child artifact must be accepted exactly once");
    };
    assert!(presentation.presents(inactive_artifact));
    assert!(!presentation.presents(active_artifact));
    assert_first_live_child_admitted(&session, child, version);
    assert!(
        session.presented_surface(child.surface()).is_some(),
        "the child's exact first-live output is independent of source repaint"
    );
    assert!(
        session
            .engine
            .presentation_retention_manifest()
            .retains(inactive_artifact.output().key()),
        "the exact presented child output must retain its receiver resources"
    );
    assert!(
        !session
            .engine
            .presentation_retention_manifest()
            .retains(active_artifact.output().key()),
        "the discarded active popup output must not keep receiver resources alive"
    );
    assert_eq!(session.engine.presentation_requirements().popup(), inactive);
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .len(),
        0
    );

    // The source independently retires its old popup visual.
    let source_output = emit_inactive_first_live_source_output(&mut session, root);
    let source_artifact = source_output.presentation_artifact();
    assert!(session.presented_surface(child.surface()).is_some());
    session
        .report_surface_presentation(source_output, SurfacePresentationResult::Presented)
        .expect("the surviving source records its fresh inactive presentation");
    assert!(session.presented_surface(child.surface()).is_some());
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the source must settle its exact inactive successor without readmitting the child");
    };
    assert!(presentation.presents(source_artifact));
    assert_first_live_child_admitted(&session, child, version);
    assert_eq!(session.engine.presentation_requirements().popup(), inactive);
    assert_eq!(session.committed_source_sequence, sequence);
    assert!(
        session
            .presented_surface(root.surface())
            .expect("the source presents its inactive successor")
            .matches_presentation_artifact(source_artifact)
    );
    assert!(
        session
            .presented_surface(child.surface())
            .expect("source repaint preserves the child's original authority")
            .matches_presentation_artifact(inactive_artifact)
    );

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_first_live_child_admitted(&session, child, version);
    assert_eq!(session.committed_source_sequence, sequence);
    assert!(
        session
            .presented_surface(child.surface())
            .expect("the exact inactive presentation remains authoritative")
            .matches_presentation_artifact(inactive_artifact)
    );
}

#[test]
fn disabling_native_policy_cancels_pending_first_live_without_transfer_or_late_admission() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let source = session.engine.workspace().clone();
    let (saga, create) = session
        .engine
        .viewport()
        .native_create_sagas()
        .find(|(_, saga)| saga.binding() == child.binding)
        .map(|(id, saga)| (id, saga.create()))
        .expect("the pending first-live saga retains its original create");
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let output = emit_first_live_child_output(&mut session, root, child);
    let artifact = output.presentation_artifact();
    assert_pending_first_live_source(&session, root, child, version);

    let mut policy = session.engine.policy().clone();
    policy.set_allow_native_surfaces(false);
    let mut replace = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the public policy replacement frame begins");
    replace
        .replace_policy(policy)
        .expect("the public policy replacement revokes native creation");
    replace
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the policy frame settles the old presentation roster");
    let mut report = replace
        .commit()
        .expect("revoking the pending command must cancel instead of rolling back invariants");
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_eq!(session.version().epoch(), version.epoch());
    assert_eq!(
        session.version().revision().get(),
        version.revision().get() + 1,
        "policy replacement advances once without a topology transfer"
    );
    let cancelled_version = session.version();
    assert_eq!(session.committed_source_sequence, sequence + 1);
    assert_eq!(
        session.engine.workspace(),
        &source,
        "policy cancellation must preserve the entire source topology"
    );
    assert!(session.engine.viewport().native_create_saga(saga).is_none());
    assert!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .is_none()
    );
    assert!(session.engine.scene().surface(child.surface()).is_none());
    assert!(
        session
            .engine
            .presentation_requirements()
            .surface(child.surface())
            .is_none()
    );
    assert!(session.presented_surface(child.surface()).is_none());
    let retirement = session
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == child.binding).then_some(retirement))
        .expect("policy cancellation transfers the exact child to its cleanup owner");
    assert_eq!(
        retirement.origin(),
        crate::frame::BindingRetirementOrigin::NativeCreateAborted { create }
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    let compensation = take_only_native_effect(&mut report);
    assert!(matches!(
        compensation.operation(),
        NativeEffectOperation::CompensatingClose { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Close(_)) = compensation.accepted() else {
        panic!("the original child cleanup must retain its exact close acknowledgement");
    };

    // A late real output cannot restore a saga whose policy authority was revoked.
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the original affine output can still report its late terminal result");
    assert_eq!(session.engine.workspace(), &source);
    assert!(session.presented_surface(child.surface()).is_none());
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the late cancelled child output must settle exactly once");
    };
    assert!(presentation.retires(artifact));
    assert_eq!(session.version(), cancelled_version);
    assert_eq!(session.committed_source_sequence, sequence + 1);
    assert_eq!(session.engine.workspace(), &source);
    assert!(session.engine.viewport().native_create_saga(saga).is_none());
    assert!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .is_none()
    );
    assert!(session.presented_surface(child.surface()).is_none());
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("the cancelled child's resource remains owned by exact cleanup");

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_eq!(session.version(), cancelled_version);
    assert_eq!(session.committed_source_sequence, sequence + 1);
    assert_eq!(session.engine.workspace(), &source);
    assert!(session.presented_surface(child.surface()).is_none());
}

#[test]
fn dropped_first_live_output_requires_new_measured_exact_presented_output() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let output = emit_first_live_child_output(&mut session, root, child);
    let dropped_artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .expect("the exact dropped first-live child output records");
    assert_pending_first_live_source(&session, root, child, version);
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the dropped first-live output must settle its exact emission");
    };
    assert!(presentation.retires(dropped_artifact));
    assert_pending_first_live_source(&session, root, child, version);
    assert!(
        commit_managed_frame_report(&mut session)
            .native_admissions()
            .is_empty()
    );
    assert_pending_first_live_source(&session, root, child, version);

    let replacement = emit_first_live_child_output(&mut session, root, child);
    let replacement_artifact = replacement.presentation_artifact();
    assert_ne!(replacement_artifact, dropped_artifact);
    assert_pending_first_live_source(&session, root, child, version);
    session
        .report_surface_presentation(replacement, SurfacePresentationResult::Presented)
        .expect("only the newly measured and painted exact child output is presented");
    assert_pending_first_live_source(&session, root, child, version);
    let report = commit_managed_frame_report(&mut session);
    assert_eq!(report.native_admissions(), &[child]);
    let [presentation] = report.surface_presentation_commits() else {
        panic!("the replacement first-live output must settle exactly once");
    };
    assert!(presentation.presents(replacement_artifact));
    assert!(!presentation.presents(dropped_artifact));
    assert_first_live_child_admitted(&session, child, version);
    assert!(
        session
            .presented_surface(child.surface())
            .expect("the replacement output grants exact child presentation authority")
            .matches_presentation_artifact(replacement_artifact)
    );
    assert_eq!(session.committed_source_sequence, sequence);
}

#[test]
fn unreported_and_abandoned_first_live_output_preserve_source_ownership() {
    let (mut session, root, child) = pending_first_live_native_create();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let resources = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let output = emit_first_live_child_output(&mut session, root, child);
    let artifact = output.presentation_artifact();

    // Keeping an affine output alive without reporting it grants no authority.
    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert_pending_first_live_source(&session, root, child, version);
    drop(output);
    assert_pending_first_live_source(&session, root, child, version);

    let report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    let [presentation] = report.surface_presentation_commits() else {
        panic!("dropping the affine output must retire its exact emission");
    };
    assert!(presentation.retires(artifact));
    assert_pending_first_live_source(&session, root, child, version);
    assert!(
        commit_managed_frame_report(&mut session)
            .native_admissions()
            .is_empty()
    );
    assert_pending_first_live_source(&session, root, child, version);
    assert_eq!(session.committed_source_sequence, sequence);

    // Abandoning an output does not cancel its saga or release its resource.
    assert_eq!(resources.len(), 1);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("the unadmitted child retains its exact staging resource");
}

fn first_live_measurements(
    requirements: &crate::scene_manifest::SurfaceRequirements,
) -> crate::scene_manifest::SurfaceMeasurements {
    use crate::scene_manifest::{Measurement, SurfaceMeasurements, TabIntrinsic, TabStripMetrics};

    let bounds = crate::geometry::LogicalRect::new(0.0, 0.0, 420.0, 320.0)
        .expect("the child surface bounds validate");
    let minimum =
        crate::geometry::LogicalSize::new(32.0, 24.0).expect("the pane minimum validates");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("the exact surface bounds answer is unique");
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, Measurement::Measured(bounds))
            .expect("the exact popup-plane bounds answer is unique");
    }
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("each exact pane minimum answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(
                    TabIntrinsic::new(80.0).expect("the tab intrinsic validates"),
                ),
            )
            .expect("each exact tab intrinsic answer is unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0).expect("the empty strip metrics validate"),
                ),
            )
            .expect("each exact tab-strip answer is unique");
    }
    measurements
}

#[test]
fn native_first_live_projection_compiles_child_without_publishing_ownership() {
    use crate::frame::NativeCreatePhase;

    let (session, root, child) = pending_first_live_native_create();

    let (saga_id, saga) = session
        .engine
        .viewport()
        .native_create_sagas()
        .find(|(_, saga)| saga.binding() == child.binding)
        .expect("the exact child create saga is retained");
    let initial_version = session.version();
    let initial_source_sequence = session.committed_source_sequence;
    let initial_phase = saga.phase();
    let initial_admission = session
        .engine
        .viewport()
        .viewport(child.surface())
        .expect("the child binding is reserved")
        .admission();
    assert_eq!(initial_admission, ViewportAdmission::Pending);
    assert!(matches!(
        initial_phase,
        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert!(
        session
            .engine
            .workspace()
            .surface(child.surface())
            .is_none()
    );

    let source_ownership = [ITEM, SECOND_ITEM].map(|item| {
        let source = session.view().item(item).expect("the source item exists");
        (item, source.root(), source.surface())
    });
    assert_eq!(
        source_ownership,
        [
            (ITEM, ROOT, root.surface()),
            (SECOND_ITEM, ROOT, root.surface()),
        ],
        "the source still owns both items before temporary projection"
    );
    let source_selected = [ITEM, SECOND_ITEM]
        .into_iter()
        .find(|item| {
            session
                .view()
                .item(*item)
                .is_some_and(|source| source.is_selected())
        })
        .expect("the source tabs leaf has one selected item");

    let projection = session
        .engine
        .native_first_live_projection(saga_id)
        .expect("temporary first-live projection preparation succeeds")
        .expect("the pending child has a current first-live projection");
    assert_eq!(projection.surface(), child.surface());
    assert_ne!(projection.version(), initial_version);
    let stamp = projection.stamp();
    assert!(
        session
            .engine
            .native_first_live_projection_is_current(stamp)
    );
    let measurements = first_live_measurements(projection.requirements());
    let plan = projection
        .compile(&measurements)
        .expect("the exact child requirements compile into a complete plan");

    assert_eq!(plan.surface(), child.surface());
    assert_eq!(plan.measurement_ticket(), Some(measurements.ticket()));
    for item in [ITEM, SECOND_ITEM] {
        assert!(
            plan.tab_records().iter().any(|tab| tab.id().item == item),
            "the temporary child plan contains a tab record for {item:?}"
        );
    }
    let selected_panes = plan
        .pane_records()
        .iter()
        .filter(|pane| pane.selected().is_some())
        .collect::<Vec<_>>();
    assert_eq!(selected_panes.len(), 1);
    assert_eq!(selected_panes[0].id().root, ROOT);
    assert_eq!(selected_panes[0].selected(), Some(source_selected));
    let mut program_steps = 0;
    plan.visit_surface_program(|_| {
        program_steps += 1;
        Ok::<(), ()>(())
    })
    .expect("the temporary plan exposes a valid complete surface program");
    assert!(program_steps > 0);

    drop(projection);
    drop(plan);
    assert_eq!(session.version(), initial_version);
    assert_eq!(session.committed_source_sequence, initial_source_sequence);
    assert_eq!(
        session.engine.workspace().surface(child.surface()),
        None,
        "compiling and dropping the temporary plan does not publish a child surface"
    );
    for (item, source_root, source_surface) in source_ownership {
        let source = session
            .view()
            .item(item)
            .expect("the source item remains present");
        assert_eq!(
            (source.root(), source.surface()),
            (source_root, source_surface)
        );
    }
    assert_eq!(
        session
            .engine
            .viewport()
            .native_create_saga(saga_id)
            .expect("the create saga remains pending")
            .phase(),
        initial_phase
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(child.surface())
            .expect("the child binding remains reserved")
            .admission(),
        initial_admission
    );

    assert!(
        session
            .engine
            .native_first_live_projection_is_current(stamp)
    );
}
