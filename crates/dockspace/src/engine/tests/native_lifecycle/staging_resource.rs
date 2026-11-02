use super::*;

use crate::frame::NativeCreatePhase;
use crate::viewport_registry::ViewportAdmission;

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

fn assert_pending_first_live_resource(
    fixture: &BackgroundFixture,
    request: crate::frame::NativeCreateRequest,
    descriptor: &crate::presentation_observation::NativeStagingResourceDescriptor,
) {
    let surface = request.binding().surface();
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("the unadmitted child must retain its exact create saga");
    assert_eq!(saga.binding(), request.binding());
    assert_eq!(saga.resource(), descriptor.id());
    assert!(matches!(
        saga.phase(),
        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert!(fixture.engine.workspace().surface(surface).is_none());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(surface)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(ViewportAdmission::Pending)
    );
    assert!(fixture.engine.interaction_authority(surface).is_none());
    assert_eq!(retained_resource(fixture, descriptor.id()), *descriptor);
    fixture
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("the pending first-live child must own its exact retained resource");
}

#[test]
fn retained_resource_spans_staging_until_atomic_first_live_transfer() {
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
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

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

    assert_eq!(
        fixture.engine.workspace(),
        &workspace,
        "PostShow placeholder presentation must preserve durable source ownership"
    );
    assert_eq!(
        fixture.engine.version(),
        version,
        "PostShow placeholder presentation must preserve the workspace version"
    );
    assert_pending_first_live_resource(&fixture, request, &descriptor);

    let surface = request.binding().surface();
    let bounds =
        LogicalRect::new(0.0, 0.0, 300.0, 220.0).expect("native target test bounds must be valid");
    let measurements = surface_measurements(&fixture.engine, surface, bounds);
    let (_, admitted) = publish_surface_projection_with_transition(
        &mut fixture.engine,
        fixture.presentation_host,
        surface,
        measurements,
    );
    let committed = admitted
        .events()
        .iter()
        .find(|event| {
            matches!(
                event.kind(),
                crate::event::WorkspaceEventKind::CommandCommitted(_)
            )
        })
        .expect("first-live ownership transfer publishes one command event");
    assert!(matches!(
        committed.cause(),
        crate::event::ReductionCause::SurfacePresentationObservationBatch { .. }
    ));

    assert_eq!(fixture.engine.version().epoch(), version.epoch());
    assert_eq!(
        fixture.engine.version().revision().get(),
        version.revision().get() + 1,
        "exact first-live presentation must publish ownership exactly once"
    );
    assert_eq!(
        fixture
            .engine
            .workspace()
            .presentation_for_root(RootId::new(194)),
        Some(crate::RootPresentationOwner::Main { surface })
    );
    assert_eq!(
        fixture.engine.workspace().node(fixture.source_tabs),
        Some(&Node::tabs([ItemId::new(2)])),
        "the admitted child must atomically receive the retained source item"
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .viewport(surface)
            .map(crate::viewport_registry::ViewportRecord::admission),
        Some(ViewportAdmission::Admitted)
    );
    assert!(fixture.engine.interaction_authority(surface).is_some());
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
        "first-live admission must atomically release the retained staging resource"
    );
    fixture
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("atomic transfer and admission must leave no orphaned staging resource");
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

    let _ = publish_background_native_window(
        &mut fixture,
        request,
        WindowPresentationState::Minimized,
        3,
    );
    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 4);
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
fn pre_show_survives_later_hidden_observation_without_repeated_create_ack() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(197));
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("the create saga must retain its source resource")
        .resource();
    let descriptor = retained_resource(&fixture, resource);
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let _ = fixture.engine.viewport.take_new_effects();

    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 2);
    let first = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );

    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let snapshot = background_native_window_snapshot_with_capabilities_and_acknowledgement(
        &mut fixture,
        request,
        WindowPresentationState::Hidden,
        3,
        GlobalFocusedWindow::Foreign,
        capabilities,
        None,
    );
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("the later hidden observation must reduce without repeating the create ack");

    let reissued = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PreShow,
    );
    assert_eq!(reissued, first);
    assert_eq!(reissued.retained_resource(), first.retained_resource());
    assert_eq!(
        reissued.basis().presentation_observation_generation(),
        crate::viewport::PresentationObservationGeneration::new(2)
    );
    let frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    assert!(
        frame
            .view()
            .native_staging_presentations()
            .any(|presentation| presentation == reissued),
        "the later one-shot-ack observation must leave a current pre-show slot"
    );

    let pre_show_presented = present_background_native_staging(&mut fixture, reissued);
    assert!(pre_show_presented.platform_effects().iter().any(|effect| {
        matches!(
            effect.effect(),
            PlatformEffect::ShowWindow { binding, .. } if *binding == request.binding()
        )
    }));
    let _ =
        publish_background_native_window(&mut fixture, request, WindowPresentationState::Hidden, 4);
    let _ = publish_background_native_window(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
        5,
    );
    let post_show = current_native_staging_presentation(
        &fixture,
        request,
        crate::presentation_observation::NativeStagingPresentationPhase::PostShow,
    );
    let staged = present_background_native_staging(&mut fixture, post_show);

    assert!(
        staged.platform_effects().iter().all(|effect| {
            !matches!(
                effect.effect(),
                PlatformEffect::CompensatingClose { binding, .. }
                    if *binding == request.binding()
            )
        }),
        "a later compatible hidden basis must not invalidate the one-shot create acknowledgement"
    );
    assert_eq!(
        fixture.engine.workspace(),
        &workspace,
        "a reissued pre-show basis must preserve source ownership through PostShow"
    );
    assert_eq!(fixture.engine.version(), version);
    assert_pending_first_live_resource(&fixture, request, &descriptor);
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

