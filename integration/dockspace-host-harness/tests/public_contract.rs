use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePreviewVisual, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceSession, DockspaceVisualId, DockspaceVisualKind, HostFrameReport, HostInputOutcome,
    HostPointerInput, HostPointerRoute, PaintedSurfaceOutput, PresentedDockReceiver,
    PresentedDockspaceSurface, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
    SurfacePaintDisposition, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
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

struct HostPointerScene {
    a: PresentedDockspaceSurface,
    b: PresentedDockspaceSurface,
    source: PresentedDockReceiver,
    target: PresentedDockReceiver,
    source_visual: DockspaceVisualId,
    moved: LogicalPoint,
}

impl HostPointerScene {
    fn press(&self) -> HostPointerInput<'_> {
        let route = HostPointerRoute::new(self.a, self.source.center());
        HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&self.source),
        )
    }

    fn target_edge(&self, event: SurfacePointerEvent) -> HostPointerInput<'_> {
        HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::new(self.a, self.moved),
            HostPointerRoute::new(self.b, self.target.center()),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&self.source).with_hover(&self.target),
        )
    }

    fn batch(&self) -> [HostPointerInput<'_>; 3] {
        [
            self.press(),
            self.target_edge(SurfacePointerEvent::Moved),
            self.target_edge(SurfacePointerEvent::ButtonReleased(
                SurfacePointerButton::Primary,
            )),
        ]
    }
}

fn record_host_pointer_surfaces(frame: &mut DockspaceHostFrame<'_>) -> [RecordingRenderer; 2] {
    [A, B].map(|surface| {
        let (recording, receipt) =
            RecordingRenderer::execute(frame.paint_plan(surface).unwrap().unwrap()).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        recording
    })
}

fn host_pointer_fixture() -> (DockspaceSession, HostPointerScene) {
    host_pointer_fixture_for_item(FIRST)
}

fn host_pointer_fixture_for_item(source_item: ItemId) -> (DockspaceSession, HostPointerScene) {
    let mut session = session();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let source_descriptor = frame
        .paint_plan(A)
        .unwrap()
        .unwrap()
        .tab_receiver(source_item)
        .expect("A's actual paint plan exposes the requested source tab receiver");
    let target_descriptor = frame
        .paint_plan(B)
        .unwrap()
        .unwrap()
        .center_drop_receiver_for_item(THIRD)
        .expect("B's actual paint plan exposes its center HoverDrop receiver");
    assert_eq!(target_descriptor.role(), DockspaceReceiverRole::DropTarget);
    let [a_records, b_records] = record_host_pointer_surfaces(&mut frame);
    let outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
    }
    assert_eq!(settle(&mut session).surface_presentation_commits().len(), 2);
    assert!(
        a_records
            .records
            .contains(&OwnedRecord::Receiver(source_descriptor))
    );
    let sibling_descriptor = b_records
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::Receiver(receiver)
                if receiver.role() == DockspaceReceiverRole::TabBody
                    && receiver.bounds() == source_descriptor.bounds() =>
            {
                Some(*receiver)
            }
            _ => None,
        })
        .expect("B's actual recording contains a tab receiver at the same coordinates");
    assert_eq!(source_descriptor.bounds(), sibling_descriptor.bounds());
    assert_eq!(source_descriptor.center(), sibling_descriptor.center());
    assert_ne!(source_descriptor, sibling_descriptor);
    let source_visual = a_records
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::Geometry { visual, bounds, .. }
                if visual.kind() == DockspaceVisualKind::Tab
                    && bounds.contains(source_descriptor.center()) =>
            {
                Some(*visual)
            }
            _ => None,
        })
        .unwrap();
    let scene = HostPointerScene {
        a: session.presented_surface(A).unwrap(),
        b: session.presented_surface(B).unwrap(),
        source: session.bind_presented_receiver(&source_descriptor).unwrap(),
        target: session.bind_presented_receiver(&target_descriptor).unwrap(),
        source_visual,
        moved: LogicalPoint::new(
            source_descriptor.bounds().x() + source_descriptor.bounds().width() * 0.9,
            source_descriptor.center().y(),
        )
        .unwrap(),
    };
    assert!(scene.source.bounds().contains(scene.moved));
    assert!(scene.moved.x() - scene.source.center().x() >= 12.0);
    assert_ne!(scene.moved, scene.target.center());
    (session, scene)
}

