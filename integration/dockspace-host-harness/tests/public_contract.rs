use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery as Delivery, DockspaceManipulationFeedback,
    DockspaceManipulationStatus as Status, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceReceiverRole, DockspaceSession, DockspaceVisualId, DockspaceVisualKind,
    HostFrameReport, HostInputOutcome, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, SurfacePaintDisposition, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult as ResultKind, SurfaceScrollDelta, SurfaceScrollDeviceId,
    SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase,
    SurfaceUnavailableReason, TabListMenuMetrics, TabStripControlKind, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const C: SurfaceId = SurfaceId::new(3);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);

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
        measure_overflow_surface(frame, surface);
    }
}

fn measure_overflow_surface(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
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
                        TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                            .unwrap(),
                    )
                    .with_scroll_forward(
                        TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
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

fn open_tab_list_menu(session: &mut DockspaceSession, surface: SurfaceId) {
    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(surface).unwrap().unwrap();
    let mut control = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabStripControl(candidate),
            ..
        } = step
            && candidate.kind() == TabStripControlKind::TabListMenu
        {
            assert!(candidate.enabled());
            assert!(!candidate.menu_open());
            assert!(control.replace(candidate).is_none());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let open = plan
        .prepare_tab_strip_control_activation(
            control.expect("the overflow surface must expose its tab-list control"),
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
}

#[test]
fn review_popup_on_a_is_interactive_when_b_has_no_new_output() {
    let mut session = session();
    let mut narrow = session.begin_host_frame().unwrap();
    measure_overflow(&mut narrow);
    narrow.commit().unwrap();
    for surface in [A, B] {
        let (_, output) = paint(&mut session, surface);
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
    }
    settle(&mut session);
    assert!(session.presented_surface(A).is_some());
    assert!(session.presented_surface(B).is_some());

    open_tab_list_menu(&mut session, A);
    let mut measured = session.begin_host_frame().unwrap();
    measure_overflow(&mut measured);
    measured.commit().unwrap();

    // B may be minimized or independently scheduled; only A presents a successor.
    let (_, output) = paint(&mut session, A);
    session
        .report_surface_presentation(output, ResultKind::Presented)
        .unwrap();
    settle(&mut session);
    assert!(
        session.presented_surface(A).is_some(),
        "the popup owner must be interactive without waiting for B's successor output"
    );
}

fn popup_layers(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) -> (bool, bool) {
    let plan = frame
        .paint_plan(surface)
        .unwrap()
        .expect("the retained surface must expose its program without popup remeasurement");
    let mut menu = false;
    let mut backdrop = false;
    plan.visit_surface_program(|step| {
        match step {
            Step::PopupMenu { .. } => menu = true,
            Step::PopupBackdrop(_) => backdrop = true,
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    (menu, backdrop)
}

fn host_pointer_popup_fixture() -> (
    DockspaceSession,
    PresentedDockspaceSurface,
    PresentedDockReceiver,
) {
    let mut session = session();
    session.enable_host_pointer().unwrap();
    let mut narrow = session.begin_host_frame().unwrap();
    measure_overflow_surface(&mut narrow, A);
    narrow
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    narrow.commit().unwrap();

    let mut initial = session.begin_host_frame().unwrap();
    let target = initial
        .paint_plan(B)
        .unwrap()
        .unwrap()
        .tab_receiver(FOURTH)
        .expect("B must expose its unselected tab before A opens the menu");
    record_host_pointer_surfaces(&mut initial);
    for output in initial.commit().unwrap().take_painted_outputs() {
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
    }
    settle(&mut session);
    let b = session.presented_surface(B).unwrap();
    let target = session.bind_presented_receiver(&target).unwrap();

    open_tab_list_menu(&mut session, A);
    // Only the popup owner measures and presents; B retains its pre-popup output.
    let mut measured = session.begin_host_frame().unwrap();
    measure_overflow_surface(&mut measured, A);
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();
    let (_, output) = paint(&mut session, A);
    session
        .report_surface_presentation(output, ResultKind::Presented)
        .unwrap();
    settle(&mut session);
    assert!(session.presented_surface(A).is_some());
    assert_eq!(session.presented_surface(B), Some(b));
    assert_eq!(
        session.bind_presented_receiver(&target.descriptor()),
        Some(target)
    );
    let mut inspect = session.begin_host_frame().unwrap();
    assert_eq!(popup_layers(&mut inspect, A), (true, true));
    assert_eq!(popup_layers(&mut inspect, B), (false, false));
    drop(inspect);
    (session, b, target)
}

fn assert_popup_press_leaves_content_idle(session: &DockspaceSession, report: &HostFrameReport) {
    assert!(!report.workspace_changed());
    assert_eq!(report.before(), report.after());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(report.manipulation_feedback().is_empty());
    assert!(report.inputs().iter().all(|input| !matches!(
        input,
        HostInputOutcome::ProductActionApplied(_)
            | HostInputOutcome::ProductPresentationActionRequested { .. }
    )));
    for item in [FIRST, THIRD] {
        assert!(session.view().item(item).unwrap().is_selected());
    }
    for item in [SECOND, FOURTH] {
        assert!(!session.view().item(item).unwrap().is_selected());
    }
}

#[test]
fn host_pointer_other_surface_press_dismisses_popup_without_activating_the_tab_or_drag() {
    let (mut session, b, target) = host_pointer_popup_fixture();
    let route = HostPointerRoute::new(b, target.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&target),
        ))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_popup_press_leaves_content_idle(&session, &report);
    assert!(report.published_state_changed());
    assert_eq!(session.presented_surface(B), Some(b));

    let mut measured = session.begin_host_frame().unwrap();
    measure_overflow_surface(&mut measured, A);
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();
    let mut inspect = session.begin_host_frame().unwrap();
    assert_eq!(popup_layers(&mut inspect, A), (false, false));
    assert_eq!(popup_layers(&mut inspect, B), (false, false));
}

#[test]
fn host_pointer_blocked_foreign_or_stale_delivery_cannot_dismiss_another_surfaces_popup() {
    for scenario in ["blocked", "foreign", "stale"] {
        let (mut session, mut b, target) = host_pointer_popup_fixture();
        let foreign = (scenario == "foreign").then(host_pointer_popup_fixture);
        let delivery = foreign
            .as_ref()
            .map_or(target, |(_, _, receiver)| *receiver);
        if scenario == "foreign" {
            assert_ne!(delivery, target);
            assert_eq!(delivery.center(), target.center());
        }
        if scenario == "stale" {
            let (_, output) = paint(&mut session, B);
            session
                .report_surface_presentation(output, ResultKind::Presented)
                .unwrap();
            settle(&mut session);
            let current = session.presented_surface(B).unwrap();
            assert_ne!(current, b);
            let current_receiver = session
                .bind_presented_receiver(&target.descriptor())
                .expect("B's unchanged tab must remain bindable through the current output");
            assert_ne!(
                current_receiver, target,
                "the old receiver must be stale even when the semantic scene is unchanged"
            );
            b = current;
        }
        let route = HostPointerRoute::new(b, target.center());
        let facts = if scenario == "blocked" {
            SurfacePointerReceiverFacts::unknown().with_blocked_delivery(&b)
        } else {
            SurfacePointerReceiverFacts::delivery(&delivery)
        };
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            ))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_popup_press_leaves_content_idle(&session, &report);
        assert_eq!(session.presented_surface(B), Some(b), "{scenario}");
        let mut inspect = session.begin_host_frame().unwrap();
        assert_eq!(popup_layers(&mut inspect, A), (true, true), "{scenario}");
        assert_eq!(popup_layers(&mut inspect, B), (false, false), "{scenario}");
    }
}

#[test]
fn host_pointer_other_surface_drag_only_press_dismisses_popup_without_starting_group_drag() {
    for framework_blocked in [false, true] {
        let (mut session, b, _) = host_pointer_popup_fixture();
        let mut inspect = session.begin_host_frame().unwrap();
        let plan = inspect.paint_plan(B).unwrap().unwrap();
        let mut grip = None;
        plan.visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } = step
            {
                grip = plan.receiver_for_tab_group_grip(bar);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        let grip = grip.expect("B's retained tab group must expose a drag-only grip");
        assert_eq!(grip.role(), DockspaceReceiverRole::TabGroupGrip);
        drop(inspect);
        let grip = session.bind_presented_receiver(&grip).unwrap();
        let hit = session.host_pointer_hit(B, grip.center()).unwrap();
        assert!(hit.delivery().is_none());
        assert_eq!(hit.drag_delivery(), Some(grip));

        // Unknown click delivery is independent of an exact Dock drag receiver.
        let facts = if framework_blocked {
            SurfacePointerReceiverFacts::unknown().with_blocked_delivery(&b)
        } else {
            SurfacePointerReceiverFacts::unknown().with_drag_delivery(&grip)
        };
        let route = HostPointerRoute::new(b, grip.center());
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            ))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert_popup_press_leaves_content_idle(&session, &report);
        assert_eq!(session.presented_surface(B), Some(b));

        let mut measured = session.begin_host_frame().unwrap();
        measure_overflow_surface(&mut measured, A);
        measured
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        measured.commit().unwrap();
        let mut inspect = session.begin_host_frame().unwrap();
        assert_eq!(
            popup_layers(&mut inspect, A),
            (framework_blocked, framework_blocked),
            "a valid drag lane must dismiss the popup; blocked delivery must preserve it"
        );
        assert_eq!(popup_layers(&mut inspect, B), (false, false));
    }
}