#[test]
fn cancelling_post_show_or_first_live_retains_cleanup_resource_despite_late_output() {
    for first_live in [false, true] {
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut fixture = background_fixture(policy);
        let request = install_pending_native_root_reservation(&mut fixture, RootId::new(198));
        let resource = fixture
            .engine
            .viewport()
            .native_create_saga(request.saga())
            .expect("the exact create saga must own its resource")
            .resource();
        let descriptor = retained_resource(&fixture, resource);
        let workspace = fixture.engine.workspace().clone();
        let version = fixture.engine.version();
        let source_binding = background_source_binding(&fixture);
        let _ = fixture.engine.viewport.take_new_effects();
        advance_pending_native_root_to_awaiting_visible(&mut fixture, request);
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
        let delayed_output = if first_live {
            let _ = present_background_native_staging(&mut fixture, post_show);
            assert_pending_first_live_resource(&fixture, request, &descriptor);
            let surface = request.binding().surface();
            let contribution =
                begin_surface_measurement(&fixture.engine, surface, background_bounds());
            let _ = submit_surface_measurement(
                &mut fixture.engine,
                fixture.presentation_host,
                contribution,
            );
            let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
            for obligation in frame.issue_presentation_obligations().unwrap() {
                if obligation.slot().surface() == surface {
                    let token = frame.view().begin_surface_contribution(surface).unwrap();
                    let interaction = frame.view().presentation_interaction(surface).unwrap();
                    frame
                        .record_painted_surface_contribution(obligation, token, interaction)
                        .expect("only the prospective child emits an exact content output");
                } else {
                    frame
                        .settle_presentation_obligation(
                            obligation,
                            HostPresentationDisposition::Unavailable(
                                HostPresentationUnavailableReason::OutputNotProduced,
                            ),
                        )
                        .unwrap();
                }
            }
            complete_surface_contribution_roster(&mut frame);
            let emitted = frame
                .finish(&mut fixture.engine)
                .expect("the prospective child must emit a real semantic output");
            let output = emitted
                .presentation_emissions()
                .iter()
                .map(|emission| emission.output())
                .find(|output| output.surface() == surface)
                .expect("the exact first-live child output must remain unreported");
            assert!(matches!(
                output.payload(),
                HostPresentationOutputPayload::Paint { .. }
            ));
            assert_pending_first_live_resource(&fixture, request, &descriptor);
            output
        } else {
            emit_background_native_staging(&mut fixture, post_show)
        };
        assert_eq!(
            delayed_output.endpoint(),
            crate::presentation_observation::HostPresentationEndpoint::Native(request.binding())
        );
        assert_eq!(fixture.engine.workspace(), &workspace);
        assert_eq!(fixture.engine.version(), version);

        let cancelled = submit_test_input(
            &mut fixture.engine,
            fixture.presentation_host,
            EngineInput::CancelNativeCreate {
                expected: version,
                saga: request.saga(),
            },
        )
        .expect("the exact pending create must cancel before first-live ownership transfer");
        assert!(matches!(
            cancelled.reduced_inputs().last().unwrap().outcome(),
            InputOutcome::NativeCreateCancelled { saga, .. } if *saga == request.saga()
        ));
        assert!(
            fixture
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .is_none()
        );
        assert!(
            fixture
                .engine
                .viewport()
                .viewport(request.binding().surface())
                .is_none()
        );
        let retirement = fixture
            .engine
            .viewport()
            .binding_retirements()
            .find_map(|(binding, retirement)| {
                (binding == request.binding()).then(|| retirement.clone())
            })
            .expect("cancelling an observed child must retain exact binding cleanup ownership");
        assert_eq!(
            retirement.origin(),
            crate::frame::BindingRetirementOrigin::NativeCreateAborted {
                create: request.effect(),
            }
        );
        assert_eq!(retained_resource(&fixture, resource), descriptor);
        fixture
            .engine
            .viewport()
            .validate_native_staging_resource_conservation()
            .expect("cancellation must transfer the resource to exact binding cleanup");

        let late = observe_background_presentation_output(&mut fixture, delayed_output);
        assert!(late.platform_effects().is_empty());
        assert_eq!(fixture.engine.workspace(), &workspace);
        assert_eq!(fixture.engine.version(), version);
        assert_eq!(background_source_binding(&fixture), source_binding);
        assert_eq!(retained_resource(&fixture, resource), descriptor);
        assert_eq!(
            fixture
                .engine
                .viewport()
                .binding_retirements()
                .find_map(|(binding, current)| {
                    (binding == request.binding()).then_some(current)
                }),
            Some(&retirement),
            "late staging or semantic presentation must not consume or reissue cleanup ownership"
        );
        assert!(
            fixture
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .is_none()
        );
        assert!(
            fixture
                .engine
                .interaction_authority(request.binding().surface())
                .is_none()
        );
        fixture
            .engine
            .viewport()
            .validate_native_staging_resource_conservation()
            .expect("late output must preserve exact cleanup resource conservation");
    }
}

#[test]
fn another_bindings_presented_output_cannot_admit_or_release_first_live_resource() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(199));
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("the target create saga must retain its resource")
        .resource();
    let descriptor = retained_resource(&fixture, resource);
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let _ = fixture.engine.viewport.take_new_effects();
    let _ = advance_pending_native_root_to_ownership_transfer(&mut fixture, request);
    assert_pending_first_live_resource(&fixture, request, &descriptor);

    let source_binding = background_source_binding(&fixture);
    assert_ne!(source_binding, request.binding());
    let measurements = surface_measurements(&fixture.engine, SOURCE_SURFACE, background_bounds());
    let (_, output, _) = publish_surface_projection_with_output(
        &mut fixture.engine,
        fixture.presentation_host,
        SOURCE_SURFACE,
        measurements,
    );
    assert_eq!(
        output.endpoint(),
        crate::presentation_observation::HostPresentationEndpoint::Native(source_binding)
    );
    assert!(matches!(
        output.payload(),
        HostPresentationOutputPayload::Paint { .. }
    ));
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert_pending_first_live_resource(&fixture, request, &descriptor);
    assert!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .next()
            .is_none()
    );
}
