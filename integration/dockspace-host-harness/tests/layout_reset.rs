//! Application-owned defaults use the public reset transaction without serialization.

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy, DockSourceRule};
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspacePaneFocusObservation,
    DockspacePaneFocusRequest, DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport,
    HostInputOutcome, SurfacePaintDisposition, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn durable_rect() -> LogicalRect {
    LogicalRect::new(200.0, 140.0, 360.0, 280.0).unwrap()
}

fn default_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs_with_selection([FIRST, SECOND], Some(SECOND)).unwrap(),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(
            FLOATING_ROOT,
            DockspaceNode::tabs_with_selection([THIRD, FOURTH], Some(FOURTH)).unwrap(),
        ),
        durable_rect(),
    ))])
    .unwrap()
}

fn single_pane_layout(item: ItemId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([item])),
    )])
    .unwrap()
}

fn policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    policy
}

fn session(layout: DockspaceLayout) -> DockspaceSession {
    let mut session = DockspaceSession::from_layout(layout, policy()).unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    session
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
        LogicalSize::new(48.0, 32.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    measure(&mut frame);
    frame.commit().unwrap()
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn close_root(session: &mut DockspaceSession, root: RootId) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_root_current(root).unwrap();
    let report = commit_measured(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the real root-close request must publish its decision plan");
    };
    let mut frame = session.begin_host_frame().unwrap();
    for item in plan.items() {
        frame
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .unwrap();
    }
    assert!(commit_measured(frame).workspace_changed());
    assert!(session.view().root(root).is_none());
}

fn assert_default_content(
    session: &DockspaceSession,
) -> (SurfaceId, RootId, RootId, FloatingPresentationId) {
    let view = session.view();
    let first = view.item(FIRST).unwrap();
    let second = view.item(SECOND).unwrap();
    let third = view.item(THIRD).unwrap();
    let fourth = view.item(FOURTH).unwrap();
    assert_eq!(view.surfaces().count(), 1);
    assert_eq!(first.surface(), third.surface());
    assert_eq!(first.root(), second.root());
    assert_eq!(third.root(), fourth.root());
    assert_ne!(first.root(), third.root());
    assert_eq!(first.tabs().items(), &[FIRST, SECOND]);
    assert_eq!(third.tabs().items(), &[THIRD, FOURTH]);
    assert_eq!(first.tabs().selected(), Some(SECOND));
    assert_eq!(third.tabs().selected(), Some(FOURTH));
    assert!(!first.is_selected());
    assert!(!third.is_selected());
    assert!(second.is_selected());
    assert!(fourth.is_selected());
    let floating = third.contained().unwrap();
    assert_eq!(fourth.contained(), Some(floating));
    assert_eq!(view.contained(floating).unwrap().rect(), durable_rect());
    assert_eq!(
        view.surface(first.surface())
            .unwrap()
            .main_root()
            .unwrap()
            .id(),
        first.root()
    );
    (first.surface(), first.root(), third.root(), floating)
}

#[test]
fn review_root_policy_survives_closing_and_restoring_defaults() {
    use dockspace::model::{DockAnchor, DockspaceActionRejection};

    let defaults = default_layout();
    let mut policy = policy();
    let mut source = DockSourceRule::new();
    source.set_enabled(false);
    policy.set_source_rule(FLOATING_ROOT, source);
    let mut session = DockspaceSession::from_layout(defaults.clone(), policy).unwrap();
    commit_measured(session.begin_host_frame().unwrap());

    let mut blocked = session.begin_host_frame().unwrap();
    blocked
        .dock_root_current(
            FLOATING_ROOT,
            DockPlacement::Center(DockAnchor::Item(FIRST)),
        )
        .unwrap();
    let before = commit_deferred(blocked);
    assert_eq!(
        before.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PolicyDenied
        )]
    );

    close_root(&mut session, FLOATING_ROOT);
    let reset = session.prepare_layout_reset(defaults).unwrap();
    commit_measured(session.begin_layout_reset_frame(&reset).unwrap());
    let restored_root = session.view().item(THIRD).unwrap().root();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_root_current(
            restored_root,
            DockPlacement::Center(DockAnchor::Item(FIRST)),
        )
        .unwrap();
    let report = commit_deferred(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PolicyDenied
        )],
        "restoring defaults must preserve the root's source restriction"
    );
}

