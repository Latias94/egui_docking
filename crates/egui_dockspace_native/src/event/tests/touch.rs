use super::*;

fn touch_event(
    id: u64,
    phase: TouchPhase,
    location: PhysicalPosition<f64>,
    facts: PointerEventFacts,
) -> WindowEvent {
    WindowEvent::Touch(Touch {
        device_id: DeviceId::dummy(),
        phase,
        location,
        facts,
        force: None,
        id,
    })
}

#[test]
fn exact_touch_pair_uses_one_local_lane_even_with_finite_desktop_facts() {
    let (_session, binding, viewports) = local_fixture();
    let facts = PointerEventFacts {
        desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
        hover: PointerWindowRoute::Unknown,
        capture: PointerWindowRoute::Unknown,
        ..pointer_facts(20.0, 40.0, 1.0)
    };
    for (terminal, terminal_event) in [
        (
            TouchPhase::Ended,
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        ),
        (
            TouchPhase::Cancelled,
            NativePointerEvent::StreamCancelled(
                dockspace::runtime::NativePointerCancelReason::ExplicitPlatformCancellation,
            ),
        ),
    ] {
        let mut translator = NativePointerTranslator::default();
        for (ordinal, phase, event) in [
            (
                1,
                TouchPhase::Started,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            ),
            (2, TouchPhase::Moved, NativePointerEvent::Moved),
            (3, terminal, terminal_event),
        ] {
            let record = captured(
                ordinal,
                touch_event(7, phase, PhysicalPosition::new(20.0, 40.0), facts),
                &viewports,
                geometry(1.0),
            );
            let Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Exact(sample),
            )) = record.pointer_location()
            else {
                panic!("touch must retain one exact local sample without requesting a handoff");
            };
            assert_eq!(
                sample.client_position(),
                PhysicalPoint::new(20.0, 40.0).unwrap()
            );
            assert_eq!(
                sample.surface_position(),
                LogicalPoint::new(20.0, 40.0).unwrap()
            );
            let delivery = if phase == TouchPhase::Cancelled {
                NativePointerOwner::Unknown
            } else {
                NativePointerOwner::Native(binding)
            };
            assert_eq!(
                translator.translate(&record, |_| panic!(
                    "touch cannot consult a desktop work area"
                )),
                NativePointerTranslation::Input(NativePointerInput::new_local(
                    NativePointerId::new(1),
                    event,
                    NativeLocalPointerLocation::Exact(sample),
                    delivery,
                    NativePointerOwner::Unknown,
                )),
            );
        }
        assert!(!translator.references_binding(binding));
    }
}

#[test]
fn touch_routes_come_from_facts_while_delivery_keeps_the_callback_binding() {
    let (_session, binding, viewports) = local_fixture();
    for (hover, capture, expected_hover, expected_capture) in [
        (
            PointerWindowRoute::Foreign,
            PointerWindowRoute::Window(WindowId::from(WINDOW)),
            NativePointerRouteSnapshot::Foreign,
            NativePointerRouteSnapshot::Dock(binding),
        ),
        (
            PointerWindowRoute::Window(WindowId::from(WINDOW)),
            PointerWindowRoute::None,
            NativePointerRouteSnapshot::Dock(binding),
            NativePointerRouteSnapshot::None,
        ),
        (
            PointerWindowRoute::Window(WindowId::from(WINDOW + 1)),
            PointerWindowRoute::Window(WindowId::from(WINDOW + 1)),
            NativePointerRouteSnapshot::Unknown,
            NativePointerRouteSnapshot::Unknown,
        ),
    ] {
        let record = captured(
            1,
            touch_event(
                7,
                TouchPhase::Started,
                PhysicalPosition::new(20.0, 40.0),
                PointerEventFacts {
                    hover,
                    capture,
                    ..pointer_facts(20.0, 40.0, 1.0)
                },
            ),
            &viewports,
            geometry(1.0),
        );
        let routes = record.pointer_routes().unwrap();
        assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(binding));
        assert_eq!(routes.hover(), expected_hover);
        assert_eq!(routes.capture(), expected_capture);
    }
}

