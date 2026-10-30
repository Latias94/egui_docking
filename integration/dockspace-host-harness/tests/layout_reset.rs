//! Application-owned defaults use the public reset transaction without serialization.

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePaneFocusObservation, DockspacePaneFocusRequest,
    DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport, HostInputOutcome,
    SurfacePaintDisposition, SurfacePresentationResult, SurfaceUnavailableReason,
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
fn cloned_defaults_reset_repeatedly_after_surface_root_and_floating_retirement() {
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

    let mut previous_ids = original;
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
        assert_ne!(ids.0, SURFACE);
        assert_ne!(ids.1, ROOT);
        assert_ne!(ids.2, FLOATING_ROOT);
        assert_ne!(ids.3, FLOATING);
        assert_ne!(ids.0, previous_ids.0);
        assert_ne!(ids.1, previous_ids.1);
        assert_ne!(ids.2, previous_ids.2);
        assert_ne!(ids.3, previous_ids.3);
        previous_ids = ids;
    }
    assert!(defaults.view().surface(SURFACE).is_some());
    assert!(defaults.view().root(ROOT).is_some());
    assert_eq!(
        defaults.view().contained(FLOATING).unwrap().rect(),
        durable_rect()
    );
}

#[test]
fn reset_reserves_incoming_fresh_identities_before_rebasing_retired_ones() {
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
    assert!(first.surface().get() > fresh_surface.get());
    assert_eq!(third.surface(), first.surface());
    assert!(first.root().get() > fresh_floating_root.get());
    assert!(third.root().get() > fresh_floating_root.get());
    assert_ne!(first.root(), third.root());
    assert_eq!(first.tabs().items(), &[FIRST, SECOND]);
    assert_eq!(first.tabs().selected(), Some(SECOND));
    assert_eq!(third.tabs().items(), &[THIRD, FOURTH]);
    assert_eq!(third.tabs().selected(), Some(FOURTH));
    let rebased_floating = third.contained().unwrap();
    assert!(rebased_floating.get() > fresh_floating.get());
    assert_eq!(
        view.contained(rebased_floating).unwrap().rect(),
        durable_rect()
    );

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
fn reset_authority_rejects_locked_foreign_and_stale_preparations_without_mutation() {
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
    let error = match session.begin_layout_reset_frame(&prepared) {
        Ok(_) => panic!("a reset prepared before locking must not bypass the lock"),
        Err(error) => error,
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
    let error = match other.begin_layout_reset_frame(&prepared) {
        Ok(_) => panic!("a reset must remain bound to its preparing session"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(other.version(), other_version);
    assert_eq!(assert_default_content(&other), other_ids);

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit_measured(frame);
    let changed = session.version();
    let error = match session.begin_layout_reset_frame(&prepared) {
        Ok(_) => panic!("a committed semantic mutation must stale the preparation"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(session.version(), changed);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    let reset = session.prepare_layout_reset(defaults).unwrap();
    commit_measured(session.begin_layout_reset_frame(&reset).unwrap());
    assert_default_content(&session);
    let current = session.version();
    let error = match session.begin_layout_reset_frame(&reset) {
        Ok(_) => panic!("a successful reset must consume the prepared version"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(session.version(), current);
}

#[test]
fn dropped_reset_retries_exactly_and_old_action_output_and_focus_cannot_authorize() {
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

    let candidate_ids = {
        let mut candidate = session.begin_layout_reset_frame(&reset).unwrap();
        let main = candidate.view().item(SECOND).unwrap();
        let contained = candidate.view().item(FOURTH).unwrap();
        let ids = (
            main.surface(),
            main.root(),
            contained.root(),
            contained.contained(),
        );
        assert!(main.is_selected());
        assert_eq!(
            candidate.select_item_current(FIRST).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        assert_eq!(
            candidate.replace_policy(current_policy).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        measure(&mut candidate);
        assert!(
            candidate.paint_plan(ids.0).unwrap().is_none(),
            "measurement contributions install the replacement scene only when their frame commits"
        );
        ids
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

    let retry = session.begin_layout_reset_frame(&reset).unwrap();
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
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::LayoutReset {
            previous: before,
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
    assert_eq!(fresh.item(), Some(SECOND));
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
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
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
