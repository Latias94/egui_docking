use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    HostFrameReport, HostInputOutcome, NativeCloseState, NativeHostCapabilities,
    NativeWindowInputState, NativeWindowPresentationState, SurfacePaintProgramOperation,
    SurfacePaintProgramStep, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use eframe::{NativeViewportVisibilityStatus, egui};
use winit::event::WindowEvent;

use super::*;
use crate::viewport_callback::NativeViewportVisibilityRecord;

mod budget;
mod callback_prefix;
mod close;
mod combined_soak;
mod dispatch_terminal;
mod docking_enabled;
mod first_live_cancellation;
mod focus;
mod held_close;
mod ingress_create;
mod input_capacity;
mod input_control;
mod local_pointer;
mod overload;
mod owner_storage;
mod preparation_revocation;
mod replacement;
mod retirement;
mod storage;
mod touch;
mod wheel;
mod work_area;
mod workspace_replacement;

const FIRST_SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);

fn test_budget() -> NativeInputBudget {
    NativeInputBudget::new(&dockspace::runtime::NativeMonotonicClock::new(|| {
        std::time::Duration::ZERO
    }))
}

fn test_managed_capabilities() -> NativeHostCapabilities {
    crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows)
}

fn native_rect() -> NativePhysicalRect {
    NativePhysicalRect::new(100, 200, 800, 600)
}

fn ready_window_facts(x: f64) -> NativeWindowFacts {
    let content = PhysicalRect::new(x, 0.0, 640.0, 480.0).expect("test content bounds validate");
    let outer =
        PhysicalRect::new(x - 8.0, -30.0, 656.0, 518.0).expect("test outer bounds validate");
    let scale = ScaleFactor::new(1.0).expect("test scale validates");
    NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(outer)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
        .with_close(NativeCloseState::Clear, None)
}

#[test]
fn deferred_viewport_request_preserves_integral_physical_pixels() {
    let exact = PhysicalRect::new(-120.0, 48.0, 800.0, 600.0).expect("test rect is valid");
    let rect = native_create_rect(exact).expect("integral physical rect is accepted");

    assert_eq!(rect.x(), -120);
    assert_eq!(rect.y(), 48);
    assert_eq!(rect.width(), 800);
    assert_eq!(rect.height(), 600);
}

#[test]
fn deferred_viewport_request_quantizes_position_and_expands_fractional_extents() {
    let fractional = PhysicalRect::new(-120.375, -30.625, 420.25, 320.75).unwrap();
    let rect = native_create_rect(fractional).unwrap();
    assert_eq!(rect, NativePhysicalRect::new(-120, -31, 421, 321));
    let subpixel = PhysicalRect::new(10.5, 20.125, 0.125, 0.25).unwrap();
    assert_eq!(
        native_create_rect(subpixel),
        Some(NativePhysicalRect::new(11, 20, 1, 1))
    );
}

#[test]
fn deferred_viewport_request_rejects_empty_and_out_of_range_geometry() {
    let empty = PhysicalRect::new(10.0, 20.0, 0.0, 600.0).expect("empty rect is representable");
    assert!(native_create_rect(empty).is_none());
    for rect in [
        PhysicalRect::new(f64::from(i32::MAX) + 0.5, 0.0, 10.0, 10.0).unwrap(),
        PhysicalRect::new(f64::from(i32::MIN) - 0.5, 0.0, 10.0, 10.0).unwrap(),
        PhysicalRect::new(0.0, 0.0, f64::from(u32::MAX) + 0.25, 10.0).unwrap(),
        PhysicalRect::new(0.0, 0.0, 10.0, f64::from(u32::MAX) + 0.25).unwrap(),
    ] {
        assert!(native_create_rect(rect).is_none(), "{rect:?}");
    }
}

#[test]
fn no_frame_invalidation_requires_existing_presented_surface_authority() {
    assert!(!should_record_no_frame_invalidation(true, true, false));
    assert!(should_record_no_frame_invalidation(true, true, true));
    assert!(!should_record_no_frame_invalidation(false, true, true));
    assert!(!should_record_no_frame_invalidation(true, false, true));
}

#[test]
fn no_frame_invalidation_waits_for_terminal_and_rejects_stale_routes() {
    assert_eq!(
        no_frame_invalidation_readiness(true, true, true),
        NoFrameInvalidationReadiness::WaitingForTerminal
    );
    assert_eq!(
        no_frame_invalidation_readiness(false, true, true),
        NoFrameInvalidationReadiness::Ready
    );
    assert_eq!(
        no_frame_invalidation_readiness(false, false, true),
        NoFrameInvalidationReadiness::Stale
    );
    assert_eq!(
        no_frame_invalidation_readiness(false, true, false),
        NoFrameInvalidationReadiness::Stale
    );
}

#[test]
fn quarantine_preserves_a_terminal_observed_no_frame_invalidation() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    let invalidation = PendingNoFrameInvalidation::terminal_observed_for_test(
        ViewportId::ROOT,
        WindowId::from(11),
        binding,
    );
    native
        .bridge
        .retain_no_frame(ViewportId::ROOT, binding, invalidation)
        .unwrap();

    let _ = native.quarantine_after_fatal();

    assert_eq!(
        native.bridge.no_frame_for_binding(binding),
        Some(invalidation),
        "fatal quarantine must not erase a physical NoFrame awaiting core invalidation"
    );
}

