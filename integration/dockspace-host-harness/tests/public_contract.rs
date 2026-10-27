use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceSession,
    DockspaceVisualKind, HostFrameReport, HostInputOutcome, PaintedSurfaceOutput,
    SurfaceMeasurementAnswer, SurfaceMeasurementRequest, SurfacePaintDisposition,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult as ResultKind, SurfaceUnavailableReason, TabListMenuMetrics,
    TabStripControlKind, TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement,
    TabStripMetrics, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn bounds(width: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, 600.0).unwrap()
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([THIRD, FOURTH])),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, 800.0);
    frame.commit().unwrap();
    session
}

fn measure(frame: &mut DockspaceHostFrame<'_>, width: f64) {
    let metrics =
        UniformSurfaceMetrics::new(bounds(width), LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
            .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
}

fn measure_overflow(frame: &mut DockspaceHostFrame<'_>) {
    for surface in frame.surfaces() {
        frame
            .measure_surface_with(surface, |request| match request {
                SurfaceMeasurementRequest::DockBounds { .. }
                | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(bounds(160.0))
                }
                SurfaceMeasurementRequest::PaneMinimum { .. } => {
                    SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                }
                SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                    SurfaceMeasurementAnswer::TabIntrinsic(96.0)
                }
                SurfaceMeasurementRequest::TabStrip { .. } => {
                    let controls = TabStripControlMetrics::new(4.0)
                        .unwrap()
                        .with_scroll_backward(
                            TabStripControlMetric::new(
                                18.0,
                                TabStripControlPlacement::OverlayLeading,
                            )
                            .unwrap(),
                        )
                        .with_scroll_forward(
                            TabStripControlMetric::new(
                                18.0,
                                TabStripControlPlacement::OverlayTrailing,
                            )
                            .unwrap(),
                        )
                        .with_tab_list_menu(
                            TabStripControlMetric::new(
                                20.0,
                                TabStripControlPlacement::ReservedTrailing,
                            )
                            .unwrap(),
                        );
                    SurfaceMeasurementAnswer::TabStrip(
                        TabStripMetrics::new(0.0, 0.0)
                            .unwrap()
                            .with_controls(controls)
                            .with_tab_list_menu(
                                TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).unwrap(),
                            ),
                    )
                }
            })
            .unwrap();
    }
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn paint(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> (RecordingRenderer, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(surface).unwrap().unwrap();
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert_eq!(output.surface(), surface);
    assert!(
        matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
        if output.matches_semantic_output(*semantic))
    );
    (recording, output)
}

fn tab_receiver(recording: &RecordingRenderer) -> DockspaceReceiverDescriptor {
    recording
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::Receiver(receiver)
                if receiver.role() == DockspaceReceiverRole::TabBody =>
            {
                Some(*receiver)
            }
            _ => None,
        })
        .unwrap()
}

#[test]
fn owned_program_survives_frame_and_only_presented_commit_grants_receivers() {
    for result in [ResultKind::Presented, ResultKind::Dropped] {
        let mut session = session();
        let height = session.presentation_config().tab_bar_height();
        let (recording, output) = paint(&mut session, A);
        // All observations below run after the paint frame and its borrows ended.
        assert_eq!(
            recording.records.first(),
            Some(&OwnedRecord::RootBegin(RootId::new(1)))
        );
        for required in [
            OwnedRecord::RootEnd(RootId::new(1)),
            OwnedRecord::InteractionFeedback,
        ] {
            assert!(recording.records.contains(&required));
        }
        let content_bounds = LogicalRect::new(0.0, height, 800.0, 600.0 - height).unwrap();
        assert!(recording.records.iter().any(|record| matches!(record,
            OwnedRecord::PaneContent {
                visual, item: Some(FIRST), bounds, paint: SurfacePaintDisposition::Visible,
            } if visual.kind() == DockspaceVisualKind::Pane && *bounds == content_bounds
        )));
        let tabs: Vec<_> = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::Geometry { visual, .. }
                    if visual.kind() == DockspaceVisualKind::Tab =>
                {
                    Some(*visual)
                }
                _ => None,
            })
            .collect();
        assert_eq!(tabs.len(), 2);
        assert_ne!(tabs[0], tabs[1]);
        let descriptor = tab_receiver(&recording);
        let artifact = output.presentation_artifact();
        assert!(session.bind_presented_receiver(&descriptor).is_none());
        session.report_surface_presentation(output, result).unwrap();
        assert!(session.bind_presented_receiver(&descriptor).is_none());
        // Abandoning a candidate observation must not publish its authority.
        drop(session.begin_host_frame().unwrap());
        assert!(session.bind_presented_receiver(&descriptor).is_none());
        let committed = settle(&mut session);
        let [ack] = committed.surface_presentation_commits() else {
            panic!("one exact stream commits")
        };
        let presented = result == ResultKind::Presented;
        assert_eq!(ack.presents(artifact), presented);
        assert_eq!(ack.retires(artifact), !presented);
        assert_eq!(session.presented_surface(A).is_some(), presented);
        assert_eq!(
            session.bind_presented_receiver(&descriptor).is_some(),
            presented
        );
        assert!(session.presented_surface(B).is_none());
    }
}

