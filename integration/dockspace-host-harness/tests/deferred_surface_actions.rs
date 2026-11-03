//! Deferred local actions retain captured meaning across unpublished paint passes.

use dockspace::close::{CloseDecision, ClosePlanTarget};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DeferredSurfaceAction, DockPresentationConfig, DockspaceHostFrame,
    DockspacePresentationCommandKind, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostPointerInput, HostPointerRoute, PreparedSurfaceAction, SurfaceContainedResizeAdjustment,
    SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Question, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton as Button, SurfacePointerCapture, SurfacePointerEvent as Event,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceSplitterAdjustment, SurfaceUnavailableReason, TabListMenuMetrics, TabStripControlKind,
    TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const OTHER: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const FLOAT_ROOT: RootId = RootId::new(2);
const FLOAT: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const FLOAT_FIRST: ItemId = ItemId::new(3);
const FLOAT_SECOND: ItemId = ItemId::new(4);
const OTHER_FIRST: ItemId = ItemId::new(5);
const OTHER_SECOND: ItemId = ItemId::new(6);
const UNSEEN: ItemId = ItemId::new(7);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOAT,
            DockspaceRootLayout::new(FLOAT_ROOT, DockspaceNode::tabs([FLOAT_FIRST, FLOAT_SECOND])),
            rect(80.0, 100.0, 360.0, 260.0),
        )),
        DockspaceSurfaceLayout::new(
            OTHER,
            DockspaceRootLayout::new(
                RootId::new(3),
                DockspaceNode::central_tabs([OTHER_FIRST, OTHER_SECOND]),
            ),
        ),
    ])
    .unwrap()
}

fn session() -> DockspaceSession {
    DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap()
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 900.0, 650.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn capture(
    session: &mut DockspaceSession,
    prepare: impl FnOnce(SurfacePaintPlan<'_>) -> PreparedSurfaceAction,
) -> DeferredSurfaceAction {
    let mut discarded_pass = session.begin_host_frame().unwrap();
    discarded_pass.measure_surface(SURFACE, metrics()).unwrap();
    let action = prepare(discarded_pass.paint_plan(SURFACE).unwrap().unwrap()).into_deferred();
    drop(discarded_pass);
    action
}

fn submit(session: &mut DockspaceSession, action: DeferredSurfaceAction) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_deferred_surface_action(action).unwrap();
    commit(frame)
}

fn assert_stale(report: &HostFrameReport) {
    assert!(
        !report.workspace_changed(),
        "a stale witness must remain inert: {report:?}"
    );
    assert!(
        matches!(report.inputs(), [HostInputOutcome::StaleRejected { .. }]),
        "{report:?}"
    );
}

#[test]
fn unpublished_discarded_pass_keeps_its_selection_witness_for_the_next_update() {
    let mut session = session();
    let before = session.version();
    let action = capture(&mut session, |plan| {
        plan.prepare_tab_select(SECOND).unwrap()
    });
    assert_eq!(session.version(), before);
    assert!(session.presented_surface(SURFACE).is_none());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    let report = submit(&mut session, action);
    assert!(report.workspace_changed(), "{report:?}");
    assert!(session.view().item(SECOND).unwrap().is_selected());
}

#[test]
fn unrelated_revision_does_not_retarget_or_expire_the_frozen_selection() {
    let mut session = session();
    let old = capture(&mut session, |plan| {
        plan.prepare_tab_select(SECOND).unwrap()
    });
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(OTHER_SECOND).unwrap();
    assert!(commit(frame).workspace_changed());
    let fresh = capture(&mut session, |plan| {
        plan.prepare_tab_select(SECOND).unwrap()
    });
    assert_ne!(old.expected_version(), fresh.expected_version());
    assert!(old.same_intent(&fresh));
    assert!(submit(&mut session, old).workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(session.view().item(OTHER_SECOND).unwrap().is_selected());
}

#[test]
fn foreign_witness_is_a_typed_input_rejection_and_does_not_poison_the_frame() {
    let mut origin = session();
    let foreign = capture(&mut origin, |plan| plan.prepare_tab_select(SECOND).unwrap());
    let mut destination = session();
    let mut frame = destination.begin_host_frame().unwrap();
    frame.submit_deferred_surface_action(foreign).unwrap();
    frame.select_item_current(OTHER_SECOND).unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.inputs().first(),
        Some(HostInputOutcome::StaleRejected { .. })
    ));
    assert!(destination.view().item(FIRST).unwrap().is_selected());
    assert!(destination.view().item(OTHER_SECOND).unwrap().is_selected());
}

