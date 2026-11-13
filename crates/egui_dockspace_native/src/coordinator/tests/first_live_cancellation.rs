//! Tokenless shutdown cancels content-ready children without moving source ownership.

use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId, NativeWindowPlacement};
use dockspace::runtime::PaintedSurfaceOutput;

use super::*;
use crate::window_snapshot::CompiledWindowObservation;
use crate::work_area::FrozenWorkAreaRoster;

fn item_location(
    native: &NativeCoordinator,
    item: ItemId,
) -> (
    SurfaceId,
    RootId,
    Option<(FloatingPresentationId, LogicalRect)>,
) {
    let view = native.session.view();
    let item = view.item(item).unwrap();
    (
        item.surface(),
        item.root(),
        item.contained()
            .map(|floating| (floating, view.contained(floating).unwrap().rect())),
    )
}

fn commit_model_frame(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    report
}

#[inline(never)]
fn present_one_host(native: &mut NativeCoordinator, surface: SurfaceId) {
    let mut measured = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measured
        .measure_surface(
            surface,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();
    let mut painted = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = painted
        .paint_plan(surface)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    painted.confirm_surface_painted(receipt).unwrap();
    painted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = painted.commit().unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    commit_model_frame(native);
}

fn prepare_child_content(
    native: &mut NativeCoordinator,
    root: RootId,
    x: f64,
    observations: &[(NativeSurfaceBinding, NativeWindowFacts)],
) -> NativeSurfaceBinding {
    let version = native.session.version();
    let items = native
        .session
        .view()
        .root(root)
        .unwrap()
        .content()
        .unwrap()
        .tabs()
        .unwrap()
        .items()
        .to_vec();
    let source_locations = items
        .into_iter()
        .map(|item| (item, item_location(native, item)))
        .collect::<Vec<_>>();
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            root,
            NativeWindowPlacement::new(PhysicalRect::new(x, 0.0, 640.0, 480.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let [create] = report.take_native_effects().try_into().unwrap();
    let child = match create.operation() {
        NativeEffectOperation::CreateWindow { binding, .. } => *binding,
        operation => panic!("expected the model create, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = create.accepted() else {
        panic!("the create supplies its exact acknowledgement");
    };
    native
        .session
        .report_managed_native_snapshot(
            observations.iter().copied().chain([(
                child,
                ready_window_facts(x).with_create_readiness(
                    dockspace::runtime::NativeCreateReadiness::Prepared,
                    Some(acknowledgement),
                ),
            )]),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit_model_frame(native);
    assert_eq!(native.session.version(), version);
    for (item, location) in source_locations {
        assert_eq!(
            item_location(native, item),
            location,
            "Prepared permits child content without moving the source item, root or rect"
        );
    }
    assert!(native.session.view().surface(child.surface()).is_none());
    assert!(native.session.presented_surface(child.surface()).is_none());
    assert!(native.session.is_current_native_binding(child));
    assert!(native.session.has_pending_native_create());
    child
}

fn content_ready_child() -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
) {
    let mut native = coordinator();
    let (host, source) = register_root_and_child(&mut native);
    for (binding, viewport, window) in [
        (host, ViewportId::ROOT, WindowId::from(11)),
        (source, viewport_id_for(source), WindowId::from(22)),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(policy).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    paint_and_present_all_surfaces(&mut native);

    let child = prepare_child_content(
        &mut native,
        RootId::new(2),
        1400.0,
        &[
            (host, ready_window_facts(0.0)),
            (source, ready_window_facts(720.0)),
        ],
    );
    native
        .bind_viewport(viewport_id_for(child), WindowId::from(33), child)
        .unwrap();
    assert_source_retained(&native, source, child);
    (native, host, source, child)
}

fn quarantine_without_source_measurement(native: &mut NativeCoordinator) {
    // Invalidate source measurements; ordinary cancellation needs no recovery geometry.
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration
        .replace_presentation_config(
            dockspace::runtime::DockPresentationConfig::builder()
                .tab_bar_height(31.0)
                .build()
                .unwrap(),
        )
        .unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    commit_model_frame(native);
    assert!(native.quarantine_after_fatal().is_empty());
}

#[test]
fn content_ready_child_preserves_bridge_paint_before_durable_ownership() {
    let (native, _, source, child) = content_ready_child();
    assert_source_retained(&native, source, child);
    assert!(native.session.view().surface(child.surface()).is_none());
    let current = native.session.is_current_native_binding(child);
    let recognized = native.session.recognizes_native_binding(child);
    assert!(current && recognized);
    for disposition in [
        DeferredViewportPaint::Semantic(child),
        DeferredViewportPaint::Retain(child),
    ] {
        assert_eq!(
            resolve_deferred_binding_paint(disposition, true, current, recognized),
            disposition,
            "a pending exact child must paint before ownership can transfer"
        );
        assert_eq!(
            resolve_deferred_binding_paint(disposition, false, current, recognized),
            DeferredViewportPaint::Waiting,
            "a foreign physical route cannot paint the pending child"
        );
        assert_eq!(
            resolve_deferred_binding_paint(disposition, true, false, recognized),
            DeferredViewportPaint::Retain(child),
            "a retired binding cannot emit new semantic content"
        );
        assert_eq!(
            resolve_deferred_binding_paint(disposition, true, false, false),
            DeferredViewportPaint::Waiting,
            "a forgotten lifetime cannot reuse the child's paint permission"
        );
    }
}

fn assert_source_retained(
    native: &NativeCoordinator,
    source: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
) {
    assert_eq!(
        item_location(native, ItemId::new(2)),
        (source.surface(), RootId::new(2), None)
    );
    assert_eq!(
        native
            .session
            .view()
            .surface(source.surface())
            .unwrap()
            .main_root()
            .unwrap()
            .id(),
        RootId::new(2)
    );
    assert_eq!(
        native.viewport_binding(viewport_id_for(source)),
        Some(source)
    );
    assert!(native.session.is_current_native_binding(source));
    assert!(native.session.view().surface(child.surface()).is_none());
    assert!(native.session.presented_surface(child.surface()).is_none());
    assert_eq!(native.lifecycle_progress().first_live_admissions(), 0);
    assert_eq!(native.bridge.pending_output_count(), 0);
}

#[test]
fn shutdown_cancels_content_ready_child_once_without_source_measurement() {
    let (mut native, _, source, child) = content_ready_child();
    quarantine_without_source_measurement(&mut native);
    let context = egui::Context::default();
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();

    let mut exhausted = test_budget();
    exhausted.complete_unsequenced_records(256);
    let deferred = native.advance_shutdown_boundary(&context, &mut exhausted);
    assert!(deferred.errors.is_empty(), "{deferred:#?}");
    assert!(!deferred.transition_source_exhausted);
    assert!(deferred.repaint_viewports.contains(&ViewportId::ROOT));
    assert!(native.session.has_pending_native_create());
    assert!(native.session.is_current_native_binding(child));
    assert!(
        native
            .bridge
            .retirement_can_begin_release(viewport_id_for(child), child)
    );
    assert_eq!(native.session.version(), version);
    assert_source_retained(&native, source, child);

    let mut budget = test_budget();
    budget.complete_unsequenced_records(255);
    let cancelled = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(cancelled.errors.is_empty(), "{cancelled:#?}");
    assert!(cancelled.progress);
    assert!(!cancelled.transition_source_exhausted);
    assert!(cancelled.repaint_viewports.contains(&ViewportId::ROOT));
    assert!(!cancelled.commands_dispatched);
    assert!(cancelled.abandoned_outputs.is_empty());
    assert_eq!(
        budget.remaining_records(),
        0,
        "one cancel spends one credit"
    );
    assert!(!native.session.has_pending_native_create());
    assert!(!native.session.is_current_native_binding(child));
    assert!(native.session.recognizes_native_binding(child));
    assert_eq!(native.viewport_binding(viewport_id_for(child)), Some(child));
    assert!(
        !native
            .bridge
            .retirement_can_begin_release(viewport_id_for(child), child)
    );
    assert_source_retained(&native, source, child);

    for _ in 0..2 {
        let mut budget = test_budget();
        let waiting = native.advance_shutdown_boundary(&context, &mut budget);
        assert!(waiting.errors.is_empty(), "{waiting:#?}");
        assert!(!waiting.transition_source_exhausted);
        assert!(!waiting.commands_dispatched);
        assert!(waiting.abandoned_outputs.is_empty());
        assert_eq!(budget.remaining_records(), 256);
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert_eq!(native.session.version(), version);
        assert!(!native.session.has_pending_native_create());
        assert!(native.session.recognizes_native_binding(child));
        assert_eq!(native.viewport_binding(viewport_id_for(child)), Some(child));
        assert!(!native.is_quiescent());
        assert_source_retained(&native, source, child);
    }
}

fn paint_child_output(
    native: &mut NativeCoordinator,
    child: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let mut painted = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(painted.surfaces().contains(&child.surface()));
    painted
        .measure_surface(
            child.surface(),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let mut panes = Vec::new();
    let receipt = painted
        .paint_plan(child.surface())
        .unwrap()
        .unwrap()
        .execute_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Pane(pane),
                ..
            } = step
            {
                panes.push((pane.root(), pane.selected()));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(panes, [(RootId::new(2), Some(ItemId::new(2)))]);
    painted.confirm_surface_painted(receipt).unwrap();
    painted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = painted.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert_eq!(output.surface(), child.surface());
    assert!(output.matches_native_binding(child));
    output
}

#[test]
fn native_lifecycle_events_wait_for_first_live_ownership_and_do_not_repeat() {
    use dockspace::runtime::DockspaceEventKind;

    let (mut native, _, source, child) = content_ready_child();
    let (sender, receiver) = std::sync::mpsc::channel();
    native.set_event_handler(move |events| sender.send(events.to_vec()).unwrap());
    let output = paint_child_output(&mut native, child);
    let pending = commit_model_frame(&mut native);
    assert!(pending.events().is_empty());
    native.settle_host_frame_report(&pending).unwrap();
    assert_source_retained(&native, source, child);
    assert!(receiver.try_recv().is_err());

    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(report.native_admissions(), &[child]);
    assert_eq!(
        report
            .events()
            .iter()
            .filter(|event| {
                matches!(event.kind(), DockspaceEventKind::RootRehomed {
                    root,
                    surface,
                    floating: None,
                } if *root == RootId::new(2) && *surface == child.surface())
            })
            .count(),
        1
    );
    native.settle_host_frame_report(&report).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), report.events());

    let settled = commit_model_frame(&mut native);
    assert!(settled.events().is_empty());
    native.settle_host_frame_report(&settled).unwrap();
    assert!(receiver.try_recv().is_err());
}

#[test]
fn shutdown_cancellation_does_not_notify_a_native_move_as_completed() {
    let (mut native, _, source, child) = content_ready_child();
    let (sender, receiver) = std::sync::mpsc::channel();
    native.set_event_handler(move |events| sender.send(events.to_vec()).unwrap());
    quarantine_without_source_measurement(&mut native);
    let cancelled = native.advance_shutdown_boundary(&egui::Context::default(), &mut test_budget());
    assert!(cancelled.errors.is_empty(), "{cancelled:#?}");
    assert!(!native.session.has_pending_native_create());
    assert_source_retained(&native, source, child);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn first_live_report_grants_presented_authority_without_changing_physical_coordinates() {
    let (mut native, _, source, child) = content_ready_child();
    let viewport = viewport_id_for(child);
    let window = WindowId::from(33);
    assert!(native.queued_snapshot.is_none());
    native.refresh_local_coordinates();
    let physical = native.session.native_local_coordinates(child).unwrap();
    assert!(native.session.presented_surface(child.surface()).is_none());
    assert_eq!(
        native.bridge.with_viewports(|routes| {
            routes.local_coordinates_for_event(window, Some(viewport))
        }),
        Some(physical),
    );

    let output = paint_child_output(&mut native, child);
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert_source_retained(&native, source, child);
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(report.native_admissions(), &[child]);
    let committed = native.session.native_local_coordinates(child).unwrap();
    assert_eq!(committed, physical);
    assert!(native.session.presented_surface(child.surface()).is_some());
    assert_eq!(committed.binding(), child.window());
    assert_eq!(
        native.bridge.with_viewports(|routes| {
            routes.local_coordinates_for_event(window, Some(viewport))
        }),
        Some(physical),
        "physical client coordinates do not grant the child's first-live surface authority",
    );
    assert!(native.queued_snapshot.is_none());
    assert!(!native.settle_host_frame_report(&report).unwrap());
    assert_eq!(
        native.bridge.with_viewports(|routes| {
            routes.local_coordinates_for_event(window, Some(viewport))
        }),
        Some(committed),
        "output-only first-live admission preserves the existing physical event route",
    );
    assert!(!native.settle_host_frame_report(&report).unwrap());
}

#[test]
fn shutdown_cancelled_child_cannot_admit_a_late_real_pane_output() {
    let (mut native, _, source, child) = content_ready_child();
    let version = native.session.version();
    let output = paint_child_output(&mut native, child);
    assert_source_retained(&native, source, child);
    assert_eq!(native.session.version(), version);
    assert!(native.quarantine_after_fatal().is_empty());

    let context = egui::Context::default();
    let mut budget = test_budget();
    let cancelled = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(cancelled.errors.is_empty(), "{cancelled:#?}");
    assert!(cancelled.progress);
    assert_eq!(budget.remaining_records(), 255);
    assert!(!native.session.has_pending_native_create());
    assert!(!native.session.is_current_native_binding(child));
    assert_source_retained(&native, source, child);

    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert_source_retained(&native, source, child);
    commit_model_frame(&mut native);
    assert_source_retained(&native, source, child);
    assert_eq!(native.session.version(), version);
    assert!(!native.session.has_pending_native_create());
    assert!(native.session.recognizes_native_binding(child));

    let mut budget = test_budget();
    let waiting = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(waiting.errors.is_empty(), "{waiting:#?}");
    assert!(!waiting.transition_source_exhausted);
    assert_eq!(budget.remaining_records(), 256);
    assert_source_retained(&native, source, child);
}

#[test]
fn shutdown_cancelled_child_drains_after_exact_destroyed_and_clean_roster() {
    let (mut native, host, source, child) = content_ready_child();
    quarantine_without_source_measurement(&mut native);
    let context = egui::Context::default();
    let version = native.session.version();
    let cancelled = native.advance_shutdown_boundary(&context, &mut test_budget());
    assert!(cancelled.errors.is_empty(), "{cancelled:#?}");
    assert!(cancelled.progress);
    assert!(!native.session.has_pending_native_create());
    assert_source_retained(&native, source, child);

    let waiting = native.advance_shutdown_boundary(&context, &mut test_budget());
    assert!(waiting.errors.is_empty(), "{waiting:#?}");
    assert!(!waiting.transition_source_exhausted);
    assert!(native.session.recognizes_native_binding(child));
    assert!(!native.is_quiescent());

    let viewport = viewport_id_for(child);
    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            41,
            WindowId::from(33),
            Some(viewport),
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.binding(), Some(child));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed.clone()));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let observed = native.advance_shutdown_boundary(&context, &mut test_budget());
    assert!(observed.errors.is_empty(), "{observed:#?}");
    assert!(observed.progress);
    assert!(!observed.transition_source_exhausted);
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    assert_eq!(native.viewport_binding(viewport), Some(child));
    let tombstones = native.bridge.retirement_destroyed_observations();
    assert_eq!(tombstones.len(), 1);
    assert_eq!(tombstones[0].0, child);

    // The adapter adds its receipt-derived tombstone to this clean live roster.
    native.bridge.push_terminal_viewport_roster(
        NativeViewportRosterRecord::for_test_with_backend_and_work_areas(
            [
                CompiledWindowObservation::new(host, ready_window_facts(0.0)),
                CompiledWindowObservation::new(source, ready_window_facts(720.0)),
            ],
            FrozenWorkAreaRoster::Unknown,
            eframe::NativeWindowingBackend::Windows,
        ),
    );
    let exhausted = (0..8).any(|_| {
        let advance = native.advance_shutdown_boundary(&context, &mut test_budget());
        assert!(advance.errors.is_empty(), "{advance:#?}");
        assert!(!advance.commands_dispatched);
        assert!(advance.abandoned_outputs.is_empty());
        assert_source_retained(&native, source, child);
        assert_eq!(native.session.version(), version);
        advance.transition_source_exhausted
    });
    assert!(exhausted, "exact Destroyed and clean facts must drain");
    assert_eq!(native.viewport_binding(viewport), None);
    assert!(!native.session.recognizes_native_binding(child));
    assert!(!native.bridge.references_binding(child));
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
    assert!(native.pending_effect_results.is_empty());
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    assert!(native.is_quiescent());

    for _ in 0..2 {
        let advance = native.advance_shutdown_boundary(&context, &mut test_budget());
        assert!(advance.errors.is_empty(), "{advance:#?}");
        assert!(advance.transition_source_exhausted);
        assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
        assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
        assert_source_retained(&native, source, child);
    }
}

#[test]
fn shutdown_cancels_content_ready_children_in_saga_order_one_per_boundary() {
    let contained_rect = LogicalRect::new(40.0, 40.0, 320.0, 240.0).unwrap();
    let layout = DockspaceLayout::new([(FIRST_SURFACE, 1, 3), (SECOND_SURFACE, 2, 4)].map(
        |(surface, main, child)| {
            DockspaceSurfaceLayout::new(
                surface,
                DockspaceRootLayout::new(
                    RootId::new(main),
                    DockspaceNode::central_tabs([ItemId::new(main)]),
                ),
            )
            .with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(child),
                DockspaceRootLayout::new(
                    RootId::new(child),
                    DockspaceNode::tabs([ItemId::new(child)]),
                ),
                contained_rect,
            ))
        },
    ))
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut native =
        NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new())).unwrap();
    let (first_host, second_host) = register_roots(&mut native);
    for (binding, viewport, window) in [
        (first_host, ViewportId::ROOT, WindowId::from(11)),
        (
            second_host,
            viewport_id_for(second_host),
            WindowId::from(22),
        ),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    let observations = [
        (first_host, ready_window_facts(0.0)),
        (second_host, ready_window_facts(720.0)),
    ];
    native
        .report_snapshot(
            observations,
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    commit_model_frame(&mut native);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(policy).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    paint_and_present_all_surfaces(&mut native);

    let first_child = prepare_child_content(&mut native, RootId::new(3), 1400.0, &observations);
    // Refresh only the second source; the first child still awaits its real output.
    present_one_host(&mut native, second_host.surface());
    let second_child = prepare_child_content(
        &mut native,
        RootId::new(4),
        2100.0,
        &[
            observations[0],
            observations[1],
            (
                first_child,
                ready_window_facts(1400.0).with_create_readiness(
                    dockspace::runtime::NativeCreateReadiness::Created,
                    None,
                ),
            ),
        ],
    );
    for (binding, window) in [
        (first_child, WindowId::from(33)),
        (second_child, WindowId::from(44)),
    ] {
        native
            .bind_viewport(viewport_id_for(binding), window, binding)
            .unwrap();
    }
    quarantine_without_source_measurement(&mut native);

    let source_locations = [
        (
            ItemId::new(3),
            (
                first_host.surface(),
                RootId::new(3),
                Some((FloatingPresentationId::new(3), contained_rect)),
            ),
        ),
        (
            ItemId::new(4),
            (
                second_host.surface(),
                RootId::new(4),
                Some((FloatingPresentationId::new(4), contained_rect)),
            ),
        ),
    ];
    let context = egui::Context::default();
    let version = native.session.version();
    for (index, child) in [first_child, second_child].into_iter().enumerate() {
        let mut budget = test_budget();
        let cancelled = native.advance_shutdown_boundary(&context, &mut budget);
        assert!(cancelled.errors.is_empty(), "{cancelled:#?}");
        assert!(cancelled.progress);
        assert!(!cancelled.transition_source_exhausted);
        assert!(cancelled.repaint_viewports.contains(&ViewportId::ROOT));
        assert!(!cancelled.commands_dispatched);
        assert!(cancelled.abandoned_outputs.is_empty());
        assert_eq!(budget.remaining_records(), 255);
        assert_eq!(native.session.version(), version);
        assert!(!native.session.is_current_native_binding(child));
        assert!(native.session.recognizes_native_binding(child));
        assert!(
            !native
                .bridge
                .retirement_can_begin_release(viewport_id_for(child), child)
        );
        assert_eq!(native.session.has_pending_native_create(), index == 0);
        assert_eq!(
            native.session.is_current_native_binding(second_child),
            index == 0,
            "the second saga must remain current until the second boundary"
        );
        for (item, location) in source_locations {
            assert_eq!(item_location(&native, item), location);
        }
        for binding in [first_child, second_child] {
            assert_eq!(
                native.viewport_binding(viewport_id_for(binding)),
                Some(binding)
            );
            assert!(native.session.view().surface(binding.surface()).is_none());
            assert!(
                native
                    .session
                    .presented_surface(binding.surface())
                    .is_none()
            );
        }
        assert_eq!(native.lifecycle_progress().first_live_admissions(), 0);
        assert_eq!(native.bridge.pending_output_count(), 0);
    }

    let mut budget = test_budget();
    let waiting = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(waiting.errors.is_empty(), "{waiting:#?}");
    assert!(!waiting.transition_source_exhausted);
    assert_eq!(budget.remaining_records(), 256);
    assert!(!native.session.has_pending_native_create());
    assert!(!native.is_quiescent());
    assert_eq!(native.session.version(), version);
    for (item, location) in source_locations {
        assert_eq!(item_location(&native, item), location);
    }
}