#[test]
fn delayed_a_cannot_replace_newer_b_when_results_arrive_in_reverse_order() {
    for older_result in [ResultKind::Presented, ResultKind::Dropped] {
        let mut session = session();
        let (older_records, older) = paint(&mut session, A);
        let older_artifact = older.presentation_artifact();
        let older_receiver = tab_receiver(&older_records);
        let mut resized = session.begin_host_frame().unwrap();
        measure(&mut resized, 1000.0);
        resized.commit().unwrap();
        let (newer_records, newer) = paint(&mut session, A);
        let newer_artifact = newer.presentation_artifact();
        assert_ne!(older_artifact, newer_artifact);
        assert_ne!(older_records.records.last(), newer_records.records.last());
        let newer_receiver = tab_receiver(&newer_records);
        session
            .report_surface_presentation(newer, ResultKind::Presented)
            .unwrap();
        let pending = settle(&mut session);
        assert!(pending.surface_presentation_commits().is_empty());
        assert!(session.bind_presented_receiver(&newer_receiver).is_none());
        session
            .report_surface_presentation(older, older_result)
            .unwrap();
        let committed = settle(&mut session);
        let [ack] = committed.surface_presentation_commits() else {
            panic!("the complete prefix commits")
        };
        assert!(ack.presents(newer_artifact));
        assert!(ack.retires(older_artifact));
        assert!(!ack.presents(older_artifact));
        assert!(session.bind_presented_receiver(&older_receiver).is_none());
        assert!(session.bind_presented_receiver(&newer_receiver).is_some());
        let unchanged = settle(&mut session);
        assert!(unchanged.surface_presentation_commits().is_empty());
        assert!(session.bind_presented_receiver(&newer_receiver).is_some());
    }
}