#[test]
fn touch_local_pair_rejects_mismatched_or_non_finite_points_without_desktop_fallback() {
    let (_session, binding, viewports) = local_fixture();
    let finite = PhysicalPosition::new(20.0, 40.0);
    let nan = PhysicalPosition::new(f64::NAN, 40.0);
    let infinity = PhysicalPosition::new(20.0, f64::INFINITY);
    let facts = PointerEventFacts {
        desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
        ..pointer_facts(finite.x, finite.y, 1.0)
    };
    for (location, surface_position) in [
        (PhysicalPosition::new(21.0, 40.0), Some(finite)),
        (finite, None),
        (finite, Some(nan)),
        (nan, Some(finite)),
        (nan, Some(nan)),
        (infinity, Some(infinity)),
    ] {
        let record = captured(
            1,
            touch_event(
                7,
                TouchPhase::Started,
                location,
                PointerEventFacts {
                    surface_position,
                    ..facts
                },
            ),
            &viewports,
            geometry(1.0),
        );
        assert_eq!(
            record.pointer_location(),
            Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Unknown
            )),
        );
        assert_eq!(
            NativePointerTranslator::default()
                .translate(&record, |_| panic!("invalid touch stays local")),
            NativePointerTranslation::Input(NativePointerInput::new_local(
                NativePointerId::new(1),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativeLocalPointerLocation::Unknown,
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            )),
        );
    }
}

#[test]
fn touch_capture_requires_original_geometry_and_committed_callback_authority() {
    let (_session, _binding, viewports) = local_fixture();
    let coordinates =
        viewports.local_coordinates_for_event(WindowId::from(WINDOW), Some(ViewportId::ROOT));
    let facts = PointerEventFacts {
        desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
        ..pointer_facts(20.0, 40.0, 1.0)
    };
    let mut resized = facts;
    resized.geometry.as_mut().unwrap().surface_size.width += 1;
    let mut invalid_scale = facts;
    invalid_scale.geometry.as_mut().unwrap().scale_factor = f64::NAN;
    for (facts, coordinates, callback) in [
        (facts, None, Some(geometry(1.0))),
        (facts, coordinates, None),
        (facts, coordinates, Some(geometry(2.0))),
        (resized, coordinates, Some(geometry(1.0))),
        (invalid_scale, coordinates, Some(geometry(1.0))),
    ] {
        let record = NativeWindowEventRecord::for_test_local_capture(
            1,
            WindowId::from(WINDOW),
            Some(ViewportId::ROOT),
            touch_event(
                7,
                TouchPhase::Started,
                PhysicalPosition::new(20.0, 40.0),
                facts,
            ),
            &viewports,
            coordinates,
            callback,
        );
        assert_eq!(
            record.pointer_location(),
            Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Unknown
            )),
        );
    }
    let without_callback = NativeWindowEventRecord::for_test_snapshot(
        2,
        WindowId::from(WINDOW),
        Some(ViewportId::ROOT),
        touch_event(
            7,
            TouchPhase::Started,
            PhysicalPosition::new(20.0, 40.0),
            facts,
        ),
        &viewports,
    );
    assert_eq!(
        without_callback.pointer_location(),
        Some(CapturedNativePointerLocation::NativeLocal(
            NativeLocalPointerLocation::Unknown
        )),
    );
}

#[test]
fn touch_without_event_geometry_preserves_the_unknown_desktop_lane() {
    let (_session, binding, viewports) = local_fixture();
    let facts = PointerEventFacts {
        surface_position: Some(PhysicalPosition::new(20.0, 40.0)),
        desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
        ..PointerEventFacts::default()
    };
    let record = captured(
        1,
        touch_event(
            7,
            TouchPhase::Started,
            PhysicalPosition::new(20.0, 40.0),
            facts,
        ),
        &viewports,
        geometry(1.0),
    );
    assert_eq!(
        record.pointer_location(),
        Some(CapturedNativePointerLocation::Desktop)
    );
    assert_eq!(
        NativePointerTranslator::default()
            .translate(&record, |_| panic!("legacy touch has no desktop point")),
        NativePointerTranslation::Input(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None
            ),
            NativePointerOwner::Native(binding),
            NativePointerOwner::Unknown,
        )),
    );
}

