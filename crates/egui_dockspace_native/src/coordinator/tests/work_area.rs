use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
};
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute, WindowEvent,
};

use super::*;
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::work_area::FrozenWorkAreaRoster;

fn display_bounds() -> PhysicalRect {
    PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate")
}

fn usable_work_area() -> PhysicalRect {
    PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate")
}

fn publish_work_areas(
    native: &mut NativeCoordinator,
    bindings: [NativeSurfaceBinding; 2],
    work_areas: FrozenWorkAreaRoster,
) {
    let roster = NativeViewportRosterRecord::for_test_with_work_areas(
        bindings.into_iter().map(|binding| {
            crate::window_snapshot::CompiledWindowObservation::new(
                binding,
                NativeWindowFacts::live(),
            )
        }),
        work_areas,
    );
    native.bridge.push_viewport_roster(roster);
    assert!(
        native
            .reduce_next_viewport_roster()
            .expect("the complete viewport roster reduces")
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native snapshot frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the native snapshot frame settles every surface");
    let report = frame.commit().expect("the native snapshot commits");
    assert!(native.settle_host_frame_inputs(report.inputs()));
}

#[test]
fn outside_all_translation_uses_the_exact_committed_work_area() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    publish_work_areas(
        &mut native,
        [first, second],
        FrozenWorkAreaRoster::for_test_exact([(
            1,
            display_bounds(),
            usable_work_area(),
            ScaleFactor::new(1.0).expect("work-area scale validates"),
        )]),
    );

    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            surface_position: None,
            desktop_position: Some(PhysicalPosition::new(1600.0, 1060.0)),
            modifiers: None,
            hover: PointerWindowRoute::None,
            capture: PointerWindowRoute::Window(first_window),
        },
    };
    let record = {
        let viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_snapshot(
            1,
            first_window,
            Some(ViewportId::ROOT),
            event,
            &viewports,
        )
    };

    let point = PhysicalPoint::new(1600.0, 1060.0).expect("desktop point validates");
    let work_area = native
        .work_area_for_point(point)
        .expect("the committed display route resolves the outside-all point");
    let mut translator = NativePointerTranslator::default();
    let actual = translator.translate(&record, |point| native.work_area_for_point(point));
    let expected = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(point),
            NativePointerHover::OutsideAll,
            Some(work_area),
        ),
        NativePointerOwner::Native(first),
        NativePointerOwner::Native(first),
    );

    assert_eq!(actual, NativePointerTranslation::Input(expected));
}

#[test]
fn ambiguous_display_routes_fail_closed_instead_of_selecting_by_order() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    publish_work_areas(
        &mut native,
        [first, second],
        FrozenWorkAreaRoster::for_test_exact([
            (
                1,
                display_bounds(),
                usable_work_area(),
                ScaleFactor::new(1.0).expect("work-area scale validates"),
            ),
            (
                2,
                PhysicalRect::new(1600.0, 0.0, 1920.0, 1080.0)
                    .expect("second display bounds validate"),
                PhysicalRect::new(1600.0, 0.0, 1920.0, 1040.0)
                    .expect("second work-area bounds validate"),
                ScaleFactor::new(1.0).expect("work-area scale validates"),
            ),
        ]),
    );

    assert_eq!(
        native.work_area_for_point(
            PhysicalPoint::new(1800.0, 500.0).expect("overlap point validates")
        ),
        None
    );
}

#[test]
fn committed_unknown_roster_revokes_the_previous_work_area_route() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    publish_work_areas(
        &mut native,
        [first, second],
        FrozenWorkAreaRoster::for_test_exact([(
            1,
            display_bounds(),
            usable_work_area(),
            ScaleFactor::new(1.0).expect("work-area scale validates"),
        )]),
    );
    let point = PhysicalPoint::new(1600.0, 1060.0).expect("desktop point validates");
    assert!(native.work_area_for_point(point).is_some());

    publish_work_areas(&mut native, [first, second], FrozenWorkAreaRoster::Unknown);

    assert_eq!(native.work_area_for_point(point), None);
}