#[test]
fn pending_surfaces_present_independently_and_keep_equal_coordinate_receivers_isolated() {
    for a_result in [ResultKind::Presented, ResultKind::Dropped] {
        let mut session = session();
        let mut frame = session.begin_host_frame().unwrap();
        let (a_records, a_receipt) =
            RecordingRenderer::execute(frame.paint_plan(A).unwrap().unwrap()).unwrap();
        frame.confirm_surface_painted(a_receipt).unwrap();
        let (b_records, b_receipt) =
            RecordingRenderer::execute(frame.paint_plan(B).unwrap().unwrap()).unwrap();
        frame.confirm_surface_painted(b_receipt).unwrap();
        let mut outputs = frame.commit().unwrap().take_painted_outputs();
        assert_eq!(outputs.len(), 2);
        let a_index = outputs
            .iter()
            .position(|output| output.surface() == A)
            .unwrap();
        let a_output = outputs.remove(a_index);
        let b_output = outputs.pop().unwrap();
        assert_eq!(b_output.surface(), B);
        let a_artifact = a_output.presentation_artifact();
        let b_artifact = b_output.presentation_artifact();
        let a_receiver = tab_receiver(&a_records);
        let b_receiver = tab_receiver(&b_records);
        assert_eq!(a_receiver.bounds(), b_receiver.bounds());
        assert_eq!(a_receiver.center(), b_receiver.center());
        assert_ne!(a_receiver, b_receiver);
        assert!(session.bind_presented_receiver(&a_receiver).is_none());
        assert!(session.bind_presented_receiver(&b_receiver).is_none());

        session
            .report_surface_presentation(b_output, ResultKind::Presented)
            .unwrap();
        assert!(session.bind_presented_receiver(&b_receiver).is_none());
        let b_committed = settle(&mut session);
        let [b_ack] = b_committed.surface_presentation_commits() else {
            panic!("B must settle without waiting for A's pending result")
        };
        assert!(b_ack.presents(b_artifact));
        assert!(!b_ack.retires(b_artifact));
        assert!(!b_ack.settles(a_artifact));
        assert!(session.presented_surface(A).is_none());
        assert!(session.bind_presented_receiver(&a_receiver).is_none());
        let b_authority = session.presented_surface(B).unwrap();
        assert_eq!(b_authority.surface(), B);
        let b_bound_receiver = session.bind_presented_receiver(&b_receiver).unwrap();

        session
            .report_surface_presentation(a_output, a_result)
            .unwrap();
        assert!(session.presented_surface(A).is_none());
        assert_eq!(session.presented_surface(B), Some(b_authority));
        let a_committed = settle(&mut session);
        let [a_ack] = a_committed.surface_presentation_commits() else {
            panic!("A must settle only its own stream")
        };
        let a_presented = a_result == ResultKind::Presented;
        assert_eq!(a_ack.presents(a_artifact), a_presented);
        assert_eq!(a_ack.retires(a_artifact), !a_presented);
        assert!(!a_ack.settles(b_artifact));
        assert_eq!(session.presented_surface(A).is_some(), a_presented);
        assert_eq!(
            session.bind_presented_receiver(&a_receiver).is_some(),
            a_presented
        );
        assert_eq!(session.presented_surface(B), Some(b_authority));
        assert_eq!(
            session.bind_presented_receiver(&b_receiver),
            Some(b_bound_receiver)
        );
        let unchanged = settle(&mut session);
        assert!(unchanged.surface_presentation_commits().is_empty());
        assert_eq!(session.presented_surface(B), Some(b_authority));
        assert_eq!(
            session.bind_presented_receiver(&b_receiver),
            Some(b_bound_receiver)
        );
    }
}