#[test]
fn replacing_the_epoch_rejects_a_witness_even_when_stable_identities_match() {
    let mut session = session();
    let action = capture(&mut session, |plan| {
        plan.prepare_tab_select(SECOND).unwrap()
    });
    let reset = session.prepare_layout_reset(layout()).unwrap();
    commit(session.begin_layout_reset_frame(&reset).unwrap());
    assert_stale(&submit(&mut session, action));
    assert!(session.view().item(FIRST).unwrap().is_selected());
}

#[test]
fn policy_and_presentation_configuration_replacements_expire_the_witness() {
    for replace_policy in [false, true] {
        let mut session = session();
        let action = capture(&mut session, |plan| {
            plan.prepare_tab_select(SECOND).unwrap()
        });
        let mut frame = session.begin_host_frame().unwrap();
        if replace_policy {
            let mut policy = DockPolicy::default();
            policy.set_arrangement_locked(true);
            frame.replace_policy(policy).unwrap();
        } else {
            frame
                .replace_presentation_config(
                    DockPresentationConfig::builder()
                        .splitter_keyboard_step(24.0)
                        .build()
                        .unwrap(),
                )
                .unwrap();
        }
        commit(frame);
        assert_stale(&submit(&mut session, action));
    }
}

#[test]
fn changed_item_owner_cannot_reinterpret_a_selection() {
    let mut session = session();
    let action = capture(&mut session, |plan| {
        plan.prepare_tab_select(SECOND).unwrap()
    });
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(SECOND, DockPlacement::After(OTHER_FIRST))
        .unwrap();
    assert!(commit(frame).workspace_changed());
    assert_stale(&submit(&mut session, action));
    assert_eq!(session.view().item(SECOND).unwrap().surface(), OTHER);
}

#[test]
fn same_bar_new_members_do_not_expand_item_selection_or_close_authority() {
    for close_item in [false, true] {
        let mut session = session();
        let item_action = capture(&mut session, |plan| {
            if close_item {
                plan.prepare_tab_close(FLOAT_SECOND).unwrap()
            } else {
                plan.prepare_tab_select(FLOAT_SECOND).unwrap()
            }
        });
        let root_close = capture(&mut session, |plan| {
            plan.prepare_root_close(FLOAT_ROOT).unwrap()
        });
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .open_item_current(UNSEEN, DockPlacement::After(FLOAT_SECOND))
            .unwrap();
        assert!(commit(frame).workspace_changed());
        assert_stale(&submit(&mut session, root_close));
        let report = submit(&mut session, item_action);
        if close_item {
            let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
                panic!("the unchanged item close must produce one plan: {report:?}");
            };
            assert_eq!(plan.target(), ClosePlanTarget::Item { item: FLOAT_SECOND });
            assert_eq!(plan.items().len(), 1);
            assert_eq!(plan.items()[0].item(), FLOAT_SECOND);
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .resolve_close(
                    plan.request(),
                    plan.items()[0].token(),
                    CloseDecision::Allow,
                )
                .unwrap();
            assert!(commit(frame).workspace_changed());
            assert!(session.view().item(FLOAT_SECOND).is_none());
            assert_eq!(
                session.view().item(UNSEEN).unwrap().tabs().items(),
                &[FLOAT_FIRST, UNSEEN]
            );
        } else {
            assert!(report.workspace_changed(), "{report:?}");
            assert!(session.view().item(FLOAT_SECOND).unwrap().is_selected());
            assert_eq!(
                session.view().item(UNSEEN).unwrap().tabs().items(),
                &[FLOAT_FIRST, FLOAT_SECOND, UNSEEN]
            );
        }
        assert_eq!(
            session.view().item(UNSEEN).unwrap().contained(),
            Some(FLOAT)
        );
    }
}