fn focus_request(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> Option<DockspacePaneFocusRequest> {
    let mut frame = session.begin_host_frame().unwrap();
    let request = frame
        .paint_plan(surface)
        .unwrap()
        .unwrap()
        .pane_focus_request();
    commit_deferred(frame);
    request
}

#[test]
fn cloned_defaults_preserve_semantic_identities_across_repeated_epoch_replacements() {
    let defaults = default_layout();
    let mut session = session(defaults.clone());
    let original = assert_default_content(&session);
    assert_eq!(original, (SURFACE, ROOT, FLOATING_ROOT, FLOATING));

    close_root(&mut session, ROOT);
    assert!(session.view().surface(SURFACE).is_some());
    close_root(&mut session, FLOATING_ROOT);
    assert!(session.view().surface(SURFACE).is_none());
    assert!(session.view().contained(FLOATING).is_none());
    for item in [FIRST, SECOND, THIRD, FOURTH] {
        assert!(session.view().item(item).is_none());
    }

    for _ in 0..3 {
        let before = session.version();
        let reset = session.prepare_layout_reset(defaults.clone()).unwrap();
        let report = commit_measured(session.begin_layout_reset_frame(&reset).unwrap());
        assert!(report.workspace_changed());
        assert_ne!(session.version(), before);
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::LayoutReset {
                previous: before,
                current: session.version(),
            }]
        );
        let ids = assert_default_content(&session);
        assert_eq!(ids, original);
    }
    assert!(defaults.view().surface(SURFACE).is_some());
    assert!(defaults.view().root(ROOT).is_some());
    assert_eq!(
        defaults.view().contained(FLOATING).unwrap().rect(),
        durable_rect()
    );
}

#[test]
fn reset_preserves_both_retired_and_fresh_declared_identities() {
    let defaults = default_layout();
    let mut session = session(defaults.clone());
    close_root(&mut session, ROOT);
    close_root(&mut session, FLOATING_ROOT);

    let fresh_surface = SurfaceId::new(2);
    let fresh_root = RootId::new(3);
    let fresh_floating_root = RootId::new(4);
    let fresh_floating = FloatingPresentationId::new(2);
    let fifth = ItemId::new(5);
    let sixth = ItemId::new(6);
    let fresh_rect = LogicalRect::new(30.0, 40.0, 240.0, 180.0).unwrap();
    let incoming = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                ROOT,
                DockspaceNode::central_tabs_with_selection([FIRST, SECOND], Some(SECOND)).unwrap(),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(
                FLOATING_ROOT,
                DockspaceNode::tabs_with_selection([THIRD, FOURTH], Some(FOURTH)).unwrap(),
            ),
            durable_rect(),
        )),
        DockspaceSurfaceLayout::new(
            fresh_surface,
            DockspaceRootLayout::new(fresh_root, DockspaceNode::central_tabs([fifth])),
        )
        .with_contained(DockspaceContainedLayout::new(
            fresh_floating,
            DockspaceRootLayout::new(fresh_floating_root, DockspaceNode::tabs([sixth])),
            fresh_rect,
        )),
    ])
    .unwrap();
    let reset = session.prepare_layout_reset(incoming).unwrap();
    commit_measured(session.begin_layout_reset_frame(&reset).unwrap());

    let view = session.view();
    assert_eq!(view.surfaces().count(), 2);
    let first = view.item(FIRST).unwrap();
    let third = view.item(THIRD).unwrap();
    assert_eq!(first.surface(), SURFACE);
    assert_eq!(third.surface(), first.surface());
    assert_eq!(first.root(), ROOT);
    assert_eq!(third.root(), FLOATING_ROOT);
    assert_ne!(first.root(), third.root());
    assert_eq!(first.tabs().items(), &[FIRST, SECOND]);
    assert_eq!(first.tabs().selected(), Some(SECOND));
    assert_eq!(third.tabs().items(), &[THIRD, FOURTH]);
    assert_eq!(third.tabs().selected(), Some(FOURTH));
    let floating = third.contained().unwrap();
    assert_eq!(floating, FLOATING);
    assert_eq!(view.contained(floating).unwrap().rect(), durable_rect());

    let fifth = view.item(fifth).unwrap();
    let sixth = view.item(sixth).unwrap();
    assert_eq!((fifth.surface(), fifth.root()), (fresh_surface, fresh_root));
    assert_eq!(
        (sixth.surface(), sixth.root(), sixth.contained()),
        (fresh_surface, fresh_floating_root, Some(fresh_floating))
    );
    assert!(fifth.is_selected());
    assert!(sixth.is_selected());
    assert_eq!(view.contained(fresh_floating).unwrap().rect(), fresh_rect);
}

