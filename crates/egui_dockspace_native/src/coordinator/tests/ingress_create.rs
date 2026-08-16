use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use dockspace::runtime::{
    NativeEffectOperation, NativePointerRoster, NativeWindowInputState,
    NativeWindowPresentationState, SurfacePresentationResult, UniformSurfaceMetrics,
};
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute, WindowEvent,
};

use super::*;
use crate::work_area::FrozenWorkAreaRoster;

const SOURCE_SURFACE: SurfaceId = SurfaceId::new(41);
const SOURCE_ROOT: RootId = RootId::new(51);
const SOURCE_ITEM: ItemId = ItemId::new(61);

fn source_window() -> WindowId {
    WindowId::from(71)
}

fn tear_off_coordinator() -> NativeCoordinator {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SOURCE_SURFACE,
        DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::central_tabs([SOURCE_ITEM])),
    )])
    .expect("tear-off layout validates");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session =
        DockspaceSession::from_layout(layout, policy).expect("tear-off session initializes");
    NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new()))
        .expect("managed native coordinator enrolls")
}

fn ready_window_facts(bounds: PhysicalRect, scale: ScaleFactor) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

fn register_source(native: &mut NativeCoordinator) -> NativeSurfaceBinding {
    native
        .register_native_root(SOURCE_SURFACE, HostWindowToken::new(71))
        .expect("source root registration queues");
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("source registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source registration frame settles the surface");
    let report = frame.commit().expect("source registration frame commits");
    let binding = report
        .inputs()
        .iter()
        .find_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .expect("source registration publishes one binding");
    native
        .bind_viewport(ViewportId::ROOT, source_window(), binding)
        .expect("source viewport binds");
    binding
}

fn publish_desktop_authority(native: &mut NativeCoordinator, binding: NativeSurfaceBinding) {
    let scale = ScaleFactor::new(1.0).expect("test scale validates");
    let window_bounds =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("source window bounds validate");
    let display_bounds =
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate");
    let work_area = PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate");
    let roster = NativeViewportRosterRecord::for_test_with_work_areas(
        [crate::window_snapshot::CompiledWindowObservation::new(
            binding,
            ready_window_facts(window_bounds, scale),
        )],
        FrozenWorkAreaRoster::for_test_exact([(1, display_bounds, work_area, scale)]),
    );
    native
        .bridge
        .push_record(HostRecord::ViewportRoster(roster));
    assert!(
        native
            .reduce_callback_head()
            .expect("complete native roster reduces")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("native roster frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("native roster frame settles the surface");
    let report = frame.commit().expect("native roster frame commits");
    assert!(native.settle_host_frame_inputs(report.inputs()));
}

fn measure_and_present_source(
    native: &mut NativeCoordinator,
) -> dockspace::runtime::DockspaceReceiverDescriptor {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("logical bounds validate"),
        LogicalSize::new(0.0, 0.0).expect("logical minimum validates"),
        80.0,
    )
    .expect("uniform metrics validate");
    let mut measurement = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("measurement frame begins");
    measurement
        .frame
        .measure_surface(SOURCE_SURFACE, metrics)
        .expect("source surface measures");
    measurement
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("measurement frame settles the surface");
    measurement.commit().expect("measurement frame commits");

    let mut paint = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("paint frame begins");
    let plan = paint
        .frame
        .paint_plan(SOURCE_SURFACE)
        .expect("source paint plan lookup succeeds")
        .expect("source surface is ready");
    let descriptor = plan.tab_receiver(SOURCE_ITEM);
    paint
        .confirm_surface_painted(SOURCE_SURFACE)
        .expect("source surface paint confirms");
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("paint frame settles the surface");
    let mut report = paint.commit().expect("paint frame commits");
    assert!(report.take_native_effects().is_empty());
    let outputs = report.take_painted_outputs();
    let [output] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one source output, got {}", outputs.len())
    });
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("source output presentation records");
    let mut observed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("source presentation frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("source presentation frame settles the surface");
    let mut observed_report = observed
        .commit()
        .expect("source presentation frame commits");
    assert!(observed_report.take_native_effects().is_empty());
    descriptor.expect("source tab exposes a drag receiver")
}