#[test]
fn unrelated_sibling_bar_changes_do_not_expire_item_actions_in_the_same_root() {
    for close_item in [false, true] {
        let content = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::central_tabs([FIRST, SECOND]),
                DockspaceNode::tabs([FLOAT_FIRST, FLOAT_SECOND]),
            ],
        )
        .unwrap();
        let mut session = DockspaceSession::from_layout(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, content),
            )])
            .unwrap(),
            DockPolicy::default(),
        )
        .unwrap();
        let action = capture(&mut session, |plan| {
            if close_item {
                plan.prepare_tab_close(SECOND).unwrap()
            } else {
                plan.prepare_tab_select(SECOND).unwrap()
            }
        });
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .open_item_current(UNSEEN, DockPlacement::After(FLOAT_SECOND))
            .unwrap();
        assert!(commit(frame).workspace_changed());
        let report = submit(&mut session, action);
        if close_item {
            let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
                panic!("the sibling mutation must not expire an item close: {report:?}");
            };
            assert_eq!(plan.target(), ClosePlanTarget::Item { item: SECOND });
            assert_eq!(
                plan.items()
                    .iter()
                    .map(|item| item.item())
                    .collect::<Vec<_>>(),
                [SECOND]
            );
        } else {
            assert!(report.workspace_changed(), "{report:?}");
            assert!(session.view().item(SECOND).unwrap().is_selected());
        }
        assert_eq!(
            session.view().item(UNSEEN).unwrap().tabs().items(),
            &[FLOAT_FIRST, FLOAT_SECOND, UNSEEN]
        );
    }
}

#[test]
fn item_contained_and_root_close_use_the_unpublished_captured_plan() {
    for kind in 0..3 {
        let mut session = session();
        let action = capture(&mut session, |plan| match kind {
            0 => plan.prepare_tab_close(FLOAT_SECOND).unwrap(),
            1 => plan.prepare_contained_close(FLOAT).unwrap(),
            _ => plan.prepare_root_close(FLOAT_ROOT).unwrap(),
        });
        let report = submit(&mut session, action);
        let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
            panic!("the captured close must produce an actionable plan: {report:?}");
        };
        let expected = if kind == 0 {
            ClosePlanTarget::Item { item: FLOAT_SECOND }
        } else {
            ClosePlanTarget::Root { root: FLOAT_ROOT }
        };
        assert_eq!(plan.target(), expected);
        let items = plan
            .items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>();
        assert_eq!(
            items,
            if kind == 0 {
                vec![FLOAT_SECOND]
            } else {
                vec![FLOAT_FIRST, FLOAT_SECOND]
            }
        );
    }
}

#[test]
fn group_actions_never_capture_an_unseen_member_from_the_current_workspace() {
    for kind in 0..3 {
        let mut session = session();
        let action = capture(&mut session, |plan| match kind {
            0 => plan.prepare_contained_close(FLOAT).unwrap(),
            1 => plan.prepare_root_close(FLOAT_ROOT).unwrap(),
            _ => {
                let command = plan
                    .presentation_command(FLOAT_ROOT, DockspacePresentationCommandKind::DockCenter)
                    .unwrap()
                    .unwrap();
                plan.prepare_presentation_command(command).unwrap()
            }
        });
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .open_item_current(UNSEEN, DockPlacement::After(FLOAT_SECOND))
            .unwrap();
        assert!(commit(frame).workspace_changed());
        assert_stale(&submit(&mut session, action));
        assert_eq!(
            session.view().item(UNSEEN).unwrap().contained(),
            Some(FLOAT)
        );
    }
}

#[test]
fn presentation_command_checks_both_frozen_roots_before_recapturing_a_group() {
    for change_destination in [false, true] {
        let mut session = session();
        let action = capture(&mut session, |plan| {
            let command = plan
                .presentation_command(FLOAT_ROOT, DockspacePresentationCommandKind::DockCenter)
                .unwrap()
                .unwrap();
            plan.prepare_presentation_command(command).unwrap()
        });
        let mut frame = session.begin_host_frame().unwrap();
        if change_destination {
            frame
                .open_item_current(UNSEEN, DockPlacement::After(SECOND))
                .unwrap();
        } else {
            frame.select_item_current(OTHER_SECOND).unwrap();
        }
        assert!(commit(frame).workspace_changed());
        let report = submit(&mut session, action);
        if change_destination {
            assert_stale(&report);
            assert_eq!(
                session.view().item(FLOAT_FIRST).unwrap().contained(),
                Some(FLOAT)
            );
        } else {
            assert!(report.workspace_changed(), "{report:?}");
            assert_eq!(session.view().item(FLOAT_FIRST).unwrap().root(), ROOT);
            assert!(session.view().contained(FLOAT).is_none());
        }
    }
}

