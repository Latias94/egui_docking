//! Either endpoint can revoke a released drag before its held preview settles.
//!
//! Controlled window snapshots exercise reducer authority, not native
//! `ScaleFactorChanged` callbacks or GPU presentation.

use super::*;
use dockspace::runtime::PaintedSurfaceOutput;

mod window_change;

use window_change::WindowChange;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Endpoint {
    Target,
    Source,
}

impl Endpoint {
    const fn index(self) -> usize {
        match self {
            Self::Target => 0,
            Self::Source => 1,
        }
    }
}

fn endpoint_revocation(
    endpoint: Endpoint,
    change: WindowChange,
    result: SurfacePresentationResult,
) {
    let seed = 0x0e90_ca1e
        + 16 * endpoint as u64
        + 2 * change as u64
        + u64::from(matches!(result, SurfacePresentationResult::Dropped));
    let mut fixture = dragging(seed);
    fixture
        .trace
        .event(("endpoint invalidation", endpoint, change, result));
    let stale = hold_target_preview(&mut fixture.native, fixture.host);
    let pending = release(&mut fixture);
    let version = fixture.native.session.version();
    let mut windows = [
        (fixture.host, ready_window_facts(0.0)),
        (fixture.child, ready_window_facts(720.0)),
    ];
    let binding = windows[endpoint.index()].0;
    let original_coordinates = fixture.native.session.native_local_coordinates(binding);
    let original_placement = fixture.native.session.native_surface_placement(binding);
    assert!(original_coordinates.is_some());
    assert!(original_placement.is_some());
    windows[endpoint.index()].1 = change.facts(x(binding.surface()));
    fixture
        .native
        .report_snapshot(
            windows,
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();

    drop(
        fixture
            .native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(fixture.native.session.version(), version);
    assert!(fixture.native.session.has_pending_presentation());
    assert_child_retained(&fixture.native, fixture.child);
    assert_eq!(
        fixture.native.session.native_local_coordinates(binding),
        original_coordinates,
        "{}: dropping invalidation must preserve committed coordinates",
        fixture.trace.replay()
    );
    assert_eq!(
        fixture.native.session.native_surface_placement(binding),
        original_placement,
        "{}: dropping invalidation must preserve committed placement",
        fixture.trace.replay()
    );

    let mut invalidated = commit_boundary(&mut fixture.native);
    fixture
        .native
        .settle_host_frame_report(&invalidated)
        .unwrap();
    let [cancelled] = invalidated.manipulation_feedback() else {
        panic!("{}: {invalidated:?}", fixture.trace.replay());
    };
    let cancelled = *cancelled;
    assert_eq!(cancelled.manipulation_id(), pending.manipulation_id());
    assert!(
        matches!(cancelled.status(), Status::Cancelled(_)),
        "{}: {invalidated:?}",
        fixture.trace.replay()
    );
    assert!(!invalidated.workspace_changed());
    assert_eq!(invalidated.take_native_effects(), []);
    assert_eq!(fixture.native.session.version(), version);
    assert!(!fixture.native.session.has_pending_presentation());
    assert!(!fixture.native.session.has_active_gesture());
    assert_child_retained(&fixture.native, fixture.child);
    change.assert_applied(&fixture.native, binding);

    settle_revoked_preview(&mut fixture, stale, result, endpoint);

    fixture
        .native
        .report_snapshot(
            [
                (fixture.host, ready_window_facts(0.0)),
                (fixture.child, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    let mut restored = commit_boundary(&mut fixture.native);
    fixture.native.settle_host_frame_report(&restored).unwrap();
    assert_eq!(restored.manipulation_feedback(), []);
    assert_eq!(restored.take_native_effects(), []);
    assert!(!restored.workspace_changed());
    assert_eq!(fixture.native.session.version(), version);
    assert_child_retained(&fixture.native, fixture.child);
    let effects = retry_last_pane_after_cancellation(
        &mut fixture.native,
        fixture.host,
        fixture.child,
        cancelled,
        &mut fixture.trace,
    );
    assert!(matches!(effects.as_slice(), [effect]
        if effect.operation() == &NativeEffectOperation::ReleaseChild { binding: fixture.child }));
    retire_child(
        &mut fixture.native,
        fixture.host,
        fixture.child,
        effects,
        &mut fixture.trace,
    );
}

fn settle_revoked_preview(
    fixture: &mut DragFixture,
    stale: PaintedSurfaceOutput,
    result: SurfacePresentationResult,
    endpoint: Endpoint,
) {
    let version = fixture.native.session.version();
    let artifact = stale.presentation_artifact();
    fixture
        .native
        .session
        .report_surface_presentation(stale, result)
        .unwrap();
    drop(
        fixture
            .native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(fixture.native.session.version(), version);
    assert_child_retained(&fixture.native, fixture.child);
    let mut late = commit_boundary(&mut fixture.native);
    fixture.native.settle_host_frame_report(&late).unwrap();
    assert_eq!(
        late.surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1,
        "{}: the exact held output must settle after terminal replay: {late:?}",
        fixture.trace.replay()
    );
    // Changing the source does not invalidate the target's coordinates. Its
    // renderer terminal must settle without reviving the cancelled mutation.
    if endpoint == Endpoint::Target || result == SurfacePresentationResult::Dropped {
        assert!(
            late.surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact)),
            "{}: a target-revoked or dropped artifact must retire: {late:?}",
            fixture.trace.replay()
        );
    }
    assert_eq!(late.manipulation_feedback(), []);
    assert_eq!(late.take_native_effects(), []);
    assert!(!late.workspace_changed());
    assert_eq!(fixture.native.session.version(), version);
    assert_child_retained(&fixture.native, fixture.child);
    assert_terminal_stays_settled(&mut fixture.native);
}

fn endpoint_changes(endpoints: &[Endpoint], changes: &[WindowChange]) {
    for &endpoint in endpoints {
        for &change in changes {
            for result in [
                SurfacePresentationResult::Presented,
                SurfacePresentationResult::Dropped,
            ] {
                endpoint_revocation(endpoint, change, result);
            }
        }
    }
}

#[test]
fn a_target_coordinate_or_visibility_change_revokes_a_waiting_last_pane_release() {
    endpoint_changes(
        &[Endpoint::Target],
        &[WindowChange::Hidden, WindowChange::PresentationScale],
    );
}

#[test]
fn a_source_coordinate_or_visibility_change_revokes_a_waiting_last_pane_release() {
    endpoint_changes(
        &[Endpoint::Source],
        &[WindowChange::Hidden, WindowChange::PresentationScale],
    );
}

#[test]
fn a_native_dpi_change_alone_revokes_either_endpoint_without_changing_presentation_scale() {
    endpoint_changes(
        &[Endpoint::Source, Endpoint::Target],
        &[WindowChange::NativeDpi],
    );
}

#[test]
fn desktop_origin_moves_in_both_directions_revoke_either_pending_endpoint() {
    endpoint_changes(
        &[Endpoint::Source, Endpoint::Target],
        &[WindowChange::NegativeOrigin, WindowChange::PositiveOrigin],
    );
}

#[test]
fn a_client_physical_resize_revokes_either_pending_endpoint() {
    endpoint_changes(
        &[Endpoint::Source, Endpoint::Target],
        &[WindowChange::ClientResize],
    );
}
