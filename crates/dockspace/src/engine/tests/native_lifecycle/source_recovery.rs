//! Source recovery revokes pending first-live content without a workspace edit.

use super::*;

use crate::frame::NativeCreatePhase;
use crate::presentation_observation::{
    HostPresentationContinuation, HostPresentationEndpoint, HostPresentationOutput,
};
use crate::runtime::{
    DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspaceRuntimeErrorKind,
};
use crate::surface_recovery::SurfaceRecoveryBlockedReason;
use crate::viewport::PresentationObservationGeneration;
use crate::viewport_registry::ViewportAdmission;

fn observed_source_window(
    binding: ViewportBinding,
    generation: u64,
    x: f64,
    presentation: WindowPresentationState,
) -> ObservedWindow {
    let scale = ScaleFactor::new(1.0).expect("the native scale must be valid");
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(x, 0.0, 400.0, 300.0)
                    .expect("the native content bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(x - 8.0, -30.0, 416.0, 338.0)
                    .expect("the native outer bounds must be valid"),
            ),
            Authority::Known(scale),
            Authority::Known(scale),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(presentation),
        ))
}

fn publish_source_windows(
    fixture: &mut BackgroundFixture,
    generation: u64,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
) -> EngineTransition {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    let snapshot = test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        windows,
        close_observations,
        known_work_area_observation(
            generation,
            vec![ObservedWorkArea::new(
                WorkAreaToken::new(93),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0)
                    .expect("the native work area must be valid"),
                ScaleFactor::new(1.0).expect("the work-area scale must be valid"),
            )],
        ),
    )
    .expect("the complete native snapshot must be canonical");
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
    .expect("the complete native snapshot must reduce")
}

fn native_child_source_fixture() -> BackgroundFixture {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let provider = test_platform_provider(&mut fixture.engine);
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: TARGET_SURFACE,
            token: WindowToken::new(2),
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("the recovery host must register through the engine");
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::BootstrapChildViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token: WindowToken::new(1),
            recovery: SurfaceRecoveryBootstrap::new(TARGET_SURFACE),
        },
    )
    .expect("core must mint the native child's binding and recovery obligation");
    let host = fixture
        .engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .unwrap()
        .binding();
    let source = background_source_binding(&fixture);
    publish_source_windows(
        &mut fixture,
        1,
        vec![
            observed_source_window(host, 1, 0.0, WindowPresentationState::Visible),
            observed_source_window(source, 1, 500.0, WindowPresentationState::Visible),
        ],
        Vec::new(),
    );
    publish_background_scene(&mut fixture);
    let record = fixture.engine.viewport().viewport(SOURCE_SURFACE).unwrap();
    assert_eq!(record.role(), ViewportRole::Child);
    assert_eq!(record.admission(), ViewportAdmission::Admitted);
    assert_eq!(
        fixture
            .engine
            .surface_recovery_target(SOURCE_SURFACE)
            .unwrap()
            .host_surface(),
        TARGET_SURFACE,
    );
    fixture
}