#[test]
fn popup_barrier_ack_preserves_first_surface_artifact_until_sibling_presents() {
    let mut session = session();
    let mut narrow = session.begin_host_frame().unwrap();
    measure_overflow(&mut narrow);
    narrow.commit().unwrap();

    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(A).unwrap().unwrap();
    let mut menu_control = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabStripControl(control),
            ..
        } = step
            && control.kind() == TabStripControlKind::TabListMenu
        {
            assert!(control.enabled());
            assert!(!control.menu_open());
            assert!(menu_control.replace(control).is_none());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let open = plan
        .prepare_tab_strip_control_activation(
            menu_control.expect("the narrow strip must expose its tab-list menu control"),
        )
        .unwrap();
    inspect
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    inspect.commit().unwrap();

    let mut open_frame = session.begin_host_frame().unwrap();
    open_frame.submit_surface_action(open).unwrap();
    open_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    open_frame.commit().unwrap();
    let mut publish_popup = session.begin_host_frame().unwrap();
    measure_overflow(&mut publish_popup);
    publish_popup.commit().unwrap();

    let mut painted = session.begin_host_frame().unwrap();
    let mut recordings = Vec::new();
    for surface in [A, B] {
        let plan = painted.paint_plan(surface).unwrap().unwrap();
        let mut backdrop_receiver = None;
        let mut menu_seen = false;
        plan.visit_surface_program(|step| {
            match step {
                Step::PopupBackdrop(backdrop) => {
                    let receiver = plan.receiver_for_tab_list_menu_backdrop(backdrop).unwrap();
                    assert_eq!(receiver.role(), DockspaceReceiverRole::TabListMenuBackdrop);
                    assert!(backdrop_receiver.replace(receiver).is_none());
                }
                Step::PopupMenu { .. } => menu_seen = true,
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        assert_eq!(menu_seen, surface == A);
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        painted.confirm_surface_painted(receipt).unwrap();
        recordings.push((recording, backdrop_receiver.unwrap()));
    }
    let mut outputs = painted.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    let a_index = outputs
        .iter()
        .position(|output| output.surface() == A)
        .unwrap();
    let a_output = outputs.remove(a_index);
    let b_output = outputs.pop().unwrap();
    assert_eq!(b_output.surface(), B);
    let a_artifact = a_output.presentation_artifact();
    let b_artifact = b_output.presentation_artifact();
    let [(a_records, a_receiver), (b_records, b_receiver)] = recordings.as_slice() else {
        panic!("both surfaces must retain their owned popup recordings")
    };
    assert!(
        matches!(a_records.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
        if a_output.matches_semantic_output(*semantic))
    );
    assert!(
        matches!(b_records.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
        if b_output.matches_semantic_output(*semantic))
    );
    assert!(session.presented_surface(A).is_none());
    assert!(session.presented_surface(B).is_none());

    session
        .report_surface_presentation(a_output, ResultKind::Presented)
        .unwrap();
    let mut a_committed = settle(&mut session);
    assert!(a_committed.take_painted_outputs().is_empty());
    let [a_ack] = a_committed.surface_presentation_commits() else {
        panic!("the first popup proof must receive its exact acknowledgement")
    };
    assert!(a_ack.presents(a_artifact));
    assert!(!a_ack.retires(a_artifact));
    assert!(!a_ack.settles(b_artifact));
    assert!(session.presented_surface(A).is_none());
    assert!(session.presented_surface(B).is_none());
    assert!(session.bind_presented_receiver(a_receiver).is_none());
    assert!(session.bind_presented_receiver(b_receiver).is_none());

    // A's acknowledged recording stays usable while the popup barrier hides
    // its accepted proof. Only B emits the remaining final-presentation report.
    session
        .report_surface_presentation(b_output, ResultKind::Presented)
        .unwrap();
    let mut b_committed = settle(&mut session);
    assert!(b_committed.take_painted_outputs().is_empty());
    let [b_ack] = b_committed.surface_presentation_commits() else {
        panic!("the sibling alone must complete the popup presentation barrier")
    };
    assert!(b_ack.presents(b_artifact));
    assert!(!b_ack.retires(b_artifact));
    assert!(!b_ack.settles(a_artifact));
    assert_eq!(session.presented_surface(A).unwrap().surface(), A);
    assert_eq!(session.presented_surface(B).unwrap().surface(), B);
    let a_bound_receiver = session.bind_presented_receiver(a_receiver).unwrap();
    let b_bound_receiver = session.bind_presented_receiver(b_receiver).unwrap();
    let mut unchanged = settle(&mut session);
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert!(unchanged.take_painted_outputs().is_empty());
    assert_eq!(
        session.bind_presented_receiver(a_receiver),
        Some(a_bound_receiver)
    );
    assert_eq!(
        session.bind_presented_receiver(b_receiver),
        Some(b_bound_receiver)
    );
}

#[test]
fn late_presented_output_of_redocked_surface_retires_without_reviving_authority() {
    let mut session = session();
    let (removed_records, removed_output) = paint(&mut session, A);
    let removed_artifact = removed_output.presentation_artifact();
    let removed_receiver = tab_receiver(&removed_records);

    let action = session.prepare_dock_root(
        RootId::new(1),
        DockPlacement::Center(DockAnchor::Item(THIRD)),
    );
    let mut redock = session.begin_host_frame().unwrap();
    let submitted = redock.submit_prepared_action(action).unwrap();
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let redocked = redock.commit().unwrap();
    assert!(matches!(redocked.submitted_action_outcome(submitted), Some(
        HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::RootDocked {
            root, target_root, items, changed: true,
        })
    ) if *root == RootId::new(1)
        && *target_root == RootId::new(2)
        && items.as_slice() == [FIRST, SECOND]));
    assert!(redocked.surface_presentation_commits().is_empty());
    assert!(session.view().surface(A).is_none());
    assert!(session.view().root(RootId::new(1)).is_none());
    for item in [FIRST, SECOND, THIRD, FOURTH] {
        assert_eq!(session.view().item(item).unwrap().surface(), B);
    }
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&removed_receiver).is_none());

    let mut measured = session.begin_host_frame().unwrap();
    measure(&mut measured, 800.0);
    measured.commit().unwrap();
    let (surviving_records, surviving_output) = paint(&mut session, B);
    let surviving_artifact = surviving_output.presentation_artifact();
    let surviving_receiver = tab_receiver(&surviving_records);
    session
        .report_surface_presentation(surviving_output, ResultKind::Presented)
        .unwrap();
    let surviving_committed = settle(&mut session);
    let [surviving_ack] = surviving_committed.surface_presentation_commits() else {
        panic!("the surviving surface must present while the removed output is pending")
    };
    assert!(surviving_ack.presents(surviving_artifact));
    assert!(!surviving_ack.settles(removed_artifact));
    let surviving_authority = session.presented_surface(B).unwrap();
    let surviving_bound_receiver = session
        .bind_presented_receiver(&surviving_receiver)
        .unwrap();

    session
        .report_surface_presentation(removed_output, ResultKind::Presented)
        .unwrap();
    assert!(session.presented_surface(A).is_none());
    let retired = settle(&mut session);
    let [retired_ack] = retired.surface_presentation_commits() else {
        panic!("the late removed-surface output must receive a terminal acknowledgement")
    };
    assert!(retired_ack.settles(removed_artifact));
    assert!(retired_ack.retires(removed_artifact));
    assert!(!retired_ack.presents(removed_artifact));
    assert!(!retired_ack.settles(surviving_artifact));
    assert!(session.view().surface(A).is_none());
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&removed_receiver).is_none());
    assert_eq!(session.presented_surface(B), Some(surviving_authority));
    assert_eq!(
        session.bind_presented_receiver(&surviving_receiver),
        Some(surviving_bound_receiver)
    );
    let unchanged = settle(&mut session);
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert_eq!(session.presented_surface(B), Some(surviving_authority));
}