#[test]
fn shutdown_commits_terminal_no_frame_invalidation_before_clearing_it() {
    let mut native = coordinator();
    let (root_binding, child_binding) = register_root_and_child(&mut native);
    let child_viewport = ViewportId::from_hash_of("shutdown-no-frame-child");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root_binding)
        .expect("the root route binds");
    native
        .bind_viewport(child_viewport, WindowId::from(22), child_binding)
        .expect("the child route binds");
    let _ = paint_and_present_all_surfaces(&mut native);
    assert!(native.session.presented_surface(SECOND_SURFACE).is_some());

    let invalidation = PendingNoFrameInvalidation::terminal_observed_for_test(
        child_viewport,
        WindowId::from(22),
        child_binding,
    );
    native
        .bridge
        .retain_no_frame(child_viewport, child_binding, invalidation)
        .unwrap();
    let _ = native.quarantine_after_fatal();

    let advance = native.advance_shutdown_boundary(&egui::Context::default(), &mut test_budget());

    assert!(advance.errors.is_empty());
    assert!(
        native.bridge.pending_no_frame_count() == 0,
        "the obligation clears only after its core frame commits"
    );
    assert!(
        native.session.presented_surface(SECOND_SURFACE).is_none(),
        "physical NoFrame and receiver retirement must be joined by core authority invalidation"
    );
}

#[test]
fn native_effect_rejection_policy_retains_state_races_and_capacity_backpressure() {
    assert!(native_effect_submission_rejection_is_recoverable(
        NativeHostErrorKind::StaleBinding
    ));
    assert!(native_effect_submission_rejection_is_recoverable(
        NativeHostErrorKind::OperationConflict
    ));
    assert!(native_effect_submission_rejection_is_recoverable(
        NativeHostErrorKind::InputCapacity
    ));

    for fatal in [
        NativeHostErrorKind::NotEnabled,
        NativeHostErrorKind::AlreadyEnabled,
        NativeHostErrorKind::InvalidFacts,
        NativeHostErrorKind::Unsupported,
        NativeHostErrorKind::Internal,
    ] {
        assert!(!native_effect_submission_rejection_is_recoverable(fatal));
    }
}

fn coordinator() -> NativeCoordinator {
    NativeCoordinator::new(
        coordinator_session(),
        NativePointerRoster::Exact(Vec::new()),
    )
    .expect("managed coordinator enrolls")
}

fn coordinator_with_clock(clock: dockspace::runtime::NativeMonotonicClock) -> NativeCoordinator {
    NativeCoordinator::new_with_storage_allowance(
        coordinator_session(),
        NativePointerRoster::Exact(Vec::new()),
        None,
        None,
        None,
        Some(clock),
    )
    .expect("managed coordinator enrolls with the test clock")
}

fn coordinator_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        ),
    ])
    .expect("test layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default()).expect("test session initializes")
}

fn register_roots(
    coordinator: &mut NativeCoordinator,
) -> (NativeSurfaceBinding, NativeSurfaceBinding) {
    coordinator
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .expect("first root registration queues");
    coordinator
        .register_native_root(SECOND_SURFACE, HostWindowToken::new(22))
        .expect("second root registration queues");
    let mut frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    let report = frame.commit().expect("registration frame commits");
    let bindings: Vec<_> = report
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    let first = bindings
        .iter()
        .copied()
        .find(|binding| binding.surface() == FIRST_SURFACE)
        .expect("first binding emitted");
    let second = bindings
        .iter()
        .copied()
        .find(|binding| binding.surface() == SECOND_SURFACE)
        .expect("second binding emitted");
    (first, second)
}