#[test]
fn host_pointer_other_surface_wheel_preserves_popup_and_underlying_strip_offset() {
    let (mut session, _, _) = host_pointer_popup_fixture();
    let mut measured = session.begin_host_frame().unwrap();
    measure_overflow_surface(&mut measured, B);
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();

    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(B).unwrap().unwrap();
    let mut strip = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            strip = Some((
                plan.receiver_for_tab_strip_scroll(bar).unwrap(),
                bar.scroll_offset(),
                bar.maximum_scroll_offset(),
            ));
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (scroll, offset, maximum) = strip.expect("B must expose its overflowing strip");
    assert_eq!(offset, 0.0);
    assert!(
        maximum > 40.0,
        "the wheel must have room to scroll B's strip"
    );
    drop(inspect);
    let (_, output) = paint(&mut session, B);
    session
        .report_surface_presentation(output, ResultKind::Presented)
        .unwrap();
    settle(&mut session);
    let b = session.presented_surface(B).unwrap();
    let scroll = session.bind_presented_receiver(&scroll).unwrap();

    let route = HostPointerRoute::new(b, scroll.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                SurfaceScrollDeviceId::new(1),
                SurfaceScrollPhase::Discrete {
                    delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
                },
                SurfaceScrollMomentum::Direct,
                SurfaceScrollModifiers::new(false, false, false, false),
            )),
            route,
            route,
            SurfacePointerCapture::None,
            SurfacePointerReceiverFacts::delivery(&scroll),
        ))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_popup_press_leaves_content_idle(&session, &report);
    assert!(session.pointer_continuation().is_none());
    assert_eq!(session.presented_surface(B), Some(b));

    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(B).unwrap().unwrap();
    let mut after = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            after = Some((bar.scroll_offset(), bar.maximum_scroll_offset()));
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(
        after,
        Some((offset, maximum)),
        "a non-owner wheel must not scroll the strip underneath the popup"
    );
    assert_eq!(popup_layers(&mut inspect, A), (true, true));
    assert_eq!(popup_layers(&mut inspect, B), (false, false));
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
fn popup_owner_ack_is_independent_and_preserves_its_artifact_when_sibling_presents() {
    let mut session = session();
    let mut narrow = session.begin_host_frame().unwrap();
    measure_overflow(&mut narrow);
    narrow.commit().unwrap();

    open_tab_list_menu(&mut session, A);
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
        assert_eq!(backdrop_receiver.is_some(), surface == A);
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        painted.confirm_surface_painted(receipt).unwrap();
        recordings.push((recording, backdrop_receiver));
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
    let a_receiver = a_receiver
        .as_ref()
        .expect("the popup owner must expose its backdrop receiver");
    assert!(b_receiver.is_none());
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
    assert_eq!(session.presented_surface(A).unwrap().surface(), A);
    assert!(session.presented_surface(B).is_none());
    let a_bound_receiver = session
        .bind_presented_receiver(a_receiver)
        .expect("the popup owner must become interactive after its own presentation");

    // B settles independently without changing A's accepted popup artifact or authority.
    session
        .report_surface_presentation(b_output, ResultKind::Presented)
        .unwrap();
    let mut b_committed = settle(&mut session);
    assert!(b_committed.take_painted_outputs().is_empty());
    let [b_ack] = b_committed.surface_presentation_commits() else {
        panic!("the sibling must settle its own presentation independently")
    };
    assert!(b_ack.presents(b_artifact));
    assert!(!b_ack.retires(b_artifact));
    assert!(!b_ack.settles(a_artifact));
    assert_eq!(session.presented_surface(A).unwrap().surface(), A);
    assert_eq!(session.presented_surface(B).unwrap().surface(), B);
    assert_eq!(
        session.bind_presented_receiver(a_receiver),
        Some(a_bound_receiver)
    );
    let mut unchanged = settle(&mut session);
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert!(unchanged.take_painted_outputs().is_empty());
    assert_eq!(
        session.bind_presented_receiver(a_receiver),
        Some(a_bound_receiver)
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
fn late_presented_output_keeps_its_exact_tab_receivers_after_selection() {
    let mut session = session();
    let (older_records, older_output) = paint(&mut session, A);
    let older_artifact = older_output.presentation_artifact();
    let older_receiver = tab_receiver(&older_records);

    // Selection changes the next layout, while the eventual actual output still
    // identifies the same tabs. Its receivers require current semantic admission.
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
        panic!("the exact delayed output must settle independently of the newer candidate")
    };
    assert!(!retired_ack.retires(older_artifact));
    assert!(retired_ack.presents(older_artifact));
    assert!(session.view().surface(A).is_some());
    assert!(session.presented_surface(A).is_some());
    assert!(session.bind_presented_receiver(&older_receiver).is_some());

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
    fn from_presented(
        session: &DockspaceSession,
        source_surface: SurfaceId,
        target_surface: SurfaceId,
        source_descriptor: DockspaceReceiverDescriptor,
        target_descriptor: DockspaceReceiverDescriptor,
        source_recording: &RecordingRenderer,
    ) -> Self {
        let source_visual = source_recording
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
            .expect("the presented source has an actual recorded tab visual");
        let scene = Self {
            a: session.presented_surface(source_surface).unwrap(),
            b: session.presented_surface(target_surface).unwrap(),
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
        scene
    }

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
    let scene = HostPointerScene::from_presented(
        &session,
        A,
        B,
        source_descriptor,
        target_descriptor,
        &a_records,
    );
    (session, scene)
}

fn host_pointer_independent_targets_fixture() -> (DockspaceSession, [HostPointerScene; 2]) {
    let mut session = session();
    let mut attach = session.begin_host_frame().unwrap();
    attach
        .attach_empty_surface_current(C, RootId::new(3))
        .unwrap();
    attach
        .open_item_current(
            FIFTH,
            DockPlacement::Center(DockAnchor::Central(RootId::new(3))),
        )
        .unwrap();
    measure(&mut attach, 800.0);
    attach.commit().unwrap();
    session.enable_host_pointer().unwrap();

    let mut initial = session.begin_host_frame().unwrap();
    let sources = [FIRST, SECOND].map(|item| {
        initial
            .paint_plan(A)
            .unwrap()
            .unwrap()
            .tab_receiver(item)
            .unwrap()
    });
    let targets = [(B, THIRD), (C, FIFTH)].map(|(surface, item)| {
        initial
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .center_drop_receiver_for_item(item)
            .unwrap()
    });
    let [a_recording, _] = record_host_pointer_surfaces(&mut initial);
    let (_, receipt) = RecordingRenderer::execute(initial.paint_plan(C).unwrap().unwrap()).unwrap();
    initial.confirm_surface_painted(receipt).unwrap();
    let outputs = initial.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 3);
    for output in outputs {
        session
            .report_surface_presentation(output, ResultKind::Presented)
            .unwrap();
    }
    assert_eq!(settle(&mut session).surface_presentation_commits().len(), 3);
    let scenes = [B, C].map(|surface| {
        let index = usize::from(surface == C);
        HostPointerScene::from_presented(
            &session,
            A,
            surface,
            sources[index],
            targets[index],
            &a_recording,
        )
    });
    (session, scenes)
}

fn assert_immediate_dock(report: &HostFrameReport) -> DockspaceManipulationFeedback {
    assert!(report.workspace_changed());
    let [delivered] = report.manipulation_feedback() else {
        panic!("an ordinary release must publish exactly one immediate terminal")
    };
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    *delivered
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
fn host_pointer_ordinary_release_delivers_in_its_successful_frame_without_preview_paint() {
    let (mut session, scene) = host_pointer_fixture();
    assert_ne!(scene.moved, scene.target.center());
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer_batch(scene.batch()).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();

    assert_immediate_dock(&report);
    assert_ne!(session.version(), before);
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(
        session.view().item(SECOND).unwrap().tabs().items(),
        &[SECOND]
    );
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[THIRD, FOURTH, FIRST]
    );
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    let unchanged = settle(&mut session);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.manipulation_feedback().is_empty());
}