fn push_window_event(native: &mut NativeCoordinator, ordinal: u64, event: WindowEvent) {
    let record = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            ordinal,
            source_window(),
            Some(ViewportId::ROOT),
            event,
            &mut viewports,
        )
    };
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert!(
        native
            .reduce_callback_head()
            .expect("native event reduces into one callback boundary")
    );
}

fn pointer_frame(
    native: &mut NativeCoordinator,
    receiver: dockspace::runtime::DockspaceReceiverDescriptor,
) -> dockspace::runtime::HostFrameReport {
    let mut frame = native
        .begin_host_frame(|query| {
            query
                .bind_receiver(&receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        })
        .expect("pointer callback frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("pointer callback frame settles the surface");
    frame.commit().expect("pointer callback frame commits")
}

fn pointer_facts(
    desktop: PhysicalPoint,
    hover: PointerWindowRoute,
    capture: PointerWindowRoute,
) -> PointerEventFacts {
    PointerEventFacts {
        surface_position: Some(PhysicalPosition::new(desktop.x(), desktop.y())),
        desktop_position: Some(PhysicalPosition::new(desktop.x(), desktop.y())),
        modifiers: None,
        hover,
        capture,
    }
}

fn request_native_child(
    native: &mut NativeCoordinator,
    receiver: dockspace::runtime::DockspaceReceiverDescriptor,
) -> NativeSurfaceBinding {
    let press = receiver.center();
    let press_physical = PhysicalPoint::new(press.x(), press.y())
        .expect("receiver center converts to desktop pixels at scale one");
    push_window_event(
        native,
        1,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: pointer_facts(
                press_physical,
                PointerWindowRoute::Window(source_window()),
                PointerWindowRoute::Window(source_window()),
            ),
        },
    );
    let mut press_report = pointer_frame(native, receiver);
    native
        .accept_native_effects(press_report.take_native_effects())
        .expect("press effects settle through the production coordinator");

    let outside = PhysicalPoint::new(1000.0, 700.0).expect("outside point validates");
    push_window_event(
        native,
        2,
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(outside.x(), outside.y()),
            facts: pointer_facts(
                outside,
                PointerWindowRoute::None,
                PointerWindowRoute::Window(source_window()),
            ),
        },
    );
    let mut move_report = pointer_frame(native, receiver);
    native
        .accept_native_effects(move_report.take_native_effects())
        .expect("move effects settle through the production coordinator");

    let mut preview = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("preview paint frame begins");
    let plan = preview
        .frame
        .paint_plan(SOURCE_SURFACE)
        .expect("preview paint plan lookup succeeds")
        .expect("source remains paintable while native creation is pending");
    assert!(
        plan.drag_preview().is_some(),
        "outside move publishes a preview"
    );
    preview
        .confirm_surface_painted(SOURCE_SURFACE)
        .expect("complete preview paint confirms");
    preview
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("preview paint frame settles the surface");
    let mut preview_report = preview.commit().expect("preview paint frame commits");
    assert!(preview_report.take_native_effects().is_empty());
    let outputs = preview_report.take_painted_outputs();
    let [preview_output] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one preview output, got {}", outputs.len())
    });
    native
        .session
        .report_surface_presentation(preview_output, SurfacePresentationResult::Presented)
        .expect("preview presentation records");
    let mut presented = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("preview presentation frame begins");
    presented
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("preview presentation frame settles the surface");
    let mut preview_presentation_report = presented
        .commit()
        .expect("preview presentation frame commits");
    native
        .accept_native_effects(preview_presentation_report.take_native_effects())
        .expect("preview effects settle through the production coordinator");

    push_window_event(
        native,
        3,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: pointer_facts(outside, PointerWindowRoute::None, PointerWindowRoute::None),
        },
    );
    let mut release_report = pointer_frame(native, receiver);
    let effects = release_report.take_native_effects();
    let create_bindings = effects
        .iter()
        .filter_map(|request| match request.operation() {
            NativeEffectOperation::CreateWindow { binding, .. } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(create_bindings.len(), 1, "one native child is requested");
    assert_ne!(create_bindings[0].surface(), SOURCE_SURFACE);
    native
        .accept_native_effects(effects)
        .expect("create effect enters the production deferred-viewport driver");
    assert_eq!(
        native.deferred_viewport_specs().len(),
        1,
        "the emitted create request is retained by the product coordinator"
    );

    create_bindings[0]
}

#[test]
fn window_event_outside_all_release_requests_create_without_transferring_source() {
    let mut native = tear_off_coordinator();
    let source_binding = register_source(&mut native);
    publish_desktop_authority(&mut native, source_binding);
    let receiver = measure_and_present_source(&mut native);
    let source_before = native
        .session
        .view()
        .item(SOURCE_ITEM)
        .expect("source item remains open before the drag");
    let source_surface = source_before.surface();
    let source_root = source_before.root();

    let child = request_native_child(&mut native, receiver);
    assert_ne!(child.surface(), source_surface);

    let source_after = native
        .session
        .view()
        .item(SOURCE_ITEM)
        .expect("source item stays reachable until the native lifecycle barrier");
    assert_eq!(source_after.surface(), source_surface);
    assert_eq!(source_after.root(), source_root);
}

#[test]
fn shutdown_cancels_unadmitted_create_without_declaring_a_window() {
    let mut native = tear_off_coordinator();
    let source_binding = register_source(&mut native);
    publish_desktop_authority(&mut native, source_binding);
    let receiver = measure_and_present_source(&mut native);
    let child = request_native_child(&mut native, receiver);
    let child_viewport = viewport_id_for(child);

    assert_eq!(native.viewport_binding(child_viewport), Some(child));
    assert!(native.effects.references_binding(child));
    assert!(native.bridge.references_binding(child));

    assert!(native.quarantine_after_fatal().is_empty());

    assert_eq!(native.viewport_binding(child_viewport), None);
    assert!(native.deferred_viewport_specs().is_empty());
    assert!(!native.effects.references_binding(child));
    assert!(!native.bridge.references_binding(child));

    let advance = native
        .advance_shutdown_boundary()
        .expect("the provider-stopped create result commits");
    assert!(advance.commands.is_empty());
    assert!(advance.abandoned_outputs.is_empty());
    assert!(!native.session.is_current_native_binding(child));
}

#[test]
fn observed_pre_admission_close_emits_and_accepts_compensating_close_without_output_token() {
    let mut native = tear_off_coordinator();
    let source_binding = register_source(&mut native);
    publish_desktop_authority(&mut native, source_binding);
    let receiver = measure_and_present_source(&mut native);
    let child = request_native_child(&mut native, receiver);

    native
        .report_snapshot(
            [
                (source_binding, NativeWindowFacts::live()),
                (child, NativeWindowFacts::live()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the pending child becomes an observed live native window");
    let mut observed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the observed child frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the observed child frame settles every surface");
    let mut observed_report = observed.commit().expect("the observed child frame commits");
    assert!(observed_report.take_native_effects().is_empty());

    native
        .publish_close(child, NativeCloseState::Requested, None)
        .expect("the pre-admission close edge records");
    let mut close = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the pre-admission close frame begins");
    close
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pre-admission close frame settles every surface");
    let mut close_report = close
        .commit()
        .expect("the pre-admission close frame commits");
    let effects = close_report.take_native_effects();
    assert!(
        matches!(
            effects.as_slice(),
            [request]
                if matches!(
                    request.operation(),
                    NativeEffectOperation::CompensatingClose { binding } if *binding == child
                )
        ),
        "one exact compensating close is emitted: {effects:#?}"
    );

    native
        .accept_native_effects(effects)
        .expect("the compensating close enters the exact retirement path");
    let viewport = viewport_id_for(child);
    assert_eq!(native.viewport_binding(viewport), Some(child));
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .all(|spec| spec.binding() != child),
        "the compensating close stops re-declaring the unadmitted child"
    );
    assert!(
        !native.retirements.can_begin_release(viewport, child),
        "the close acknowledgement is retained until an exact Destroyed callback"
    );
}