#[test]
fn terminal_reset_rejects_locked_foreign_and_stale_preparations_without_mutation() {
    let defaults = default_layout();
    let mut session = session(defaults.clone());
    let prepared = session.prepare_layout_reset(defaults.clone()).unwrap();
    let mut locked = session.policy().clone();
    locked.set_arrangement_locked(true);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(locked).unwrap();
    commit_deferred(frame);
    let locked_version = session.version();
    let locked_ids = assert_default_content(&session);
    assert_eq!(
        session
            .prepare_layout_reset(defaults.clone())
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    let error = {
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_layout_reset(&prepared).unwrap_err()
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);

    let mut unlocked = session.policy().clone();
    unlocked.set_arrangement_locked(false);
    let mut dropped = session.begin_host_frame().unwrap();
    dropped.replace_policy(unlocked.clone()).unwrap();
    drop(dropped);
    assert!(session.policy().arrangement_locked());
    assert_eq!(session.version(), locked_version);
    assert_eq!(assert_default_content(&session), locked_ids);
    assert!(session.prepare_layout_reset(defaults.clone()).is_err());

    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(unlocked).unwrap();
    commit_deferred(frame);
    assert!(!session.policy().arrangement_locked());
    let prepared = session.prepare_layout_reset(defaults.clone()).unwrap();

    let mut other = DockspaceSession::from_layout(defaults.clone(), policy()).unwrap();
    let other_version = other.version();
    let other_ids = assert_default_content(&other);
    let error = {
        let mut frame = other.begin_host_frame().unwrap();
        let error = frame.submit_layout_reset(&prepared).unwrap_err();
        frame.select_item_current(FIRST).unwrap();
        error
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(other.version(), other_version);
    assert_eq!(assert_default_content(&other), other_ids);

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit_measured(frame);
    let changed = session.version();
    let error = {
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_layout_reset(&prepared).unwrap_err()
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(session.version(), changed);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    let reset = session.prepare_layout_reset(defaults).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    commit_measured(frame);
    assert_default_content(&session);
    let current = session.version();
    let error = {
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_layout_reset(&reset).unwrap_err()
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(session.version(), current);
}

#[test]
fn terminal_reset_preserves_declared_identities_and_retries_without_old_authority() {
    let defaults = default_layout();
    let mut session = session(defaults.clone());
    let mut select = session.begin_host_frame().unwrap();
    select.select_item_current(FIRST).unwrap();
    commit_measured(select);

    let mut paint = session.begin_host_frame().unwrap();
    let plan = paint.paint_plan(SURFACE).unwrap().unwrap();
    let old_focus = plan.pane_focus_request().unwrap();
    assert_eq!(old_focus.item(), Some(FIRST));
    let old_observation = plan
        .prepare_pane_focus_observation(old_focus, DockspacePaneFocusObservation::Focused)
        .unwrap();
    let old_receiver = plan.tab_receiver(FIRST).unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    paint.confirm_surface_painted(receipt).unwrap();
    let mut outputs = paint.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let old_output = outputs.pop().unwrap();
    let old_artifact = old_output.presentation_artifact();
    let old_action = session.prepare_select_item(FIRST);
    let reset = session.prepare_layout_reset(defaults).unwrap();
    let before = session.version();
    let current_policy = session.policy().clone();
    let prefix = session.prepare_float_item(THIRD, SURFACE);

    let (candidate_ids, prefix_version) = {
        let mut candidate = session.begin_host_frame().unwrap();
        candidate.submit_prepared_action(prefix).unwrap();
        let floated = candidate.view().item(THIRD).unwrap();
        let prefix_root = floated.root();
        let prefix_floating = floated.contained().unwrap();
        assert_ne!(prefix_root, FLOATING_ROOT);
        assert_ne!(prefix_floating, FLOATING);
        candidate
            .dock_item_current(FOURTH, DockPlacement::After(SECOND))
            .unwrap();
        assert!(candidate.view().root(FLOATING_ROOT).is_none());
        assert!(candidate.view().contained(FLOATING).is_none());
        let prefix_version = candidate.version();
        assert_ne!(prefix_version, before);
        candidate.submit_layout_reset(&reset).unwrap();
        let main = candidate.view().item(SECOND).unwrap();
        let contained = candidate.view().item(FOURTH).unwrap();
        let ids = (
            main.surface(),
            main.root(),
            contained.root(),
            contained.contained(),
        );
        assert!(main.is_selected());
        assert_eq!(main.root(), ROOT);
        assert_eq!(contained.root(), FLOATING_ROOT);
        assert_eq!(contained.contained(), Some(FLOATING));
        assert_ne!(candidate.version(), prefix_version);
        assert_eq!(
            candidate.submit_layout_reset(&reset).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        assert_eq!(
            candidate.select_item_current(FIRST).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        assert_eq!(
            candidate.replace_policy(current_policy).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        assert_eq!(
            candidate
                .replace_presentation_config(DockPresentationConfig::default())
                .unwrap_err()
                .kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        measure(&mut candidate);
        let plan = candidate
            .paint_plan(ids.0)
            .unwrap()
            .expect("measurement compiles the replacement scene inside this private update");
        let replacement_receiver = plan.tab_receiver(SECOND).unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        assert!(
            recording
                .records
                .contains(&OwnedRecord::Receiver(replacement_receiver))
        );
        candidate.confirm_surface_painted(receipt).unwrap();
        (ids, prefix_version)
    };
    assert_eq!(session.version(), before);
    assert_eq!(session.view().item(FIRST).unwrap().root(), ROOT);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    {
        let mut untouched = session.begin_host_frame().unwrap();
        assert_eq!(
            untouched
                .paint_plan(SURFACE)
                .unwrap()
                .unwrap()
                .pane_focus_request(),
            Some(old_focus)
        );
    }

    let prefix = session.prepare_float_item(THIRD, SURFACE);
    let mut retry = session.begin_host_frame().unwrap();
    let submitted = retry.submit_prepared_action(prefix).unwrap();
    retry
        .dock_item_current(FOURTH, DockPlacement::After(SECOND))
        .unwrap();
    assert_eq!(retry.version(), prefix_version);
    retry.submit_layout_reset(&reset).unwrap();
    let main = retry.view().item(SECOND).unwrap();
    let contained = retry.view().item(FOURTH).unwrap();
    assert_eq!(
        (
            main.surface(),
            main.root(),
            contained.root(),
            contained.contained()
        ),
        candidate_ids
    );
    let report = commit_measured(retry);
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), session.version());
    assert!(report.submitted_action_outcome(submitted).is_none());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::LayoutReset {
            previous: prefix_version,
            current: session.version(),
        }]
    );
    let (surface, ..) = assert_default_content(&session);
    assert!(session.presented_surface(surface).is_none());
    assert!(session.bind_presented_receiver(&old_receiver).is_none());

    session
        .report_surface_presentation(old_output, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = commit_deferred(session.begin_host_frame().unwrap());
    let retired = settled
        .surface_presentation_commits()
        .iter()
        .find(|ack| ack.settles(old_artifact))
        .expect("the late old output must receive a terminal acknowledgement");
    assert!(retired.retires(old_artifact));
    assert!(!retired.presents(old_artifact));
    assert!(session.presented_surface(surface).is_none());
    assert!(session.bind_presented_receiver(&old_receiver).is_none());

    let mut paint = session.begin_host_frame().unwrap();
    let plan = paint.paint_plan(surface).unwrap().unwrap();
    let fresh = plan.pane_focus_request().unwrap();
    // The terminal prefix explicitly focused FOURTH. It remains selected in
    // the restored contained group, so replacement preserves that newer intent.
    assert_eq!(fresh.item(), Some(FOURTH));
    assert_ne!(fresh, old_focus);
    assert!(
        plan.prepare_pane_focus_observation(old_focus, DockspacePaneFocusObservation::Focused)
            .is_none()
    );
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    assert!(recording.records.iter().any(|record| matches!(
        record,
        OwnedRecord::PaneContent {
            item: Some(SECOND),
            paint: SurfacePaintDisposition::Visible,
            ..
        }
    )));
    paint.confirm_surface_painted(receipt).unwrap();
    let mut report = paint.commit().unwrap();
    for output in report.take_painted_outputs() {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit_deferred(session.begin_host_frame().unwrap());

    let version = session.version();
    let mut stale = session.begin_host_frame().unwrap();
    stale.submit_prepared_action(old_action).unwrap();
    stale
        .submit_pane_focus_observation(old_observation)
        .unwrap();
    let report = commit_deferred(stale);
    assert!(
        report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. }))
    );
    assert_eq!(session.version(), version);
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert_eq!(focus_request(&mut session, surface), Some(fresh));
    assert!(session.bind_presented_receiver(&old_receiver).is_none());
}

#[test]
fn managed_native_prefix_and_reset_retry_share_one_publication_boundary() {
    use dockspace::runtime::{
        HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostErrorKind,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
    };

    let mut session = session(single_pane_layout(FIRST));
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(7))
        .unwrap();
    let registration = commit_deferred(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let [HostInputOutcome::NativeSurfaceRegistered { binding: old }] = registration.inputs() else {
        panic!("the real native registration must publish its binding");
    };
    let old = *old;
    let reset = session.prepare_layout_reset(default_layout()).unwrap();
    let before = session.version();
    let error = match session.begin_layout_reset_frame(&reset) {
        Ok(_) => panic!("managed input must join the native reset entry point"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert_eq!(session.version(), before);
    session
        .record_native_pointer(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None,
            ),
            NativePointerOwner::Unknown,
            NativePointerOwner::Unknown,
        ))
        .unwrap();
    {
        let candidate = session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(candidate.view().item(SECOND).unwrap().is_selected());
    }
    assert_eq!(session.version(), before);
    assert!(session.view().item(SECOND).is_none());
    assert!(session.is_current_native_binding(old));

    let retry = session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let report = commit_measured(retry);
    assert!(!report.native_input_pending());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::LayoutReset {
            previous: before,
            current: session.version(),
        }]
    );
    let [current] = report.native_bindings() else {
        panic!("the reset must retain the registered root with a successor binding");
    };
    assert_ne!(*current, old);
    assert!(!session.is_current_native_binding(old));
    assert!(session.is_current_native_binding(*current));
    assert_default_content(&session);
}

#[test]
fn budgeted_native_prefix_rejects_reset_until_the_final_poll() {
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostErrorKind,
        NativeInputBudget, NativeMonotonicClock, NativePointerEvent, NativePointerHover,
        NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
        NativeReceiverAnswer,
    };
    use std::time::Duration;

    let mut session = session(single_pane_layout(FIRST));
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    commit_deferred(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let reset = session.prepare_layout_reset(default_layout()).unwrap();
    let before = session.version();
    let recorded = session.native_input_recorded_count();
    let movement = NativePointerInput::new(
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
        .record_native_pointer_sequence([movement; 2])
        .unwrap();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    budget.complete_unsequenced_records(255);
    let mut prefix = session
        .poll_native_host_frame(&mut budget, |_| {
            panic!("unknown pointer locations cannot ask a receiver question")
        })
        .unwrap()
        .unwrap();
    assert_eq!(budget.remaining_records(), 0);
    assert!(prefix.native_input_pending());
    let error = prefix.submit_layout_reset(&reset).unwrap_err();
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert!(prefix.view().item(SECOND).is_none());
    assert!(commit_deferred(prefix).native_input_pending());
    assert_eq!(session.version(), before);
    assert!(session.has_pending_native_input());

    let mut budget = NativeInputBudget::new(&clock);
    let mut final_prefix = session
        .poll_native_host_frame(&mut budget, |_| {
            panic!("the final unknown pointer record has no receiver")
        })
        .unwrap()
        .unwrap();
    assert_eq!(budget.remaining_records(), 255);
    assert!(!final_prefix.native_input_pending());
    final_prefix.submit_layout_reset(&reset).unwrap();
    let report = commit_measured(final_prefix);
    assert!(!report.native_input_pending());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::LayoutReset {
            previous: before,
            current: session.version(),
        }]
    );
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
    assert!(!session.has_pending_native_input());
    assert_default_content(&session);
}