fn register_root_and_child(
    coordinator: &mut NativeCoordinator,
) -> (NativeSurfaceBinding, NativeSurfaceBinding) {
    coordinator
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .expect("the recovery root registration queues");
    let mut root_frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the recovery root registration frame begins");
    root_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the recovery root registration settles every surface");
    let root_report = root_frame
        .commit()
        .expect("the recovery root registration commits");
    let root = root_report
        .inputs()
        .iter()
        .find_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding }
                if binding.surface() == FIRST_SURFACE =>
            {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the recovery root binding is published");

    coordinator
        .session
        .register_owned_native_child(SECOND_SURFACE, HostWindowToken::new(22), FIRST_SURFACE)
        .expect("the existing child bootstrap queues");
    let mut child_frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child bootstrap frame begins");
    child_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child bootstrap settles every surface");
    let child_report = child_frame.commit().expect("the child bootstrap commits");
    let child = child_report
        .inputs()
        .iter()
        .find_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding }
                if binding.surface() == SECOND_SURFACE =>
            {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the child binding is published");
    coordinator
        .report_snapshot(
            [
                (root, ready_window_facts(0.0)),
                (child, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .expect("the exact live root and child inventory records");
    let mut observed = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the live child inventory frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the live child inventory settles every surface");
    observed.commit().expect("the live child inventory commits");
    (root, child)
}

fn paint_and_present_all_surfaces(coordinator: &mut NativeCoordinator) -> HostFrameReport {
    paint_and_present_all_surfaces_with_artifacts(coordinator).0
}

fn paint_and_present_all_surfaces_with_artifacts(
    coordinator: &mut NativeCoordinator,
) -> (
    HostFrameReport,
    Vec<(
        NativeSurfaceBinding,
        dockspace::runtime::SurfacePresentationArtifact,
    )>,
) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
        80.0,
    )
    .expect("test metrics validate");
    let mut paint = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("native roster paint frame begins");
    let surfaces = paint.frame.surfaces();
    for surface in &surfaces {
        paint
            .frame
            .measure_surface(*surface, metrics)
            .expect("native roster surface measures in its paint update");
        let receipt = paint
            .frame
            .paint_plan(*surface)
            .expect("native roster paint plan resolves")
            .unwrap_or_else(|| panic!("surface {surface:?} must have a ready native paint plan"))
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("the complete native roster program executes");
        paint
            .frame
            .confirm_surface_painted(receipt)
            .expect("native roster surface paint confirms");
    }
    let mut paint_report = paint.commit().expect("native roster paint frame commits");
    assert!(paint_report.take_native_effects().is_empty());
    let outputs = paint_report.take_painted_outputs();
    assert_eq!(outputs.len(), surfaces.len());
    let mut artifacts = Vec::new();
    for output in outputs {
        let binding = coordinator
            .session
            .native_window_bindings()
            .flat_map(|window| {
                coordinator
                    .session
                    .native_surface_bindings_for_window(window)
            })
            .find(|binding| output.matches_native_binding(*binding))
            .expect("each painted output has its current exact window");
        artifacts.push((binding, output.presentation_artifact()));
        coordinator
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("native roster output presentation records");
    }

    let mut observed = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("native roster presentation frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("native roster presentation frame settles every surface");
    let report = observed
        .commit()
        .expect("native roster presentation frame commits");
    coordinator
        .settle_native_admissions(report.native_admissions())
        .expect("native roster admissions settle");
    (report, artifacts)
}

fn presented_local_coordinate_routes() -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
) {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    for (viewport, window, binding) in [
        (ViewportId::ROOT, WindowId::from(11), first),
        (
            ViewportId::from_hash_of("local-coordinate-peer"),
            WindowId::from(22),
            second,
        ),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    native
        .report_snapshot(
            [
                (first, ready_window_facts(0.0)),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    callback_prefix::commit_boundary(&mut native);
    let report = paint_and_present_all_surfaces(&mut native);
    assert!(native.queued_snapshot.is_none());
    assert!(!native.settle_host_frame_report(&report).unwrap());
    assert!(native.session.native_local_coordinates(first).is_some());
    assert!(native.session.native_local_coordinates(second).is_some());
    (native, first, second)
}

#[test]
fn local_route_coordinates_advance_only_after_commit_and_revoke_without_queued_snapshot() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let window = WindowId::from(11);
    let original = native.session.native_local_coordinates(first).unwrap();
    let original_presented = native.session.presented_surface(first.surface()).unwrap();
    let route_coordinates = |native: &NativeCoordinator| {
        native.bridge.with_viewports(|routes| {
            routes.local_coordinates_for_event(window, Some(ViewportId::ROOT))
        })
    };
    assert_eq!(route_coordinates(&native), Some(original));
    native.bridge.with_viewports(|routes| {
        assert_eq!(
            routes.local_coordinates_for_event(window, None),
            Some(original)
        );
        assert_eq!(
            routes.local_coordinates_for_event(
                window,
                Some(ViewportId::from_hash_of(
                    "foreign-local-coordinate-viewport"
                )),
            ),
            None,
        );
        assert_eq!(
            routes.local_coordinates_for_event(WindowId::from(99), None),
            None
        );
    });
    let bytes = native
        .bridge
        .with_viewports(|routes| routes.owned_data_capacity_bytes());

    native
        .report_snapshot(
            [
                (
                    first,
                    ready_window_facts(0.0)
                        .with_native_scale_factor(ScaleFactor::new(2.0).unwrap())
                        .with_presentation_scale_factor(ScaleFactor::new(2.0).unwrap()),
                ),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    native.refresh_local_coordinates();
    assert_eq!(route_coordinates(&native), Some(original));

    let mut dropped = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(dropped);
    native.refresh_local_coordinates();
    assert_eq!(
        native.session.native_local_coordinates(first),
        Some(original)
    );
    assert_eq!(route_coordinates(&native), Some(original));
    assert_eq!(
        native.session.presented_surface(first.surface()),
        Some(original_presented),
        "a dropped geometry candidate cannot revoke the published placement",
    );

    let mut retry = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = retry.commit().unwrap();
    let updated = native.session.native_local_coordinates(first).unwrap();
    assert_ne!(updated, original);
    assert!(native.session.presented_surface(first.surface()).is_none());
    assert_eq!(route_coordinates(&native), Some(original));
    assert!(native.queued_snapshot.is_none());
    assert!(!native.settle_host_frame_report(&report).unwrap());
    assert_eq!(route_coordinates(&native), Some(updated));

    native
        .report_snapshot(
            [
                (first, NativeWindowFacts::live()),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    let mut revoked = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    revoked
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = revoked.commit().unwrap();
    assert!(native.session.native_local_coordinates(first).is_none());
    assert_eq!(route_coordinates(&native), Some(updated));
    assert!(!native.settle_host_frame_report(&report).unwrap());
    assert_eq!(route_coordinates(&native), None);
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.owned_data_capacity_bytes()),
        bytes,
        "committed capability changes use only the existing inline route backing",
    );
}

#[test]
fn local_route_coordinates_are_cleared_by_exact_suppression_and_retired_replacement() {
    let (native, first, second) = presented_local_coordinate_routes();
    let (replacement, successor, _) = presented_local_coordinate_routes();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    routes.bind(viewport, window, first).unwrap();
    assert_eq!(routes.local_coordinates_for_event(window, None), None);
    routes.refresh_local_coordinates(&native.session);
    let original = native.session.native_local_coordinates(first).unwrap();
    assert_eq!(
        routes.local_coordinates_for_event(window, None),
        Some(original)
    );
    assert!(!routes.suppress_pointer_route(viewport, second));
    assert_eq!(
        routes.local_coordinates_for_event(window, None),
        Some(original)
    );
    assert!(routes.suppress_pointer_route(viewport, first));
    routes.refresh_local_coordinates(&native.session);
    assert_eq!(routes.pointer_binding_for_event(window, None), None);
    assert_eq!(routes.local_coordinates_for_event(window, None), None);
    drop(routes.remove_viewport(viewport, first).unwrap());

    for successor_window in [window, WindowId::from(33)] {
        routes.bind(viewport, window, first).unwrap();
        routes.refresh_local_coordinates(&native.session);
        assert_eq!(
            routes.local_coordinates_for_event(window, None),
            Some(original)
        );
        assert!(routes.suppress_pointer_route(viewport, first));
        let displaced = routes
            .reserve_retired_replacement(viewport, first, successor)
            .unwrap();
        drop(displaced);
        routes
            .attach_output(viewport, successor, successor_window, None)
            .unwrap();
        assert_eq!(
            routes.pointer_binding_for_event(successor_window, None),
            Some(successor.window())
        );
        assert_eq!(
            routes.local_coordinates_for_event(successor_window, None),
            None
        );
        assert!(!routes.suppress_pointer_route(viewport, first));
        routes.refresh_local_coordinates(&native.session);
        assert_eq!(
            routes.local_coordinates_for_event(successor_window, None),
            None
        );
        routes.refresh_local_coordinates(&replacement.session);
        let current = replacement
            .session
            .native_local_coordinates(successor)
            .unwrap();
        assert_eq!(
            routes.local_coordinates_for_event(successor_window, None),
            Some(current)
        );
        routes.refresh_local_coordinates(&native.session);
        assert_eq!(
            routes.local_coordinates_for_event(successor_window, None),
            None
        );
        drop(routes.remove_viewport(viewport, successor).unwrap());
        assert_eq!(
            routes.local_coordinates_for_event(successor_window, None),
            None
        );
        assert_eq!(routes.owned_data_capacity_bytes(), bytes);
    }
}

fn live_roster(
    bindings: impl IntoIterator<Item = NativeSurfaceBinding>,
) -> NativeViewportRosterRecord {
    NativeViewportRosterRecord::for_test(
        bindings.into_iter().map(|binding| {
            crate::window_snapshot::CompiledWindowObservation::new(
                binding,
                NativeWindowFacts::live(),
            )
        }),
        true,
    )
}

fn dispatch_pointer_passthrough(
    coordinator: &mut NativeCoordinator,
) -> NativeViewportPointerPassthroughCommandToken {
    let command = coordinator
        .pointer_passthrough_command()
        .expect("one pointer pass-through command is ready to dispatch");
    let context = egui::Context::default();
    let token = eframe::queue_native_viewport_pointer_passthrough(
        &context,
        command.viewport(),
        command.enabled(),
    );
    assert!(coordinator.mark_pointer_passthrough_dispatched(command, token));
    token
}

fn pointer_passthrough_token(
    viewport: ViewportId,
    enabled: bool,
) -> NativeViewportPointerPassthroughCommandToken {
    eframe::queue_native_viewport_pointer_passthrough(&egui::Context::default(), viewport, enabled)
}

#[test]
fn coordinator_owns_registration_and_viewport_identity() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("second-native-surface");

    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, WindowId::from(22), second)
        .expect("child viewport binds");

    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
    assert_eq!(native.surface_viewport(SECOND_SURFACE), Some(child));
}

#[test]
fn core_active_gesture_prevents_idle_cursor_prefix_coalescing() {
    use dockspace::geometry::PhysicalPoint;
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerOwner,
    };

    let mut native = coordinator();
    let (binding, second) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, binding)
        .expect("the root viewport binds");
    native
        .report_snapshot(
            [
                (binding, ready_window_facts(0.0)),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .expect("the exact native window facts record");
    let mut snapshot = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native snapshot frame begins");
    snapshot
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the native snapshot frame settles every surface");
    snapshot
        .commit()
        .expect("the native snapshot frame commits");
    let (_, artifacts) = paint_and_present_all_surfaces_with_artifacts(&mut native);
    let artifact = artifacts
        .iter()
        .find(|(source, _)| *source == binding)
        .map(|(_, artifact)| *artifact)
        .expect("the source retains its actual painted artifact");

    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
        80.0,
    )
    .expect("test metrics validate");
    let mut inspection = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the source inspection frame begins");
    let plan = inspection
        .frame
        .paint_plan(FIRST_SURFACE)
        .expect("the source inspection plan resolves")
        .expect("the first surface remains paintable");
    let mut tab = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::Operation {
            operation: SurfacePaintProgramOperation::Tab(record),
            ..
        } = step
        {
            assert!(tab.replace(record).is_none());
        }
        Ok::<(), ()>(())
    })
    .expect("the source inspection paint program resolves");
    let tab = tab.expect("the first surface exposes its tab");
    let receiver = plan
        .receiver_for_tab_body(tab)
        .expect("the tab has an exact drag receiver");
    let point = receiver.center();
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source inspection retains every presented surface");
    inspection.commit().expect("source inspection commits");

    // Use the existing desktop domain without changing the idle adapter translator.
    native
        .record_pointer(NativePointerInput::new(
            NativePointerId::new(100),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(
                    PhysicalPoint::new(point.x(), point.y())
                        .expect("the unit-scale source desktop point validates"),
                ),
                NativePointerHover::Dock(binding.window()),
                None,
            ),
            NativePointerOwner::Native(binding.window()),
            NativePointerOwner::Native(binding.window()),
        ))
        .expect("the native press enters the active desktop journal");
    let mut gesture = native
        .begin_host_frame(|query| match query {
            NativeReceiverQuery::WindowHit(query) if query.window() == binding.window() => {
                NativeReceiverAnswer::WindowHit(dockspace::runtime::NativeWindowHit::Surface(
                    artifact,
                ))
            }
            NativeReceiverQuery::WindowHit(_) => NativeReceiverAnswer::Unknown,
            NativeReceiverQuery::Surface(query) => query
                .bind_receiver(&receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock),
        })
        .expect("the native gesture frame begins from the presented source");
    gesture
        .frame
        .measure_surface(FIRST_SURFACE, metrics)
        .expect("the active gesture surface measures");
    gesture
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the other surface is retained");
    gesture.commit().expect("the native gesture frame commits");
    assert!(native.session.has_active_gesture());
    assert!(
        native.pointer_translator.can_coalesce_idle_cursor_moves(),
        "only the active core gesture, not adapter button state, must prevent coalescing"
    );

    for (ordinal, x) in [(41, 20.0), (42, 36.0)] {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                window,
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: winit::event::DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(x, 20.0),
                    facts: winit::event::PointerEventFacts::default(),
                },
            )));
    }

    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the active cursor prefix reduces")
            .expect("both retained records report progress")
            .get(),
        2,
        "an active core gesture must retain every cursor edge instead of coalescing"
    );
    assert_eq!(
        native
            .next_window_event()
            .expect("the raw cursor prefix was transferred")
            .map(|record| record.ordinal()),
        None,
        "the semantic recorder owns both cursor records in the retained prefix"
    );

    let mut first_boundary = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first cursor boundary begins");
    first_boundary
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the first cursor boundary settles every surface");
    first_boundary
        .commit()
        .expect("the first cursor boundary commits");

    assert!(
        !native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the retained cursor prefix is complete")
            .is_some()
    );
    assert!(
        native
            .next_window_event()
            .expect("the cursor prefix remains readable")
            .is_none(),
        "the committed prefix has no raw replay payload"
    );
}

