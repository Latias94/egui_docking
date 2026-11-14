//! Device removal joins raw wheel recovery with an actual overflowing tab strip.

use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostWindowToken, NativeMonotonicClock, NativePointerRoster, NativeReceiverPurpose,
    NativeReceiverQuery, NativeWindowHit, SurfacePaintProgramOperation, SurfacePaintProgramStep,
    SurfacePresentationArtifact, SurfacePresentationResult, UniformSurfaceMetrics,
};

use super::super::super::{
    FIRST_SURFACE, ready_window_facts, test_budget, test_managed_capabilities,
};
use super::super::{
    DeviceId, HostRecord, ModifiersState, MouseScrollDelta, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerTranslation, NativeReceiverAnswer,
    NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers,
    NativeScrollMomentum, NativeScrollPhase, NativeSurfaceBinding, NativeWindowEventRecord,
    PhysicalPoint, PhysicalPosition, PointerEventFacts, PointerWindowRoute,
    SurfaceUnavailableReason, TouchPhase, ViewportId, WindowEvent, WindowId, egui,
};
use crate::coordinator::NativeCoordinator;
use crate::event::NativeDeviceRemovalRecord;
use crate::host_frame::NativeHostFrame;

struct Strip {
    receiver: DockspaceReceiverDescriptor,
    artifact: SurfacePresentationArtifact,
    offset: f64,
    maximum: f64,
}

impl Strip {
    fn resolve(
        &self,
        binding: NativeSurfaceBinding,
        query: NativeReceiverQuery,
    ) -> NativeReceiverAnswer {
        match query {
            NativeReceiverQuery::WindowHit(query) => {
                assert_eq!(query.window(), binding.window());
                NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(self.artifact))
            }
            NativeReceiverQuery::Surface(query) => {
                assert!(matches!(
                    query.purpose(),
                    NativeReceiverPurpose::ScrollDelivery(_)
                ));
                assert!(query.matches_presentation_artifact(self.artifact));
                query
                    .bind_receiver(&self.receiver)
                    .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
            }
        }
    }
}

fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn commit(
    native: &mut NativeCoordinator,
    strip: Option<(&Strip, NativeSurfaceBinding)>,
) -> HostFrameReport {
    let report = finish(
        native
            .begin_host_frame(|query| {
                strip.map_or(NativeReceiverAnswer::Unknown, |(strip, binding)| {
                    strip.resolve(binding, query)
                })
            })
            .unwrap(),
    );
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn present(native: &mut NativeCoordinator, binding: NativeSurfaceBinding) -> Strip {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            FIRST_SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut strip = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::Operation {
            operation: SurfacePaintProgramOperation::TabBar(bar),
            ..
        } = step
        {
            let receiver = plan
                .receiver_for_tab_strip_scroll(bar)
                .expect("the actual overflowing strip must expose its scroll receiver");
            assert!(
                strip
                    .replace((receiver, bar.scroll_offset(), bar.maximum_scroll_offset()))
                    .is_none()
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (receiver, offset, maximum) = strip.unwrap();
    assert!(maximum > 0.0);
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    native.settle_host_frame_report(&report).unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    let artifact = output.presentation_artifact();
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    commit(native, None);
    Strip {
        receiver,
        artifact,
        offset,
        maximum,
    }
}

fn wheel(
    native: &NativeCoordinator,
    ordinal: u64,
    point: LogicalPoint,
    phase: TouchPhase,
    delta: f32,
) -> NativeWindowEventRecord {
    let position = PhysicalPosition::new(point.x(), point.y());
    native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            ordinal,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, delta),
                phase,
                facts: PointerEventFacts {
                    surface_position: Some(position),
                    desktop_position: Some(position),
                    modifiers: Some(ModifiersState::empty()),
                    hover: PointerWindowRoute::Window(WindowId::from(11)),
                    capture: PointerWindowRoute::None,
                    ..Default::default()
                },
            },
            routes,
        )
    })
}

fn apply(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    strip: &Strip,
    record: NativeWindowEventRecord,
) -> HostFrameReport {
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1,
    );
    commit(native, Some((strip, binding)))
}

fn expected(
    binding: NativeSurfaceBinding,
    point: LogicalPoint,
    identity: u64,
    phase: NativeScrollPhase,
) -> NativePointerTranslation {
    NativePointerTranslation::Input(NativePointerInput::new(
        NativePointerId::new(identity),
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(identity),
            phase,
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Exact {
                shift: false,
                control: false,
                alt: false,
                command: false,
            },
        )),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(PhysicalPoint::new(point.x(), point.y()).unwrap()),
            NativePointerHover::Dock(binding.window()),
            None,
        ),
        NativePointerOwner::Native(binding.window()),
        NativePointerOwner::None,
    ))
}

#[test]
fn device_removal_retries_then_scrolls_the_overflowing_strip_with_a_fresh_identity() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        FIRST_SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs((1..=24).map(ItemId::new)),
        ),
    )])
    .unwrap();
    let mut native = NativeCoordinator::new_with_storage_allowance(
        DockspaceSession::from_layout(layout, Default::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        None,
        None,
        None,
        Some(NativeMonotonicClock::new(|| Duration::ZERO)),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .unwrap();
    let binding = commit(&mut native, None).native_bindings()[0];
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    native
        .report_snapshot(
            [(binding, ready_window_facts(0.0))],
            dockspace::runtime::NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    commit(&mut native, None);
    let mut strip = present(&mut native, binding);
    let point = strip.receiver.center();
    let delta = NativeScrollDelta::Lines { x: 0.0, y: -1.0 };
    let record = wheel(&native, 1, point, TouchPhase::Moved, -1.0);
    assert_eq!(
        native
            .pointer_translator
            .clone()
            .translate(&record, |_| None),
        expected(binding, point, 1, NativeScrollPhase::Discrete { delta }),
    );
    apply(&mut native, binding, &strip, record);
    strip = present(&mut native, binding);
    assert!(strip.offset > 0.0 && strip.offset < strip.maximum);

    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    native.bridge.push_record(HostRecord::DeviceRemoval(
        NativeDeviceRemovalRecord::for_test(2),
    ));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    let reset_translator = format!("{:?}", native.pointer_translator);
    drop(
        native
            .begin_host_frame(|query| strip.resolve(binding, query))
            .unwrap(),
    );
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_pending_native_input());
    assert_eq!(format!("{:?}", native.pointer_translator), reset_translator);
    let reset = commit(&mut native, Some((&strip, binding)));
    assert_eq!(reset.inputs(), [HostInputOutcome::NativeInputDeviceRemoval]);
    assert!(!reset.workspace_changed());
    assert!(!native.session.has_pending_native_input());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    let offset_before_resume = strip.offset;

    let record = wheel(&native, 3, point, TouchPhase::Moved, -1.0);
    assert_eq!(
        native
            .pointer_translator
            .clone()
            .translate(&record, |_| None),
        expected(binding, point, 2, NativeScrollPhase::Discrete { delta }),
        "a wheel-only Moved must enroll a fresh native pointer and scroll identity",
    );
    apply(&mut native, binding, &strip, record);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    strip = present(&mut native, binding);
    assert!(strip.offset > offset_before_resume);
    assert_eq!(
        native
            .session
            .view()
            .root(RootId::new(1))
            .unwrap()
            .content()
            .unwrap()
            .tabs()
            .unwrap()
            .items(),
        (1..=24).map(ItemId::new).collect::<Vec<_>>()
    );
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert!(!native.session.has_pending_presentation());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
}
