use std::collections::BTreeMap;
use std::time::Duration;

use crate::geometry::{LogicalRect, LogicalSize};
use crate::model::{
    DockPlacement, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use crate::motion::{MotionFrame, MotionMode, MotionPreference, MotionSettings};
use crate::policy::DockPolicy;
use crate::presentation::compiler::{reset_surface_compilation_count, surface_compilation_count};
use crate::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, PaintedSurfaceOutput,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};

mod selection_rebind_tests;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 960.0, 600.0).unwrap(),
        LogicalSize::new(40.0, 30.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn motion(now: u64) -> MotionFrame {
    MotionFrame {
        now: Duration::from_millis(now),
        settings: MotionSettings::default(),
        preference: MotionPreference::Full,
    }
}

struct Sample {
    panes: BTreeMap<ItemId, LogicalRect>,
    receivers: BTreeMap<ItemId, DockspaceReceiverDescriptor>,
    partition: bool,
    splitters: usize,
    tab_offsets: Vec<f64>,
}

fn paint(session: &mut DockspaceSession, motion: MotionFrame) -> (Sample, PaintedSurfaceOutput) {
    paint_with_metrics(session, motion, metrics())
}

fn paint_with_metrics(
    session: &mut DockspaceSession,
    motion: MotionFrame,
    metrics: UniformSurfaceMetrics,
) -> (Sample, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.set_motion(motion).unwrap();
    frame.measure_surface(SURFACE, metrics).unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut sample = Sample {
        panes: BTreeMap::new(),
        receivers: BTreeMap::new(),
        partition: plan.partition_available(ROOT),
        splitters: 0,
        tab_offsets: Vec::new(),
    };
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Pane(pane),
                ..
            } => {
                if let Some(item) = pane.selected() {
                    sample.panes.insert(item, pane.bounds());
                    sample
                        .receivers
                        .insert(item, plan.receiver_for_pane(pane).unwrap());
                }
            }
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } => {
                sample.tab_offsets.push(bar.scroll_offset());
            }
            Step::Operation {
                operation: Operation::Splitter(splitter),
                ..
            } => {
                assert!(plan.receiver_for_splitter(splitter).is_some());
                sample.splitters += 1;
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    (sample, report.take_painted_outputs().pop().unwrap())
}

fn settle(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    settle(session);
}

fn fixture() -> (DockspaceSession, Sample) {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [A, B, C].map(|item| DockspaceNode::tabs([item])),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let (before, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    (session, before)
}

fn dock(session: &mut DockspaceSession, item: ItemId, after: ItemId) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(item, DockPlacement::After(after))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap().workspace_changed());
}

#[test]
fn survivor_reflow_uses_confirmed_content_and_ticks_do_not_resolve_target_again() {
    let (mut session, before) = fixture();
    dock(&mut session, C, A);
    reset_surface_compilation_count();
    let (start, output) = paint(&mut session, motion(1000));
    assert_eq!(surface_compilation_count(), 1);
    assert_eq!(start.panes[&B], before.panes[&B]);
    assert!(!start.partition);
    assert_eq!(start.splitters, 0);
    present(&mut session, output);
    reset_surface_compilation_count();
    let (middle, output) = paint(&mut session, motion(1050));
    assert_ne!(middle.panes[&B], before.panes[&B]);
    assert!(!middle.partition);
    assert!(session.surface_motion_is_active(SURFACE));
    assert_eq!(surface_compilation_count(), 0);
    present(&mut session, output);
    let (end, output) = paint(&mut session, motion(6000));
    assert!(end.partition);
    assert_eq!(end.splitters, 1);
    assert!(!session.surface_motion_is_active(SURFACE));
    assert_eq!(surface_compilation_count(), 0);
    present(&mut session, output);
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_some()
    );
}

