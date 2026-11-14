use super::*;

mod endpoint_revocation;

struct DragFixture {
    native: NativeCoordinator,
    host: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    receivers: RecordedSource,
    source: PhysicalPosition<f64>,
    target: PhysicalPosition<f64>,
    trace: Trace,
}

fn dragging(seed: u64) -> DragFixture {
    let (mut native, bindings) = routes(true);
    let [host, child]: [NativeSurfaceBinding; 2] = bindings.try_into().unwrap();
    let mut trace = Trace {
        seed,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    let (receivers, source, target) = scene(&mut native, FIRST_SURFACE);
    global_focus(&mut native, child, &mut trace);
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), SECOND_SURFACE),
        (target, None, FIRST_SURFACE),
    ] {
        apply(
            &mut native,
            child,
            mouse(
                child,
                point,
                state,
                hover,
                PointerWindowRoute::Window(window(SECOND_SURFACE)),
                None,
            ),
            &receivers,
            true,
            &mut trace,
        );
    }
    global_focus(&mut native, host, &mut trace);
    DragFixture {
        native,
        host,
        child,
        receivers,
        source,
        target,
        trace,
    }
}

fn release(fixture: &mut DragFixture) -> DockspaceManipulationFeedback {
    let version = fixture.native.session.version();
    let mut report = release_last_pane(
        &mut fixture.native,
        fixture.child,
        fixture.target,
        &fixture.receivers,
        &mut fixture.trace,
    );
    let [feedback] = report.manipulation_feedback() else {
        panic!("{}: {report:?}", fixture.trace.replay());
    };
    let feedback = *feedback;
    assert_eq!(
        feedback.status(),
        Status::Pending(PendingReason::AwaitingPresentation),
        "{}",
        fixture.trace.replay()
    );
    assert!(!report.workspace_changed(), "{}", fixture.trace.replay());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(fixture.native.session.version(), version);
    assert!(!fixture.native.session.has_active_gesture());
    assert!(fixture.native.session.has_pending_presentation());
    assert_child_retained(&fixture.native, fixture.child);
    feedback
}

fn assert_waiting(
    fixture: &mut DragFixture,
    artifacts: &[dockspace::runtime::SurfacePresentationArtifact],
) {
    let version = fixture.native.session.version();
    let mut report = commit_boundary(&mut fixture.native);
    fixture.native.settle_host_frame_report(&report).unwrap();
    assert!(
        report.manipulation_feedback().is_empty(),
        "{}: {report:?}",
        fixture.trace.replay()
    );
    assert!(report.take_native_effects().is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(fixture.native.session.version(), version);
    assert!(fixture.native.session.has_pending_presentation());
    for &artifact in artifacts {
        assert!(
            !report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.settles(artifact)),
            "{}: a later terminal cannot skip an unsettled prefix: {report:?}",
            fixture.trace.replay()
        );
    }
    assert_child_retained(&fixture.native, fixture.child);
}

fn finish_delivery(
    fixture: &mut DragFixture,
    pending: DockspaceManipulationFeedback,
    mut report: HostFrameReport,
) {
    fixture.native.settle_host_frame_report(&report).unwrap();
    let [delivered] = report.manipulation_feedback() else {
        panic!("{}: {report:?}", fixture.trace.replay());
    };
    assert_eq!(delivered.manipulation_id(), pending.manipulation_id());
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert!(!fixture.native.session.has_pending_presentation());
    assert!(!fixture.native.session.has_active_gesture());
    assert_eq!(
        fixture
            .native
            .session
            .view()
            .item(MOVING)
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
    assert!(
        fixture
            .native
            .session
            .view()
            .surface(SECOND_SURFACE)
            .is_none()
    );
    let effects = report.take_native_effects();
    assert!(matches!(effects.as_slice(), [effect]
        if effect.operation() == &NativeEffectOperation::ReleaseChild { binding: fixture.child }));
    assert_terminal_stays_settled(&mut fixture.native);
    retire_child(
        &mut fixture.native,
        fixture.host,
        fixture.child,
        effects,
        &mut fixture.trace,
    );
}

#[test]
fn previews_dropped_before_release_require_a_fresh_output_after_either_admission_order() {
    for admitted_before_release in [false, true] {
        let mut fixture = dragging(0xbef0_2e1e + u64::from(admitted_before_release));
        fixture
            .trace
            .event(("dropped before release", admitted_before_release));
        let dropped = hold_target_preview(&mut fixture.native, fixture.host);
        let artifact = dropped.presentation_artifact();
        fixture
            .native
            .session
            .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
            .unwrap();
        if admitted_before_release {
            let mut report = commit_boundary(&mut fixture.native);
            fixture.native.settle_host_frame_report(&report).unwrap();
            assert_eq!(
                report
                    .surface_presentation_commits()
                    .iter()
                    .filter(|commit| commit.retires(artifact))
                    .count(),
                1
            );
            assert!(report.manipulation_feedback().is_empty());
            assert!(!report.workspace_changed());
            assert!(report.take_native_effects().is_empty());
            assert!(fixture.native.session.has_active_gesture());
            assert_child_retained(&fixture.native, fixture.child);
        } else {
            // A failed candidate cannot move this terminal ahead of release.
            let candidate = fixture
                .native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            drop(candidate);
            assert!(fixture.native.session.has_active_gesture());
        }
        let pending = release(&mut fixture);
        assert_waiting(&mut fixture, &[]);
        let fresh = hold_target_preview(&mut fixture.native, fixture.host);
        assert_ne!(fresh.presentation_artifact(), artifact);
        fixture
            .native
            .session
            .report_surface_presentation(fresh, SurfacePresentationResult::Presented)
            .unwrap();
        let report = commit_boundary(&mut fixture.native);
        finish_delivery(&mut fixture, pending, report);
    }
}

#[test]
fn later_preview_terminals_cannot_skip_the_same_stream_pending_prefix() {
    for tail_result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut fixture = dragging(0x02e0_2de2);
        fixture.trace.event(("tail first", tail_result));
        let head = hold_target_preview(&mut fixture.native, fixture.host);
        let tail = hold_target_preview(&mut fixture.native, fixture.host);
        let head_artifact = head.presentation_artifact();
        let tail_artifact = tail.presentation_artifact();
        assert_ne!(head_artifact, tail_artifact);
        let pending = release(&mut fixture);
        let version = fixture.native.session.version();
        fixture
            .native
            .session
            .report_surface_presentation(tail, tail_result)
            .unwrap();
        assert_waiting(&mut fixture, &[head_artifact, tail_artifact]);
        fixture
            .native
            .session
            .report_surface_presentation(head, SurfacePresentationResult::Dropped)
            .unwrap();
        let candidate = fixture
            .native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        drop(candidate);
        assert_eq!(fixture.native.session.version(), version);
        assert_child_retained(&fixture.native, fixture.child);
        let mut report = commit_boundary(&mut fixture.native);
        assert_eq!(
            report
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(head_artifact) && commit.settles(tail_artifact))
                .count(),
            1
        );
        match tail_result {
            SurfacePresentationResult::Presented => {
                assert!(
                    report
                        .surface_presentation_commits()
                        .iter()
                        .any(|commit| commit.presents(tail_artifact)
                            && commit.retires(head_artifact))
                );
                finish_delivery(&mut fixture, pending, report);
            }
            SurfacePresentationResult::Dropped => {
                fixture.native.settle_host_frame_report(&report).unwrap();
                let [cancelled] = report.manipulation_feedback() else {
                    panic!("{}: {report:?}", fixture.trace.replay());
                };
                let cancelled = *cancelled;
                assert_eq!(cancelled.manipulation_id(), pending.manipulation_id());
                assert_eq!(
                    cancelled.status(),
                    Status::Cancelled(CancelReason::SceneUnavailable)
                );
                assert!(!report.workspace_changed());
                assert!(report.take_native_effects().is_empty());
                assert_eq!(fixture.native.session.version(), version);
                assert!(!fixture.native.session.has_pending_presentation());
                assert_child_retained(&fixture.native, fixture.child);
                assert_terminal_stays_settled(&mut fixture.native);
                let effects = retry_last_pane_after_cancellation(
                    &mut fixture.native,
                    fixture.host,
                    fixture.child,
                    cancelled,
                    &mut fixture.trace,
                );
                retire_child(
                    &mut fixture.native,
                    fixture.host,
                    fixture.child,
                    effects,
                    &mut fixture.trace,
                );
            }
        }
    }
}