#[test]
fn contained_resize_uses_frozen_geometry_and_invalidates_the_old_close() {
    let mut session = session();
    let old_close = capture(&mut session, |plan| {
        plan.prepare_contained_close(FLOAT).unwrap()
    });
    let resize = capture(&mut session, |plan| {
        plan.prepare_contained_resize_adjustment(
            FLOAT,
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
        )
        .unwrap()
    });
    let before = session.view().contained(FLOAT).unwrap().rect();
    assert!(submit(&mut session, resize).workspace_changed());
    assert!(session.view().contained(FLOAT).unwrap().rect().width() > before.width());
    assert_stale(&submit(&mut session, old_close));
}

fn splitter_session(junction: bool) -> DockspaceSession {
    let tabs = |item| DockspaceNode::tabs([item]);
    let content = if junction {
        let column =
            |a, b| DockspaceNode::equal_split(DockspaceAxis::Vertical, [tabs(a), tabs(b)]).unwrap();
        DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [column(FIRST, SECOND), column(FLOAT_FIRST, FLOAT_SECOND)],
        )
        .unwrap()
    } else {
        DockspaceNode::equal_split(DockspaceAxis::Horizontal, [tabs(FIRST), tabs(SECOND)]).unwrap()
    };
    DockspaceSession::from_layout(
        DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, content),
        )])
        .unwrap(),
        DockPolicy::default(),
    )
    .unwrap()
}

fn splitter_action(
    plan: SurfacePaintPlan<'_>,
    junction: bool,
    adjustment: SurfaceSplitterAdjustment,
) -> PreparedSurfaceAction {
    let mut action = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation { operation, .. } = step {
            match operation {
                Operation::Splitter(record) if !junction && action.is_none() => {
                    action = plan.prepare_splitter_adjustment(record.visual_id(), adjustment);
                }
                Operation::SplitterJunction(record) if junction && action.is_none() => {
                    action = plan.prepare_splitter_junction_adjustment(
                        record.visual_id(),
                        DockspaceAxis::Vertical,
                        adjustment,
                    );
                }
                _ => {}
            }
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    action.unwrap()
}

#[test]
fn identical_increments_from_independent_runs_both_execute_without_core_deduplication() {
    for junction in [false, true] {
        let mut session = splitter_session(junction);
        for _independent_run in 0..2 {
            let increment = capture(&mut session, |plan| {
                splitter_action(plan, junction, SurfaceSplitterAdjustment::Increment)
            });
            let repeated_pass = capture(&mut session, |plan| {
                splitter_action(plan, junction, SurfaceSplitterAdjustment::Increment)
            });
            let opposite = capture(&mut session, |plan| {
                splitter_action(plan, junction, SurfaceSplitterAdjustment::Decrement)
            });
            assert!(increment.same_intent(&repeated_pass));
            assert!(!increment.same_intent(&opposite));
            let before = session.version();
            assert!(submit(&mut session, increment).workspace_changed());
            assert_ne!(session.version(), before);
        }
    }
}

#[test]
fn changed_splitter_weights_reject_a_second_action_from_the_old_plan() {
    let mut session = splitter_session(false);
    let first = capture(&mut session, |plan| {
        splitter_action(plan, false, SurfaceSplitterAdjustment::Increment)
    });
    let stale = capture(&mut session, |plan| {
        splitter_action(plan, false, SurfaceSplitterAdjustment::Increment)
    });
    assert!(first.same_intent(&stale));
    assert!(submit(&mut session, first).workspace_changed());
    assert_stale(&submit(&mut session, stale));
}

fn overflow_measurement(question: Question) -> Answer {
    match question {
        Question::DockBounds { .. } | Question::PopupPlaneBounds { .. } => {
            Answer::Bounds(rect(0.0, 0.0, 260.0, 220.0))
        }
        Question::PaneMinimum { .. } => Answer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap()),
        Question::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
        Question::TabStrip { .. } => Answer::TabStrip(
            TabStripMetrics::new(0.0, 0.0)
                .unwrap()
                .with_controls(
                    TabStripControlMetrics::new(4.0)
                        .unwrap()
                        .with_tab_list_menu(
                            TabStripControlMetric::new(
                                20.0,
                                TabStripControlPlacement::ReservedTrailing,
                            )
                            .unwrap(),
                        ),
                )
                .with_tab_list_menu(
                    TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 140.0, 10.0).unwrap(),
                ),
        ),
    }
}

