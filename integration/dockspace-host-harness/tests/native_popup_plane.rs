//! Native popup receivers may extend beyond dock layout while remaining clipped.

use std::cell::Cell;

use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceSession,
    HostFrameReport, HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition,
    NativeHostCapabilities, NativeHostCapability, NativeLocalPointerFact,
    NativeLocalPointerLocation, NativePointerButton as Button, NativePointerEvent as Event,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativeReceiverAnswer, NativeReceiverQuery, NativeSurfaceBinding,
    NativeSurfacePlacement, NativeWindowFacts, NativeWindowHit, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, PaintedSurfaceOutput,
    SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Request,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationArtifact, SurfacePresentationResult, SurfaceUnavailableReason,
    TabListMenuMetrics, TabStripControlKind, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const THIRD: ItemId = ItemId::new(3);
const POINTER: NativePointerId = NativePointerId::new(17);

#[derive(Clone, Copy)]
enum Lane {
    Local,
    Desktop,
}

fn dock_bounds() -> LogicalRect {
    LogicalRect::new(40.0, 40.0, 160.0, 60.0).unwrap()
}

fn client_bounds() -> PhysicalRect {
    PhysicalRect::new(300.0, -200.0, 800.0, 600.0).unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    frame
        .measure_surface_with(SURFACE, |request| match request {
            Request::DockBounds { .. } => Answer::Bounds(dock_bounds()),
            Request::PopupPlaneBounds { .. } => {
                Answer::Bounds(LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
            }
            Request::PaneMinimum { .. } => {
                Answer::PaneMinimum(LogicalSize::new(16.0, 16.0).unwrap())
            }
            Request::TabIntrinsic { .. } => Answer::TabIntrinsic(96.0),
            Request::TabStrip { .. } => Answer::TabStrip(
                TabStripMetrics::new(0.0, 0.0)
                    .unwrap()
                    .with_controls(
                        TabStripControlMetrics::new(0.0)
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
                        TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).unwrap(),
                    ),
            ),
        })
        .unwrap();
}

fn toggle_menu(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut action = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabStripControl(control),
            ..
        } = step
            && control.kind() == TabStripControlKind::TabListMenu
        {
            action = Some(plan.prepare_tab_strip_control_activation(control).unwrap());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    finish(frame);

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.submit_surface_action(action.unwrap()).unwrap();
    measure(&mut frame);
    finish(frame);
}

struct PopupScene {
    artifact: SurfacePresentationArtifact,
    row: Option<DockspaceReceiverDescriptor>,
    frame: Option<DockspaceReceiverDescriptor>,
    backdrop: Option<DockspaceReceiverDescriptor>,
}

impl PopupScene {
    fn answer(&self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        let query = match query {
            NativeReceiverQuery::WindowHit(_) => {
                return NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(self.artifact));
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        assert!(query.matches_presentation_artifact(self.artifact));
        let point = query.point().unwrap();
        [self.row, self.frame, self.backdrop]
            .into_iter()
            .flatten()
            .filter(|receiver| receiver.bounds().contains(point))
            .find_map(|receiver| query.bind_receiver(&receiver))
            .map_or(
                NativeReceiverAnswer::EmptyLane(query.presented_surface()),
                NativeReceiverAnswer::Dock,
            )
    }
}

fn paint(session: &mut DockspaceSession) -> (PopupScene, PaintedSurfaceOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut row = None;
    let mut menu_frame = None;
    let mut backdrop = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::PopupMenu { menu, .. } => {
                let target = menu.rows().find(|row| row.item() == THIRD).unwrap();
                row = plan.receiver_for_tab_list_menu_row(target);
                menu_frame = plan.receiver_for_tab_list_menu_frame(menu);
            }
            Step::PopupBackdrop(record) => {
                backdrop = plan.receiver_for_tab_list_menu_backdrop(record);
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let [output]: [PaintedSurfaceOutput; 1] =
        finish(frame).take_painted_outputs().try_into().unwrap();
    (
        PopupScene {
            artifact: output.presentation_artifact(),
            row,
            frame: menu_frame,
            backdrop,
        },
        output,
    )
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(
        idle(session)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
}

fn fixture() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    PopupScene,
    PaintedSurfaceOutput,
) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs((1..=4).map(ItemId::new)),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::AuthoritativeButtonState)
                .with(NativeHostCapability::PointerHitTestObservation)
                .with(NativeHostCapability::DesktopPointerPosition)
                .with(NativeHostCapability::HoveredWindow),
        )
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let [binding]: [NativeSurfaceBinding; 1] =
        idle(&mut session).native_bindings().try_into().unwrap();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live()
                    .with_content_bounds(client_bounds())
                    .with_content_size(client_bounds().size())
                    .with_native_scale_factor(ScaleFactor::new(2.0).unwrap())
                    .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    finish(frame);
    toggle_menu(&mut session);
    let (scene, output) = paint(&mut session);
    let row = scene.row.unwrap();
    assert!(!dock_bounds().contains(row.center()));
    assert!(scene.backdrop.unwrap().bounds().contains(row.center()));
    (session, binding, scene, output)
}

