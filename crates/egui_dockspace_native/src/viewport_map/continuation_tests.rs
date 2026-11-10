//! Physical route continuation borrows exact placement authority from core.

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, HostFrameReport, HostWindowToken, NativePointerRoster,
    NativeSurfacePlacement, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, SurfaceUnavailableReason,
};

use super::*;

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn fixture() -> (
    DockspaceSession,
    NativeViewportMap,
    Vec<NativeSurfaceBinding>,
) {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let registration = finish(session.begin_host_frame().unwrap());
    let original = registration.native_bindings()[0];
    let mut routes = NativeViewportMap::default();
    routes
        .bind(ViewportId::ROOT, WindowId::from(91), original)
        .unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                original.window(),
                NativeWindowFacts::live()
                    .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    finish(session.begin_host_frame().unwrap());
    let placements = [1, 2].map(|id| {
        let x = (id - 1) as f64 * 400.0;
        NativeSurfacePlacement::new(
            SurfaceId::new(id),
            LogicalSize::new(350.0, 500.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            scale,
            PhysicalRect::new(x, 0.0, 350.0, 500.0).unwrap(),
        )
    });
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    finish(frame);
    let current = session
        .native_surface_bindings_for_window(original.window())
        .collect::<Vec<_>>();
    assert_eq!(current.len(), 2);
    assert!(current.iter().all(|binding| *binding != original));
    assert_eq!(routes.binding(ViewportId::ROOT), Some(original));
    (session, routes, current)
}

fn replacement<'a>(
    session: &'a mut DockspaceSession,
    current: &[NativeSurfaceBinding],
) -> DockspaceHostFrame<'a> {
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_native_layout_reset_with_roots(
            &reset,
            current.iter().map(|binding| (*binding, binding.surface())),
        )
        .unwrap();
    frame
}

#[test]
fn initial_registration_can_continue_before_a_route_or_committed_roster_exists() {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let continuations = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert_eq!(continuations.len(), 1);
    let predecessor = continuations[0].predecessor();
    assert_eq!(
        continuations[0].successor().surface(),
        predecessor.surface()
    );
    let mut routes = NativeViewportMap::default();
    assert_eq!(
        routes
            .workspace_root_continuation_viewport(continuations[0])
            .unwrap(),
        None
    );
    assert!(
        routes
            .validate_workspace_root_continuation_rosters(
                || continuations.iter().copied(),
                |_| std::iter::empty(),
            )
            .is_ok()
    );

    routes
        .bind(ViewportId::ROOT, WindowId::from(91), predecessor)
        .unwrap();
    assert!(
        routes
            .validate_workspace_root_continuation_rosters(
                || continuations.iter().copied(),
                |_| std::iter::empty(),
            )
            .is_err(),
        "an attached route still requires its complete committed predecessor roster"
    );
}

#[test]
fn changed_placement_generation_continues_one_window_with_its_complete_exact_roster() {
    let (mut session, mut routes, current) = fixture();
    let frame = replacement(&mut session, &current);
    let continuations = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert_eq!(continuations.len(), 2);
    for &continuation in &continuations {
        assert_eq!(
            routes
                .workspace_root_continuation_viewport(continuation)
                .unwrap(),
            Some(ViewportId::ROOT)
        );
    }
    let committed = |window| {
        current
            .iter()
            .copied()
            .filter(move |binding| binding.window() == window)
    };
    assert!(
        routes
            .validate_workspace_root_continuation_rosters(
                || continuations.iter().copied(),
                committed,
            )
            .is_ok()
    );
    assert!(
        routes
            .validate_workspace_root_continuation_rosters(
                || continuations[..1].iter().copied(),
                committed,
            )
            .is_err(),
        "physical equality cannot authorize a partial placement roster"
    );
    let report = finish(frame);
    assert_eq!(report.native_root_continuations(), continuations);
    let before = routes.owned_data_capacity_bytes();
    routes.commit_workspace_root_continuations(report.native_root_continuations());
    assert_eq!(routes.owned_data_capacity_bytes(), before);
    assert_eq!(routes.viewports.len(), 1);
    assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(91)));
    assert!(
        continuations
            .iter()
            .all(|continuation| routes.window_binding(ViewportId::ROOT)
                == Some(continuation.successor().window()))
    );
}

#[test]
fn continuation_rejects_another_window_domain_and_a_later_epoch_without_rebinding_the_route() {
    let (mut session, routes, current) = fixture();
    let (mut foreign, _, foreign_current) = fixture();
    let foreign_frame = replacement(&mut foreign, &foreign_current);
    for continuation in foreign_frame.proposed_native_root_continuations() {
        assert!(
            routes
                .workspace_root_continuation_viewport(continuation)
                .is_err()
        );
    }
    drop(foreign_frame);

    let first = finish(replacement(&mut session, &current));
    let successor_window = first.native_root_continuations()[0].successor().window();
    let successors = session
        .native_surface_bindings_for_window(successor_window)
        .collect::<Vec<_>>();
    let second = replacement(&mut session, &successors);
    for continuation in second.proposed_native_root_continuations() {
        assert_eq!(
            continuation.predecessor().window_token(),
            current[0].window_token()
        );
        assert_ne!(continuation.predecessor().window(), current[0].window());
        assert!(
            routes
                .workspace_root_continuation_viewport(continuation)
                .is_err()
        );
    }
    assert_eq!(
        routes.window_binding(ViewportId::ROOT),
        Some(current[0].window())
    );
}