fn assert_target_preview(recording: &RecordingRenderer) {
    assert!(
        recording.records.iter().any(|record| matches!(
            record,
            OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface: B, .. })
        )),
        "the B hover route must own the actual rendered preview"
    );
}

fn assert_no_pointer_preview(recordings: &[RecordingRenderer; 2]) {
    assert!(
        recordings
            .iter()
            .flat_map(|recording| &recording.records)
            .all(|record| !matches!(record, OwnedRecord::Preview(_)))
    );
}

fn settle_pointer_outputs(
    session: &mut DockspaceSession,
    report: &mut HostFrameReport,
    target_result: ResultKind,
) -> HostFrameReport {
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    let mut artifacts = Vec::new();
    for output in outputs {
        let result = if output.surface() == B {
            target_result
        } else {
            ResultKind::Presented
        };
        artifacts.push((output.presentation_artifact(), result));
        session.report_surface_presentation(output, result).unwrap();
    }
    let settled = settle(session);
    for (artifact, result) in artifacts {
        let commits: Vec<_> = settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .collect();
        assert_eq!(commits.len(), 1, "each exact output must settle once");
        if result == ResultKind::Dropped {
            assert!(commits[0].retires(artifact));
        }
    }
    settled
}

#[test]
fn host_pointer_same_batch_release_waits_for_the_actual_target_preview() {
    for target_result in [ResultKind::Presented, ResultKind::Dropped] {
        let (mut session, scene) = host_pointer_fixture();
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer_batch(scene.batch()).unwrap();
        let recordings = record_host_pointer_surfaces(&mut frame);
        assert_target_preview(&recordings[1]);
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
        assert!(
            !session.has_active_gesture(),
            "release must relinquish the active pointer owner"
        );
        assert!(session.has_pending_presentation());
        assert!(session.surface_requires_presentation(B));
        assert!(!session.surface_requires_presentation(A));

        let settled = settle_pointer_outputs(&mut session, &mut report, target_result);
        let presented = target_result == ResultKind::Presented;
        assert_eq!(
            session.view().item(FIRST).unwrap().surface(),
            if presented { B } else { A }
        );
        assert_eq!(settled.workspace_changed(), presented);
        assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
        assert_eq!(session.view().item(THIRD).unwrap().surface(), B);
        assert_eq!(session.view().item(FOURTH).unwrap().surface(), B);
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(B));
        assert!(session.disable_host_pointer().unwrap().is_some());
        assert!(session.disable_host_pointer().unwrap().is_none());
    }
}

#[test]
fn host_pointer_unselected_source_continues_same_batch_drag_after_selection() {
    let (mut session, scene) = host_pointer_fixture_for_item(SECOND);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer_batch(scene.batch()).unwrap();
    // Selection changes A's content requirement within this same input frame.
    measure(&mut frame, 800.0);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut input_report = frame.commit().unwrap();
    assert!(input_report.take_painted_outputs().is_empty());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_active_gesture());
    assert!(session.has_pending_presentation());
    assert!(session.surface_requires_presentation(B));
    assert!(!session.surface_requires_presentation(A));
    assert!(input_report.repaint_surfaces().contains(&B));

    // The committed measurements become Ready for a separate real paint pass.
    let mut paint_frame = session.begin_host_frame().unwrap();
    let recordings = record_host_pointer_surfaces(&mut paint_frame);
    assert_target_preview(&recordings[1]);
    let mut report = paint_frame.commit().unwrap();
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert!(!session.has_active_gesture());
    assert!(session.has_pending_presentation());
    assert!(session.surface_requires_presentation(B));

    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(settled.workspace_changed());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.view().item(THIRD).unwrap().surface(), B);
    assert_eq!(session.view().item(FOURTH).unwrap().surface(), B);
    assert_eq!(
        session
            .view()
            .item(SECOND)
            .unwrap()
            .tabs()
            .items()
            .iter()
            .filter(|item| **item == SECOND)
            .count(),
        1,
    );
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(B));
}