#[test]
fn host_pointer_new_press_and_dropped_successor_cannot_undo_an_ordinary_release() {
    let (mut session, scene) = host_pointer_fixture();
    let mut inspect = session.begin_host_frame().unwrap();
    let second_descriptor = inspect
        .paint_plan(A)
        .unwrap()
        .unwrap()
        .tab_receiver(SECOND)
        .unwrap();
    drop(inspect);
    let second = session.bind_presented_receiver(&second_descriptor).unwrap();
    let route = HostPointerRoute::new(scene.a, second.center());

    let mut release = session.begin_host_frame().unwrap();
    release.submit_host_pointer_batch(scene.batch()).unwrap();
    release
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let delivered = assert_immediate_dock(&release.commit().unwrap());
    let delivered_version = session.version();
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);

    // Encode P1, but keep P0 on screen throughout the next press.
    let mut render = session.begin_host_frame().unwrap();
    measure(&mut render, 800.0);
    record_host_pointer_surfaces(&mut render);
    let successor_outputs = render.commit().unwrap().take_painted_outputs();
    assert_eq!(successor_outputs.len(), 2);
    assert_eq!(session.presented_surface(A), Some(scene.a));
    assert_eq!(
        session.bind_presented_receiver(&second_descriptor),
        Some(second)
    );

    let mut press = session.begin_host_frame().unwrap();
    press
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&second),
        ))
        .unwrap();
    press
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let pressed = press.commit().unwrap();
    assert!(!pressed.workspace_changed());
    assert!(pressed.manipulation_feedback().is_empty());
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), delivered_version);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);

    let mut artifacts = Vec::new();
    for output in successor_outputs {
        artifacts.push(output.presentation_artifact());
        session
            .report_surface_presentation(output, ResultKind::Dropped)
            .unwrap();
    }
    let dropped = settle(&mut session);
    for artifact in artifacts {
        assert!(
            dropped
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact))
        );
    }
    assert!(!dropped.workspace_changed());
    assert!(
        dropped
            .manipulation_feedback()
            .iter()
            .all(|feedback| feedback.manipulation_id() != delivered.manipulation_id())
    );
    assert_eq!(session.version(), delivered_version);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);

    let mut end = session.begin_host_frame().unwrap();
    end.submit_host_pointer(HostPointerInput::new(
        SurfacePointerId::new(1),
        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::delivery(&second),
    ))
    .unwrap();
    end.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let ended = end.commit().unwrap();
    assert!(!ended.workspace_changed());
    assert!(
        ended
            .manipulation_feedback()
            .iter()
            .all(|feedback| feedback.manipulation_id() != delivered.manipulation_id())
    );
    assert!(!session.has_active_gesture());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
}