#[test]
fn native_admission_releases_the_exact_hidden_render_lease() {
    let mut native = coordinator();
    assert_eq!(
        native.lifecycle_progress(),
        NativeLifecycleProgress::default()
    );
    let (_, binding) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("first-live-hidden-render");

    native
        .bridge
        .reserve_viewport_route(child, binding)
        .expect("the deferred viewport reserves its exact binding");
    assert!(
        native.bridge.reserve_create(child, binding, native_rect()),
        "create retention installs the hidden-render lease"
    );
    assert!(
        native
            .native_host_handler()
            .render_hidden_deferred_viewport(child)
    );

    assert!(
        native
            .settle_native_admissions(&[binding])
            .expect("the exact admitted route settles")
    );
    assert_eq!(native.lifecycle_progress().first_live_admissions(), 1);
    assert!(
        !native
            .native_host_handler()
            .render_hidden_deferred_viewport(child),
        "first-live admission retires staging-only hidden rendering"
    );
    assert!(
        !native
            .settle_native_admissions(&[binding])
            .expect("duplicate settlement is idempotent")
    );
    assert_eq!(native.lifecycle_progress().first_live_admissions(), 1);
}

#[test]
fn viewport_mapping_rejects_aliasing_without_mutation() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, WindowId::from(22), second)
        .expect("child viewport binds");

    assert!(matches!(
        native.bind_viewport(ViewportId::ROOT, WindowId::from(22), second),
        Err(NativeViewportBindingError::ViewportAlreadyBound {
            viewport: ViewportId::ROOT,
            existing: FIRST_SURFACE,
        })
    ));
    assert!(matches!(
        native.bind_viewport(child, WindowId::from(11), first),
        Err(NativeViewportBindingError::ViewportAlreadyBound {
            viewport,
            existing: SECOND_SURFACE,
        }) if viewport == child
    ));
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
    assert_eq!(native.viewport_binding(child), Some(second));
}

