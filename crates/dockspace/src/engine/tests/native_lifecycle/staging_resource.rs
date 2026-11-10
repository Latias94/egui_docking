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
fn retained_resource_spans_preparation_until_atomic_first_live_transfer() {
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

    advance_pending_native_root_to_ownership_transfer(&mut fixture, request);

    assert_eq!(
        fixture.engine.workspace(),
        &workspace,
        "Prepared native readiness must preserve durable source ownership"
    );
    assert_eq!(
        fixture.engine.version(),
        version,
        "Prepared native readiness must preserve the workspace version"
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
fn prepared_continuity_survives_later_observation_without_repeated_create_ack() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(197));
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .unwrap()
        .resource();
    let descriptor = retained_resource(&fixture, resource);
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let _ = fixture.engine.viewport.take_new_effects();
    publish_background_native_window(&mut fixture, request, WindowPresentationState::Visible, 2);
    let basis = fixture
        .engine
        .viewport()
        .native_ready_basis(request.binding())
        .unwrap();
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let snapshot = background_native_window_snapshot_with_capabilities_and_acknowledgement(
        &mut fixture,
        request,
        WindowPresentationState::Visible,
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
    .unwrap();
    assert_eq!(
        fixture
            .engine
            .viewport()
            .native_ready_basis(request.binding()),
        Some(basis)
    );
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert_pending_first_live_resource(&fixture, request, &descriptor);
}

#[test]
fn unavailable_first_live_output_preserves_phase_and_resource() {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let request = install_pending_native_root_reservation(&mut fixture, RootId::new(196));
    let _ = fixture.engine.viewport.take_new_effects();
    advance_pending_native_root_to_ownership_transfer(&mut fixture, request);
    let resource = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .unwrap()
        .resource();
    let descriptor = retained_resource(&fixture, resource);
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let transition = frame.finish(&mut fixture.engine).unwrap();
    assert!(transition.platform_effects().is_empty());
    assert_pending_first_live_resource(&fixture, request, &descriptor);
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
fn cancelling_first_live_retains_cleanup_resource_despite_late_output() {
    {
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
        advance_pending_native_root_to_ownership_transfer(&mut fixture, request);
        let delayed_output = {
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
            "late semantic presentation must not consume or reissue cleanup ownership"
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