#[test]
fn host_pointer_wrong_manipulation_surface_and_hover_motion_do_not_cross_source_threshold() {
    let (mut session, scene) = host_pointer_fixture();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            scene.press(),
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::Moved,
                HostPointerRoute::new(scene.b, LogicalPoint::new(700.0, 500.0).unwrap()),
                HostPointerRoute::new(scene.b, scene.target.center()),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::hover(&scene.target),
            ),
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::Moved,
                HostPointerRoute::new(scene.a, scene.source.center()),
                HostPointerRoute::new(scene.b, scene.target.center()),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::hover(&scene.target),
            ),
        ])
        .unwrap();
    let recordings = record_host_pointer_surfaces(&mut frame);
    assert_no_pointer_preview(&recordings);
    assert!(
        recordings[0].records.iter().any(|record| matches!(record,
            OwnedRecord::Geometry { visual, paint: SurfacePaintDisposition::Visible, .. }
                if *visual == scene.source_visual
        )),
        "B coordinates must not turn A's source tab into a drag gap"
    );
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert!(session.disable_host_pointer().unwrap().is_some());
}

#[test]
fn host_pointer_unknown_hover_cannot_create_an_outside_all_drop() {
    let (mut session, scene) = host_pointer_fixture();
    let unknown_hover = |event| {
        HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::new(scene.a, LogicalPoint::new(1200.0, 900.0).unwrap()),
            HostPointerRoute::Unknown,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        )
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            scene.press(),
            unknown_hover(SurfacePointerEvent::Moved),
            unknown_hover(SurfacePointerEvent::ButtonReleased(
                SurfacePointerButton::Primary,
            )),
        ])
        .unwrap();
    let recordings = record_host_pointer_surfaces(&mut frame);
    assert_no_pointer_preview(&recordings);
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(!settled.workspace_changed());
    assert_eq!(
        session
            .view()
            .surfaces()
            .map(|surface| surface.id())
            .collect::<Vec<_>>(),
        [A, B]
    );
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert!(session.view().item(FIRST).unwrap().contained().is_none());
    assert!(!session.has_active_gesture());
}

#[test]
fn host_pointer_unknown_release_on_current_target_clears_preview_without_poisoning_later_actions() {
    let (mut session, mut scene) = host_pointer_fixture();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([scene.press(), scene.target_edge(SurfacePointerEvent::Moved)])
        .unwrap();
    let recordings = record_host_pointer_surfaces(&mut frame);
    assert_target_preview(&recordings[1]);
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert!(session.has_active_gesture());
    assert!(
        !settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented)
            .workspace_changed()
    );
    assert!(session.has_active_gesture());

    scene.a = session.presented_surface(A).unwrap();
    scene.b = session.presented_surface(B).unwrap();
    assert!(
        session
            .host_pointer_hit(B, scene.target.center())
            .unwrap()
            .hover()
            .is_some(),
        "only the release facts are unknown; the presented target route remains valid"
    );
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            HostPointerRoute::new(scene.a, scene.moved),
            HostPointerRoute::new(scene.b, scene.target.center()),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        )])
        .unwrap();
    let recordings = record_host_pointer_surfaces(&mut frame);
    assert_no_pointer_preview(&recordings);
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert!(!session.has_active_gesture());
    assert!(
        !settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented)
            .workspace_changed()
    );
    assert_eq!(session.version(), before);
    assert_eq!(
        session
            .view()
            .surfaces()
            .map(|surface| surface.id())
            .collect::<Vec<_>>(),
        [A, B]
    );
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.view().item(FIRST).unwrap().root(), RootId::new(1));
    assert!(session.view().item(FIRST).unwrap().contained().is_none());
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );

    let mut inspect = session.begin_host_frame().unwrap();
    let descriptor = inspect
        .paint_plan(A)
        .unwrap()
        .unwrap()
        .tab_receiver(SECOND)
        .unwrap();
    drop(inspect);
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let route = HostPointerRoute::new(session.presented_surface(A).unwrap(), receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(
            [
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            ]
            .map(|event| {
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    event,
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                )
            }),
        )
        .unwrap();
    measure(&mut frame, 800.0);
    assert!(frame.commit().unwrap().workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_active_gesture());

    let command = session.prepare_dock_item(SECOND, DockPlacement::After(FOURTH));
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.submit_prepared_action(command).unwrap();
    measure(&mut frame, 800.0);
    let report = frame.commit().unwrap();
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: SECOND,
                changed: true,
                ..
            }
        ))
    ));
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
}