#[test]
fn host_pointer_two_ordinary_releases_against_p0_are_invariant_under_batch_partition() {
    let partitions: &[&[usize]] = &[&[6], &[3, 3], &[2, 2, 2], &[1, 1, 1, 1, 1, 1]];
    let topology = |session: &DockspaceSession| {
        [FIRST, SECOND, THIRD, FOURTH, FIFTH].map(|item| {
            let view = session.view().item(item).unwrap();
            (
                item,
                view.surface(),
                view.root(),
                view.tabs().items().to_vec(),
                view.tabs().selected(),
            )
        })
    };
    let mut baseline = None;
    for partition in partitions {
        let (mut session, scenes) = host_pointer_independent_targets_fixture();
        // P0 is presented once. All six edges retain those exact capabilities;
        // the only later presentation observation is P1 after both releases.
        let trace = [scenes[0].batch(), scenes[1].batch()].concat();
        let mut consumed = 0;
        let mut deliveries = Vec::new();
        for &count in *partition {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer_batch(trace[consumed..consumed + count].iter().copied())
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            let mut report = frame.commit().unwrap();
            assert!(report.take_painted_outputs().is_empty());
            deliveries.extend_from_slice(report.manipulation_feedback());
            consumed += count;
            assert_eq!(
                deliveries.len(),
                usize::from(consumed >= 3) + usize::from(consumed >= 6),
                "each ordinary release publishes its terminal in that committed batch: {partition:?}"
            );
            assert_eq!(
                session.view().item(FIRST).unwrap().surface(),
                if consumed >= 3 { B } else { A },
                "{partition:?}"
            );
            assert_eq!(
                session.view().item(SECOND).unwrap().surface(),
                if consumed >= 6 { C } else { A },
                "{partition:?}"
            );
        }
        assert_eq!(consumed, trace.len());
        assert_eq!(deliveries.len(), 2, "{partition:?}");
        assert!(
            deliveries
                .iter()
                .all(|feedback| feedback.status() == Status::Delivered(Delivery::Dock)),
            "{partition:?}"
        );
        assert_ne!(
            deliveries[0].manipulation_id(),
            deliveries[1].manipulation_id()
        );
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_presentation());
        assert_eq!(
            session.view().item(FIRST).unwrap().tabs().items(),
            &[THIRD, FOURTH, FIRST]
        );
        assert_eq!(
            session.view().item(SECOND).unwrap().tabs().items(),
            &[FIFTH, SECOND]
        );
        let accepted = topology(&session);

        let mut render = session.begin_host_frame().unwrap();
        measure(&mut render, 800.0);
        for surface in render.surfaces() {
            let (_, receipt) =
                RecordingRenderer::execute(render.paint_plan(surface).unwrap().unwrap()).unwrap();
            render.confirm_surface_painted(receipt).unwrap();
        }
        let outputs = render.commit().unwrap().take_painted_outputs();
        assert_eq!(outputs.len(), 3);
        for output in outputs {
            session
                .report_surface_presentation(output, ResultKind::Presented)
                .unwrap();
        }
        let presented = settle(&mut session);
        assert_eq!(presented.surface_presentation_commits().len(), 3);
        assert!(!presented.workspace_changed());
        assert!(presented.manipulation_feedback().is_empty());
        assert_eq!(topology(&session), accepted);
        if let Some(expected) = &baseline {
            assert_eq!(&accepted, expected, "{partition:?}");
        } else {
            baseline = Some(accepted);
        }
    }
}