fn click_inputs(
    session: &DockspaceSession,
    binding: NativeSurfaceBinding,
    point: LogicalPoint,
    lane: Lane,
) -> [NativePointerInput; 2] {
    [
        Event::ButtonPressed(Button::Primary),
        Event::ButtonReleased(Button::Primary),
    ]
    .map(|event| {
        let capture = if matches!(event, Event::ButtonReleased(_)) {
            NativePointerOwner::None
        } else {
            NativePointerOwner::Native(binding.window())
        };
        let delivery = NativePointerOwner::Native(binding.window());
        match lane {
            Lane::Local => {
                let coordinates = session.native_local_coordinates(binding.window()).unwrap();
                let geometry = coordinates.geometry();
                let sample = coordinates
                    .capture(
                        binding.window(),
                        NativeLocalPointerFact::new(
                            PhysicalPoint::new(point.x(), point.y()).unwrap(),
                            geometry.content_size(),
                            geometry.native_scale_factor(),
                        ),
                        geometry,
                    )
                    .unwrap();
                NativePointerInput::new_local(
                    POINTER,
                    event,
                    NativeLocalPointerLocation::Exact(sample),
                    delivery,
                    capture,
                )
            }
            Lane::Desktop => NativePointerInput::new(
                POINTER,
                event,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(
                        PhysicalPoint::new(
                            client_bounds().x() + point.x(),
                            client_bounds().y() + point.y(),
                        )
                        .unwrap(),
                    ),
                    NativePointerHover::Dock(binding.window()),
                    None,
                ),
                delivery,
                capture,
            ),
        }
    })
}

fn selects_popup_row(lane: Lane) {
    let (mut session, binding, scene, output) = fixture();
    present(&mut session, output);
    let inputs = click_inputs(&session, binding, scene.row.unwrap().center(), lane);
    session.record_native_pointer_batch(inputs).unwrap();
    let mut candidate = session
        .begin_native_host_frame(|query| scene.answer(query))
        .unwrap();
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(candidate);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    let report = finish(
        session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap(),
    );
    assert!(report.workspace_changed());
    assert!(session.view().item(THIRD).unwrap().is_selected());
    assert!(!report.native_input_pending());
}

#[test]
fn native_local_popup_row_outside_dock_bounds_uses_exact_presented_receivers() {
    selects_popup_row(Lane::Local);
}

#[test]
fn native_desktop_popup_row_outside_dock_bounds_uses_exact_presented_receivers() {
    selects_popup_row(Lane::Desktop);
}

#[test]
fn unpresented_popup_rows_do_not_authorize_native_input() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene, output) = fixture();
        session
            .record_native_pointer_batch(click_inputs(
                &session,
                binding,
                scene.row.unwrap().center(),
                lane,
            ))
            .unwrap();
        assert!(
            !finish(
                session
                    .begin_native_host_frame(|query| scene.answer(query))
                    .unwrap()
            )
            .workspace_changed()
        );
        assert!(session.view().item(FIRST).unwrap().is_selected());
        present(&mut session, output);
    }
}

#[test]
fn popup_rows_outside_the_physical_placement_clip_do_not_authorize_native_input() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene, output) = fixture();
        present(&mut session, output);
        let clip = PhysicalRect::new(0.0, 0.0, 800.0, 110.0).unwrap();
        let point = scene.row.unwrap().center();
        assert!(!clip.contains(PhysicalPoint::new(point.x(), point.y()).unwrap()));
        let placement = NativeSurfacePlacement::new(
            SURFACE,
            LogicalSize::new(800.0, 600.0).unwrap(),
            PhysicalPoint::new(0.0, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            clip,
        );
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .set_native_surface_placements(binding.window(), &[placement])
            .unwrap();
        measure(&mut frame);
        finish(frame);
        let (clipped, output) = paint(&mut session);
        present(&mut session, output);
        let current = session
            .native_surface_bindings_for_window(binding.window())
            .next()
            .unwrap();
        session
            .record_native_pointer_batch(click_inputs(&session, current, point, lane))
            .unwrap();
        let queries = Cell::new(0);
        let report = finish(
            session
                .begin_native_host_frame(|query| {
                    if matches!(query, NativeReceiverQuery::Surface(_)) {
                        queries.set(queries.get() + 1);
                    }
                    clipped.answer(query)
                })
                .unwrap(),
        );
        assert_eq!(queries.get(), 0);
        assert!(!report.workspace_changed());
        assert!(session.view().item(FIRST).unwrap().is_selected());
    }
}

#[test]
fn closed_popup_artifacts_cannot_restore_native_receivers_outside_dock_bounds() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene, output) = fixture();
        present(&mut session, output);
        toggle_menu(&mut session);
        let (_, output) = paint(&mut session);
        present(&mut session, output);
        session
            .record_native_pointer_batch(click_inputs(
                &session,
                binding,
                scene.row.unwrap().center(),
                lane,
            ))
            .unwrap();
        let report = finish(
            session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        assert!(session.view().item(FIRST).unwrap().is_selected());
    }
}

#[test]
fn active_popup_blank_space_is_a_backdrop_and_closed_popup_space_has_no_native_receiver() {
    for lane in [Lane::Local, Lane::Desktop] {
        let (mut session, binding, scene, output) = fixture();
        present(&mut session, output);
        let blank = LogicalPoint::new(700.0, 400.0).unwrap();
        assert!(!dock_bounds().contains(blank));
        assert!(!scene.frame.unwrap().bounds().contains(blank));
        let hit = session.host_pointer_hit(SURFACE, blank).unwrap();
        assert_eq!(
            hit.hover().unwrap().descriptor().role(),
            DockspaceReceiverRole::TabListMenuBackdrop
        );

        toggle_menu(&mut session);
        let (closed, output) = paint(&mut session);
        assert!(closed.backdrop.is_none());
        present(&mut session, output);
        session
            .record_native_pointer_batch(click_inputs(&session, binding, blank, lane))
            .unwrap();
        let queries = Cell::new(0);
        let report = finish(
            session
                .begin_native_host_frame(|query| {
                    if matches!(query, NativeReceiverQuery::Surface(_)) {
                        queries.set(queries.get() + 1);
                    }
                    closed.answer(query)
                })
                .unwrap(),
        );
        assert_eq!(queries.get(), 0);
        assert!(!report.workspace_changed());
        assert!(session.view().item(FIRST).unwrap().is_selected());
    }
}