#[test]
fn host_pointer_dropped_frame_retries_the_batch_and_duplicate_release_cannot_deliver_twice() {
    let (mut session, scene) = host_pointer_fixture();
    let mut abandoned = session.begin_host_frame().unwrap();
    abandoned.submit_host_pointer_batch(scene.batch()).unwrap();
    drop(abandoned);
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);

    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer_batch(scene.batch())
        .expect("dropping a frame must not consume its sequence or button edges");
    let recordings = record_host_pointer_surfaces(&mut retry);
    assert_target_preview(&recordings[1]);
    let mut report = retry.commit().unwrap();

    let mut repeated = session.begin_host_frame().unwrap();
    let duplicate = scene.target_edge(SurfacePointerEvent::ButtonReleased(
        SurfacePointerButton::Primary,
    ));
    if repeated.submit_host_pointer_batch([duplicate]).is_ok() {
        repeated
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let repeated = repeated.commit().unwrap();
        assert!(!repeated.workspace_changed());
        assert!(repeated.presentation_transitions().is_empty());
    } else {
        drop(repeated);
    }
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(settled.workspace_changed());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(
        session
            .view()
            .item(FIRST)
            .unwrap()
            .tabs()
            .items()
            .iter()
            .filter(|item| **item == FIRST)
            .count(),
        1
    );
    assert!(!session.has_active_gesture());
    let unchanged = settle(&mut session);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert!(unchanged.presentation_transitions().is_empty());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
}

#[test]
fn host_pointer_stale_route_cannot_borrow_a_current_receiver() {
    let (mut session, scene) = host_pointer_fixture();
    let (recording, output) = paint(&mut session, A);
    session
        .report_surface_presentation(output, ResultKind::Presented)
        .unwrap();
    settle(&mut session);
    let current = session.presented_surface(A).unwrap();
    assert_ne!(
        current, scene.a,
        "a later actual presentation changes route authority"
    );
    let descriptor = recording
        .records
        .iter()
        .filter_map(|record| match record {
            OwnedRecord::Receiver(receiver)
                if receiver.role() == DockspaceReceiverRole::TabBody =>
            {
                Some(*receiver)
            }
            _ => None,
        })
        .nth(1)
        .unwrap();
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let input = |event, surface| {
        HostPointerInput::new(
            SurfacePointerId::new(2),
            event,
            HostPointerRoute::new(surface, receiver.center()),
            HostPointerRoute::new(surface, receiver.center()),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver),
        )
    };
    let mut stale = session.begin_host_frame().unwrap();
    if stale
        .submit_host_pointer_batch([
            input(
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                scene.a,
            ),
            input(
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                scene.a,
            ),
        ])
        .is_ok()
    {
        stale
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(
            !stale.commit().unwrap().workspace_changed(),
            "a current receiver must not refresh an older route's presentation proof"
        );
    } else {
        drop(stale);
    }
    assert!(!session.has_active_gesture());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());

    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer_batch([
            input(
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                current,
            ),
            input(
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                current,
            ),
        ])
        .unwrap();
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    retry.commit().unwrap();
    assert!(session.view().item(SECOND).unwrap().is_selected());
}

#[test]
fn host_pointer_foreign_session_routes_and_receivers_cannot_authorize_matching_surfaces() {
    let (mut session, scene) = host_pointer_fixture();
    let (foreign_session, foreign) = host_pointer_fixture();
    assert_eq!(foreign.a.surface(), scene.a.surface());
    assert_eq!(foreign.source.bounds(), scene.source.bounds());
    assert_ne!(foreign.a, scene.a);

    let mut frame = session.begin_host_frame().unwrap();
    if frame.submit_host_pointer_batch(foreign.batch()).is_ok() {
        let recordings = record_host_pointer_surfaces(&mut frame);
        assert_no_pointer_preview(&recordings);
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
        assert!(!settled.workspace_changed());
    } else {
        drop(frame);
    }
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert_eq!(session.view().item(THIRD).unwrap().surface(), B);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(foreign_session.view().item(FIRST).unwrap().surface(), A);
    assert!(!foreign_session.has_active_gesture());
}

