use super::compiler::compile_window_fact;
use super::*;
use crate::effect::{EffectId, EffectLedger, PlatformEffect};
use crate::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{ItemId, RootId};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::policy::DockPolicy;
use crate::runtime::native_effect::{NativeEffectDropQueue, NativeEffectRequest};
use crate::runtime::{
    DockspaceSemanticOutput, NativeDispatchFailure, NativeReceiverPurpose, NativeReceiverQuery,
    PaintedSurfaceOutput, PresentedDockspaceSurface, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use crate::viewport::{InventoryGeneration, WindowIncarnation};

mod managed_lifecycle;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const WINDOW: HostWindowToken = HostWindowToken::new(41);

#[test]
fn admission_diff_reports_only_the_same_pending_binding() {
    let domain = crate::ids::EngineAuthorityDomainId::new_for_test(1);
    let pending = ViewportBinding::new(
        domain,
        crate::ids::WorkspaceEpoch::new(1),
        SURFACE,
        WindowToken::new(1),
        WindowIncarnation::new(1),
    );
    let replacement = ViewportBinding::new(
        domain,
        crate::ids::WorkspaceEpoch::new(1),
        SURFACE,
        WindowToken::new(1),
        WindowIncarnation::new(2),
    );

    assert_eq!(
        newly_admitted_bindings(
            &BTreeMap::from([(pending, ViewportAdmission::Pending)]),
            &BTreeMap::from([(pending, ViewportAdmission::Admitted)]),
        ),
        vec![pending]
    );
    assert!(
        newly_admitted_bindings(
            &BTreeMap::from([(pending, ViewportAdmission::Pending)]),
            &BTreeMap::from([(replacement, ViewportAdmission::Admitted)]),
        )
        .is_empty(),
        "a recreated binding cannot inherit its predecessor's admission"
    );
    assert!(
        newly_admitted_bindings(
            &BTreeMap::new(),
            &BTreeMap::from([(pending, ViewportAdmission::Admitted)]),
        )
        .is_empty(),
        "an already-admitted external registration is not a first-live transition"
    );
}

fn native_root_session() -> (DockspaceSession, NativeSurfaceBinding) {
    native_root_session_with_profile(NativeHostProfile::ObservedRoots)
}

fn native_root_session_with_profile(
    profile: NativeHostProfile,
) -> (DockspaceSession, NativeSurfaceBinding) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_workspace_for_test(
        builder
            .build()
            .expect("the native test workspace validates"),
        DockPolicy::default(),
    )
    .expect("the native test session initializes");
    match profile {
        NativeHostProfile::ObservedRoots => session
            .enable_observed_native_roots()
            .expect("the observed-root provider enrolls"),
        NativeHostProfile::ManagedDesktop => session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("the managed provider enrolls"),
    };
    session
        .register_native_root(SURFACE, WINDOW)
        .expect("the root registration records");
    let mut frame = session
        .begin_host_frame()
        .expect("the registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the registration frame settles every surface");
    let report = frame.commit().expect("the registration frame commits");
    let binding = match report.inputs() {
        [super::super::HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    (session, binding)
}

fn paint_native_output(
    session: &mut DockspaceSession,
) -> (PaintedSurfaceOutput, DockspaceSemanticOutput) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
        80.0,
    )
    .expect("test metrics validate");
    let mut measured = session
        .begin_host_frame()
        .expect("native measurement frame begins");
    measured
        .measure_surface(SURFACE, metrics)
        .expect("native surface measurements stage");
    measured
        .commit()
        .expect("native surface measurements commit");

    let mut painted = session
        .begin_host_frame()
        .expect("native paint frame begins");
    let semantic_output = painted
        .paint_plan(SURFACE)
        .expect("native paint plan lookup succeeds")
        .expect("native paint plan is ready")
        .semantic_output();
    painted
        .confirm_surface_painted(SURFACE)
        .expect("native surface paint stages");
    let mut report = painted.commit().expect("native surface paint emits");
    let output = report
        .take_painted_outputs()
        .pop()
        .expect("native paint emits one exact output");
    (output, semantic_output)
}

