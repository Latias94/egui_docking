//! Popup clicks cross the actual egui hit snapshot and native input reducer.

use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, RootId,
};
use dockspace::runtime::{
    DockPresentationConfig, DockspaceReceiverRole, DockspaceSession, HostInputOutcome,
    HostWindowToken, NativeCloseState, NativeCreateStrategy, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativePointerButton, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePaintProgramOperation, SurfacePaintProgramStep, SurfacePresentationResult,
    SurfaceUnavailableReason, TabStripControlKind,
};
use egui::{Id, Pos2, RawInput, Rect, Vec2};
use egui_dockspace::{DockStyle, native_support};

use super::tests::TestPanes;
use super::*;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the popup regression crosses measurement, real painting, presentation and native pointer delivery"
)]
fn tab_list_menu_rows_route_native_click_and_drag_after_ui_zoom() {
    for zoom in [1.0_f32, 1.2] {
        let (mut session, binding) = managed_session(zoom);
        let context = egui::Context::default();
        context.options_mut(|options| options.zoom_factor = zoom);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 480.0) / zoom);
        let input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let style = DockStyle {
            tab_min_width: 400.0,
            tab_max_width: 400.0,
            ..Default::default()
        };
        let mut panes = TestPanes;
        let mut store = NativeReceiverStore::default();
        let mut row_point = None;
        let mut frame_point = None;
        let mut backdrop_point = None;

        for menu_open in [false, true] {
            context
                .run_ui(input.clone(), |ui| {
                    let mut frame = session
                        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                        .unwrap();
                    native_support::measure_surface(
                        &mut frame,
                        SURFACE,
                        ui,
                        ui.max_rect(),
                        ui.max_rect(),
                        &panes,
                        &style,
                    )
                    .unwrap();
                    frame.commit().unwrap();
                })
                .drop_without_applying_deltas();

            let mut open = None;
            let mut painted = None;
            let mut output = None;
            let mut scroll_identities = Vec::new();
            context
                .run_ui(input.clone(), |ui| {
                    let mut frame = session
                        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                        .unwrap();
                    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
                    let mut saw_menu = false;
                    plan.visit_surface_program(|step| {
                        match step {
                            SurfacePaintProgramStep::Operation {
                                operation: SurfacePaintProgramOperation::TabStripControl(control),
                                ..
                            } if control.kind() == TabStripControlKind::TabListMenu
                                && !menu_open =>
                            {
                                open = plan.prepare_tab_strip_control_activation(control);
                            }
                            SurfacePaintProgramStep::PopupMenu { menu, .. } => {
                                saw_menu = true;
                                let bounds = menu.bounds();
                                frame_point = Some(
                                    LogicalPoint::new(bounds.x() + 1.0, bounds.y() + 1.0).unwrap(),
                                );
                                let row = menu.rows().find(|row| row.item() == SECOND).unwrap();
                                assert!(!row.selected());
                                let bounds = row.interaction_bounds().unwrap();
                                row_point = Some(
                                    LogicalPoint::new(
                                        bounds.x() + bounds.width() * 0.5,
                                        bounds.y() + bounds.height() * 0.5,
                                    )
                                    .unwrap(),
                                );
                            }
                            SurfacePaintProgramStep::PopupBackdrop(backdrop) => {
                                let bounds = backdrop.bounds();
                                backdrop_point = Some(
                                    LogicalPoint::new(bounds.x() + 1.0, bounds.y() + 1.0).unwrap(),
                                );
                            }
                            _ => {}
                        }
                        Ok::<(), ()>(())
                    })
                    .unwrap();
                    assert_eq!(saw_menu, menu_open);
                    let mut register_scroll = |ui: &egui::Ui, rect: Rect, id: Id| {
                        let identity = ui.register_scroll_hit_candidate(rect, id);
                        scroll_identities.push(identity);
                        (identity.id(), identity.layer_id())
                    };
                    let mut paint = native_support::paint_surface(
                        &mut frame,
                        Id::unique("native-menu-click"),
                        SURFACE,
                        ui,
                        &mut panes,
                        &style,
                        &mut register_scroll,
                    )
                    .unwrap();
                    assert!(paint.confirm_into(&mut frame).unwrap());
                    output = frame.commit().unwrap().take_painted_outputs().pop();
                    painted = Some(paint);
                })
                .drop_without_applying_deltas();
            let paint = painted.unwrap();
            let output = output.unwrap();
            let artifact = output.presentation_artifact();
            let hit_authority = context
                .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
                .unwrap();
            let receivers = paint.receivers().collect();
            let scroll_receivers = paint
                .scroll_receivers()
                .zip(scroll_identities)
                .map(|(receiver, identity)| PresentedScrollReceiver { receiver, identity })
                .collect();
            session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            let mut settled = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            settled
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            settled.commit().unwrap();
            store.presented.insert(
                SURFACE,
                vec![PresentedReceiverSet {
                    binding,
                    viewport: egui::ViewportId::ROOT,
                    hit_authority,
                    artifact,
                    receivers,
                    scroll_receivers,
                    focus_bindings: paint.pane_focus_bindings().to_vec(),
                }],
            );
            if !menu_open {
                let mut frame = session
                    .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                    .unwrap();
                frame.submit_surface_action(open.unwrap()).unwrap();
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                frame.commit().unwrap();
            }
        }

        let point = row_point.unwrap();
        let mut click_queried = false;
        let mut drag_queried = false;
        let mut click_role = None;
        let mut drag_role = None;
        for event in [
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
        ] {
            session
                .record_native_pointer(NativePointerInput::new(
                    NativePointerId::new(1),
                    event,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(
                            PhysicalPoint::new(
                                point.x() * f64::from(zoom),
                                point.y() * f64::from(zoom),
                            )
                            .unwrap(),
                        ),
                        NativePointerHover::Dock(binding),
                        None,
                    ),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                ))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|query| {
                    let answer = store.resolve(query);
                    let role = match answer {
                        NativeReceiverAnswer::Dock(receiver) => Some(receiver.descriptor().role()),
                        _ => None,
                    };
                    match query.purpose() {
                        NativeReceiverPurpose::ClickDelivery => {
                            click_queried = true;
                            click_role = role;
                        }
                        NativeReceiverPurpose::DragDelivery => {
                            drag_queried = true;
                            drag_role = role;
                        }
                        _ => {}
                    }
                    answer
                })
                .expect(
                    "actual popup receivers must agree with core for both native pointer lanes",
                );
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
        }
        assert!(click_queried && drag_queried);
        assert_eq!(click_role, Some(DockspaceReceiverRole::TabListMenuRow));
        assert_eq!(
            drag_role,
            Some(DockspaceReceiverRole::TabListMenuBlocker),
            "the popup frame must block the underlying pane's drag lane"
        );
        assert!(session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.view().item(FIRST).unwrap().is_selected());

        let presented = &store.presented[&SURFACE][0];
        for (point, expected) in [
            (
                frame_point.unwrap(),
                DockspaceReceiverRole::TabListMenuBlocker,
            ),
            (
                backdrop_point.unwrap(),
                DockspaceReceiverRole::TabListMenuBackdrop,
            ),
        ] {
            let hit = presented
                .hit_authority
                .hit_test(logical_pos(point).unwrap())
                .unwrap();
            for (lane, identity) in [("click", hit.click()), ("drag", hit.drag())] {
                let identity = identity.expect("popup chrome must block both pointer lanes");
                let mut receivers = presented.receivers.iter().filter(|receiver| {
                    receiver.widget_id() == identity.id()
                        && receiver.layer_id() == identity.layer_id()
                });
                assert_eq!(
                    receivers.next().map(|receiver| receiver.role()),
                    Some(expected),
                    "popup {expected:?} must own the {lane} lane at zoom {zoom}",
                );
                assert!(
                    receivers.next().is_none(),
                    "the actual hit must have one receiver"
                );
            }
        }

        context
            .run_ui(input.clone(), |ui| {
                let mut frame = session
                    .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                    .unwrap();
                native_support::measure_surface(
                    &mut frame,
                    SURFACE,
                    ui,
                    ui.max_rect(),
                    ui.max_rect(),
                    &panes,
                    &style,
                )
                .unwrap();
                frame.commit().unwrap();
            })
            .drop_without_applying_deltas();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .visit_surface_program(|step| {
                assert!(
                    !matches!(step, SurfacePaintProgramStep::PopupMenu { .. }),
                    "selection must close the popup"
                );
                Ok::<(), ()>(())
            })
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }
}

fn managed_session(zoom: f32) -> (DockspaceSession, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let presentation = DockPresentationConfig::builder()
        .tab_min_width(400.0)
        .tab_max_width(400.0)
        .build()
        .unwrap();
    let mut session = DockspaceSession::from_layout_with_presentation_config(
        layout,
        Default::default(),
        presentation,
    )
    .unwrap();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(104))
        .unwrap();
    let mut registration = session.begin_host_frame().unwrap();
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = registration.commit().unwrap();
    let binding = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let facts = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(f64::from(zoom)).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let mut snapshot = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    snapshot
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    snapshot.commit().unwrap();
    (session, binding)
}