#[test]
fn viewport_mapping_rejects_a_foreign_same_surface_binding() {
    let mut current_coordinator = coordinator();
    let (current, _) = register_roots(&mut current_coordinator);
    let mut foreign = coordinator();
    let (foreign_same_surface, _) = register_roots(&mut foreign);

    assert_eq!(current.surface(), foreign_same_surface.surface());
    assert_ne!(current, foreign_same_surface);
    assert!(matches!(
        current_coordinator.bind_viewport(
            ViewportId::ROOT,
            WindowId::from(11),
            foreign_same_surface,
        ),
        Err(NativeViewportBindingError::BindingNotCurrent {
            viewport: ViewportId::ROOT,
            surface: FIRST_SURFACE,
        })
    ));
    assert_eq!(current_coordinator.viewport_binding(ViewportId::ROOT), None);
}

#[test]
fn reserved_replacement_revokes_the_predecessor_window_route() {
    let mut native = coordinator();
    let (_, predecessor) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("replacement-native-surface");
    let predecessor_window = WindowId::from(22);
    native
        .bind_viewport(child, predecessor_window, predecessor)
        .expect("predecessor binds");

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    assert!(
        native
            .bridge
            .with_viewports_mut(|routes| { routes.suppress_pointer_route(child, predecessor) })
    );
    native
        .bridge
        .reserve_retired_viewport_replacement(child, predecessor, successor)
        .expect("successor reservation replaces the exact predecessor");

    native.bridge.with_viewports(|routes| {
        assert_eq!(routes.binding(child), Some(successor));
        assert_eq!(
            routes.binding_for_event(predecessor_window, Some(child)),
            None
        );
    });
}