#[test]
fn budgeted_native_terminal_reset_resumes_receivers_and_retries_one_publication_boundary() {
    use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
    use dockspace::runtime::{
        HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition,
        NativeHostCapabilities, NativeHostCapability, NativeInputBudget, NativeMonotonicClock,
        NativePointerButton, NativePointerEvent, NativePointerHover, NativePointerId,
        NativePointerInput, NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
        NativeReceiverPurpose, NativeReceiverQuery, NativeWindowFacts, NativeWindowHit,
        NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
        PointerDeviceKind, PointerEventMetadata,
    };
    use std::time::Duration;

    for synchronous_wrapper in [false, true] {
        let mut session = session(default_layout());
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        let capabilities = [
            NativeHostCapability::AuthoritativeInventory,
            NativeHostCapability::AuthoritativeButtonState,
            NativeHostCapability::PointerHitTestObservation,
            NativeHostCapability::DesktopPointerPosition,
            NativeHostCapability::HoveredWindow,
            NativeHostCapability::GlobalWindowPlacement,
        ]
        .into_iter()
        .fold(NativeHostCapabilities::none_supported(), |caps, cap| {
            caps.with(cap)
        });
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        session
            .register_native_root(SURFACE, HostWindowToken::new(7))
            .unwrap();
        let registration = commit_deferred(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        let [old] = registration.native_bindings() else {
            panic!("the public native root registration must publish one binding");
        };
        let old = *old;
        let content = PhysicalRect::new(100.0, 80.0, 900.0, 650.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        let facts = NativeWindowFacts::live()
            .with_content_bounds(content)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible);
        session
            .report_managed_native_snapshot([(old, facts)], NativeWorkAreaRoster::Unknown)
            .unwrap();
        commit_measured(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );

        let mut paint = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let plan = paint.paint_plan(SURFACE).unwrap().unwrap();
        let receiver = plan.tab_receiver(FIRST).unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        assert!(recording.records.contains(&OwnedRecord::Receiver(receiver)));
        paint.confirm_surface_painted(receipt).unwrap();
        let [output] = paint
            .commit()
            .unwrap()
            .take_painted_outputs()
            .try_into()
            .unwrap();
        assert!(output.matches_native_binding(old));
        let artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        commit_deferred(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert!(!session.has_pending_native_input());

        let reset = session.prepare_layout_reset(default_layout()).unwrap();
        let before = session.version();
        let recorded = session.native_input_recorded_count();
        let point = receiver.center();
        let metadata =
            PointerEventMetadata::new(PointerDeviceKind::Touch, Duration::from_millis(10));
        let release_metadata =
            PointerEventMetadata::new(PointerDeviceKind::Touch, Duration::from_millis(20));
        let location = NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(
                PhysicalPoint::new(content.x() + point.x(), content.y() + point.y()).unwrap(),
            ),
            NativePointerHover::Dock(old.window()),
            None,
        );
        let press = NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            location,
            NativePointerOwner::Native(old.window()),
            NativePointerOwner::Native(old.window()),
        )
        .with_metadata(metadata);
        let release = NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
            location,
            NativePointerOwner::Native(old.window()),
            NativePointerOwner::Native(old.window()),
        )
        .with_metadata(release_metadata);
        session
            .record_native_pointer_batch([press, release])
            .unwrap();
        assert_eq!(session.native_input_recorded_count(), recorded + 2);

        let query_metadata = |query: NativeReceiverQuery| match query {
            NativeReceiverQuery::WindowHit(query) => query.metadata(),
            NativeReceiverQuery::Surface(query) => query.metadata(),
        };
        let resolve = |query| match query {
            NativeReceiverQuery::WindowHit(query) => {
                assert_eq!(query.window(), old.window());
                assert_eq!(
                    query.point(),
                    PhysicalPoint::new(point.x(), point.y()).unwrap()
                );
                NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(artifact))
            }
            NativeReceiverQuery::Surface(query) => {
                assert!(query.matches_native_binding(old));
                assert!(query.matches_presentation_artifact(artifact));
                assert_eq!(query.point(), Some(point));
                NativeReceiverAnswer::Dock(query.bind_receiver(&receiver).unwrap())
            }
        };
        let clock = NativeMonotonicClock::new(|| Duration::ZERO);
        let mut first_budget = NativeInputBudget::new(&clock);
        first_budget.complete_unsequenced_records(255);
        let mut queries = Vec::new();
        assert!(
            session
                .poll_native_host_frame(&mut first_budget, |query| {
                    assert_eq!(query_metadata(query), metadata);
                    queries.push(query);
                    resolve(query)
                })
                .unwrap()
                .is_none(),
            "the completed press must remain private inside its atomic input group"
        );
        assert!(first_budget.is_exhausted());
        assert_eq!(first_budget.remaining_records(), 0);
        assert_eq!(first_budget.elapsed(), Duration::ZERO);
        assert!(
            queries
                .iter()
                .any(|query| matches!(query, NativeReceiverQuery::WindowHit(_)))
        );
        assert_eq!(
            queries
                .iter()
                .filter_map(|query| match query {
                    NativeReceiverQuery::Surface(query) => Some(*query.purpose()),
                    NativeReceiverQuery::WindowHit(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                NativeReceiverPurpose::ClickDelivery,
                NativeReceiverPurpose::DragDelivery
            ]
        );
        assert_eq!(session.version(), before);
        assert!(session.view().item(SECOND).unwrap().is_selected());
        assert!(session.has_pending_native_input());
        assert!(session.is_current_native_binding(old));
        assert_eq!(session.native_input_recorded_count(), recorded + 2);

        let mut tail_budget = NativeInputBudget::new(&clock);
        let mut remaining_queries = Vec::new();
        let mut candidate = session
            .poll_native_host_frame(&mut tail_budget, |query| {
                assert_eq!(query_metadata(query), release_metadata);
                remaining_queries.push(query);
                resolve(query)
            })
            .unwrap()
            .unwrap();
        assert_eq!(tail_budget.remaining_records(), 255);
        assert_eq!(
            remaining_queries
                .iter()
                .filter_map(|query| match query {
                    NativeReceiverQuery::Surface(query) => Some(*query.purpose()),
                    NativeReceiverQuery::WindowHit(_) => None,
                })
                .collect::<Vec<_>>(),
            [
                NativeReceiverPurpose::ClickDelivery,
                NativeReceiverPurpose::DragDelivery
            ]
        );
        assert!(!candidate.native_input_pending());
        assert!(candidate.view().item(FIRST).unwrap().is_selected());
        let prefix_version = candidate.version();
        assert_ne!(prefix_version, before);
        candidate.submit_layout_reset(&reset).unwrap();
        let replacement_version = candidate.version();
        assert!(candidate.view().item(SECOND).unwrap().is_selected());
        measure(&mut candidate);
        drop(candidate);
        assert_eq!(session.version(), before);
        assert!(session.view().item(SECOND).unwrap().is_selected());
        assert!(!session.has_active_gesture());
        assert!(session.has_pending_native_input());
        assert!(session.is_current_native_binding(old));
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert_eq!(session.native_input_recorded_count(), recorded + 2);

        let mut retry_budget = NativeInputBudget::new(&clock);
        let mut retry_queries = Vec::new();
        let retry = if synchronous_wrapper {
            retry_budget.complete_unsequenced_records(255);
            assert!(
                session
                    .poll_native_host_frame(&mut retry_budget, |query| {
                        assert_eq!(query_metadata(query), metadata);
                        retry_queries.push(query);
                        resolve(query)
                    })
                    .unwrap()
                    .is_none()
            );
            assert_eq!(retry_budget.remaining_records(), 0);
            assert_eq!(retry_queries, queries);
            assert_eq!(session.version(), before);
            assert!(session.view().item(SECOND).unwrap().is_selected());
            assert!(session.has_pending_native_input());
            assert!(session.is_current_native_binding(old));
            assert_eq!(session.native_input_recorded_count(), recorded + 2);
            session
                .begin_native_layout_reset_frame(&reset, |query| {
                    assert_eq!(query_metadata(query), release_metadata);
                    retry_queries.push(query);
                    resolve(query)
                })
                .unwrap()
        } else {
            let mut retry = session
                .poll_native_host_frame(&mut retry_budget, |query| {
                    retry_queries.push(query);
                    resolve(query)
                })
                .unwrap()
                .unwrap();
            assert_eq!(retry_budget.remaining_records(), 254);
            assert!(retry.view().item(FIRST).unwrap().is_selected());
            assert_eq!(retry.version(), prefix_version);
            retry.submit_layout_reset(&reset).unwrap();
            retry
        };
        assert_eq!(retry_queries, [queries, remaining_queries].concat());
        assert!(!retry.native_input_pending());
        assert!(retry.view().item(SECOND).unwrap().is_selected());
        assert_eq!(retry.version(), replacement_version);
        let report = commit_measured(retry);
        assert!(report.workspace_changed());
        assert!(!report.native_input_pending());
        assert_eq!(report.before(), before);
        assert_eq!(report.after(), replacement_version);
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::LayoutReset {
                previous: prefix_version,
                current: replacement_version,
            }]
        );
        let (surface, ..) = assert_default_content(&session);
        let [current] = report.native_bindings() else {
            panic!("reset and its native prefix must publish one root successor roster");
        };
        assert_ne!(*current, old);
        assert_eq!(current.surface(), surface);
        assert_eq!(current.window_token(), old.window_token());
        assert!(!session.is_current_native_binding(old));
        assert!(session.is_current_native_binding(*current));
        assert!(!session.has_pending_native_input());
        assert!(!session.has_active_gesture());
        assert!(session.bind_presented_receiver(&receiver).is_none());
        assert_eq!(session.native_input_recorded_count(), recorded + 2);

        let mut idle_budget = NativeInputBudget::new(&clock);
        let idle = session
            .poll_native_host_frame(&mut idle_budget, |_| {
                panic!("the committed input group must not query its retired receiver again")
            })
            .unwrap()
            .unwrap();
        assert_eq!(idle_budget.remaining_records(), 256);
        assert!(!idle.native_input_pending());
        let idle = commit_deferred(idle);
        assert!(idle.inputs().is_empty());
        assert!(!idle.workspace_changed());
        assert_eq!(idle.native_bindings(), &[*current]);
        assert_eq!(session.version(), replacement_version);
        assert_eq!(session.native_input_recorded_count(), recorded + 2);
    }
}

