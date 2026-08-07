use super::compiler::compile_window_fact;
use super::*;
use crate::effect::EffectId;
use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{ItemId, RootId};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::policy::DockPolicy;
use crate::scene_manifest::MeasurementUnavailableReason;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const WINDOW: HostWindowToken = HostWindowToken::new(41);

fn native_root_session() -> (DockspaceSession, NativeSurfaceLease) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = DockspaceSession::new(
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
        .complete_unpainted_surfaces(MeasurementUnavailableReason::Deferred)
        .expect("the registration frame settles every surface");
    let report = frame.commit().expect("the registration frame commits");
    let lease = match report.inputs() {
        [super::super::HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    (session, lease)
}

#[test]
fn live_window_facts_leave_independent_authority_unknown() {
    let (_session, lease) = native_root_session();
    let compiled = compile_window_fact(lease.provider, lease.binding, NativeWindowFacts::live(), 1)
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
    let (session, lease) = native_root_session();

    let snapshot = session
        .capture_native_inventory_unknown()
        .expect("the active provider captures an inventory tombstone");

    assert_eq!(snapshot.provider, lease.provider);
    assert_eq!(snapshot.bindings, vec![lease.binding]);
    assert!(matches!(
        snapshot.snapshot.inventory_observation().roster(),
        Authority::Unknown(AuthorityUnavailableReason::NotReported)
    ));
    assert!(snapshot.snapshot.window_observations().is_empty());
    assert!(snapshot.snapshot.close_observations().is_empty());
}

#[test]
fn live_binding_cannot_be_declared_quiescent() {
    let (mut session, lease) = native_root_session();

    assert!(matches!(
        session.report_native_binding_quiescence(lease),
        Err(DockspaceRuntimeError::Native(
            NativePlatformError::BindingStillLive { surface: SURFACE }
        ))
    ));
}

#[test]
fn joined_provider_replacement_rotates_surface_capabilities() {
    let (mut session, predecessor) = native_root_session();

    session
        .begin_native_provider_replacement()
        .expect("the predecessor drains into one joined reservation");
    assert_eq!(session.native_surface(SURFACE), None);
    assert!(matches!(
        session.capture_native_snapshot([(predecessor, NativeWindowFacts::live())]),
        Err(NativePlatformError::ProviderReplacementPending)
    ));
    assert!(matches!(
        session.register_native_root(SURFACE, WINDOW),
        Err(DockspaceRuntimeError::Native(
            NativePlatformError::ProviderReplacementPending
        ))
    ));

    session
        .finish_native_provider_replacement()
        .expect("the joined successor activates");
    let successor = session
        .native_surface(SURFACE)
        .expect("the successor rebuilds the exact binding roster");
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

    assert_eq!(session.native_surface(SURFACE), None);
    assert!(matches!(
        session.capture_native_snapshot(std::iter::empty()),
        Err(NativePlatformError::ProviderUnavailable)
    ));

    session
        .enable_native_platform(NativePlatformMode::ObservedRoots)
        .expect("the host explicitly enrolls a fresh provider");
    let successor = session
        .native_surface(SURFACE)
        .expect("reenrollment reconstructs the current binding roster");
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
    session
        .finish_native_provider_replacement()
        .expect("the joined successor activates");
    let successor = session
        .native_surface(SURFACE)
        .expect("the successor rebuilds the exact binding roster");

    assert!(matches!(
        session.publish_native_close(
            successor,
            NativeCloseState::Clear,
            Some(stale_acknowledgement),
        ),
        Err(DockspaceRuntimeError::Native(
            NativePlatformError::EffectAcknowledgementProviderMismatch { surface: SURFACE }
        ))
    ));
}

#[test]
fn destroyed_fact_can_acknowledge_the_exact_destructive_effect() {
    let (_session, lease) = native_root_session();
    let effect = EffectId::new(7);
    let acknowledgement = NativeCloseEffectAcknowledgement {
        provider: lease.provider,
        binding: lease.binding,
        effect,
    };
    let compiled = compile_window_fact(
        lease.provider,
        lease.binding,
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