#[test]
fn viewport_create_failure_freezes_binding_and_blocks_the_boundary() {
    let mut native = coordinator();
    let (_, failed_binding) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("failed-native-surface");
    native
        .bridge
        .reserve_viewport_route(child, failed_binding)
        .expect("failed viewport was reserved before scheduling");
    assert!(
        native
            .bridge
            .reserve_create_for_test(child, failed_binding, native_rect())
    );
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::RepaintRoot
    );
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::Wait,
        "duplicate backend retries retain one terminal callback record"
    );

    let failure = native
        .next_viewport_create_failure()
        .expect("failure prefix is readable")
        .expect("failed create is retained");
    assert_eq!(failure.viewport(), child);
    assert_eq!(failure.binding(), failed_binding);
    assert_eq!(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect_err("unacknowledged create failure blocks the host frame")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    assert!(matches!(
        native
            .bridge
            .with_viewports_mut(|routes| {
                assert!(routes.suppress_pointer_route(child, failed_binding));
                routes.reserve_retired_replacement(child, failed_binding, successor)
            }),
        Err(NativeViewportBindingError::BindingNotCurrent { viewport, .. })
            if viewport == child
    ));
    assert_eq!(failure.binding(), failed_binding);
    assert_eq!(native.viewport_binding(child), Some(failed_binding));

    native
        .acknowledge_viewport_create_failure(failure, true)
        .expect("exact frozen failure is acknowledged");
    assert_eq!(
        native.bridge.create_binding(child),
        None,
        "acknowledgement clears the exact failed create reservation"
    );
    assert_eq!(native.viewport_binding(child), None);
    assert!(
        native
            .next_viewport_create_failure()
            .expect("failure prefix remains readable")
            .is_none(),
        "duplicate failure callbacks are coalesced"
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("acknowledged failure releases the callback boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("host frame commits");
}

#[test]
fn shutdown_preserves_callback_error_when_reduction_fails() {
    let mut native = coordinator();
    let (_, failed_binding) = register_roots(&mut native);
    let failed_viewport = ViewportId::from_hash_of("shutdown-partial-create");
    native
        .bridge
        .reserve_viewport_route(failed_viewport, failed_binding)
        .expect("the test viewport route reserves");
    assert!(
        native
            .bridge
            .reserve_create_for_test(failed_viewport, failed_binding, native_rect(),)
    );
    assert_eq!(
        native
            .bridge
            .record_viewport_create_failure_for_test(failed_viewport),
        NativeHostWake::RepaintRoot
    );
    assert!(native.quarantine_after_fatal().is_empty());

    let advance = native.advance_shutdown_boundary(&egui::Context::default(), &mut test_budget());
    assert!(!advance.progress);
    assert!(!advance.commands_dispatched);
    assert!(advance.errors.iter().any(|error| {
        std::error::Error::source(error)
            .is_some_and(|source| source.to_string().contains("creation failure"))
    }));
}

#[test]
fn visibility_callbacks_preserve_order_and_ignore_unowned_requests() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("visibility-callback");
    let window = WindowId::from(22);
    native
        .bind_viewport(child, window, second)
        .expect("child viewport binds");

    let shown = NativeViewportVisibilityRecord::for_test(
        eframe::queue_native_viewport_visibility(&eframe::egui::Context::default(), child, true),
        child,
        window,
        second,
        true,
        NativeViewportVisibilityStatus::Dispatched,
    );
    let hidden = NativeViewportVisibilityRecord::for_test(
        eframe::queue_native_viewport_visibility(&eframe::egui::Context::default(), child, false),
        child,
        window,
        second,
        false,
        NativeViewportVisibilityStatus::Dispatched,
    );
    for callback in [shown, hidden, shown] {
        native
            .bridge
            .push_record(HostRecord::ViewportVisibility(callback));
    }
    assert!(native.bridge.references_binding(second));

    for _ in 0..3 {
        assert!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .expect("unowned visibility dispatch remains an inert exact callback")
                .is_some()
        );
        let mut boundary = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the visibility callback boundary begins");
        boundary
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the visibility callback boundary settles every surface");
        boundary
            .commit()
            .expect("the visibility callback boundary commits");
    }

    assert!(
        native.bridge.references_binding(second),
        "the live route retains its prepaid resource slot"
    );
    assert!(!native.bridge.has_pending_callback_records());
    assert_eq!(native.viewport_binding(child), Some(second));
    assert!(native.session.is_current_native_binding(first));
    assert!(native.session.is_current_native_binding(second));
}

#[test]
fn reduced_visibility_rearms_a_terminal_roster_after_fatal() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("visibility-terminal-roster");
    let window = WindowId::from(22);
    native
        .bind_viewport(child, window, second)
        .expect("child viewport binds");
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the initial exact roster is accepted")
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the initial roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the initial roster boundary settles every surface");
    let roster_report = roster_frame
        .commit()
        .expect("the initial roster boundary commits");
    assert!(
        native
            .settle_host_frame_inputs(roster_report.inputs())
            .expect("the exact roster snapshot settles")
    );
    assert!(!native.bridge.viewport_roster_dirty());

    native.bridge.push_record(HostRecord::ViewportVisibility(
        NativeViewportVisibilityRecord::for_test(
            eframe::queue_native_viewport_visibility(
                &eframe::egui::Context::default(),
                child,
                false,
            ),
            child,
            window,
            second,
            false,
            NativeViewportVisibilityStatus::Dispatched,
        ),
    ));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the visibility callback is reduced before the fatal boundary")
            .is_some()
    );
    assert!(native.bridge.viewport_roster_dirty());

    assert!(native.quarantine_after_fatal().is_empty());
    assert!(native.bridge.has_pending_coordinator_work());
}

#[test]
fn the_last_snapshot_outcome_settles_the_exact_queued_roster() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the exact roster is accepted")
    );
    let roster = native
        .queued_snapshot
        .as_ref()
        .expect("the reduced roster retains one exact snapshot")
        .roster
        .clone();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the snapshot boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the snapshot boundary settles every surface");
    frame
        .commit()
        .expect("the snapshot boundary commits before host settlement");

    let duplicate = [
        HostInputOutcome::NativePlatformSnapshotApplied {
            close_requests: Vec::new(),
        },
        HostInputOutcome::NativePlatformSnapshotApplied {
            close_requests: Vec::new(),
        },
    ];
    assert!(
        native
            .settle_host_frame_inputs(&duplicate)
            .expect("an earlier provider-baseline snapshot may share the host frame")
    );
    assert!(
        !native.bridge.settle_viewport_roster(&roster, true),
        "the last snapshot outcome must consume the exact queued roster envelope"
    );
}

#[test]
fn a_trailing_rejected_snapshot_does_not_commit_an_earlier_applied_outcome() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the exact roster is accepted")
    );
    let roster = native
        .queued_snapshot
        .as_ref()
        .expect("the reduced roster retains one exact snapshot")
        .roster
        .clone();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the snapshot boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the snapshot boundary settles every surface");
    frame
        .commit()
        .expect("the snapshot boundary commits before host settlement");

    let outcomes = [
        HostInputOutcome::NativePlatformSnapshotApplied {
            close_requests: Vec::new(),
        },
        HostInputOutcome::NativePlatformSnapshotStale,
    ];
    assert!(
        !native
            .settle_host_frame_inputs(&outcomes)
            .expect("the last snapshot terminal belongs to the queued roster")
    );
    assert!(
        !native.bridge.settle_viewport_roster(&roster, false),
        "a rejected last outcome must still consume the exact queued roster envelope"
    );
}