#[test]
fn late_presented_output_cannot_authorize_a_scene_invalidated_by_selection() {
    let mut session = session();
    let (older_records, older_output) = paint(&mut session, A);
    let older_artifact = older_output.presentation_artifact();
    let older_receiver = tab_receiver(&older_records);

    // Selecting another pane changes its measurement requirement. The old,
    // never-presented scene cannot survive that change as a confirmed fallback.
    let action = session.prepare_select_item(SECOND);
    let mut selection = session.begin_host_frame().unwrap();
    let submitted = selection.submit_prepared_action(action).unwrap();
    measure(&mut selection, 800.0);
    let selected = selection.commit().unwrap();
    assert!(matches!(
        selected.submitted_action_outcome(submitted),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }
        ))
    ));
    assert!(session.view().surface(A).is_some());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(session.presented_surface(A).is_none());

    session
        .report_surface_presentation(older_output, ResultKind::Presented)
        .unwrap();
    let retired = settle(&mut session);
    let [retired_ack] = retired.surface_presentation_commits() else {
        panic!("the invalidated scene's late output must settle without promotion")
    };
    assert!(retired_ack.retires(older_artifact));
    assert!(!retired_ack.presents(older_artifact));
    assert!(session.view().surface(A).is_some());
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&older_receiver).is_none());

    let (current_records, current_output) = paint(&mut session, A);
    assert!(current_records.records.iter().any(|record| matches!(
        record,
        OwnedRecord::PaneContent {
            item: Some(SECOND),
            ..
        }
    )));
    let current_artifact = current_output.presentation_artifact();
    let current_receiver = tab_receiver(&current_records);
    assert_ne!(current_receiver, older_receiver);
    session
        .report_surface_presentation(current_output, ResultKind::Presented)
        .unwrap();
    let current_committed = settle(&mut session);
    let [current_ack] = current_committed.surface_presentation_commits() else {
        panic!("the replacement scene must grant authority after its own presentation")
    };
    assert!(current_ack.presents(current_artifact));
    assert!(!current_ack.presents(older_artifact));
    assert!(session.bind_presented_receiver(&current_receiver).is_some());
    assert!(session.bind_presented_receiver(&older_receiver).is_none());
}