#[test]
fn host_pointer_stale_and_foreign_manipulation_cannot_cross_an_armed_source_threshold() {
    for foreign in [false, true] {
        let (mut session, mut scene) = host_pointer_fixture();
        let stale = scene.a;
        let (recording, output) = paint(&mut session, A);
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
        settle(&mut session);
        scene.a = session.presented_surface(A).unwrap();
        scene.source = session
            .bind_presented_receiver(&tab_receiver(&recording))
            .unwrap();
        assert_ne!(stale, scene.a);
        let invalid = if foreign {
            host_pointer_fixture().1.a
        } else {
            stale
        };
        assert_eq!(invalid.surface(), scene.a.surface());
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                scene.press(),
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    HostPointerRoute::new(invalid, LogicalPoint::new(700.0, 500.0).unwrap()),
                    HostPointerRoute::new(scene.b, scene.target.center()),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::hover(&scene.target),
                ),
                scene.target_edge(SurfacePointerEvent::ButtonReleased(
                    SurfacePointerButton::Primary,
                )),
            ])
            .unwrap();
        let recordings = record_host_pointer_surfaces(&mut frame);
        assert_no_pointer_preview(&recordings);
        assert!(recordings[0].records.iter().any(|record| matches!(record,
            OwnedRecord::Geometry { visual, paint: SurfacePaintDisposition::Visible, .. }
                if *visual == scene.source_visual
        )));
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(!session.has_active_gesture());
        let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
        assert!(!settled.workspace_changed());
        assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
        assert!(session.disable_host_pointer().unwrap().is_some());
    }
}

#[test]
fn host_pointer_stale_and_foreign_hover_cannot_authorize_a_captured_drop() {
    for foreign in [false, true] {
        let (mut session, mut scene) = host_pointer_fixture();
        let stale = scene.b;
        let mut inspect = session.begin_host_frame().unwrap();
        let descriptor = inspect
            .paint_plan(B)
            .unwrap()
            .unwrap()
            .center_drop_receiver_for_item(THIRD)
            .unwrap();
        drop(inspect);
        let (_, output) = paint(&mut session, B);
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
        settle(&mut session);
        scene.b = session.presented_surface(B).unwrap();
        scene.target = session.bind_presented_receiver(&descriptor).unwrap();
        assert_ne!(stale, scene.b);
        let invalid = if foreign {
            host_pointer_fixture().1.b
        } else {
            stale
        };
        assert_eq!(invalid.surface(), scene.b.surface());
        let invalid_hover = |event| {
            HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                HostPointerRoute::new(scene.a, scene.moved),
                HostPointerRoute::new(invalid, scene.target.center()),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::hover(&scene.target),
            )
        };
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                scene.press(),
                invalid_hover(SurfacePointerEvent::Moved),
                invalid_hover(SurfacePointerEvent::ButtonReleased(
                    SurfacePointerButton::Primary,
                )),
            ])
            .unwrap();
        let recordings = record_host_pointer_surfaces(&mut frame);
        assert_no_pointer_preview(&recordings);
        let mut report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), before);
        assert!(!session.has_active_gesture());
        let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
        assert!(!settled.workspace_changed());
        assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
        assert!(session.disable_host_pointer().unwrap().is_some());
    }
}