#[test]
fn settled_creation_acknowledgement_rearms_identical_roster_capture() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the acknowledgement roster is accepted")
    );
    native
        .queued_snapshot
        .as_mut()
        .expect("the reduced roster retains one exact snapshot")
        .acknowledgements
        .push(NativeSnapshotAcknowledgements {
            binding: second,
            create: true,
            preparation: None,
        });

    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the acknowledgement boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the acknowledgement boundary settles every surface");
    let report = frame
        .commit()
        .expect("the acknowledgement boundary commits");
    assert!(
        native
            .settle_host_frame_inputs(report.inputs())
            .expect("the exact native snapshot settles")
    );
    assert!(
        native.bridge.viewport_roster_dirty(),
        "clearing an applied acknowledgement must force a later provider capture"
    );
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the identical post-acknowledgement roster is accepted")
    );
}

#[test]
fn stale_failure_acknowledgement_cannot_clear_a_successor_create_reservation() {
    let mut native = coordinator();
    let (_, failed_binding) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("failed-native-surface-aba");
    native
        .bridge
        .reserve_viewport_route(child, failed_binding)
        .expect("failed viewport was reserved before scheduling");
    assert!(
        native
            .bridge
            .reserve_create(child, failed_binding, native_rect())
    );
    assert_eq!(
        native.bridge.record_viewport_create_failure_for_test(child),
        NativeHostWake::RepaintRoot
    );
    let failure = native
        .next_viewport_create_failure()
        .expect("failure prefix is readable")
        .expect("failed create is retained");

    let mut successor_source = coordinator();
    let (_, successor) = register_roots(&mut successor_source);
    assert!(
        native
            .bridge
            .with_viewports_mut(|routes| {
                assert!(routes.suppress_pointer_route(child, failed_binding));
                routes.reserve_retired_replacement(child, failed_binding, successor)
            })
            .is_err(),
        "an uncommitted create receipt cannot be overwritten by a successor"
    );
    native
        .acknowledge_viewport_create_failure(failure, true)
        .unwrap();
    // The replacement session makes this successor current before the
    // production create reservation can admit its exact binding.
    std::mem::swap(&mut native.session, &mut successor_source.session);
    native
        .bridge
        .reserve_viewport_route(child, successor)
        .unwrap();
    assert!(
        native
            .bridge
            .reserve_create(child, successor, native_rect()),
        "the successor may start only after the predecessor callback is retired"
    );
    assert_eq!(native.bridge.create_binding(child), Some(successor));
    assert_eq!(native.viewport_binding(child), Some(successor));
    assert!(
        native
            .acknowledge_viewport_create_failure(failure, true)
            .is_err()
    );
    assert_eq!(native.bridge.create_binding(child), Some(successor));
    assert_eq!(native.viewport_binding(child), Some(successor));
}

#[test]
fn output_reservation_binds_only_after_the_ui_callback_updates_the_viewport() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let mut reservation = OutputReservation::unbound_for_test();

    assert_eq!(reservation.binding(), None);
    assert!(reservation.attach(second));
    assert_eq!(reservation.binding(), Some(second));
    assert!(reservation.attach(second));
    assert!(!reservation.attach(first));
}

#[test]
fn staged_viewport_output_requires_an_admitted_create_attempt() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("staged-output");
    let window = WindowId::from(99);

    native
        .bridge
        .reserve_viewport_route(viewport, binding)
        .expect("the deferred viewport reserves its exact binding");

    native.bridge.with_viewports(|routes| {
        assert_eq!(
            routes.binding_for_event(window, Some(viewport)),
            None,
            "the reverse window route is intentionally absent before attachment"
        );
        assert_eq!(
            routes.binding_for_output(viewport, window, None),
            None,
            "a logical reservation alone cannot authorize a physical output"
        );
        assert_eq!(
            routes.binding_for_roster(viewport, window),
            None,
            "a root roster cannot retarget an unattached predecessor window"
        );
    });
}

#[test]
fn window_event_remains_pending_until_exact_acknowledgement() {
    let mut coordinator = coordinator();
    let (first, _) = register_roots(&mut coordinator);
    let window = WindowId::from(11);
    coordinator
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    let event = NativeWindowEventRecord::for_test(
        7,
        window,
        Some(ViewportId::ROOT),
        Some(first),
        WindowEvent::Focused(true),
    );
    coordinator
        .bridge
        .push_record(HostRecord::WindowEvent(event));

    let pending = coordinator
        .next_window_event()
        .expect("journal inspection succeeds")
        .expect("event remains visible");
    assert_eq!(pending.ordinal(), 7);
    assert_eq!(pending.binding(), Some(first));
    assert_eq!(
        coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect_err("unacknowledged event blocks the core boundary")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );
    assert_eq!(
        coordinator
            .acknowledge_window_event(8)
            .expect_err("wrong event identity is rejected")
            .kind(),
        crate::NativeRuntimeErrorKind::HostProtocol
    );
    assert_eq!(
        coordinator
            .next_window_event()
            .expect("journal remains readable")
            .map(|event| event.ordinal()),
        Some(7)
    );
    coordinator
        .acknowledge_window_event(7)
        .expect("exact event acknowledgement advances the journal");
    assert!(coordinator.bridge.callback_boundary_pending());

    let mut frame = coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("acknowledged journal permits the next core boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("frame commits");
    assert!(!coordinator.bridge.callback_boundary_pending());
}

#[test]
fn focus_callbacks_require_distinct_committed_barriers() {
    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            10,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::Focused(true),
        )));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            11,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::Focused(false),
        )));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the first inert event is acknowledged")
            .is_some()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert!(
        !native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("a second callback cannot join the pending boundary")
            .is_some()
    );
    assert_eq!(
        native
            .next_window_event()
            .expect("the next callback remains readable")
            .map(|event| event.ordinal()),
        Some(11)
    );

    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the acknowledged boundary can commit before the later event");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("first callback boundary commits");

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the later event becomes the next boundary")
            .is_some()
    );
    assert!(
        native
            .next_window_event()
            .expect("the second event was acknowledged")
            .is_none()
    );
}

