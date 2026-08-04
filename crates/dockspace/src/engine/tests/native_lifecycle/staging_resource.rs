use super::*;

fn retained_resource(
    fixture: &BackgroundFixture,
    resource: crate::presentation_observation::NativeStagingResourceId,
) -> crate::presentation_observation::NativeStagingResourceDescriptor {
    let frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .view()
        .retained_native_staging_resources()
        .find(|descriptor| descriptor.id() == resource)
        .cloned()
        .expect("the host frame must expose the retained staging resource")
}

#[test]
fn retained_resource_spans_staging_transfer_and_first_live() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(194));
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("native create saga must exist");
    let resource = saga.resource();
    let source_presentation = saga.prepared().source_presentation();
    let payload = saga.prepared().payload().clone();

    let descriptor = retained_resource(&fixture, resource);
    assert_eq!(descriptor.source_presentation(), source_presentation);
    assert_eq!(descriptor.payload(), &payload);
    let _ = fixture.engine.viewport.take_new_effects();

    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 2);
    let pre_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    assert_eq!(pre_show.retained_resource(), Some(resource));
    assert_eq!(
        pre_show.basis().platform_provider(),
        fixture
            .engine
            .viewport()
            .platform_provider()
            .expect("platform provider must remain active")
    );

    let shown = present_background_native_staging(&mut fixture, pre_show);
    let pre_show_proof = shown
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ShowWindow {
                binding,
                after_pre_show,
                ..
            } if *binding == request.binding() => Some(*after_pre_show),
            _ => None,
        })
        .expect("show must carry the exact pre-show proof");
    assert_eq!(pre_show_proof.request(), pre_show);
    assert_eq!(pre_show_proof.retained_resource(), Some(resource));

    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 3);
    let _ = publish_background_native_window(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        4,
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    assert_eq!(post_show.retained_resource(), Some(resource));
    let _ = present_background_native_staging(&mut fixture, post_show);

    assert!(
        fixture
            .engine
            .workspace()
            .surface(request.binding().surface())
            .is_some(),
        "ownership transfer must commit before first-live admission"
    );
    assert_eq!(retained_resource(&fixture, resource).id(), resource);

    let surface = request.binding().surface();
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, surface, bounds);
    let _ = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        surface,
        measurements,
    );

    let frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert!(
        frame
            .view()
            .retained_native_staging_resources()
            .all(|descriptor| descriptor.id() != resource),
        "first-live admission must atomically release the retained staging resource"
    );
}

#[test]
fn delayed_pre_show_output_cannot_cross_a_reissued_basis() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(195));
    let _ = fixture.engine.viewport.take_new_effects();

    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 2);
    let first = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    let delayed_output = emit_background_native_staging(&mut fixture, first);

    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 3);
    let reissued = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    assert_ne!(reissued, first);
    assert_eq!(reissued.retained_resource(), first.retained_resource());
    assert_ne!(
        reissued.basis().presentation_observation_generation(),
        first.basis().presentation_observation_generation()
    );

    let delayed = observe_background_presentation_output(&mut fixture, delayed_output);
    assert!(delayed.platform_effects().iter().all(|emission| {
        !matches!(
            emission.effect(),
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    }));
    assert_eq!(
        current_native_staging_presentation(
            &fixture,
            request,
            crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
        ),
        reissued
    );

    let current_output = emit_background_native_staging(&mut fixture, reissued);
    let current = observe_background_presentation_output(&mut fixture, current_output);
    let current_proof = current
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ShowWindow {
                binding,
                after_pre_show,
                ..
            } if *binding == request.binding() => Some(*after_pre_show),
            _ => None,
        })
        .expect("current pre-show output must authorize showing the window");
    assert_eq!(current_proof.request(), reissued);
    assert!(current_proof.matches_output(current_output));
    assert!(
        !current_proof.matches_output(delayed_output),
        "a reissued proof must reject an older output for the same retained resource"
    );
}

#[test]
fn unavailable_staging_slot_preserves_phase_and_resource() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(196));
    let _ = fixture.engine.viewport.take_new_effects();
    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 2);
    let presentation = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );

    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    for obligation in frame
        .issue_presentation_obligations()
        .expect("host frame must issue its exact physical roster")
    {
        let disposition = if obligation.slot().native_staging() == Some(presentation) {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::RetainedResourceUnavailable,
            )
        } else {
            HostPresentationDisposition::Unavailable(
                HostPresentationUnavailableReason::OutputNotProduced,
            )
        };
        frame
            .settle_presentation_obligation(obligation, disposition)
            .expect("every affine presentation obligation must settle exactly once");
    }
    complete_surface_contribution_roster(&mut frame);
    let transition = frame
        .finish(&mut fixture.engine)
        .expect("explicit unavailable staging output must reduce");

    assert!(transition.platform_effects().iter().all(|emission| {
        !matches!(
            emission.effect(),
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    }));
    assert_eq!(
        current_native_staging_presentation(
            &fixture,
            request,
            crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
        ),
        presentation
    );
    let resource = presentation
        .retained_resource()
        .expect("native create staging must retain its source resource");
    assert_eq!(retained_resource(&fixture, resource).id(), resource);
}

#[test]
fn cancelling_an_unemitted_create_releases_its_retained_resource() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(197));
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("native create saga must own its retained resource")
        .resource();
    assert_eq!(retained_resource(&fixture, resource).id(), resource);

    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::CancelNativeCreate {
            expected,
            saga: request.saga(),
        },
    )
    .expect("an unemitted native create must cancel atomically");

    assert!(transition.platform_effects().is_empty());
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .is_none()
    );
    let frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert!(
        frame
            .view()
            .retained_native_staging_resources()
            .all(|descriptor| descriptor.id() != resource),
        "cancelling before create extraction must release the resource immediately"
    );
}