#[test]
fn host_pointer_old_group_grip_rejects_unseen_members_but_current_item_stays_usable() {
    let (mut session, scene) = host_pointer_fixture();
    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(A).unwrap().unwrap();
    let mut grip_descriptor = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            grip_descriptor = plan.receiver_for_tab_group_grip(bar);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let grip_descriptor = grip_descriptor.expect("P0 exposes A's whole-group grip");
    drop(inspect);
    let grip = session.bind_presented_receiver(&grip_descriptor).unwrap();

    let mut add = session.begin_host_frame().unwrap();
    add.open_item_current(FIFTH, DockPlacement::After(SECOND))
        .unwrap();
    add.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut added = add.commit().unwrap();
    assert!(added.workspace_changed());
    assert!(added.take_painted_outputs().is_empty());
    assert_eq!(
        session.view().item(FIFTH).unwrap().tabs().items(),
        &[FIRST, SECOND, FIFTH]
    );
    assert_eq!(session.presented_surface(A), Some(scene.a));
    assert!(
        session.bind_presented_receiver(&grip_descriptor).is_none(),
        "P0's [FIRST, SECOND] grip must not capture the unpainted FIFTH"
    );
    assert_eq!(
        session.bind_presented_receiver(&scene.source.descriptor()),
        Some(scene.source)
    );

    let route = HostPointerRoute::new(scene.a, grip.center());
    let moved = LogicalPoint::new(grip.center().x() + 32.0, grip.center().y()).unwrap();
    let target_edge = |event| {
        HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::new(scene.a, moved),
            HostPointerRoute::new(scene.b, scene.target.center()),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown()
                .with_drag_delivery(&grip)
                .with_hover(&scene.target),
        )
    };
    let mut stale_press = session.begin_host_frame().unwrap();
    stale_press
        .submit_host_pointer_batch([
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_drag_delivery(&grip),
            ),
            target_edge(SurfacePointerEvent::Moved),
            target_edge(SurfacePointerEvent::ButtonReleased(
                SurfacePointerButton::Primary,
            )),
        ])
        .unwrap();
    stale_press
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let stale = stale_press.commit().unwrap();
    assert!(!stale.workspace_changed());
    assert!(
        stale
            .manipulation_feedback()
            .iter()
            .all(|feedback| !matches!(
                feedback.status(),
                Status::Delivered(_) | Status::Pending(_)
            ))
    );
    assert_eq!(
        session.view().item(FIFTH).unwrap().tabs().items(),
        &[FIRST, SECOND, FIFTH]
    );
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());

    // The same visible output can still name an unchanged individual item.
    let mut item_press = session.begin_host_frame().unwrap();
    item_press.submit_host_pointer_batch(scene.batch()).unwrap();
    item_press
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert_immediate_dock(&item_press.commit().unwrap());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(session.view().item(FIFTH).unwrap().surface(), A);
    assert_eq!(
        session.view().item(FIFTH).unwrap().tabs().items(),
        &[SECOND, FIFTH]
    );
}