#[test]
fn destroyed_event_is_published_only_with_the_next_complete_roster() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            12,
            first_window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::Destroyed,
        )));
    native.bridge.push_viewport_roster(live_roster([second]));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact destruction callback is retained")
            .is_some()
    );
    assert!(native.session.is_current_native_binding(first));
    let mut event_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destruction callback boundary begins");
    event_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    event_frame
        .commit()
        .expect("the destruction callback boundary commits");
    assert!(native.session.is_current_native_binding(first));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the next complete roster publishes the tombstone")
            .is_some()
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the exact roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    let report = roster_frame
        .commit()
        .expect("the exact roster with destruction commits");

    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.session.is_current_native_binding(second));
}

#[test]
fn close_requested_event_publishes_one_exact_close_edge() {
    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            13,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::CloseRequested,
        )));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact close request is retained")
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the close-request boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    let report = frame.commit().expect("the close-request boundary commits");

    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativeCloseObservationApplied { close_requests }
            if close_requests.len() == 1
    )));
}

#[test]
fn complete_root_roster_publishes_one_exact_platform_snapshot() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));

    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("complete roster is accepted")
    );
    assert!(native.bridge.callback_boundary_pending());

    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("accepted roster enters the next core boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    let report = frame.commit().expect("roster frame commits");

    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
}

#[test]
fn incomplete_root_roster_revokes_inventory_without_destroying_bindings() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native.bridge.push_viewport_roster(live_roster([first]));

    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("incomplete roster fails closed to unknown")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("unknown inventory still forms a valid boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("unknown inventory frame commits");

    assert!(native.session.is_current_native_binding(first));
    assert!(native.session.is_current_native_binding(second));
}

#[test]
fn invalid_root_window_facts_fail_closed_to_unknown() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let invalid = NativeViewportRosterRecord::for_test(
        [crate::window_snapshot::CompiledWindowObservation::new(
            first,
            NativeWindowFacts::live(),
        )],
        false,
    );
    native.bridge.push_viewport_roster(invalid);

    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("invalid facts revoke authority instead of poisoning the mailbox")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("unknown inventory enters a normal boundary");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host settles every surface");
    frame.commit().expect("unknown inventory frame commits");

    assert!(native.session.is_current_native_binding(first));
    assert!(native.session.is_current_native_binding(second));
}

#[test]
fn pointer_routes_are_frozen_before_viewport_replacement() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    use crate::event::NativePointerRouteSnapshot;

    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");

    let event = WindowEvent::MouseInput {
        primary_press: None,
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            geometry: None,
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(second_window),
            capture: PointerWindowRoute::Window(first_window),
        },
    };
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            7,
            first_window,
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    });

    let mut successor_source = coordinator();
    let (successor, _) = register_roots(&mut successor_source);
    assert!(
        native.bridge.with_viewports_mut(|routes| {
            routes.suppress_pointer_route(ViewportId::ROOT, first)
        })
    );
    native
        .bridge
        .reserve_retired_viewport_replacement(ViewportId::ROOT, first, successor)
        .expect("test viewport replacement succeeds");
    native
        .bridge
        .with_viewports_mut(|routes| {
            routes.attach_output(ViewportId::ROOT, successor, WindowId::from(33), None)
        })
        .unwrap();

    let routes = record.pointer_routes().expect("pointer routes are frozen");
    assert_eq!(
        routes.delivery(),
        NativePointerRouteSnapshot::Dock(first.window())
    );
    assert_eq!(
        routes.hover(),
        NativePointerRouteSnapshot::Dock(second.window())
    );
    assert_eq!(
        routes.capture(),
        NativePointerRouteSnapshot::Dock(first.window())
    );
    assert_ne!(
        routes.delivery(),
        NativePointerRouteSnapshot::Dock(successor.window())
    );
}

#[test]
fn pointer_translation_preserves_delivery_hover_and_capture() {
    use dockspace::geometry::PhysicalPoint;
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerOwner,
    };
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};

    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("second-native-surface");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");
    let event = WindowEvent::MouseInput {
        primary_press: None,
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            geometry: None,
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(second_window),
            capture: PointerWindowRoute::Window(first_window),
        },
    };
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            8,
            first_window,
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    });

    let mut translator = NativePointerTranslator::default();
    let actual = translator.translate(&record, |_| None);
    let expected = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(
                PhysicalPoint::new(110.0, 220.0).expect("test point validates"),
            ),
            NativePointerHover::Dock(second.window()),
            None,
        ),
        NativePointerOwner::Native(first.window()),
        NativePointerOwner::Native(first.window()),
    );
    assert_eq!(actual, NativePointerTranslation::Input(expected));
}

#[test]
fn pointer_reduction_acknowledges_only_after_core_acceptance() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
    };

    let mut native = coordinator();
    let (first, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, first)
        .expect("root viewport binds");
    let event = WindowEvent::MouseInput {
        primary_press: None,
        device_id: DeviceId::dummy(),
        state: ElementState::Pressed,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            geometry: None,
            surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
            desktop_position: Some(PhysicalPosition::new(10.0, 20.0)),
            modifiers: None,
            hover: PointerWindowRoute::Window(window),
            capture: PointerWindowRoute::Window(window),
        },
    };
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(9, window, Some(ViewportId::ROOT), event, routes)
    });
    native.bridge.push_record(HostRecord::WindowEvent(record));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("core accepts the current pointer binding")
            .is_some()
    );
    assert!(
        native
            .next_window_event()
            .expect("the accepted pointer event is acknowledged")
            .is_none()
    );
}

#[test]
fn only_unattached_unsubmitted_not_presented_outputs_need_adapter_abandonment() {
    assert!(output_terminal_requires_adapter_abandonment(
        NativeOutputStatus::NotPresented,
        false,
        false,
    ));
    assert!(!output_terminal_requires_adapter_abandonment(
        NativeOutputStatus::Presented,
        false,
        false,
    ));
    assert!(!output_terminal_requires_adapter_abandonment(
        NativeOutputStatus::NotPresented,
        true,
        false,
    ));
    assert!(!output_terminal_requires_adapter_abandonment(
        NativeOutputStatus::NotPresented,
        false,
        true,
    ));
}
