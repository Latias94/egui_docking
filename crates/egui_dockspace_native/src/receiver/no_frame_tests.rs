//! A physical NoFrame terminal revokes every placement in its framebuffer.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, RootId,
};
use dockspace::runtime::{
    DockspaceSession, HostWindowToken, NativeLocalPointerFact, NativeLocalPointerLocation,
    NativePointerButton, NativePointerEvent, NativePointerId, NativePointerInput,
    NativePointerOwner, NativePointerRoster, NativeSurfacePlacement, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use egui_dockspace::{DockStyle, native_support};
use winit::window::WindowId;

use super::tests::TestPanes;
use super::*;
use crate::coordinator::{
    NoFrameTerminal, PendingNoFrameInvalidation, defer_no_frame_invalidations,
};
use crate::mailbox::NativeHostBridge;
use crate::viewport_map::NativeViewportMap;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);

fn idle(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the regression paints a real two-surface hit graph, replaces it with application UI and commits one physical terminal"
)]
fn application_only_no_frame_retires_both_presented_placements_after_the_window_terminal() {
    let layout = DockspaceLayout::new([A, B].map(|surface| {
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(surface.get()),
                DockspaceNode::central_tabs([ItemId::new(surface.get())]),
            ),
        )
    }))
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, Default::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(A, HostWindowToken::new(1))
        .unwrap();
    idle(&mut session);
    let window = session.native_window_bindings().next().unwrap();
    let original = session
        .native_surface_bindings_for_window(window)
        .next()
        .unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                window,
                NativeWindowFacts::live()
                    .with_content_bounds(bounds)
                    .with_outer_bounds(bounds)
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .set_native_surface_placements(
            window,
            &[A, B].map(|surface| {
                let x = if surface == A { 0.0 } else { 320.0 };
                NativeSurfacePlacement::new(
                    surface,
                    LogicalSize::new(640.0, 480.0).unwrap(),
                    PhysicalPoint::new(0.0, 0.0).unwrap(),
                    scale,
                    PhysicalRect::new(x, 0.0, 320.0, 480.0).unwrap(),
                )
            }),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let bindings = session
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 2);
    assert!(!session.is_current_native_binding(original));
    let native_window = WindowId::from(1);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge
        .bind_viewport_route(egui::ViewportId::ROOT, native_window, original)
        .unwrap();
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        ..Default::default()
    };
    let mut panes = TestPanes;
    let style = DockStyle::default();
    let mut painted = Vec::new();
    let mut outputs = Vec::new();
    let mut old_b = None;
    context
        .run_ui(input.clone(), |ui| {
            painted.clear();
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            for surface in [A, B] {
                let x = if surface == A { 0.0_f32 } else { 320.0_f32 };
                let bounds = LogicalRect::new(f64::from(x), 0.0, 320.0, 480.0).unwrap();
                frame
                    .measure_surface(
                        surface,
                        UniformSurfaceMetrics::new(
                            bounds,
                            LogicalSize::new(32.0, 24.0).unwrap(),
                            80.0,
                        )
                        .unwrap(),
                    )
                    .unwrap();
                if surface == B {
                    old_b = Some(
                        frame
                            .paint_plan(B)
                            .unwrap()
                            .unwrap()
                            .tab_receiver(ItemId::new(2))
                            .unwrap(),
                    );
                }
                let mut child = ui.new_child(egui::UiBuilder::new().id_salt(surface).max_rect(
                    egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(320.0, 480.0)),
                ));
                let mut register_scroll = |ui: &egui::Ui, rect: egui::Rect, id: egui::Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    (identity.id(), identity.layer_id())
                };
                let mut paint = native_support::paint_surface(
                    &mut frame,
                    egui::Id::unique(surface),
                    surface,
                    &mut child,
                    &mut panes,
                    &style,
                    &egui_dockspace::DockInputConfig::default(),
                    &mut register_scroll,
                )
                .unwrap();
                assert!(paint.confirm_into(&mut frame).unwrap());
                painted.push((surface, paint));
            }
            outputs = frame.commit().unwrap().take_painted_outputs();
        })
        .drop_without_applying_deltas();
    let authority = Arc::new(
        context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .unwrap(),
    );
    let mut receivers = NativeReceiverStore::default();
    for output in &outputs {
        let binding = *bindings
            .iter()
            .find(|binding| binding.surface() == output.surface())
            .unwrap();
        let (_, paint) = painted
            .iter()
            .find(|(surface, _)| *surface == output.surface())
            .unwrap();
        assert!(output.matches_native_binding(binding));
        assert!(output.matches_semantic_output(paint.semantic_output().unwrap()));
        receivers.presented.insert(
            binding.surface(),
            vec![PresentedReceiverSet {
                binding,
                viewport: egui::ViewportId::ROOT,
                hit_authority: Arc::clone(&authority),
                pixels_per_point: f64::from(context.pixels_per_point()),
                artifact: output.presentation_artifact(),
                receivers: paint.receivers().collect(),
                scroll_receivers: Vec::new(),
                focus_bindings: paint.pane_focus_bindings().to_vec(),
            }],
        );
    }
    session
        .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
        .unwrap();
    idle(&mut session);
    assert!(
        [A, B]
            .into_iter()
            .all(|surface| session.presented_surface(surface).is_some())
    );
    assert!(
        bindings
            .iter()
            .all(|binding| receivers.references_binding(*binding))
    );

    context
        .run_ui(input, |ui| {
            let _ = ui.button("Application UI replaced the complete native framebuffer");
        })
        .drop_without_applying_deltas();
    // The terminal belongs to the physical route, whose old placement tag
    // deliberately predates this window's current two-placement roster.
    let invalidation = PendingNoFrameInvalidation {
        terminal: NoFrameTerminal::Observed,
        viewport: egui::ViewportId::ROOT,
        window: native_window,
        create_attempt: None,
        binding: original,
    };
    bridge
        .retain_no_frame(egui::ViewportId::ROOT, original, invalidation)
        .unwrap();
    receivers.retire_window(window);
    assert!(
        bindings
            .iter()
            .all(|binding| !receivers.references_binding(*binding))
    );
    let mut prepared = bridge.prepare_no_frame_invalidations(&session).unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| receivers.resolve(query))
        .unwrap();
    prepared.select_for_frame(&frame, true);
    assert_eq!(prepared.ready, [invalidation]);
    defer_no_frame_invalidations(&mut frame, &prepared.ready).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    bridge.finish_no_frame_invalidations(&prepared.ready);
    assert!(
        [A, B]
            .into_iter()
            .all(|surface| session.presented_surface(surface).is_none())
    );
    assert_eq!(bridge.pending_no_frame_count(), 0);

    let point = old_b.unwrap().center();
    let coordinates = session.native_local_coordinates(window).unwrap();
    let geometry = coordinates.geometry();
    let sample = coordinates
        .capture(
            window,
            NativeLocalPointerFact::new(
                PhysicalPoint::new(point.x(), point.y()).unwrap(),
                geometry.content_size(),
                geometry.native_scale_factor(),
            ),
            geometry,
        )
        .unwrap();
    let version = session.version();
    session
        .record_native_pointer(NativePointerInput::new_local(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeLocalPointerLocation::Exact(sample),
            NativePointerOwner::Native(window),
            NativePointerOwner::Native(window),
        ))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|query| receivers.resolve(query))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert_eq!(session.version(), version);
    assert!(
        !session.has_active_gesture(),
        "the application's framebuffer cannot inherit B's old hit graph"
    );
}
