use super::compiler::compile_window_fact;
use super::*;
use crate::effect::EffectId;
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{ItemId, RootId};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::policy::DockPolicy;
use crate::runtime::SurfaceUnavailableReason;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const WINDOW: HostWindowToken = HostWindowToken::new(41);

fn native_root_session() -> (DockspaceSession, NativeSurfaceBinding) {
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
    session
        .enable_native_platform(NativePlatformMode::ObservedRoots)
        .expect("the test provider enrolls");
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
fn unknown_inventory_capture_does_not_infer_destruction() {
    let (session, binding) = native_root_session();

    let snapshot = session
        .capture_native_inventory_unknown()
        .expect("the active provider captures an inventory tombstone");

    assert_eq!(snapshot.provider, binding.provider);
    assert_eq!(snapshot.bindings, vec![binding.binding]);
    assert!(matches!(
        snapshot.snapshot.inventory_observation().roster(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(snapshot.snapshot.window_observations().is_empty());
    assert!(snapshot.snapshot.close_observations().is_empty());
}

#[test]
fn live_binding_cannot_be_declared_quiescent() {
    let (mut session, binding) = native_root_session();

    let error = session
        .report_native_binding_quiescence(binding)
        .expect_err("a live binding cannot be declared quiescent");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::BindingStillLive { surface: SURFACE })
    ));
}

#[test]
fn repeated_native_enable_never_reissues_live_bindings() {
    let (mut session, binding) = native_root_session();

    let error = session
        .enable_native_platform(NativePlatformMode::ObservedRoots)
        .expect_err("repeated enrollment cannot recapture the current binding roster");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::ProviderAlreadyEnabled)
    ));
    session
        .capture_native_snapshot([(binding, NativeWindowFacts::live())])
        .expect("the originally issued binding remains the sole live capability");
}

#[test]
fn joined_provider_replacement_rotates_surface_capabilities() {
    let (mut session, predecessor) = native_root_session();

    session
        .begin_native_provider_replacement()
        .expect("the predecessor drains into one joined reservation");
    assert!(matches!(
        session.capture_native_snapshot([(predecessor, NativeWindowFacts::live())]),
        Err(NativePlatformError::ProviderReplacementPending)
    ));
    let error = session
        .register_native_root(SURFACE, WINDOW)
        .expect_err("registration waits for provider replacement");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::ProviderReplacementPending)
    ));

    let successor = session
        .finish_native_provider_replacement()
        .expect("the joined successor activates")
        .into_iter()
        .find(|binding| binding.surface() == SURFACE)
        .expect("the successor reports the exact binding roster");
    assert_ne!(successor, predecessor);
    assert_eq!(successor.surface(), predecessor.surface());
    assert_eq!(successor.window_token(), predecessor.window_token());
    assert!(matches!(
        session.capture_native_snapshot([(predecessor, NativeWindowFacts::live())]),
        Err(NativePlatformError::StaleSurface { surface: SURFACE })
    ));
    session
        .capture_native_snapshot([(successor, NativeWindowFacts::live())])
        .expect("the successor capability captures the current roster");
}

#[test]
fn replacement_abort_leaves_the_session_unenrolled_until_explicit_reenable() {
    let (mut session, predecessor) = native_root_session();
    session
        .begin_native_provider_replacement()
        .expect("the predecessor drains into one joined reservation");
    session
        .abort_native_provider_replacement()
        .expect("the reserved successor aborts");

    assert!(matches!(
        session.capture_native_snapshot(std::iter::empty()),
        Err(NativePlatformError::ProviderUnavailable)
    ));

    let successor = session
        .enable_native_platform(NativePlatformMode::ObservedRoots)
        .expect("the host explicitly enrolls a fresh provider")
        .into_iter()
        .find(|binding| binding.surface() == SURFACE)
        .expect("reenrollment returns the current binding roster");
    assert_ne!(successor, predecessor);
}

#[test]
fn predecessor_effect_acknowledgement_cannot_authorize_the_successor() {
    let (mut session, predecessor) = native_root_session();
    let stale_acknowledgement = NativeCloseEffectAcknowledgement {
        provider: predecessor.provider,
        binding: predecessor.binding,
        effect: EffectId::new(1),
    };
    session
        .begin_native_provider_replacement()
        .expect("the predecessor drains into one joined reservation");
    let successor = session
        .finish_native_provider_replacement()
        .expect("the joined successor activates")
        .into_iter()
        .find(|binding| binding.surface() == SURFACE)
        .expect("the successor reports the exact binding roster");

    let error = session
        .publish_native_close(
            successor,
            NativeCloseState::Clear,
            Some(stale_acknowledgement),
        )
        .expect_err("a predecessor acknowledgement cannot authorize the successor");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::EffectAcknowledgementProviderMismatch { surface: SURFACE })
    ));
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