fn emit_pending_content(
    fixture: &mut BackgroundFixture,
    surface: SurfaceId,
) -> HostPresentationOutput {
    let contribution = begin_surface_measurement(&fixture.engine, surface, background_bounds());
    submit_surface_measurement(&mut fixture.engine, fixture.presentation_host, contribution);
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    for obligation in frame.issue_presentation_obligations().unwrap() {
        if obligation.slot() == (HostPresentationSlot::Surface { surface }) {
            let token = frame.view().begin_surface_contribution(surface).unwrap();
            let interaction = frame.view().presentation_interaction(surface).unwrap();
            frame
                .record_painted_surface_contribution(obligation, token, interaction)
                .expect("only the prospective target paints its exact content output");
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
    let transition = frame
        .finish(&mut fixture.engine)
        .expect("the content output must emit");
    let outputs = transition
        .presentation_emissions()
        .iter()
        .map(|emission| emission.output())
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        panic!("only the pending target must emit, got {outputs:?}");
    };
    assert!(matches!(
        output.payload(),
        HostPresentationOutputPayload::Paint { .. }
    ));
    assert_eq!(
        output.continuation(),
        HostPresentationContinuation::Presented
    );
    *output
}

#[test]
fn source_recovery_cancels_first_live_projection_without_workspace_or_version_change() {
    let mut fixture = native_child_source_fixture();
    let source = background_source_binding(&fixture);
    let host = fixture
        .engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .unwrap()
        .binding();

    // AlreadyNative disables the painted menu entry. The current-action facade
    // instead submits TearOffNative directly, so exercise that same reducer input.
    let mut menu_session = DockspaceSession::from_engine_for_test(
        fixture.engine.candidate(),
        fixture.presentation_host,
    );
    let mut menu_frame = menu_session.begin_host_frame().unwrap();
    let menu = menu_frame.paint_plan(SOURCE_SURFACE).unwrap().unwrap();
    assert_eq!(
        menu.presentation_command(
            SOURCE_ROOT,
            DockspacePresentationCommandKind::MoveToNewWindow
        )
        .unwrap()
        .unwrap()
        .unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::AlreadyNative),
    );
    drop(menu_frame);
    drop(menu_session);

    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let created = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::TearOffNative {
            expected: version,
            source: crate::model::NativeTearOffSource::Root(SOURCE_ROOT),
            placement: Some(crate::model::NativeWindowPlacement::new(
                PhysicalRect::new(1000.0, 0.0, 400.0, 300.0)
                    .expect("the prospective window placement must be valid"),
            )),
        },
    )
    .expect("the current-action reducer must accept a native child source");
    let target = match created.reduced_inputs()[0].outcome() {
        InputOutcome::ProductActionProcessed {
            outcome:
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    root,
                    source_surface,
                    target_surface,
                    ..
                },
            ..
        } if *root == SOURCE_ROOT && *source_surface == SOURCE_SURFACE => *target_surface,
        outcome => {
            panic!("expected a real native create from the current-action input, got {outcome:?}")
        }
    };
    let (saga, binding, create, resource) = fixture
        .engine
        .viewport()
        .native_create_sagas()
        .find_map(|(saga, create)| {
            (create.binding().surface() == target).then_some((
                saga,
                create.binding(),
                create.create(),
                create.resource(),
            ))
        })
        .expect("the accepted command must own a real native create saga");
    assert!(created.platform_effects().iter().any(|effect| {
        effect.id() == create
            && matches!(effect.effect(), PlatformEffect::CreateWindow { binding: actual, .. } if *actual == binding)
    }));

    for (generation, presentation, readiness) in [
        (
            2,
            WindowPresentationState::Hidden,
            crate::platform::NativeCreateReadiness::Created,
        ),
        (
            3,
            WindowPresentationState::Visible,
            crate::platform::NativeCreateReadiness::Prepared,
        ),
    ] {
        publish_source_windows(
            &mut fixture,
            generation,
            vec![
                observed_source_window(host, generation, 0.0, WindowPresentationState::Visible),
                observed_source_window(source, generation, 500.0, WindowPresentationState::Visible),
                observed_source_window(binding, generation, 1000.0, presentation)
                    .with_create_preparation(
                        crate::platform::WindowCreatePreparationObservation::new(
                            binding,
                            CoordinateObservationGeneration::new(generation),
                            Authority::Known(readiness),
                            (generation == 2).then_some(create),
                        ),
                    ),
            ],
            Vec::new(),
        );
    }
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(saga)
            .unwrap()
            .phase(),
        NativeCreatePhase::AwaitingFirstLivePresentation { .. },
    ));
    let old_output = emit_pending_content(&mut fixture, target);
    assert_eq!(
        old_output.endpoint(),
        HostPresentationEndpoint::Native(binding)
    );
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SOURCE_SURFACE)
            .is_none()
    );
    assert!(
        fixture
            .engine
            .presentation_requirements()
            .surface(target)
            .is_some()
    );

    let unavailable_host =
        ObservedWindow::new(host).with_coordinate_observation(WindowCoordinateObservation::new(
            host,
            CoordinateObservationGeneration::new(4),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ));
    let destroyed = publish_source_windows(
        &mut fixture,
        4,
        vec![
            unavailable_host,
            observed_source_window(binding, 4, 1000.0, WindowPresentationState::Visible)
                .with_create_preparation(crate::platform::WindowCreatePreparationObservation::new(
                    binding,
                    CoordinateObservationGeneration::new(4),
                    Authority::Known(crate::platform::NativeCreateReadiness::Prepared),
                    None,
                )),
        ],
        vec![WindowCloseObservation::new(
            source,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )],
    );
    let recovery = fixture
        .engine
        .pending_surface_recovery(SOURCE_SURFACE)
        .expect("the destroyed source must acquire a pending recovery roster");
    assert!(recovery.matches_workspace(&workspace));
    assert_eq!(
        fixture
            .engine
            .viewport()
            .recovery_pending(SOURCE_SURFACE)
            .expect("the original source destruction must retain a recovery owner")
            .source_binding(),
        source,
    );
    assert!(matches!(
        fixture.engine.surface_recovery.blocked(SOURCE_SURFACE),
        Some(SurfaceRecoveryBlockedReason::HostAuthorityUnavailable { surface }) if *surface == TARGET_SURFACE,
    ));
    assert_eq!(
        fixture.engine.workspace(),
        &workspace,
        "source recovery must not transfer any content"
    );
    assert_eq!(
        fixture.engine.version(),
        version,
        "only lifecycle authority changed"
    );
    assert!(
        fixture.engine.viewport().native_create_saga(saga).is_none(),
        "recovery must revoke the target during this snapshot's requirement rebuild"
    );
    assert!(fixture.engine.viewport().viewport(target).is_none());
    assert!(fixture.engine.scene().surface(target).is_none());
    assert!(
        fixture
            .engine
            .presentation_requirements()
            .surface(target)
            .is_none()
    );
    assert!(matches!(
        fixture.engine.begin_surface_contribution(target),
        Err(SurfaceContributionBeginError::SurfaceOutsideRoster { surface }) if surface == target,
    ));
    assert_eq!(
        destroyed.platform_effects().iter().filter(|effect| {
            matches!(effect.effect(), PlatformEffect::CompensatingClose { binding: actual, .. } if *actual == binding)
        }).count(),
        1,
        "the revoked target must enter its original native-create cleanup exactly once",
    );
    let retirement = fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(actual, retirement)| (actual == binding).then(|| retirement.clone()))
        .expect("the cancelled target must retain its exact cleanup owner");
    assert_eq!(
        retirement.origin(),
        crate::frame::BindingRetirementOrigin::NativeCreateAborted { create }
    );
    assert!(
        fixture
            .engine
            .viewport()
            .retained_native_staging_resources()
            .any(|descriptor| descriptor.id() == resource)
    );

    let late = observe_background_presentation_output(&mut fixture, old_output);
    assert!(
        late.presentation_observations()
            .iter()
            .any(|outcome| matches!(
                outcome,
                crate::presentation_observation::HostPresentationObservationOutcome::Retired {
                    stream,
                    settled_through,
                    presented: Authority::Known(Some(presented)),
                    promotion_eligible: false,
                    ..
                } if *stream == old_output.stream()
                    && *settled_through == old_output.key()
                    && *presented == old_output.key(),
            )),
        "the old exact Presented output must settle its own emission"
    );
    assert!(
        late.platform_effects().is_empty(),
        "late Presented must not readmit or reissue cleanup"
    );
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SOURCE_SURFACE)
            .is_some()
    );
    assert!(fixture.engine.viewport().native_create_saga(saga).is_none());
    assert!(fixture.engine.viewport().viewport(target).is_none());
    assert!(fixture.engine.interaction_authority(target).is_none());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .find_map(|(actual, current)| (actual == binding).then_some(current)),
        Some(&retirement)
    );
    fixture
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("the late output must preserve ordinary create-resource conservation");

    let mut session =
        DockspaceSession::from_engine_for_test(fixture.engine, fixture.presentation_host);
    let mut frame = session
        .begin_host_frame()
        .expect("the public measurement frame must begin");
    assert!(!frame.surfaces().contains(&target));
    let error = frame
        .measure_surface_with(target, |_| {
            panic!("the revoked target must be rejected before requesting any measurements");
        })
        .expect_err("public measurement must reject the removed target at roster lookup");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
}