#[cfg(feature = "serde")]
#[test]
fn document_bound_reset_reuses_closed_key_history_without_replacing_durable_sidecars() {
    use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};

    let document = DockspaceDocumentId::from_bytes([0x61; 16]);
    let keys = [
        ("first", FIRST),
        ("second", SECOND),
        ("third", THIRD),
        ("fourth", FOURTH),
    ];
    let mut bootstrap = DockspaceDocumentBootstrap::new(document);
    for (key, item) in keys {
        assert_eq!(bootstrap.ensure_item(key).unwrap(), item);
    }
    let defaults = default_layout();
    let mut session =
        DockspaceSession::from_persistent_layout(defaults.clone(), policy(), bootstrap).unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    close_root(&mut session, ROOT);
    close_root(&mut session, FLOATING_ROOT);
    session.save_document_json().unwrap();
    let generation = session.next_document_generation();
    assert_eq!(generation, Some(1));
    let before = session.version();
    let reset = session.prepare_layout_reset(defaults).unwrap();
    // Reset does not replace the append-only key map prepared document restore owns.
    let later = session.ensure_external_item("later").unwrap();
    assert!(![FIRST, SECOND, THIRD, FOURTH].contains(&later));
    {
        let candidate = session.begin_layout_reset_frame(&reset).unwrap();
        for (key, item) in keys {
            assert_eq!(candidate.external_key_for_item(item), Some(key));
        }
    }
    assert_eq!(session.version(), before);
    assert!(session.view().surfaces().next().is_none());
    assert_eq!(session.next_document_generation(), generation);
    commit_measured(session.begin_layout_reset_frame(&reset).unwrap());
    assert_default_content(&session);
    assert_eq!(session.document_id(), Some(document));
    assert_eq!(session.next_document_generation(), generation);
    for (key, item) in keys {
        assert_eq!(session.item_id_for_external_key(key), Some(item));
        assert_eq!(session.external_key_for_item(item), Some(key));
    }
    assert_eq!(session.item_id_for_external_key("later"), Some(later));
    assert_eq!(session.external_key_for_item(later), Some("later"));
    let published = session.version();
    let error = session
        .prepare_layout_reset(single_pane_layout(ItemId::new(99)))
        .unwrap_err();
    assert_eq!(
        error.kind(),
        DockspaceRuntimeErrorKind::InvalidConfiguration
    );
    assert_eq!(session.version(), published);
    assert_eq!(session.next_document_generation(), generation);
    assert_eq!(session.item_id_for_external_key("later"), Some(later));
    assert_default_content(&session);
    session.save_document_json().unwrap();
    assert_eq!(session.next_document_generation(), Some(2));
}