fn capture_overflow(
    session: &mut DockspaceSession,
    prepare: impl FnOnce(SurfacePaintPlan<'_>) -> PreparedSurfaceAction,
) -> DeferredSurfaceAction {
    let mut discarded_pass = session.begin_host_frame().unwrap();
    discarded_pass
        .measure_surface_with(SURFACE, overflow_measurement)
        .unwrap();
    let action = prepare(discarded_pass.paint_plan(SURFACE).unwrap().unwrap()).into_deferred();
    drop(discarded_pass);
    action
}

fn open_menu(plan: SurfacePaintPlan<'_>) -> PreparedSurfaceAction {
    let mut action = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabStripControl(control),
            ..
        } = step
            && control.kind() == TabStripControlKind::TabListMenu
        {
            action = plan.prepare_tab_strip_control_activation(control);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    action.unwrap()
}

#[test]
fn tab_control_validates_its_own_membership_without_freezing_a_sibling_bar() {
    for change_own_bar in [false, true] {
        let content = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::central_tabs([FIRST, SECOND, FLOAT_FIRST, FLOAT_SECOND]),
                DockspaceNode::tabs([OTHER_FIRST, OTHER_SECOND]),
            ],
        )
        .unwrap();
        let mut session = DockspaceSession::from_layout(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, content),
            )])
            .unwrap(),
            DockPolicy::default(),
        )
        .unwrap();
        let action = capture_overflow(&mut session, |plan| {
            let mut action = None;
            plan.visit_surface_program(|step| {
                if let Step::Operation {
                    operation: Operation::TabStripControl(control),
                    ..
                } = step
                    && control.kind() == TabStripControlKind::TabListMenu
                    && control.bounds().x() < 130.0
                {
                    action = plan.prepare_tab_strip_control_activation(control);
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            action.unwrap()
        });
        let mut frame = session.begin_host_frame().unwrap();
        let anchor = if change_own_bar {
            FLOAT_SECOND
        } else {
            OTHER_SECOND
        };
        frame
            .open_item_current(UNSEEN, DockPlacement::After(anchor))
            .unwrap();
        assert!(commit(frame).workspace_changed());
        let report = submit(&mut session, action);
        if change_own_bar {
            assert_stale(&report);
        } else {
            assert!(
                !report
                    .inputs()
                    .iter()
                    .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. })),
                "{report:?}"
            );
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .measure_surface_with(SURFACE, overflow_measurement)
                .unwrap();
            let mut menu = false;
            frame
                .paint_plan(SURFACE)
                .unwrap()
                .unwrap()
                .visit_surface_program(|step| {
                    menu |= matches!(step, Step::PopupMenu { .. });
                    Ok::<(), ()>(())
                })
                .unwrap();
            assert!(menu, "the unchanged source bar must open its menu");
        }
    }
}