#[test]
fn host_pointer_current_route_cannot_refresh_stale_hover_facts() {
    for changed_scene in [false, true] {
        for fact_kind in ["clear", "candidate", "no_receiver", "blocked"] {
            let (mut session, mut scene) = host_pointer_fixture();
            let stale_surface = scene.b;
            let stale_receiver = scene.target;
            if changed_scene {
                let mut frame = session.begin_host_frame().unwrap();
                frame
                    .measure_surface(
                        B,
                        UniformSurfaceMetrics::new(
                            bounds(1000.0),
                            LogicalSize::new(64.0, 48.0).unwrap(),
                            96.0,
                        )
                        .unwrap(),
                    )
                    .unwrap();
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                frame.commit().unwrap();
            }
            let mut inspect = session.begin_host_frame().unwrap();
            let descriptor = inspect
                .paint_plan(B)
                .unwrap()
                .unwrap()
                .center_drop_receiver_for_item(THIRD)
                .unwrap();
            drop(inspect);
            let (_, output) = paint(&mut session, B);
            let artifact = output.presentation_artifact();
            session
                .report_surface_presentation(output, ResultKind::Presented)
                .unwrap();
            let settled = settle(&mut session);
            let [ack] = settled.surface_presentation_commits() else {
                panic!("the replacement B output must settle exactly once")
            };
            assert!(ack.presents(artifact));
            scene.b = session.presented_surface(B).unwrap();
            scene.target = session.bind_presented_receiver(&descriptor).unwrap();
            assert_eq!(session.presented_surface(A), Some(scene.a));
            assert_ne!(scene.b, stale_surface);
            assert_ne!(scene.target, stale_receiver);
            assert_eq!(
                scene.target.descriptor() == stale_receiver.descriptor(),
                !changed_scene,
                "even a new emission of the same semantic scene must revoke old facts"
            );
            let facts = match fact_kind {
                "clear" => {
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&stale_surface)
                }
                "candidate" => SurfacePointerReceiverFacts::hover(&stale_receiver),
                "no_receiver" => SurfacePointerReceiverFacts::no_hover(&stale_surface),
                "blocked" => SurfacePointerReceiverFacts::blocked_hover(&stale_surface),
                _ => unreachable!(),
            };
            let edge = |event| {
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    event,
                    HostPointerRoute::new(scene.a, scene.moved),
                    HostPointerRoute::new(scene.b, scene.target.center()),
                    SurfacePointerCapture::ProviderEndpoint,
                    facts,
                )
            };
            let before = session.version();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer_batch([
                    scene.press(),
                    edge(SurfacePointerEvent::Moved),
                    edge(SurfacePointerEvent::ButtonReleased(
                        SurfacePointerButton::Primary,
                    )),
                ])
                .unwrap();
            let recordings = record_host_pointer_surfaces(&mut frame);
            assert_no_pointer_preview(&recordings);
            let mut report = frame.commit().unwrap();
            assert!(
                !report.workspace_changed(),
                "changed scene: {changed_scene}, stale fact: {fact_kind}"
            );
            assert!(!session.has_active_gesture());
            assert!(
                !settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented)
                    .workspace_changed()
            );
            assert_eq!(session.version(), before);
            assert_eq!(session.view().surfaces().count(), 2);
            for (item, surface, root) in [
                (FIRST, A, RootId::new(1)),
                (SECOND, A, RootId::new(1)),
                (THIRD, B, RootId::new(2)),
                (FOURTH, B, RootId::new(2)),
            ] {
                let view = session.view().item(item).unwrap();
                assert_eq!(view.surface(), surface);
                assert_eq!(view.root(), root);
                assert!(view.contained().is_none());
            }
            assert!(!session.has_active_gesture());
        }
    }
}

#[test]
fn host_pointer_wrong_winner_suffix_rolls_back_its_valid_prefix_and_retries_once() {
    let (mut session, scene) = host_pointer_fixture_for_item(SECOND);
    let before = session.version();
    let a_before = session.presented_surface(A).unwrap();
    let b_before = session.presented_surface(B).unwrap();
    let mut failed = session.begin_host_frame().unwrap();
    assert!(
        failed
            .submit_host_pointer_batch([
                scene.press(),
                scene.target_edge(SurfacePointerEvent::Moved),
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                    HostPointerRoute::new(scene.a, scene.moved),
                    HostPointerRoute::new(scene.b, scene.target.center()),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::no_hover(&scene.b),
                ),
            ])
            .is_err(),
        "the release claims no receiver at B's actual current drop winner"
    );
    assert_ne!(failed.version(), before);
    assert!(failed.view().item(SECOND).unwrap().is_selected());
    drop(failed);

    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert!(!session.has_active_gesture());
    assert_eq!(session.presented_surface(A), Some(a_before));
    assert_eq!(session.presented_surface(B), Some(b_before));

    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer_batch(scene.batch())
        .expect("the original event order and provider watermark remain retryable");
    measure(&mut retry, 800.0);
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut input_report = retry.commit().unwrap();
    assert!(input_report.take_painted_outputs().is_empty());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert!(!session.has_active_gesture());

    let mut paint_frame = session.begin_host_frame().unwrap();
    let recordings = record_host_pointer_surfaces(&mut paint_frame);
    assert_target_preview(&recordings[1]);
    let mut report = paint_frame.commit().unwrap();
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    let delivered = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(delivered.workspace_changed());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(
        session
            .view()
            .item(SECOND)
            .unwrap()
            .tabs()
            .items()
            .iter()
            .filter(|item| **item == SECOND)
            .count(),
        1,
    );
    let delivered_version = session.version();
    let unchanged = settle(&mut session);
    assert_eq!(session.version(), delivered_version);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert!(unchanged.presentation_transitions().is_empty());
    assert!(!session.has_active_gesture());
    assert!(session.disable_host_pointer().unwrap().is_some());
}