#[test]
fn popup_on_another_surface_preserves_the_running_layout_sample() {
    use crate::runtime::{
        DockspaceHostFrame, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
        TabListMenuMetrics, TabStripControlKind, TabStripControlMetric, TabStripControlMetrics,
        TabStripControlPlacement, TabStripMetrics,
    };

    let menu_surface = SurfaceId::new(2);
    let menu_root = RootId::new(2);
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [A, B, C].map(|item| DockspaceNode::tabs([item])),
    )
    .unwrap();
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, content)),
        DockspaceSurfaceLayout::new(
            menu_surface,
            DockspaceRootLayout::new(menu_root, DockspaceNode::tabs((10..30).map(ItemId::new))),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let strip = TabStripMetrics::new(0.0, 0.0)
        .unwrap()
        .with_controls(
            TabStripControlMetrics::new(0.0)
                .unwrap()
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .unwrap(),
                ),
        )
        .with_tab_list_menu(TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).unwrap());
    let measure_menu = |frame: &mut DockspaceHostFrame<'_>| {
        frame
            .measure_surface_with(menu_surface, |request| match request {
                SurfaceMeasurementRequest::DockBounds { .. }
                | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(
                        LogicalRect::new(0.0, 0.0, 260.0, 180.0).unwrap(),
                    )
                }
                SurfaceMeasurementRequest::PaneMinimum { .. } => {
                    SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(40.0, 30.0).unwrap())
                }
                SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                    SurfaceMeasurementAnswer::TabIntrinsic(80.0)
                }
                SurfaceMeasurementRequest::TabStrip { .. } => {
                    SurfaceMeasurementAnswer::TabStrip(strip)
                }
            })
            .unwrap();
    };
    let mut frame = session.begin_host_frame().unwrap();
    measure_menu(&mut frame);
    frame.commit().unwrap();
    let (before, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    dock(&mut session, C, A);
    let (_, output) = paint(&mut session, motion(1000));
    present(&mut session, output);
    let (moving, output) = paint(&mut session, motion(1050));
    present(&mut session, output);
    assert!(moving.panes[&B].x() > before.panes[&B].x());
    assert!(!moving.partition);

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(menu_surface).unwrap().unwrap();
    let control = plan
        .tab_strip_controls()
        .find(|control| control.kind() == TabStripControlKind::TabListMenu)
        .unwrap();
    let open = plan
        .prepare_tab_strip_control_activation(control)
        .unwrap()
        .into_deferred();
    frame.commit().unwrap();

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_deferred_surface_action(open).unwrap();
    measure_menu(&mut frame);
    let plan = frame.paint_plan(menu_surface).unwrap().unwrap();
    assert!(plan.partition_available(menu_root));
    assert!(plan.tab_strip_controls().any(|control| control.menu_open()));
    let mut dismiss = None;
    let receipt = plan
        .execute_surface_program(|step| {
            if let Step::PopupBackdrop(backdrop) = step {
                dismiss = Some(
                    plan.prepare_tab_list_menu_dismiss(backdrop)
                        .unwrap()
                        .into_deferred(),
                );
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    present(&mut session, report.take_painted_outputs().pop().unwrap());

    let (during_popup, output) = paint(&mut session, motion(1050));
    assert_eq!(
        during_popup.panes, moving.panes,
        "another surface's popup must not replace the running sample with target geometry"
    );
    assert!(!during_popup.partition);
    assert_eq!(during_popup.splitters, 0);
    assert!(session.surface_motion_is_active(SURFACE));
    present(&mut session, output);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_deferred_surface_action(dismiss.unwrap())
        .unwrap();
    measure_menu(&mut frame);
    let plan = frame.paint_plan(menu_surface).unwrap().unwrap();
    assert!(
        plan.tab_strip_controls()
            .all(|control| !control.menu_open())
    );
    frame.commit().unwrap();
    let (after_popup, _) = paint(&mut session, motion(1060));
    assert!(after_popup.panes[&B].x() > during_popup.panes[&B].x());
    assert!(!after_popup.partition);
}

#[test]
fn unpresented_and_late_motion_outputs_never_steal_the_visible_receiver_authority() {
    let (mut session, _) = fixture();
    dock(&mut session, C, A);
    let (start, output) = paint(&mut session, motion(1000));
    present(&mut session, output);
    let visible = &start.receivers[&B];
    assert!(session.bind_presented_receiver(visible).is_some());
    let (middle, delayed) = paint(&mut session, motion(1050));
    assert!(session.bind_presented_receiver(visible).is_some());
    assert!(
        session
            .bind_presented_receiver(&middle.receivers[&B])
            .is_none()
    );
    let (end, newest) = paint(&mut session, motion(6000));
    assert!(session.bind_presented_receiver(visible).is_some());
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_none()
    );
    present(&mut session, newest);
    assert!(session.bind_presented_receiver(visible).is_none());
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_none(),
        "the terminal prefix is still blocked by the delayed output"
    );
    present(&mut session, delayed);
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_some()
    );
    assert!(
        session
            .bind_presented_receiver(&middle.receivers[&B])
            .is_none()
    );
}