#[test]
fn unpublished_tab_chrome_retains_its_popup_session_and_rejects_a_successor_popup() {
    for reopen in [false, true] {
        let mut session = DockspaceSession::from_layout(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(
                    ROOT,
                    DockspaceNode::central_tabs([FIRST, SECOND, FLOAT_FIRST, FLOAT_SECOND]),
                ),
            )])
            .unwrap(),
            DockPolicy::default(),
        )
        .unwrap();
        let open = capture_overflow(&mut session, open_menu);
        let report = submit(&mut session, open);
        assert!(
            !report
                .inputs()
                .iter()
                .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. })),
            "{report:?}"
        );
        let row = capture_overflow(&mut session, |plan| {
            let mut action = None;
            plan.visit_surface_program(|step| {
                if let Step::PopupMenu { menu, .. } = step {
                    let row = menu.rows().find(|row| row.item() == FLOAT_SECOND).unwrap();
                    action = plan.prepare_tab_list_menu_row_activation(row);
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            action.unwrap()
        });
        if reopen {
            let dismiss = capture_overflow(&mut session, |plan| {
                let mut action = None;
                plan.visit_surface_program(|step| {
                    if let Step::PopupBackdrop(backdrop) = step {
                        action = plan.prepare_tab_list_menu_dismiss(backdrop);
                    }
                    Ok::<(), ()>(())
                })
                .unwrap();
                action.unwrap()
            });
            submit(&mut session, dismiss);
            let open = capture_overflow(&mut session, open_menu);
            submit(&mut session, open);
            assert_stale(&submit(&mut session, row));
            assert!(session.view().item(FIRST).unwrap().is_selected());
        } else {
            assert!(submit(&mut session, row).workspace_changed());
            assert!(session.view().item(FLOAT_SECOND).unwrap().is_selected());
        }
    }
}

#[test]
fn only_the_owner_surface_mints_a_witness_to_dismiss_its_exact_global_popup() {
    let mut session = DockspaceSession::from_layout(
        DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(
                    ROOT,
                    DockspaceNode::central_tabs([FIRST, SECOND, FLOAT_FIRST, FLOAT_SECOND]),
                ),
            ),
            DockspaceSurfaceLayout::new(
                OTHER,
                DockspaceRootLayout::new(
                    RootId::new(3),
                    DockspaceNode::central_tabs([OTHER_FIRST, OTHER_SECOND]),
                ),
            ),
        ])
        .unwrap(),
        DockPolicy::default(),
    )
    .unwrap();
    let open = capture_overflow(&mut session, open_menu);
    submit(&mut session, open);
    let (dismiss, stale) = {
        let mut discarded_pass = session.begin_host_frame().unwrap();
        discarded_pass
            .measure_surface_with(SURFACE, overflow_measurement)
            .unwrap();
        let plan = discarded_pass.paint_plan(SURFACE).unwrap().unwrap();
        let mut backdrop = None;
        plan.visit_surface_program(|step| {
            if let Step::PopupBackdrop(record) = step {
                backdrop = Some(record);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        let backdrop = backdrop.unwrap();
        let actions = (
            plan.prepare_tab_list_menu_dismiss(backdrop)
                .unwrap()
                .into_deferred(),
            plan.prepare_tab_list_menu_dismiss(backdrop)
                .unwrap()
                .into_deferred(),
        );
        discarded_pass
            .measure_surface_with(OTHER, overflow_measurement)
            .unwrap();
        let plan = discarded_pass.paint_plan(OTHER).unwrap().unwrap();
        plan.visit_surface_program(|step| {
            assert!(
                !matches!(step, Step::PopupBackdrop(_) | Step::PopupMenu { .. }),
                "the global popup has visual controls only on its exact owner surface"
            );
            Ok::<(), ()>(())
        })
        .unwrap();
        actions
    };
    assert_eq!(dismiss.surface(), SURFACE);
    let report = submit(&mut session, dismiss);
    assert!(
        !report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. })),
        "{report:?}"
    );
    {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface_with(SURFACE, overflow_measurement)
            .unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut menu = false;
        plan.visit_surface_program(|step| {
            menu |= matches!(step, Step::PopupMenu { .. });
            Ok::<(), ()>(())
        })
        .unwrap();
        assert!(
            !menu,
            "the captured owner backdrop must dismiss its exact popup"
        );
    }
    let open = capture_overflow(&mut session, open_menu);
    submit(&mut session, open);
    assert_stale(&submit(&mut session, stale));
}

#[test]
fn deferred_escape_cancels_only_the_captured_gesture() {
    let mut session = session();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let descriptor = plan.tab_receiver(FIRST).unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    commit(session.begin_host_frame().unwrap());
    let surface = session.presented_surface(SURFACE).unwrap();
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let route = HostPointerRoute::new(surface, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::ButtonPressed(Button::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&surface),
        )])
        .unwrap();
    commit(frame);
    assert!(session.has_active_gesture());
    let cancel = capture(&mut session, |plan| plan.prepare_escape_cancel().unwrap());
    let repeated = capture(&mut session, |plan| plan.prepare_escape_cancel().unwrap());
    assert!(cancel.same_intent(&repeated));
    let report = submit(&mut session, cancel);
    assert!(
        !report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. })),
        "{report:?}"
    );
    assert!(!session.has_active_gesture());
    assert_stale(&submit(&mut session, repeated));
}