#[test]
fn a_prior_gestures_preview_cannot_authorize_the_replacement_release() {
    let mut fixture = dragging(0x00dd_9e57);
    let old = hold_target_preview(&mut fixture.native, fixture.host);
    let old_artifact = old.presentation_artifact();
    let first_pending = release(&mut fixture);
    global_focus(&mut fixture.native, fixture.child, &mut fixture.trace);
    let (_, replaced) = apply(
        &mut fixture.native,
        fixture.child,
        mouse(
            fixture.child,
            fixture.source,
            Some(ElementState::Pressed),
            SECOND_SURFACE,
            PointerWindowRoute::Window(window(SECOND_SURFACE)),
            None,
        ),
        &fixture.receivers,
        true,
        &mut fixture.trace,
    );
    let [cancelled] = replaced.manipulation_feedback() else {
        panic!("{}: {replaced:?}", fixture.trace.replay());
    };
    assert_eq!(cancelled.manipulation_id(), first_pending.manipulation_id());
    assert_eq!(
        cancelled.status(),
        Status::Cancelled(CancelReason::ReplacedByNewGesture)
    );
    assert!(!replaced.workspace_changed());
    assert!(fixture.native.session.has_active_gesture());
    assert!(!fixture.native.session.has_pending_presentation());
    apply(
        &mut fixture.native,
        fixture.child,
        mouse(
            fixture.child,
            fixture.target,
            None,
            FIRST_SURFACE,
            PointerWindowRoute::Window(window(SECOND_SURFACE)),
            None,
        ),
        &fixture.receivers,
        true,
        &mut fixture.trace,
    );
    global_focus(&mut fixture.native, fixture.host, &mut fixture.trace);
    let current = hold_target_preview(&mut fixture.native, fixture.host);
    let current_artifact = current.presentation_artifact();
    let second_pending = release(&mut fixture);
    assert_ne!(
        first_pending.manipulation_id(),
        second_pending.manipulation_id()
    );
    fixture
        .native
        .session
        .report_surface_presentation(old, SurfacePresentationResult::Presented)
        .unwrap();
    let mut old_report = commit_boundary(&mut fixture.native);
    fixture
        .native
        .settle_host_frame_report(&old_report)
        .unwrap();
    assert!(
        old_report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.settles(old_artifact))
    );
    assert!(
        !old_report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.settles(current_artifact))
    );
    assert!(
        old_report.manipulation_feedback().is_empty(),
        "{}: {old_report:?}",
        fixture.trace.replay()
    );
    assert!(old_report.take_native_effects().is_empty());
    assert!(!old_report.workspace_changed());
    assert!(fixture.native.session.has_pending_presentation());
    assert_child_retained(&fixture.native, fixture.child);
    fixture
        .native
        .session
        .report_surface_presentation(current, SurfacePresentationResult::Presented)
        .unwrap();
    let report = commit_boundary(&mut fixture.native);
    finish_delivery(&mut fixture, second_pending, report);
}