#[test]
fn cloned_touch_retains_source_contact_time_and_pair_across_route_refresh() {
    let (mut session, binding, mut viewports) = local_fixture();
    let mut facts = pointer_facts(20.0, 40.0, 1.0);
    let mut event = touch_event(
        7,
        TouchPhase::Started,
        PhysicalPosition::new(20.0, 40.0),
        facts,
    );
    let record = captured(1, event.clone(), &viewports, geometry(1.0));
    let retry = record.clone();
    let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
        original,
    ))) = record.pointer_location()
    else {
        panic!("the original touch callback has an exact local pair");
    };
    assert!(Arc::ptr_eq(
        record.pointer.as_ref().unwrap(),
        retry.pointer.as_ref().unwrap()
    ));
    assert_eq!(
        record.owned_descendant_capacity_bytes(),
        size_of::<NativePointerCallbackFacts>()
    );

    commit_geometry(&mut session, binding, geometry(2.0));
    viewports.refresh_local_coordinates(&session);
    let current = captured(
        2,
        touch_event(
            8,
            TouchPhase::Started,
            PhysicalPosition::new(20.0, 40.0),
            pointer_facts(20.0, 40.0, 2.0),
        ),
        &viewports,
        geometry(2.0),
    );
    let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
        current,
    ))) = current.pointer_location()
    else {
        panic!("a new contact captures against the refreshed geometry");
    };
    assert!(!original.same_coordinates(current));
    assert_eq!(
        original.surface_position(),
        LogicalPoint::new(20.0, 40.0).unwrap()
    );
    assert_eq!(
        current.surface_position(),
        LogicalPoint::new(10.0, 20.0).unwrap()
    );
    assert!(viewports.suppress_pointer_route(ViewportId::ROOT, binding));
    facts.surface_position = Some(PhysicalPosition::new(90.0, 100.0));
    let frozen_event = event.clone();
    let WindowEvent::Touch(touch) = &mut event else {
        unreachable!()
    };
    touch.id = 99;
    touch.location = PhysicalPosition::new(90.0, 100.0);
    touch.facts = facts;

    let captured_at = std::time::Duration::from_millis(37);
    let expected = NativePointerTranslation::Input(
        NativePointerInput::new_local(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeLocalPointerLocation::Exact(original),
            NativePointerOwner::Native(binding),
            NativePointerOwner::Native(binding),
        )
        .with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Touch,
            captured_at,
        )),
    );
    for candidate in [&record, &retry] {
        assert_eq!(candidate.event(), &frozen_event);
        assert_eq!(candidate.window_id(), WindowId::from(WINDOW));
        assert_eq!(candidate.binding(), Some(binding));
        assert_eq!(
            NativePointerTranslator::default().translate_captured(
                candidate,
                captured_at,
                |_| panic!("retry cannot refresh a touch pair")
            ),
            expected
        );
        assert!(!candidate.shares_cursor_lane(&retry));
    }
}

#[test]
fn old_touch_pair_after_rebase_stays_unknown_and_contact_end_still_terminates() {
    let (mut session, binding, mut viewports) = local_fixture();
    let facts = pointer_facts(20.0, 40.0, 1.0);
    let start = captured(
        1,
        touch_event(
            7,
            TouchPhase::Started,
            PhysicalPosition::new(20.0, 40.0),
            facts,
        ),
        &viewports,
        geometry(1.0),
    );
    let mut translator = NativePointerTranslator::default();
    assert!(matches!(
        translator.translate(&start, |_| None),
        NativePointerTranslation::Input(_)
    ));
    commit_geometry(&mut session, binding, geometry(2.0));
    viewports.refresh_local_coordinates(&session);
    for (ordinal, phase, event) in [
        (2, TouchPhase::Moved, NativePointerEvent::Moved),
        (
            3,
            TouchPhase::Ended,
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        ),
    ] {
        let record = captured(
            ordinal,
            touch_event(7, phase, PhysicalPosition::new(20.0, 40.0), facts),
            &viewports,
            geometry(2.0),
        );
        assert_eq!(
            record.pointer_location(),
            Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Unknown
            ))
        );
        assert_eq!(
            translator.translate(&record, |_| panic!(
                "old touch pair cannot fall back to desktop"
            )),
            NativePointerTranslation::Input(NativePointerInput::new_local(
                NativePointerId::new(1),
                event,
                NativeLocalPointerLocation::Unknown,
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            ))
        );
        let retry = record.clone();
        assert_eq!(retry.pointer_location(), record.pointer_location());
    }
    assert!(!translator.references_binding(binding));
    assert!(translator.can_coalesce_idle_cursor_moves());
    let late = captured(
        4,
        touch_event(
            7,
            TouchPhase::Moved,
            PhysicalPosition::new(20.0, 40.0),
            facts,
        ),
        &viewports,
        geometry(2.0),
    );
    assert_eq!(
        translator.translate(&late, |_| None),
        NativePointerTranslation::Ignored
    );
    let restart = captured(
        5,
        touch_event(
            7,
            TouchPhase::Started,
            PhysicalPosition::new(20.0, 40.0),
            pointer_facts(20.0, 40.0, 2.0),
        ),
        &viewports,
        geometry(2.0),
    );
    let Some(CapturedNativePointerLocation::NativeLocal(
        location @ NativeLocalPointerLocation::Exact(_),
    )) = restart.pointer_location()
    else {
        panic!("a new start can use the new local generation");
    };
    assert_eq!(
        translator.translate(&restart, |_| None),
        NativePointerTranslation::Input(NativePointerInput::new_local(
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            location,
            NativePointerOwner::Native(binding),
            NativePointerOwner::Native(binding),
        ))
    );
}