fn foreign_semantic_output() -> DockspaceSemanticOutput {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_workspace_for_test(
        builder
            .build()
            .expect("the foreign semantic workspace validates"),
        DockPolicy::default(),
    )
    .expect("the foreign semantic session initializes");
    paint_native_output(&mut session).1
}

#[test]
fn painted_output_matches_the_exact_native_binding() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let (_foreign_session, foreign_binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("native bounds validate");
    let scale = ScaleFactor::new(1.0).expect("native scale validates");
    let facts = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot(
            [(binding, facts)],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("native inventory records");
    let mut observed = session
        .begin_host_frame()
        .expect("native inventory frame begins");
    observed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("native inventory frame settles the surface");
    observed.commit().expect("native inventory commits");
    let (output, semantic_output) = paint_native_output(&mut session);

    assert!(output.matches_native_binding(binding));
    assert!(!output.matches_native_binding(foreign_binding));
    assert!(output.matches_semantic_output(semantic_output));
    assert!(!output.matches_semantic_output(foreign_semantic_output()));
}

#[test]
fn native_receiver_query_binds_only_the_exact_presented_output_and_binding() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let (_foreign_session, foreign_binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("native bounds validate");
    let scale = ScaleFactor::new(1.0).expect("native scale validates");
    let facts = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot(
            [(binding, facts)],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("native inventory records");
    commit_managed_frame(&mut session);
    let (output, semantic_output) = paint_native_output(&mut session);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact native output is retained for presentation");
    commit_managed_frame(&mut session);

    let mut frame = session
        .begin_host_frame()
        .expect("the presented paint plan can be inspected");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("the native surface remains ready");
    let tab = plan.tabs().next().expect("the root contains one tab");
    let descriptor = plan
        .receiver_for_tab_body(tab)
        .expect("the tab has an exact pointer receiver");
    drop(frame);

    let projection = session
        .engine
        .interaction_projection(SURFACE)
        .expect("the presented output grants interaction authority");
    let query = NativeReceiverQuery {
        purpose: NativeReceiverPurpose::DragDelivery,
        presented_surface: PresentedDockspaceSurface::from_projection(projection),
        point: Some(descriptor.center()),
    };

    assert_eq!(query.surface(), SURFACE);
    assert!(query.matches_native_binding(binding));
    assert!(!query.matches_native_binding(foreign_binding));
    assert!(query.matches_semantic_output(semantic_output));
    assert!(!query.matches_semantic_output(foreign_semantic_output()));
    assert!(query.bind_receiver(&descriptor).is_some());

    let hover_query = NativeReceiverQuery {
        purpose: NativeReceiverPurpose::HoverHit,
        ..query
    };
    assert!(hover_query.bind_receiver(&descriptor).is_none());
}

#[test]
fn live_window_facts_leave_independent_authority_unknown() {
    let (_session, binding) = native_root_session();
    let compiled = compile_window_fact(
        binding.provider,
        binding.binding,
        NativeWindowFacts::live(),
        1,
        1,
    )
    .expect("inventory-only facts compile");
    let window = compiled
        .window
        .expect("a live binding remains in inventory");
    let coordinates = window
        .coordinate_observation()
        .expect("the provider advances an explicit coordinate tombstone");
    let input = window
        .input_observation()
        .expect("the provider advances an explicit input tombstone");
    let presentation = window
        .presentation_observation()
        .expect("the provider advances an explicit presentation tombstone");

    assert!(compiled.is_live);
    assert!(!coordinates.has_authority());
    assert!(matches!(
        coordinates.content_bounds(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        coordinates.outer_bounds(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        coordinates.native_scale_factor(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        coordinates.presentation_scale_factor(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        window.input_state(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        input.state(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        presentation.state(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        window.close_requested(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(matches!(
        compiled.close.state(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
}

#[test]
fn unknown_inventory_compilation_does_not_infer_destruction() {
    let snapshot = compile_unknown_inventory_snapshot(NativeHostProfile::ObservedRoots, 1)
        .expect("the provider compiles an inventory tombstone");
    let capabilities = snapshot
        .capability_observation()
        .known_roster()
        .expect("the observed-root capability roster is exact");
    assert!(capabilities.authoritative_inventory().is_supported());
    assert!(!capabilities.native_window_lifecycle().is_supported());
    assert!(matches!(
        snapshot.inventory_observation().roster(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(snapshot.window_observations().is_empty());
    assert!(snapshot.close_observations().is_empty());
}

#[test]
fn live_binding_cannot_be_declared_quiescent() {
    let (mut session, binding) = native_root_session();

    let error = session
        .report_native_binding_quiescence(binding)
        .expect_err("a live binding cannot be declared quiescent");
    assert!(matches!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    ));
}

#[test]
fn repeated_native_enable_never_reissues_live_bindings() {
    let (mut session, binding) = native_root_session();

    let error = session
        .enable_observed_native_roots()
        .expect_err("repeated enrollment cannot recapture the current binding roster");
    assert!(matches!(
        error.native_kind(),
        Some(NativeHostErrorKind::AlreadyEnabled)
    ));
    session
        .report_native_snapshot([(binding, NativeWindowFacts::live())])
        .expect("the originally issued binding remains the sole live capability");
}

#[test]
fn existing_owned_child_bootstrap_releases_after_exact_live_observation() {
    let child_surface = SurfaceId::new(2);
    let child_root = RootId::new(2);
    let layout = crate::model::DockspaceLayout::new([
        crate::model::DockspaceSurfaceLayout::new(
            SURFACE,
            crate::model::DockspaceRootLayout::new(ROOT, crate::model::DockspaceNode::tabs([ITEM])),
        ),
        crate::model::DockspaceSurfaceLayout::new(
            child_surface,
            crate::model::DockspaceRootLayout::new(
                child_root,
                crate::model::DockspaceNode::tabs([ItemId::new(2)]),
            ),
        ),
    ])
    .expect("the child bootstrap layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("the child bootstrap session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native provider enrolls");
    session
        .register_native_root(SURFACE, WINDOW)
        .expect("the recovery host registration records");
    let mut host = session
        .begin_host_frame()
        .expect("the recovery host frame begins");
    host.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the recovery host frame settles every surface");
    let host_report = host
        .commit()
        .expect("the recovery host registration commits");
    let root_binding = match host_report.inputs() {
        [super::super::HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one recovery root registration, got {outcomes:?}"),
    };

    session
        .register_owned_native_child(child_surface, HostWindowToken::new(42), SURFACE)
        .expect("the product child bootstrap records");
    let mut child = session
        .begin_host_frame()
        .expect("the child bootstrap frame begins");
    child
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child bootstrap frame settles every surface");
    let report = child.commit().expect("the child bootstrap commits");

    assert!(matches!(
        report.inputs(),
        [super::super::HostInputOutcome::NativeSurfaceRegistered { binding }]
            if binding.surface() == child_surface
    ));
    let child_binding = match report.inputs() {
        [super::super::HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one owned child registration, got {outcomes:?}"),
    };
    session
        .report_managed_native_snapshot(
            [
                (root_binding, NativeWindowFacts::live()),
                (child_binding, NativeWindowFacts::live()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the exact live inventory records");
    commit_managed_frame(&mut session);

    let mut redock = session
        .begin_host_frame()
        .expect("the owned child redock frame begins");
    redock
        .dock_root_current(
            child_root,
            crate::model::DockPlacement::Center(crate::model::DockAnchor::Item(ITEM)),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the owned child redock settles every surface");
    let mut report = redock.commit().expect("the owned child redock commits");
    let effects = report.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [effect]
            if matches!(
                effect.operation(),
                super::super::NativeEffectOperation::ReleaseChild { binding }
                    if *binding == child_binding
            )
    ));
    let acknowledgement = effects
        .into_iter()
        .next()
        .expect("the release effect exists")
        .accepted();
    assert!(matches!(
        acknowledgement,
        Some(super::super::NativeEffectAcknowledgement::Close(_))
    ));
}

#[test]
fn native_frame_distinguishes_missing_provider_from_wrong_profile() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_workspace_for_test(
        builder.build().expect("the native workspace validates"),
        DockPolicy::default(),
    )
    .expect("the native session initializes");

    let missing = match session.begin_native_host_frame(|_| NativeReceiverAnswer::Unknown) {
        Ok(_) => panic!("a native frame cannot begin before provider enrollment"),
        Err(error) => error,
    };
    assert_eq!(missing.native_kind(), Some(NativeHostErrorKind::NotEnabled));

    session
        .enable_observed_native_roots()
        .expect("the observed-root provider enrolls");
    let wrong_profile = match session.begin_native_host_frame(|_| NativeReceiverAnswer::Unknown) {
        Ok(_) => panic!("the observed-root profile cannot drive desktop pointer input"),
        Err(error) => error,
    };
    assert_eq!(
        wrong_profile.native_kind(),
        Some(NativeHostErrorKind::Unsupported)
    );
}

#[test]
fn destroyed_fact_can_acknowledge_the_exact_destructive_effect() {
    let (_session, binding) = native_root_session();
    let effect = EffectId::new(7);
    let acknowledgement = NativeCloseEffectAcknowledgement {
        provider: binding.provider,
        binding: binding.binding,
        effect,
    };
    let compiled = compile_window_fact(
        binding.provider,
        binding.binding,
        NativeWindowFacts::destroyed_after(acknowledgement),
        1,
        1,
    )
    .expect("an exact destructive acknowledgement compiles");

    assert!(!compiled.is_live);
    assert_eq!(compiled.window, None);
    assert_eq!(
        compiled.close.known_state(),
        Some(WindowCloseState::Destroyed)
    );
    assert!(compiled.close.acknowledges(effect));
}

#[test]
fn retired_binding_accepts_only_an_exact_destroyed_tombstone() {
    let (mut session, binding) = native_root_session();
    let native = session
        .native
        .as_mut()
        .expect("the native provider remains enrolled");
    assert_eq!(native.bindings.remove(&binding.surface()), Some(binding));
    native.retired_bindings.insert(binding.binding);

    assert!(session.recognizes_native_binding(binding));
    assert!(!session.is_current_native_binding(binding));
    session
        .report_native_snapshot([(binding, NativeWindowFacts::destroyed())])
        .expect("the exact retired tombstone is recordable");

    let (mut live_session, live_binding) = native_root_session();
    let native = live_session
        .native
        .as_mut()
        .expect("the native provider remains enrolled");
    assert_eq!(
        native.bindings.remove(&live_binding.surface()),
        Some(live_binding)
    );
    native.retired_bindings.insert(live_binding.binding);
    let error = live_session
        .report_native_snapshot([(live_binding, NativeWindowFacts::live())])
        .expect_err("a retired binding cannot regain live authority");
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
}

#[test]
fn retired_binding_accepts_same_provider_effect_results() {
    let (mut session, binding) = native_root_session();
    let native = session
        .native
        .as_mut()
        .expect("the native provider remains enrolled");
    assert_eq!(native.bindings.remove(&binding.surface()), Some(binding));
    native.retired_bindings.insert(binding.binding);

    let mut ledger = EffectLedger::default();
    ledger
        .request(PlatformEffect::ReleaseChild {
            binding: binding.binding,
        })
        .expect("the destructive effect allocates");
    let predecessor_emission = ledger
        .take_new_requests(binding.provider, InventoryGeneration::new(1), |_| false)
        .expect("the destructive effect emits")
        .pop()
        .expect("one destructive emission exists");
    let delayed =
        NativeEffectRequest::from_emission(&predecessor_emission, NativeEffectDropQueue::default())
            .dispatch_failed(NativeDispatchFailure::WindowUnavailable);
    session
        .report_native_effect_result(delayed)
        .expect("the active provider may settle an emitted retired-binding effect");
}

#[test]
fn retired_binding_requires_cleanup_correlation_after_workspace_replacement() {
    let (mut session, binding) = native_root_session();
    let before = session.version();
    let mut ledger = EffectLedger::default();
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild {
            binding: binding.binding,
        })
        .expect("the destructive effect allocates");
    let predecessor_emission = ledger
        .take_new_requests(binding.provider, InventoryGeneration::new(1), |_| false)
        .expect("the destructive effect emits")
        .pop()
        .expect("one destructive emission exists");
    let delayed =
        NativeEffectRequest::from_emission(&predecessor_emission, NativeEffectDropQueue::default())
            .dispatch_failed(NativeDispatchFailure::WindowUnavailable);

    let replacement = session.workspace().clone();
    let mut frame = session
        .begin_host_frame()
        .expect("the workspace replacement frame begins");
    frame
        .append(crate::engine::EngineInput::ReplaceWorkspace(replacement))
        .expect("the replacement input appends");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the replacement frame settles every surface");
    frame
        .commit()
        .expect("the workspace replacement frame commits");

    let after = session.version();
    assert_ne!(after.epoch(), before.epoch());
    assert!(session.recognizes_native_binding(binding));
    assert!(!session.is_current_native_binding(binding));

    let error = session
        .report_native_effect_result(delayed)
        .expect_err("an old-epoch result cannot directly settle a retired binding");
    assert_eq!(error.kind(), NativeHostErrorKind::StaleBinding);
    let (_, delayed) = error.into_parts();
    assert_eq!(delayed.receipt_epoch(), before.epoch());
    assert!(delayed.can_be_correlated_by_cleanup());

    ledger
        .request_in(
            after.epoch(),
            PlatformEffect::ContinueCleanup {
                binding: binding.binding,
                predecessor,
                after: None,
            },
        )
        .expect("the successor cleanup observation allocates");
    let cleanup_emission = ledger
        .take_new_requests(binding.provider, InventoryGeneration::new(2), |_| false)
        .expect("the cleanup observation emits")
        .pop()
        .expect("one cleanup observation exists");
    let cleanup =
        NativeEffectRequest::from_emission(&cleanup_emission, NativeEffectDropQueue::default());
    let Some(crate::runtime::NativeEffectAcknowledgement::Cleanup(observation)) =
        cleanup.accepted()
    else {
        panic!("cleanup continuation yields one observation capability");
    };
    let correlated = observation
        .correlate(delayed)
        .expect("the cleanup observation correlates the delayed result");
    assert_eq!(correlated.receipt_epoch(), after.epoch());
    session
        .report_native_effect_result(correlated)
        .expect("the current-epoch cleanup result may settle the retired binding");
}

fn commit_managed_frame(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the managed frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the managed frame settles every surface");
    frame.commit().expect("the managed frame commits");
}

fn work_area(token: u64) -> NativeWorkAreaFacts {
    NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(token),
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("work area is valid"),
        ScaleFactor::new(1.0).expect("scale is valid"),
    )
}

#[test]
fn native_capability_profiles_are_fixed_and_honest() {
    let observed = compile_unknown_inventory_snapshot(NativeHostProfile::ObservedRoots, 1)
        .expect("observed capabilities compile");
    let observed = observed
        .capability_observation()
        .known_roster()
        .expect("observed capability roster is exact");
    assert!(!observed.native_window_lifecycle().is_supported());
    assert!(observed.authoritative_inventory().is_supported());
    assert!(!observed.hovered_window().is_supported());
    assert!(!observed.desktop_pointer_position().is_supported());
    assert!(!observed.authoritative_button_state().is_supported());
    assert!(!observed.global_window_placement().is_supported());
    assert!(!observed.work_area().is_supported());
    assert!(!observed.pointer_hit_test_observation().is_supported());
    assert!(!observed.pointer_hit_test_control().is_supported());
    assert!(!observed.global_focus_observation().is_supported());
    assert!(!observed.window_activation_control().is_supported());
    assert!(!observed.close_cancellation().is_supported());

    let managed = compile_unknown_inventory_snapshot(NativeHostProfile::ManagedDesktop, 1)
        .expect("managed capabilities compile");
    let managed = managed
        .capability_observation()
        .known_roster()
        .expect("managed capability roster is exact");
    assert!(managed.native_window_lifecycle().is_supported());
    assert!(managed.authoritative_inventory().is_supported());
    assert!(managed.hovered_window().is_supported());
    assert!(managed.desktop_pointer_position().is_supported());
    assert!(managed.authoritative_button_state().is_supported());
    assert!(managed.global_window_placement().is_supported());
    assert!(managed.work_area().is_supported());
    assert!(managed.pointer_hit_test_observation().is_supported());
    assert!(!managed.pointer_hit_test_control().is_supported());
    assert!(!managed.global_focus_observation().is_supported());
    assert!(!managed.window_activation_control().is_supported());
    assert!(managed.close_cancellation().is_supported());
}

#[test]
fn narrow_close_and_complete_snapshots_advance_independent_generations() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the initial complete snapshot records");
    commit_managed_frame(&mut session);

    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .expect("the narrow close observation records");
    commit_managed_frame(&mut session);

    session
        .report_native_inventory_unknown()
        .expect("an independent inventory tombstone records");
    commit_managed_frame(&mut session);

    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live().with_close(NativeCloseState::Requested, None),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the later complete snapshot keeps every stream contiguous");
    commit_managed_frame(&mut session);
}

#[test]
fn managed_work_area_binding_exists_only_after_commit_and_revisions_expire() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let token = HostWorkAreaToken::new(7);
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Exact(vec![work_area(7)]),
        )
        .expect("the exact snapshot records");
    assert_eq!(session.native_work_area(token), None);
    commit_managed_frame(&mut session);
    let first = session
        .native_work_area(token)
        .expect("commit mints exact work-area authority");

    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the explicit tombstone records");
    assert_eq!(session.native_work_area(token), Some(first));
    commit_managed_frame(&mut session);
    assert_eq!(session.native_work_area(token), None);
}

#[test]
fn invalid_managed_work_area_roster_is_rejected_atomically() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let error = session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Exact(Vec::new()),
        )
        .expect_err("an exact work-area roster cannot be empty");
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));

    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Exact(vec![work_area(7)]),
        )
        .expect("a later valid snapshot remains recordable");
    commit_managed_frame(&mut session);
    assert!(
        session
            .native_work_area(HostWorkAreaToken::new(7))
            .is_some()
    );
}

#[test]
fn empty_managed_frame_does_not_synthesize_pointer_or_backend_input() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Exact(vec![work_area(7)]),
        )
        .expect("the managed snapshot records");
    commit_managed_frame(&mut session);
    let native = session
        .native
        .as_ref()
        .expect("managed state remains active");
    let recorded_before = native.recorder.recorded_through();
    let pointer_before = native.recorder.pointer_through();

    commit_managed_frame(&mut session);

    let native = session
        .native
        .as_ref()
        .expect("managed state remains active");
    assert_eq!(native.recorder.recorded_through(), recorded_before);
    assert_eq!(native.recorder.pointer_through(), pointer_before);
}

#[test]
fn managed_pointer_enrollment_accepts_explicit_unknown_authority() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_workspace_for_test(
        builder.build().expect("the native workspace validates"),
        DockPolicy::default(),
    )
    .expect("the native session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .expect("explicit unavailable authority enrolls atomically");
    let input = NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::Moved,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    );
    session
        .record_native_pointer(input)
        .expect("the edge records after atomic authority enrollment");
}

#[test]
fn invalid_native_pointer_roster_is_rejected_before_provider_enrollment() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_workspace_for_test(
        builder.build().expect("the native workspace validates"),
        DockPolicy::default(),
    )
    .expect("the native session initializes");
    let duplicate = NativePointerState::new(
        NativePointerId::new(1),
        [NativePointerButton::Primary, NativePointerButton::Primary],
        NativePointerOwner::None,
    );
    let error = session
        .enable_managed_native_host(NativePointerRoster::Exact(vec![duplicate]))
        .expect_err("duplicate buttons cannot enroll a provider");
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));

    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("rejected facts leave provider enrollment available");
}