#[test]
fn off_and_reduced_motion_settle_without_remeasurement_or_another_wakeup() {
    for reduced in [false, true] {
        let (mut session, _) = fixture();
        dock(&mut session, C, A);
        let (_, output) = paint(&mut session, motion(1000));
        present(&mut session, output);
        assert!(session.surface_motion_is_active(SURFACE));
        reset_surface_compilation_count();
        let mut instant = motion(1010);
        if reduced {
            instant.preference = MotionPreference::Reduced;
        } else {
            instant.settings.mode = MotionMode::Off;
        }
        let (sample, output) = paint(&mut session, instant);
        assert!(sample.partition);
        assert_eq!(sample.splitters, 1);
        assert!(!session.surface_motion_is_active(SURFACE));
        assert_eq!(surface_compilation_count(), 0);
        present(&mut session, output);
    }
}

#[test]
fn long_pause_retargets_the_running_model_instead_of_rewinding_to_presented() {
    let (mut reference, _) = fixture();
    dock(&mut reference, C, A);
    let (_, output) = paint(&mut reference, motion(1000));
    present(&mut reference, output);
    let (old_target, _) = paint(&mut reference, motion(6000));

    let (mut session, _) = fixture();
    dock(&mut session, C, A);
    let (_, output) = paint(&mut session, motion(1000));
    present(&mut session, output);
    let (visible, delayed) = paint(&mut session, motion(1050));
    present(&mut session, delayed);
    assert_ne!(visible.panes[&C], old_target.panes[&C]);
    dock(&mut session, C, B);
    let (redirected, _) = paint(&mut session, motion(6000));
    assert_eq!(redirected.panes[&C], old_target.panes[&C]);
}

#[test]
fn local_tab_scroll_invalidates_cached_motion_target_with_unchanged_measurements() {
    let content = DockspaceNode::tabs((1..=20).map(ItemId::new));
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let (_, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut action = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            assert_eq!(bar.scroll_offset(), 0.0);
            assert!(bar.maximum_scroll_offset() > 100.0);
            let point = crate::geometry::LogicalPoint::new(
                bar.viewport().x() + 1.0,
                bar.viewport().y() + 1.0,
            )
            .unwrap();
            action = plan
                .prepare_tab_strip_scroll_at(bar, point, 100.0)
                .map(|action| action.into_deferred());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_deferred_surface_action(action.unwrap())
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let (scrolled, _) = paint(&mut session, motion(10));
    assert_eq!(scrolled.tab_offsets, [100.0]);
}

#[test]
fn sixteen_and_sixty_four_pane_animation_ticks_reuse_target_topology() {
    fn tree(first: u64, count: u64, axis: DockspaceAxis) -> DockspaceNode {
        if count == 1 {
            return DockspaceNode::tabs([ItemId::new(first)]);
        }
        let next = match axis {
            DockspaceAxis::Horizontal => DockspaceAxis::Vertical,
            DockspaceAxis::Vertical => DockspaceAxis::Horizontal,
        };
        DockspaceNode::equal_split(
            axis,
            [
                tree(first, count / 2, next),
                tree(first + count / 2, count / 2, next),
            ],
        )
        .unwrap()
    }
    let paint_large = |session: &mut DockspaceSession, now| {
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 3840.0, 2160.0).unwrap(),
            LogicalSize::new(40.0, 30.0).unwrap(),
            80.0,
        )
        .unwrap();
        paint_with_metrics(session, motion(now), metrics)
    };
    for count in [16, 64] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, tree(1, count, DockspaceAxis::Horizontal)),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let (_, output) = paint_large(&mut session, 0);
        present(&mut session, output);
        dock(&mut session, ItemId::new(count), A);
        let (_, output) = paint_large(&mut session, 1000);
        present(&mut session, output);
        reset_surface_compilation_count();
        crate::interaction::targeting::resolver::structural_work::reset();
        for now in [1010, 1020, 1050] {
            let (_, output) = paint_large(&mut session, now);
            present(&mut session, output);
        }
        assert!(session.surface_motion_is_active(SURFACE));
        assert_eq!(surface_compilation_count(), 0);
        let work = crate::interaction::targeting::resolver::structural_work::snapshot();
        assert_eq!(work.root_fingerprint_builds, 0);
        assert_eq!(work.root_fingerprint_node_visits, 0);
    }
}