#[test]
fn host_pointer_same_batch_release_delivers_before_the_target_output_settles() {
    for target_result in [ResultKind::Presented, ResultKind::Dropped] {
        let (mut session, scene) = host_pointer_fixture();
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer_batch(scene.batch()).unwrap();
        measure(&mut frame, 800.0);
        let recordings = record_host_pointer_surfaces(&mut frame);
        assert_no_pointer_preview(&recordings);
        let mut report = frame.commit().unwrap();
        assert_immediate_dock(&report);
        let delivered_version = session.version();
        assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
        assert!(
            !session.has_active_gesture(),
            "release must relinquish the active pointer owner"
        );
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(B));
        assert!(!session.surface_requires_presentation(A));

        let settled = settle_pointer_outputs(&mut session, &mut report, target_result);
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert!(settled.presentation_transitions().is_empty());
        assert_eq!(session.version(), delivered_version);
        assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
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
    assert_immediate_dock(&input_report);
    let delivered_version = session.version();
    assert!(input_report.take_painted_outputs().is_empty());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(B));
    assert!(!session.surface_requires_presentation(A));
    assert!(input_report.repaint_surfaces().contains(&B));

    // A later paint describes the accepted edit and cannot deliver it again.
    let mut paint_frame = session.begin_host_frame().unwrap();
    let recordings = record_host_pointer_surfaces(&mut paint_frame);
    assert_no_pointer_preview(&recordings);
    let mut report = paint_frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(session.version(), delivered_version);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert!(!session.surface_requires_presentation(B));

    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert!(settled.presentation_transitions().is_empty());
    assert_eq!(session.version(), delivered_version);
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
    let before = session.version();
    let mut abandoned = session.begin_host_frame().unwrap();
    abandoned.submit_host_pointer_batch(scene.batch()).unwrap();
    assert_ne!(abandoned.version(), before);
    assert_eq!(abandoned.view().item(FIRST).unwrap().surface(), B);
    drop(abandoned);
    assert_eq!(session.version(), before);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.presented_surface(A), Some(scene.a));
    assert_eq!(session.presented_surface(B), Some(scene.b));

    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer_batch(scene.batch())
        .expect("dropping a frame must not consume its sequence or button edges");
    measure(&mut retry, 800.0);
    let recordings = record_host_pointer_surfaces(&mut retry);
    assert_no_pointer_preview(&recordings);
    let mut report = retry.commit().unwrap();
    assert_immediate_dock(&report);
    assert_eq!(report.before(), before);
    let delivered_version = session.version();
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);

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
        assert!(repeated.manipulation_feedback().is_empty());
        assert!(repeated.presentation_transitions().is_empty());
    } else {
        drop(repeated);
    }
    assert_eq!(session.version(), delivered_version);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert!(settled.presentation_transitions().is_empty());
    assert_eq!(session.version(), delivered_version);
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
    assert_eq!(session.version(), delivered_version);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.manipulation_feedback().is_empty());
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
    assert!(!session.has_pending_presentation());
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
    assert_immediate_dock(&input_report);
    assert_eq!(input_report.before(), before);
    let delivered_version = session.version();
    assert!(input_report.take_painted_outputs().is_empty());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());

    let mut paint_frame = session.begin_host_frame().unwrap();
    let recordings = record_host_pointer_surfaces(&mut paint_frame);
    assert_no_pointer_preview(&recordings);
    let mut report = paint_frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(session.version(), delivered_version);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), B);
    let settled = settle_pointer_outputs(&mut session, &mut report, ResultKind::Presented);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert!(settled.presentation_transitions().is_empty());
    assert_eq!(session.version(), delivered_version);
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
    let unchanged = settle(&mut session);
    assert_eq!(session.version(), delivered_version);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.manipulation_feedback().is_empty());
    assert!(unchanged.surface_presentation_commits().is_empty());
    assert!(unchanged.presentation_transitions().is_empty());
    assert!(!session.has_active_gesture());
    assert!(session.disable_host_pointer().unwrap().is_some());
}