#[test]
fn dropping_older_output_unblocks_newer_presented_prefix_without_granting_old_authority() {
    let mut session = session();
    let (older_records, older) = paint(&mut session, A);
    let older_artifact = older.presentation_artifact();
    let older_receiver = tab_receiver(&older_records);
    let mut resized = session.begin_host_frame().unwrap();
    measure(&mut resized, 1000.0);
    resized.commit().unwrap();
    let (newer_records, newer) = paint(&mut session, A);
    let newer_artifact = newer.presentation_artifact();
    let newer_receiver = tab_receiver(&newer_records);
    assert_ne!(older_artifact, newer_artifact);
    assert_ne!(older_records.records.last(), newer_records.records.last());
    session
        .report_surface_presentation(newer, ResultKind::Presented)
        .unwrap();
    let pending = settle(&mut session);
    assert!(pending.surface_presentation_commits().is_empty());
    assert!(session.bind_presented_receiver(&older_receiver).is_none());
    assert!(session.bind_presented_receiver(&newer_receiver).is_none());

    drop(older);
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&older_receiver).is_none());
    assert!(session.bind_presented_receiver(&newer_receiver).is_none());
    let committed = settle(&mut session);
    let [ack] = committed.surface_presentation_commits() else {
        panic!("dropping the missing old output must unblock the complete prefix")
    };
    assert!(ack.retires(older_artifact));
    assert!(!ack.presents(older_artifact));
    assert!(ack.presents(newer_artifact));
    assert!(!ack.retires(newer_artifact));
    assert!(session.bind_presented_receiver(&older_receiver).is_none());
    let newer_bound_receiver = session.bind_presented_receiver(&newer_receiver).unwrap();
    let unchanged = settle(&mut session);
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert_eq!(
        session.bind_presented_receiver(&newer_receiver),
        Some(newer_bound_receiver)
    );
}

#[test]
fn surface_bound_tab_actions_select_independently_at_identical_coordinates() {
    let mut session = session();
    let mut previous_bounds = None;
    for (surface, target, foreign, expected) in [
        (A, SECOND, FOURTH, [SECOND, THIRD]),
        (B, FOURTH, SECOND, [SECOND, FOURTH]),
    ] {
        let mut inspect = session.begin_host_frame().unwrap();
        let plan = inspect.paint_plan(surface).unwrap().unwrap();
        let target_bounds = plan.tab_receiver(target).unwrap().bounds();
        if let Some(previous) = previous_bounds.replace(target_bounds) {
            assert_eq!(previous, target_bounds);
        }
        assert!(plan.prepare_tab_select(foreign).is_none());
        let action = plan.prepare_tab_select(target).unwrap();
        assert_eq!(action.surface(), surface);
        inspect
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        inspect.commit().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_surface_action(action).unwrap();
        measure(&mut frame, 800.0);
        let report = frame.commit().unwrap();
        assert!(report.workspace_changed());
        for item in expected {
            assert!(session.view().item(item).unwrap().is_selected());
        }
        assert!(session.presented_surface(A).is_none());
        assert!(session.presented_surface(B).is_none());
    }
}

#[test]
fn semantic_layout_command_moves_an_item_without_pointer_registration() {
    let mut session = session();
    let action = session.prepare_dock_item(SECOND, DockPlacement::After(FOURTH));
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.submit_prepared_action(action).unwrap();
    measure(&mut frame, 800.0);
    let report = frame.commit().unwrap();
    assert!(matches!(report.submitted_action_outcome(submitted), Some(
        HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Docked {
            item: SECOND, source_root, target_root, changed: true,
        })
    ) if *source_root == RootId::new(1) && *target_root == RootId::new(2)));
    let moved = session.view().item(SECOND).unwrap();
    assert_eq!(moved.surface(), B);
    assert_eq!(moved.tabs().items(), &[THIRD, FOURTH, SECOND]);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert!(session.presented_surface(A).is_none());
    assert!(session.presented_surface(B).is_none());
}

#[test]
fn failed_core_commit_does_not_publish_a_pending_presentation() {
    let mut session = session();
    let (recording, output) = paint(&mut session, A);
    let artifact = output.presentation_artifact();
    let receiver = tab_receiver(&recording);
    session
        .report_surface_presentation(output, ResultKind::Presented)
        .unwrap();

    let mut invalid = session.begin_host_frame().unwrap();
    measure(&mut invalid, 800.0);
    // Semantic input after measurement violates frame ordering and poisons this
    // candidate, including its pending presentation observation.
    assert!(
        invalid
            .dock_item_current(SECOND, DockPlacement::After(FOURTH))
            .is_err()
    );
    assert!(invalid.commit().is_err());
    assert!(session.presented_surface(A).is_none());
    assert!(session.bind_presented_receiver(&receiver).is_none());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);

    let committed = settle(&mut session);
    let [ack] = committed.surface_presentation_commits() else {
        panic!("the exact observation remains retryable after a failed commit")
    };
    assert!(ack.presents(artifact));
    assert!(session.bind_presented_receiver(&receiver).is_some());
}