#[test]
fn source_recovery_revokes_later_direct_prepared_action_in_the_same_snapshot() {
    let mut fixture = native_child_source_fixture();
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::InstallMainRoot {
                surface: TARGET_SURFACE,
                root: RootId::new(3),
                content: RootContent::OpenItem(ItemId::new(4)),
            },
        },
    )
    .expect("the recovery host must retain its main root when B leaves");
    publish_background_scene(&mut fixture);
    let source = background_source_binding(&fixture);
    let host = fixture
        .engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .unwrap()
        .binding();
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();

    // Reserve B first so its first-live action rebuilds requirements before A's.
    let requests = [(TARGET_ROOT, 1000.0), (SOURCE_ROOT, 1400.0)].map(|(root, x)| {
        let expected = fixture.engine.version();
        let created = submit_test_input(
            &mut fixture.engine,
            fixture.presentation_host,
            EngineInput::TearOffNative {
                expected,
                source: crate::model::NativeTearOffSource::Root(root),
                placement: Some(crate::model::NativeWindowPlacement::new(
                    PhysicalRect::new(x, 0.0, 400.0, 300.0).unwrap(),
                )),
            },
        )
        .expect("both independent roots must request real native creates");
        let target = match created.reduced_inputs()[0].outcome() {
            InputOutcome::ProductActionProcessed {
                outcome:
                    DockspaceActionOutcome::NativeRootTearOffRequested {
                        root: actual,
                        target_surface,
                        ..
                    },
                ..
            } if *actual == root => *target_surface,
            outcome => panic!("expected a real native create for {root}, got {outcome:?}"),
        };
        let request = fixture
            .engine
            .viewport()
            .native_create_sagas()
            .find_map(|(saga, create)| {
                (create.binding().surface() == target).then_some((
                    saga,
                    create.binding(),
                    create.create(),
                    create.resource(),
                ))
            })
            .expect("each accepted command must retain its own native create saga");
        assert!(created.platform_effects().iter().any(|effect| {
            effect.id() == request.2
                && matches!(
                    effect.effect(),
                    PlatformEffect::CreateWindow {
                        binding,

                        ..
                    } if *binding == request.1
                )
        }));
        request
    });
    let [
        (b_saga, b_binding, _, b_resource),
        (a_saga, a_binding, a_create, a_resource),
    ] = requests;
    assert!(source.surface() < b_binding.surface());
    assert!(b_binding.surface() < a_binding.surface());

    let unavailable_host = observed_source_window(host, 2, 0.0, WindowPresentationState::Visible)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            host,
            CoordinateObservationGeneration::new(2),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ));
    let mut windows = vec![unavailable_host];
    windows.extend(
        requests
            .into_iter()
            .enumerate()
            .map(|(index, (_, binding, create, _))| {
                observed_source_window(
                    binding,
                    2,
                    1000.0 + index as f64 * 400.0,
                    WindowPresentationState::Visible,
                )
                .with_create_preparation(
                    crate::platform::WindowCreatePreparationObservation::new(
                        binding,
                        CoordinateObservationGeneration::new(2),
                        Authority::Known(crate::platform::NativeCreateReadiness::Prepared),
                        Some(create),
                    ),
                )
            }),
    );
    let destroyed = publish_source_windows(
        &mut fixture,
        2,
        windows,
        vec![WindowCloseObservation::new(
            source,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )],
    );
    let actions = match destroyed.reduced_inputs()[0].outcome() {
        InputOutcome::PlatformSnapshotPublished { transition, .. } => transition.actions(),
        outcome => panic!("the real coordinator snapshot must publish, got {outcome:?}"),
    };
    assert!(matches!(
        actions,
        [
            crate::frame::ViewportLifecycleAction::SurfaceDestroyed { observation, .. },
            crate::frame::ViewportLifecycleAction::PrepareNativeCreateFirstLive { saga: b, .. },
            crate::frame::ViewportLifecycleAction::PrepareNativeCreateFirstLive { saga: a, .. },
        ] if observation.binding() == source && *b == b_saga && *a == a_saga
    ));
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert!(matches!(
        fixture.engine.surface_recovery.blocked(SOURCE_SURFACE),
        Some(SurfaceRecoveryBlockedReason::HostAuthorityUnavailable { surface })
            if *surface == TARGET_SURFACE
    ));
    assert!(
        fixture
            .engine
            .pending_surface_recovery(SOURCE_SURFACE)
            .unwrap()
            .matches_workspace(&workspace)
    );
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_saga(a_saga)
            .is_none()
    );
    assert!(
        fixture
            .engine
            .viewport()
            .viewport(a_binding.surface())
            .is_none()
    );
    assert!(
        fixture
            .engine
            .scene()
            .surface(a_binding.surface())
            .is_none()
    );
    assert!(matches!(
        fixture
            .engine
            .viewport()
            .native_create_saga(b_saga)
            .unwrap()
            .phase(),
        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert!(
        fixture
            .engine
            .scene()
            .surface(b_binding.surface())
            .is_some()
    );
    assert!(
        fixture
            .engine
            .native_admission
            .owns_native_create_barrier(b_saga, b_binding, b_resource)
    );
    assert!(
        !fixture
            .engine
            .native_admission
            .owns_native_create_barrier(a_saga, a_binding, a_resource)
    );
    assert_eq!(
        destroyed
            .platform_effects()
            .iter()
            .filter(|effect| {
                matches!(effect.effect(), PlatformEffect::CompensatingClose { binding, compensates }
                if *binding == a_binding && *compensates == a_create)
            })
            .count(),
        1,
        "revoking A must retain exactly one cleanup owner"
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .binding_retirements()
            .find_map(|(binding, retirement)| {
                (binding == a_binding).then_some(retirement.origin())
            }),
        Some(crate::frame::BindingRetirementOrigin::NativeCreateAborted { create: a_create })
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .retained_native_staging_resources()
            .map(|descriptor| descriptor.id())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([a_resource, b_resource])
    );

    // Replaying an already advanced B or a withdrawn A must not reject either owner again.
    let repeated = [actions[1].clone(), actions[2].clone(), actions[2].clone()];
    let input = fixture.engine.last_input;
    let provider = fixture.engine.platform_provider().unwrap();
    let focus_causal = mint_test_focus_causal(&mut fixture.engine);
    let recovery_batch = fixture
        .engine
        .freeze_surface_recovery_batch(input, &repeated)
        .unwrap();
    let viewport = fixture.engine.viewport().clone();
    let admission = fixture.engine.native_admission.clone();
    let mut activations = Vec::new();
    let mut events = Vec::new();
    let mut interaction_events = Vec::new();
    fixture
        .engine
        .reduce_viewport_actions(
            input,
            provider,
            focus_causal,
            &repeated,
            &recovery_batch,
            &mut activations,
            &mut events,
            &mut interaction_events,
        )
        .expect("known withdrawn or advanced actions must be idempotently inert");
    assert_eq!(fixture.engine.viewport(), &viewport);
    assert_eq!(fixture.engine.native_admission, admission);
    assert!(activations.is_empty());
    assert!(events.is_empty());
    assert!(interaction_events.is_empty());

    let mut unexplained = actions[2].clone();
    let missing = crate::ids::NativeCreateSagaId::new(u64::MAX);
    let crate::frame::ViewportLifecycleAction::PrepareNativeCreateFirstLive { saga, .. } =
        &mut unexplained
    else {
        unreachable!();
    };
    *saga = missing;
    let error = fixture
        .engine
        .reduce_viewport_actions(
            input,
            provider,
            focus_causal,
            &[unexplained],
            &recovery_batch,
            &mut activations,
            &mut events,
            &mut interaction_events,
        )
        .expect_err("an unrelated missing saga must remain an invariant error");
    assert!(matches!(
        error,
        EngineError::Viewport {
            source: crate::frame::ViewportCoordinatorError::MissingCreateSaga { saga },
            ..
        } if saga == missing
    ));
    assert_eq!(fixture.engine.viewport(), &viewport);
    assert_eq!(fixture.engine.native_admission, admission);
    fixture
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("A cleanup and B first-live must each retain their exact resource owner");
}
