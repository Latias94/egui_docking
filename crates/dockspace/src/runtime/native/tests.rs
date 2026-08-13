use super::compiler::compile_window_fact;
use super::*;
use crate::effect::EffectId;
use crate::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{ItemId, RootId};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceSemanticOutput, NativeReceiverPurpose, NativeReceiverQuery, PaintedSurfaceOutput,
    PresentedDockspaceSurface, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const WINDOW: HostWindowToken = HostWindowToken::new(41);

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
    let mut session = DockspaceSession::from_backend_workspace(
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
    let mut session = DockspaceSession::from_backend_workspace(
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
fn native_frame_distinguishes_missing_provider_from_wrong_profile() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::from_backend_workspace(
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
    assert!(managed.pointer_hit_test_control().is_supported());
    assert!(!managed.global_focus_observation().is_supported());
    assert!(!managed.window_activation_control().is_supported());
    assert!(managed.close_cancellation().is_supported());
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
    let mut session = DockspaceSession::from_backend_workspace(
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
    let mut session = DockspaceSession::from_backend_workspace(
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
