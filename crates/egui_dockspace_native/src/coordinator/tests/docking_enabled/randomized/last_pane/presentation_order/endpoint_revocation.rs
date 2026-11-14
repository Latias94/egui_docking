//! Either endpoint can revoke a released drag before its held preview settles.

use super::*;
use dockspace::runtime::PaintedSurfaceOutput;

fn endpoint_revocation(
    source_changed: bool,
    change_scale: bool,
    result: SurfacePresentationResult,
) {
    let seed = 0x0e90_ca1e
        + 4 * u64::from(source_changed)
        + 2 * u64::from(change_scale)
        + u64::from(matches!(result, SurfacePresentationResult::Dropped));
    let mut fixture = dragging(seed);
    fixture.trace.event((
        "endpoint invalidation",
        source_changed,
        change_scale,
        result,
    ));
    let stale = hold_target_preview(&mut fixture.native, fixture.host);
    let pending = release(&mut fixture);
    let version = fixture.native.session.version();
    let origin = if source_changed { 720.0 } else { 0.0 };
    let changed = if change_scale {
        ready_window_facts(origin).with_presentation_scale_factor(ScaleFactor::new(2.0).unwrap())
    } else {
        ready_window_facts(origin).with_presentation(NativeWindowPresentationState::Hidden)
    };
    let mut windows = [
        (fixture.host, ready_window_facts(0.0)),
        (fixture.child, ready_window_facts(720.0)),
    ];
    windows[usize::from(source_changed)].1 = changed;
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

    settle_revoked_preview(&mut fixture, stale, result, source_changed);

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
    source_changed: bool,
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
    if !source_changed || result == SurfacePresentationResult::Dropped {
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

#[test]
fn a_target_coordinate_or_visibility_change_revokes_a_waiting_last_pane_release() {
    for change_scale in [false, true] {
        for result in [
            SurfacePresentationResult::Presented,
            SurfacePresentationResult::Dropped,
        ] {
            endpoint_revocation(false, change_scale, result);
        }
    }
}

#[test]
fn a_source_coordinate_or_visibility_change_revokes_a_waiting_last_pane_release() {
    for change_scale in [false, true] {
        for result in [
            SurfacePresentationResult::Presented,
            SurfacePresentationResult::Dropped,
        ] {
            endpoint_revocation(true, change_scale, result);
        }
    }
}