#[test]
fn changing_surface_bounds_discards_trajectories_in_the_previous_coordinate_space() {
    for bounds in [
        LogicalRect::new(2000.0, 100.0, 960.0, 600.0).unwrap(),
        LogicalRect::new(0.0, 0.0, 400.0, 300.0).unwrap(),
    ] {
        let (mut session, _) = fixture();
        dock(&mut session, C, A);
        let (_, output) = paint(&mut session, motion(1000));
        present(&mut session, output);
        assert!(session.surface_motion_is_active(SURFACE));
        let metrics =
            UniformSurfaceMetrics::new(bounds, LogicalSize::new(40.0, 30.0).unwrap(), 80.0)
                .unwrap();
        let (sample, output) = paint_with_metrics(&mut session, motion(1010), metrics);
        assert!(sample.partition);
        assert!(!session.surface_motion_is_active(SURFACE));
        assert!(
            sample
                .panes
                .values()
                .all(|rect| rect.x() >= bounds.x() && rect.width() > 0.0)
        );
        present(&mut session, output);
    }
}

fn semantic_resize_action(
    session: &mut DockspaceSession,
    junction: bool,
) -> crate::runtime::PreparedSurfaceAction {
    use crate::runtime::SurfaceSplitterAdjustment;

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut action = None;
    plan.visit_surface_program(|step| {
        if action.is_none()
            && let Step::Operation { operation, .. } = step
        {
            action = match operation {
                Operation::Splitter(record) if !junction => plan.prepare_splitter_adjustment(
                    record.visual_id(),
                    SurfaceSplitterAdjustment::Increment,
                ),
                Operation::SplitterJunction(record) if junction => plan
                    .prepare_splitter_junction_adjustment(
                        record.visual_id(),
                        DockspaceAxis::Vertical,
                        SurfaceSplitterAdjustment::Increment,
                    ),
                _ => None,
            };
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    action.expect("the static partition must retain an operable resize control")
}

#[test]
fn semantic_splitter_and_junction_resize_stay_immediate_after_dropped_outputs() {
    for junction in [false, true] {
        for deferred in [false, true] {
            let column = |first, second| {
                DockspaceNode::equal_split(
                    DockspaceAxis::Vertical,
                    [first, second].map(|item| DockspaceNode::tabs([item])),
                )
                .unwrap()
            };
            let content = DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [column(A, B), column(C, ItemId::new(4))],
            )
            .unwrap();
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, content),
            )])
            .unwrap();
            let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
            let (before, output) = paint(&mut session, motion(0));
            present(&mut session, output);
            assert_eq!(before.splitters, 3);

            let action = semantic_resize_action(&mut session, junction);
            let mut frame = session.begin_host_frame().unwrap();
            if deferred {
                frame
                    .submit_deferred_surface_action(action.into_deferred())
                    .unwrap();
            } else {
                frame.submit_surface_action(action).unwrap();
            }
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            assert!(frame.commit().unwrap().workspace_changed());

            let (resized, dropped) = paint(&mut session, motion(1000));
            assert_ne!(resized.panes, before.panes);
            assert!(resized.partition);
            assert_eq!(resized.splitters, 3);
            assert!(!session.surface_motion_is_active(SURFACE));
            session
                .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
                .unwrap();
            settle(&mut session);

            let (retried, output) = paint(&mut session, motion(1010));
            assert_eq!(retried.panes, resized.panes);
            assert!(retried.partition);
            assert_eq!(retried.splitters, 3);
            assert!(!session.surface_motion_is_active(SURFACE));
            present(&mut session, output);
            drop(semantic_resize_action(&mut session, junction));

            dock(&mut session, C, A);
            let (_, output) = paint(&mut session, motion(1020));
            assert!(session.surface_motion_is_active(SURFACE));
            present(&mut session, output);
        }
    }
}

#[test]
fn aborted_semantic_resize_does_not_disable_later_docking_motion() {
    let (mut session, before) = fixture();
    let version = session.version();
    let action = semantic_resize_action(&mut session, false);
    {
        let mut aborted = session.begin_host_frame().unwrap();
        aborted.submit_surface_action(action).unwrap();
    }
    assert_eq!(session.version(), version);
    let (unchanged, output) = paint(&mut session, motion(10));
    assert_eq!(unchanged.panes, before.panes);
    present(&mut session, output);

    dock(&mut session, C, A);
    let (moving, output) = paint(&mut session, motion(1000));
    assert!(session.surface_motion_is_active(SURFACE));
    assert!(!moving.partition);
    assert_eq!(moving.splitters, 0);
    present(&mut session, output);
}
