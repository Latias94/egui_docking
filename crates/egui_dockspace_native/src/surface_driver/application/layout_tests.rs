//! Layout commands use the native application's bounded command and result slot.

use std::error::Error;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceRuntimeError, DockspaceSession, HostFrameReport, HostInputOutcome,
    NativeReceiverAnswer, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeHostProtocolError;
use crate::{
    NativeActionRequestError, NativeActionStatus, NativeLayoutRequestError, NativeRuntimeErrorKind,
    NativeWindowClosePolicy,
};

use super::NativeRuntimeState;

const SURFACE: SurfaceId = SurfaceId::new(1);
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(10);
const PRESET_ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

struct Panes(Vec<(ItemId, String)>);

impl PaneView for Panes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Layout command pane".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        egui_dockspace::PaneResponse::default()
    }
}

fn surface(
    id: SurfaceId,
    root: RootId,
    items: &[ItemId],
    selected: ItemId,
) -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(
        id,
        DockspaceRootLayout::new(
            root,
            DockspaceNode::central_tabs_with_selection(items.iter().copied(), Some(selected))
                .unwrap(),
        ),
    )
}

fn layout(root: RootId, items: &[ItemId], selected: ItemId) -> DockspaceLayout {
    DockspaceLayout::new([surface(SURFACE, root, items, selected)]).unwrap()
}

fn state_from_session(session: DockspaceSession) -> NativeRuntimeState<Panes> {
    NativeRuntimeState::new(
        egui::Id::unique("native-layout-command-regressions"),
        session,
        SURFACE,
        Panes(vec![
            (FIRST, "unsaved first pane".to_owned()),
            (SECOND, "unsaved second pane".to_owned()),
        ]),
        DockStyle::default(),
        NativeWindowClosePolicy::Cancel,
    )
    .unwrap()
}

fn test_state() -> NativeRuntimeState<Panes> {
    state_from_session(
        DockspaceSession::from_layout(layout(ROOT, &[FIRST, SECOND], FIRST), DockPolicy::default())
            .unwrap(),
    )
}

fn commit_queued(
    state: &mut NativeRuntimeState<Panes>,
    local_selection: Option<ItemId>,
) -> HostFrameReport {
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!frame.native_input_pending());
    let submission = action.submit(&mut frame.frame, published).unwrap();
    if let Some(item) = local_selection {
        frame.frame.select_item_current(item).unwrap();
        assert!(frame.frame.view().item(item).unwrap().is_selected());
    }
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    assert!(
        state
            .application_actions
            .settle_submission(Some(submitted), &report)
            .unwrap()
    );
    state.coordinator.settle_host_frame_report(&report).unwrap();
    report
}

fn take(state: &mut NativeRuntimeState<Panes>) -> NativeActionStatus {
    let status = state.take_action_status().unwrap().unwrap();
    assert!(state.take_action_status().is_none());
    status
}

fn assert_queue_error(error: NativeLayoutRequestError, expected: NativeActionRequestError) {
    assert_eq!(error.kind(), NativeRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<NativeActionRequestError>(),
        Some(&expected)
    );
}

#[test]
fn queued_reset_is_stale_after_publication_and_repeated_resets_preserve_semantic_identities() {
    let mut state = test_state();
    let panes = state.panes.0.clone();
    let defaults = layout(PRESET_ROOT, &[FIRST, THIRD], THIRD);
    for round in 0..2 {
        let before = state.coordinator.session().version();
        state.request_layout_reset(defaults.clone()).unwrap();
        assert_eq!(state.coordinator.session().version(), before);
        assert_queue_error(
            state
                .request_layout_reset(DockspaceLayout::empty())
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );
        assert_eq!(
            state.request_select_item(FIRST),
            Err(NativeActionRequestError::Busy)
        );
        let report = commit_queued(&mut state, Some(if round == 0 { SECOND } else { FIRST }));
        if round == 0 {
            assert!(
                report
                    .inputs()
                    .iter()
                    .any(|input| matches!(input, HostInputOutcome::NativeSurfaceRegistered { .. }))
            );
            assert!(!matches!(
                report.inputs().first(),
                Some(HostInputOutcome::LayoutReset { .. })
            ));
        }
        let mut terminals = report.inputs().iter().filter_map(|input| match input {
            HostInputOutcome::LayoutReset { previous, current } => Some((*previous, *current)),
            _ => None,
        });
        let (previous, current) = terminals.next().unwrap();
        assert!(terminals.next().is_none());
        assert_ne!(previous, report.before());
        assert_eq!(current, state.coordinator.session().version());
        assert!(
            !report
                .inputs()
                .iter()
                .any(|input| matches!(input, HostInputOutcome::ProductActionApplied(_)))
        );
        assert_eq!(
            state.request_select_item(FIRST),
            Err(NativeActionRequestError::Busy)
        );
        assert_eq!(
            take(&mut state),
            NativeActionStatus::LayoutReset { previous, current }
        );

        let restored = state.coordinator.session().view().item(THIRD).unwrap();
        assert_eq!(restored.surface(), SURFACE);
        assert_eq!(restored.tabs().items(), &[FIRST, THIRD]);
        assert!(restored.is_selected());
        assert_eq!(restored.root(), PRESET_ROOT);
        assert_ne!(current, previous);
        assert!(state.coordinator.session().view().item(SECOND).is_none());
        assert_eq!(state.panes.0, panes);
        assert!(state.shutdown.is_none());
    }

    let before = state.coordinator.session().version();
    state.request_layout_reset(defaults.clone()).unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.select_item_current(FIRST).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    let accepted = state.coordinator.session().version();
    assert_ne!(before, accepted);

    let stale = commit_queued(&mut state, None);
    assert_eq!(stale.after(), accepted);
    assert!(
        !stale
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::LayoutReset { .. }))
    );
    assert_queue_error(
        state.request_layout_reset(defaults).unwrap_err(),
        NativeActionRequestError::Busy,
    );
    assert_eq!(
        take(&mut state),
        NativeActionStatus::Stale {
            expected: before,
            accepted
        }
    );
    let retained = state.coordinator.session().view().item(FIRST).unwrap();
    assert!(retained.is_selected());
    assert_eq!(retained.root(), PRESET_ROOT);
    assert_eq!(state.panes.0, panes);
    assert!(state.shutdown.is_none());
    state.request_select_item(THIRD).unwrap();
    commit_queued(&mut state, None);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(egui_dockspace::DockspaceActionStatus::Applied(
            dockspace::model::DockspaceActionOutcome::Selected { item: THIRD, .. }
        ))
    ));
}

#[test]
fn lock_and_host_validation_leave_the_slot_free_and_stopped_takes_precedence() {
    let mut state = test_state();
    let panes = state.panes.0.clone();
    state.request_set_arrangement_locked(true).unwrap();
    commit_queued(&mut state, None);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged { locked: true, .. }
    ));
    let before = state.coordinator.session().version();
    let error = state
        .request_layout_reset(layout(PRESET_ROOT, &[FIRST, THIRD], THIRD))
        .unwrap_err();
    assert_eq!(error.kind(), NativeRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<DockspaceRuntimeError>()
            .unwrap()
            .kind(),
        NativeRuntimeErrorKind::OperationConflict
    );
    assert!(!state.application_actions.is_occupied());
    assert_eq!(state.coordinator.session().version(), before);

    state.request_set_arrangement_locked(false).unwrap();
    commit_queued(&mut state, None);
    take(&mut state);
    let before = state.coordinator.session().version();
    let missing_host =
        DockspaceLayout::new([surface(OTHER_SURFACE, ROOT, &[FIRST], FIRST)]).unwrap();
    let extra_surface = DockspaceLayout::new([
        surface(SURFACE, ROOT, &[FIRST], FIRST),
        surface(OTHER_SURFACE, RootId::new(11), &[SECOND], SECOND),
    ])
    .unwrap();
    assert!(
        state
            .coordinator
            .session()
            .prepare_layout_reset(extra_surface.clone())
            .is_ok()
    );
    for (candidate, kind) in [
        (
            missing_host.clone(),
            NativeRuntimeErrorKind::InvalidConfiguration,
        ),
        (extra_surface, NativeRuntimeErrorKind::Unsupported),
    ] {
        assert_eq!(
            state.request_layout_reset(candidate).unwrap_err().kind(),
            kind
        );
        assert!(!state.application_actions.is_occupied());
        assert!(state.take_action_status().is_none());
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(state.panes.0, panes);
        assert!(state.shutdown.is_none());
    }

    state
        .request_layout_reset(layout(PRESET_ROOT, &[FIRST], FIRST))
        .unwrap();
    state.stop_current_output(NativeHostProtocolError::RootSurfaceUnavailable(SURFACE).into());
    assert!(state.application_actions.is_occupied());
    assert_queue_error(
        state
            .request_layout_reset(missing_host.clone())
            .unwrap_err(),
        NativeActionRequestError::Stopped,
    );
    #[cfg(feature = "serde")]
    {
        assert_queue_error(
            state
                .request_document_restore_json(b"{", |_, _| {
                    panic!("shutdown must take precedence over the occupied slot and JSON decoding")
                })
                .unwrap_err(),
            NativeActionRequestError::Stopped,
        );
        assert_queue_error(
            state.save_document_json().unwrap_err(),
            NativeActionRequestError::Stopped,
        );
    }
    assert!(state.take_action_status().unwrap().is_err());
    assert!(state.take_action_status().is_none());
    assert_queue_error(
        state.request_layout_reset(missing_host).unwrap_err(),
        NativeActionRequestError::Stopped,
    );
    assert_eq!(state.panes.0, panes);
}

#[cfg(feature = "serde")]
mod persistence {
    mod service_deadline_tests;

    use std::cell::RefCell;

    use dockspace::geometry::{PhysicalRect, ScaleFactor};
    use dockspace::runtime::{
        DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceError,
        DockspacePersistenceErrorKind, HostWindowToken, NativeCloseState, NativeCreateStrategy,
        NativeInputBudget, NativeMonotonicClock, NativePointerRoster, NativeSurfaceRole,
        NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
        NativeWorkAreaRoster,
    };
    use eframe::{NativeHostWake, NativeWindowingBackend};
    use winit::window::WindowId;

    use super::*;

    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x4c; 16]);

    fn persistent_session(candidate: DockspaceLayout, policy: DockPolicy) -> DockspaceSession {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), FIRST);
        assert_eq!(bootstrap.ensure_item("pane:second").unwrap(), SECOND);
        DockspaceSession::from_persistent_layout(candidate, policy, bootstrap).unwrap()
    }

    fn persistent_state() -> NativeRuntimeState<Panes> {
        state_from_session(persistent_session(
            layout(ROOT, &[FIRST, SECOND], FIRST),
            DockPolicy::default(),
        ))
    }

    fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
        document == DOCUMENT && matches!(key, "pane:first" | "pane:second" | "pane:new")
    }

    fn child_layout() -> DockspaceLayout {
        DockspaceLayout::new([
            surface(SURFACE, ROOT, &[FIRST], FIRST),
            surface(OTHER_SURFACE, RootId::new(11), &[SECOND], SECOND),
        ])
        .unwrap()
    }

    fn ready_window_facts(x: f64) -> NativeWindowFacts {
        let scale = ScaleFactor::new(1.0).unwrap();
        NativeWindowFacts::live()
            .with_content_bounds(PhysicalRect::new(x, 0.0, 640.0, 480.0).unwrap())
            .with_outer_bounds(PhysicalRect::new(x - 8.0, -30.0, 656.0, 518.0).unwrap())
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None)
            .with_close(NativeCloseState::Clear, None)
    }

    fn commit_session_prefix(session: &mut DockspaceSession) -> HostFrameReport {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    fn saved_child_document() -> Vec<u8> {
        let mut session = persistent_session(child_layout(), DockPolicy::default());
        session
            .enable_managed_native_host(
                NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(SURFACE, HostWindowToken::new(11))
            .unwrap();
        commit_session_prefix(&mut session);
        session
            .register_owned_native_child(OTHER_SURFACE, HostWindowToken::new(22), SURFACE)
            .unwrap();
        let registration = commit_session_prefix(&mut session);
        assert_eq!(registration.native_bindings().len(), 2);
        session
            .report_managed_native_snapshot(
                registration
                    .native_bindings()
                    .iter()
                    .copied()
                    .map(|binding| {
                        let x = if binding.surface() == SURFACE {
                            0.0
                        } else {
                            720.0
                        };
                        (binding, ready_window_facts(x))
                    }),
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        commit_session_prefix(&mut session);
        session.save_document_json().unwrap()
    }

    fn commit_native_prefix(state: &mut NativeRuntimeState<Panes>) -> HostFrameReport {
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        state.coordinator.settle_host_frame_report(&report).unwrap();
        report
    }

    fn queued_child_restore() -> NativeRuntimeState<Panes> {
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut state = state_from_session(persistent_session(
            layout(ROOT, &[FIRST, SECOND], FIRST),
            policy,
        ));
        let registration = commit_native_prefix(&mut state);
        let root = registration.native_bindings()[0];
        state
            .coordinator
            .bind_viewport(egui::ViewportId::ROOT, WindowId::from(11), root)
            .unwrap();
        state
            .request_document_restore_json(&saved_child_document(), recognize)
            .unwrap();
        let mut report = commit_queued(&mut state, None);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        assert!(report.take_native_effects().is_empty());
        assert!(state.coordinator.session().has_pending_native_create());
        assert!(state.coordinator.deferred_viewport_specs().is_empty());
        assert!(state.shutdown.is_none());
        state
    }

    fn record_fresh_root_facts(state: &mut NativeRuntimeState<Panes>) {
        let root = state
            .coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .unwrap();
        state
            .coordinator
            .report_snapshot(
                [(root, ready_window_facts(0.0))],
                NativeWorkAreaRoster::Unknown,
                crate::capabilities::capabilities_for_backend(NativeWindowingBackend::Windows),
            )
            .unwrap();
    }

    #[test]
    fn root_rebinding_publishes_the_host_field_and_same_window_route_before_report_settlement() {
        let mut state = persistent_state();
        let registration = commit_native_prefix(&mut state);
        let original = registration.native_bindings()[0];
        state
            .coordinator
            .bind_viewport(egui::ViewportId::ROOT, WindowId::from(11), original)
            .unwrap();
        record_fresh_root_facts(&mut state);
        commit_native_prefix(&mut state);
        let panes = state.panes.0.clone();

        for incoming in [OTHER_SURFACE, SURFACE] {
            let predecessor = state
                .coordinator
                .viewport_binding(egui::ViewportId::ROOT)
                .unwrap();
            let before_surface = state.root_surface;
            let candidate =
                DockspaceLayout::new([surface(incoming, PRESET_ROOT, &[FIRST, SECOND], SECOND)])
                    .unwrap();
            state
                .request_layout_reset_rebinding_root(candidate, incoming)
                .unwrap();
            let action = state.application_actions.take().unwrap();
            let published = state.coordinator.session().version();
            let mut frame = state
                .coordinator
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            let submission = action.submit(&mut frame.frame, published).unwrap();
            let submitted = submission.finish(&mut frame.frame).unwrap();
            let proposals = frame
                .frame
                .proposed_native_root_continuations()
                .collect::<Vec<_>>();
            assert_eq!(proposals.len(), 1);
            assert_eq!(proposals[0].predecessor(), predecessor);
            let successor = proposals[0].successor();
            assert_ne!(successor.surface(), before_surface);
            assert_eq!(successor.surface(), incoming);
            assert_ne!(successor, predecessor);
            assert_eq!(state.root_surface, before_surface);
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            let mut report = frame
                .commit_publishing_root(&mut state.root_surface)
                .unwrap();

            // No report settlement has run: all three published identities already agree.
            assert_eq!(state.root_surface, successor.surface());
            assert_eq!(report.native_root_continuations(), proposals);
            assert_eq!(
                state.coordinator.viewport_binding(egui::ViewportId::ROOT),
                Some(successor)
            );
            assert!(
                state
                    .coordinator
                    .session()
                    .view()
                    .surface(before_surface)
                    .is_none()
            );
            assert!(
                state
                    .coordinator
                    .session()
                    .view()
                    .item(SECOND)
                    .unwrap()
                    .is_selected()
            );
            assert!(
                state
                    .coordinator
                    .session()
                    .native_local_coordinates(successor)
                    .is_none()
            );
            assert!(
                state
                    .coordinator
                    .session()
                    .presented_surface(successor.surface())
                    .is_none()
            );
            assert!(report.take_native_effects().is_empty());
            state
                .coordinator
                .dispatch_bridge()
                .with_viewports(|routes| {
                    assert_eq!(
                        routes.window(egui::ViewportId::ROOT),
                        Some(WindowId::from(11))
                    );
                    assert!(routes.has_window_receipt(egui::ViewportId::ROOT, successor));
                    assert!(!routes.has_window_receipt(egui::ViewportId::ROOT, predecessor));
                });
            assert!(
                state
                    .application_actions
                    .settle_submission(Some(submitted), &report)
                    .unwrap()
            );
            assert!(matches!(
                take(&mut state),
                NativeActionStatus::LayoutReset { .. }
            ));
            state.coordinator.settle_host_frame_report(&report).unwrap();
            state.coordinator.settle_host_frame_report(&report).unwrap();
            assert_eq!(state.root_surface, successor.surface());
            assert_eq!(state.panes.0, panes);
            record_fresh_root_facts(&mut state);
            commit_native_prefix(&mut state);
        }
    }

    #[test]
    fn root_rebinding_conflicts_leave_the_host_running_and_release_the_command_slot() {
        let mut state =
            state_from_session(persistent_session(child_layout(), DockPolicy::default()));
        let registration = commit_native_prefix(&mut state);
        let root = registration.native_bindings()[0];
        state
            .coordinator
            .bind_viewport(egui::ViewportId::ROOT, WindowId::from(11), root)
            .unwrap();
        state
            .coordinator
            .register_native_root(OTHER_SURFACE, HostWindowToken::new(22))
            .unwrap();
        let registration = commit_native_prefix(&mut state);
        let peer = *registration
            .native_bindings()
            .iter()
            .find(|binding| binding.surface() == OTHER_SURFACE)
            .unwrap();
        state
            .coordinator
            .bind_viewport(
                egui::ViewportId::from_hash_of("occupied-continuation-target"),
                WindowId::from(22),
                peer,
            )
            .unwrap();
        let candidate = || {
            DockspaceLayout::new([surface(
                OTHER_SURFACE,
                PRESET_ROOT,
                &[FIRST, SECOND],
                SECOND,
            )])
            .unwrap()
        };
        let before = state.coordinator.session().version();
        assert_eq!(
            state
                .request_layout_reset_rebinding_root(candidate(), OTHER_SURFACE)
                .unwrap_err()
                .kind(),
            NativeRuntimeErrorKind::OperationConflict
        );
        assert!(!state.application_actions.is_occupied());
        assert!(state.shutdown.is_none());
        assert_eq!(state.coordinator.session().version(), before);

        // Removing the peer from the graph leaves its physical route occupied.
        // Semantic identity preservation cannot bypass native window ownership.
        state
            .request_layout_reset(layout(PRESET_ROOT, &[FIRST, SECOND], SECOND))
            .unwrap();
        commit_queued(&mut state, None);
        take(&mut state);
        assert!(
            state
                .coordinator
                .session()
                .view()
                .surface(OTHER_SURFACE)
                .is_none()
        );
        assert!(state.coordinator.surface_viewport(OTHER_SURFACE).is_some());
        let root = state
            .coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .unwrap();
        let before = state.coordinator.session().version();
        let reset = state
            .coordinator
            .session()
            .prepare_layout_reset(candidate())
            .unwrap();
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .submit_native_layout_reset(&reset, root, OTHER_SURFACE)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let error = frame
            .commit_publishing_root(&mut state.root_surface)
            .expect_err("the retained physical route must reject publication atomically");
        assert_eq!(error.kind(), NativeRuntimeErrorKind::HostProtocol);
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(state.root_surface, root.surface());
        assert_eq!(
            state.coordinator.viewport_binding(egui::ViewportId::ROOT),
            Some(root)
        );
        assert!(
            state
                .coordinator
                .session()
                .view()
                .surface(OTHER_SURFACE)
                .is_none()
        );
        assert!(!state.application_actions.is_occupied());
        assert!(state.shutdown.is_none());

        // A preparation may lose eligibility after enqueue without changing the
        // durable workspace version. Settle only this command as rejected.
        let fresh = SurfaceId::new(9);
        let candidate =
            DockspaceLayout::new([surface(fresh, PRESET_ROOT, &[FIRST, SECOND], SECOND)]).unwrap();
        state
            .request_layout_reset_rebinding_root(candidate, fresh)
            .unwrap();
        state
            .coordinator
            .report_snapshot(
                [(
                    root,
                    ready_window_facts(0.0).with_close(NativeCloseState::Requested, None),
                )],
                NativeWorkAreaRoster::Unknown,
                crate::capabilities::capabilities_for_backend(NativeWindowingBackend::Windows),
            )
            .unwrap();
        commit_native_prefix(&mut state);
        assert_eq!(state.coordinator.session().version(), before);
        let report = commit_queued(&mut state, None);
        assert!(report.native_root_continuations().is_empty());
        assert_eq!(
            take(&mut state),
            NativeActionStatus::RootContinuationRejected {
                predecessor: root,
                incoming_root: fresh,
                kind: dockspace::runtime::DockspaceRuntimeErrorKind::OperationConflict,
            }
        );
        assert_eq!(state.root_surface, root.surface());
        assert_eq!(
            state.coordinator.viewport_binding(egui::ViewportId::ROOT),
            Some(root)
        );
        assert!(state.shutdown.is_none());
        state.request_select_item(SECOND).unwrap();
        commit_queued(&mut state, None);
        assert!(matches!(take(&mut state), NativeActionStatus::Dock(_)));
        assert!(state.shutdown.is_none());
    }

    #[test]
    fn restore_rejects_an_undeclared_extra_surface_without_occupying_the_slot() {
        let mut state = persistent_state();
        let mut external = persistent_session(child_layout(), DockPolicy::default());
        let saved = external.save_document_json().unwrap();
        let before = state.coordinator.session().version();
        let generation = state.coordinator.session().next_document_generation();
        let error = state
            .request_document_restore_json(&saved, recognize)
            .unwrap_err();
        assert_eq!(error.kind(), NativeRuntimeErrorKind::Unsupported);
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<DockspaceRuntimeError>()
                .unwrap()
                .kind(),
            NativeRuntimeErrorKind::Unsupported
        );
        assert!(!state.application_actions.is_occupied());
        assert!(state.take_action_status().is_none());
        assert!(!state.coordinator.session().has_pending_native_create());
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(
            state.coordinator.session().next_document_generation(),
            generation
        );
    }

    #[test]
    fn declared_child_restore_waits_for_fresh_facts_and_service_starts_it_once() {
        let mut state = queued_child_restore();
        let context = egui::Context::default();
        let clock = NativeMonotonicClock::new(|| std::time::Duration::ZERO);
        let mut prefix_budget = NativeInputBudget::new(&clock);
        assert!(
            state
                .service_pending_prefixes(&context, None, SURFACE, &mut prefix_budget)
                .unwrap()
        );
        assert!(prefix_budget.remaining_records() < 256);

        let mut waiting_budget = NativeInputBudget::new(&clock);
        let before = state.coordinator.session().version();
        assert!(
            state
                .service_pending_prefixes(&context, None, SURFACE, &mut waiting_budget)
                .unwrap()
        );
        assert_eq!(waiting_budget.remaining_records(), 256);
        assert_eq!(state.coordinator.session().version(), before);
        assert!(state.coordinator.session().has_pending_native_create());
        assert!(state.coordinator.deferred_viewport_specs().is_empty());

        record_fresh_root_facts(&mut state);
        let mut snapshot = commit_native_prefix(&mut state);
        assert!(snapshot.take_native_effects().is_empty());
        let mut budget = NativeInputBudget::new(&clock);
        budget.complete_unsequenced_records(255);
        assert!(
            !state
                .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                .unwrap(),
            "starting one queued child consumes the final shared record and yields"
        );
        assert_eq!(budget.remaining_records(), 0);
        let specs = state.coordinator.deferred_viewport_specs();
        assert_eq!(specs.len(), 1);
        let child = specs[0];
        assert_eq!(child.role(), NativeSurfaceRole::Child);
        assert_eq!(
            child.placement(),
            PhysicalRect::new(712.0, -30.0, 656.0, 518.0).unwrap()
        );
        assert_eq!(
            child.binding().surface(),
            state
                .coordinator
                .session()
                .view()
                .item(SECOND)
                .unwrap()
                .surface()
        );

        let mut retry_budget = NativeInputBudget::new(&clock);
        assert!(
            state
                .service_pending_prefixes(&context, None, SURFACE, &mut retry_budget)
                .unwrap()
        );
        assert_eq!(retry_budget.remaining_records(), 256);
        assert_eq!(state.coordinator.deferred_viewport_specs(), specs);
        assert!(state.coordinator.session().has_pending_native_create());
        assert!(state.shutdown.is_none());
        assert!(eframe::current_native_output_token().is_none());
    }

    #[test]
    fn service_refill_advances_close_and_ready_create_after_repeated_budget_exhaustion() {
        use std::num::NonZeroUsize;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Duration;

        use dockspace::runtime::{
            NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
        };
        use winit::dpi::PhysicalPosition;
        use winit::event::{DeviceId, PointerEventFacts, WindowEvent};

        use crate::coordinator::NativeCoordinator;
        use crate::event::NativeWindowEventRecord;
        use crate::mailbox::{HostRecord, NativeDispatch};
        use crate::surface_driver::ROOT_WINDOW_TOKEN;

        const INITIAL_BACKLOG: usize = 2 * 256 + 17;
        const REFILL_PER_TURN: usize = 16;
        const TURNS: usize = 6;

        // Age and dispatch share one clock. Dispatch itself is frozen so that
        // exhaustion measures record accounting, not OS scheduling or wall time.
        let ticks = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&ticks);
        let clock = NativeMonotonicClock::new(move || {
            Duration::from_millis(observed.load(Ordering::SeqCst))
        });
        let slots = |count| NonZeroUsize::new(count).unwrap();
        let input = NativeInputStorageLimit::new(slots(128), 512 * 1024)
            .with_producer_storage_limit(NativeProducerStorageLimit::new(
                slots(4),
                slots(1),
                128 * 1024,
            ))
            .with_report_storage_limit(NativeReportStorageLimit::new(
                slots(64),
                slots(4),
                256 * 1024,
            ));
        let storage = crate::NativeStorageLimit::new(input, 4 * 1024 * 1024, 4, 2).unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut coordinator = NativeCoordinator::new_with_combined_storage_limit_and_clock(
            persistent_session(layout(ROOT, &[FIRST, SECOND], FIRST), policy),
            NativePointerRoster::Exact(Vec::new()),
            storage.combined,
            clock.clone(),
        )
        .unwrap();
        coordinator
            .register_native_root(SURFACE, ROOT_WINDOW_TOKEN)
            .unwrap();
        let mut state = NativeRuntimeState {
            coordinator,
            root_surface: SURFACE,
            instance_id: egui::Id::unique("service-refill-close-create"),
            owned_update: None,
            pending_service: false,
            style: DockStyle::default(),
            panes: Panes(Vec::new()),
            application_actions: Default::default(),
            pass_actions: Default::default(),
            close_policy: NativeWindowClosePolicy::CloseContent,
            root_context: None,
            shutdown: None,
        };
        let context = egui::Context::default();
        let registration = commit_native_prefix(&mut state);
        let predecessor = registration.native_bindings()[0];
        let window = WindowId::from(91);
        state
            .coordinator
            .bind_viewport(egui::ViewportId::ROOT, window, predecessor)
            .unwrap();
        state
            .request_document_restore_json(&saved_child_document(), recognize)
            .unwrap();
        let mut restored = commit_queued(&mut state, None);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        assert!(restored.take_native_effects().is_empty());
        assert!(
            state
                .service_pending_prefixes(
                    &context,
                    None,
                    SURFACE,
                    &mut NativeInputBudget::new(&clock),
                )
                .unwrap()
        );
        record_fresh_root_facts(&mut state);
        commit_native_prefix(&mut state);
        let root = state
            .coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .unwrap();
        assert!(state.coordinator.session().view().item(FIRST).is_some());
        {
            let mut probe = state
                .coordinator
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            assert!(probe.frame.advance_next_native_surface_create().unwrap());
            // Dropping the eligibility probe must leave the create for service.
        }
        assert!(state.coordinator.deferred_viewport_specs().is_empty());

        let bridge = state.coordinator.dispatch_bridge();
        // This is the existing tokenless close model. It checks policy progress,
        // without claiming a successful OS Allow or fabricating a held-close key.
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            window,
            Some(egui::ViewportId::ROOT),
            Some(root),
            WindowEvent::CloseRequested,
        )));
        assert!(
            state
                .coordinator
                .reduce_callback_prefix(&context, &mut NativeInputBudget::new(&clock))
                .unwrap()
                .is_some()
        );
        let close_report = commit_native_prefix(&mut state);
        state
            .coordinator
            .settle_close_control_report(&close_report)
            .unwrap();
        let close = bridge.with_close_control(|control| {
            let mut requests = control.unsubmitted_requests();
            let close = requests.next().unwrap();
            assert!(requests.next().is_none());
            close
        });
        assert_eq!(close.binding(), root);

        let mut ordinal = 1_u64;
        let mut enqueue = |count: usize, timed: bool| {
            for _ in 0..count {
                if timed {
                    ticks.fetch_add(1, Ordering::SeqCst);
                }
                ordinal += 1;
                let captured = bridge.captured_through_for_test();
                bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                    ordinal,
                    window,
                    Some(egui::ViewportId::ROOT),
                    Some(root),
                    WindowEvent::CursorMoved {
                        device_id: DeviceId::dummy(),
                        position: PhysicalPosition::new(100.0, 100.0),
                        facts: PointerEventFacts::default(),
                    },
                )));
                assert_eq!(bridge.captured_through_for_test(), captured + 1);
            }
        };
        // Idle coalescing is permitted here, but every discarded raw callback
        // still owes one shared record credit. This is not an active-release model.
        let controlled_started_at = clock.now();
        enqueue(INITIAL_BACKLOG, false);
        assert_eq!(bridge.input_backlog_for_test().0, INITIAL_BACKLOG);

        let mut exhausted_turns = 0;
        let mut close_progress_turn = None;
        let mut create_progress_turn = None;
        let mut child = None;
        for turn in 0..TURNS {
            // Refill even after either owner progresses. A passing drain must
            // not rely on stopping ingress before service can run either owner.
            enqueue(REFILL_PER_TURN, true);
            let accepted = bridge.captured_through_for_test();
            let mut budget = NativeInputBudget::new(&clock);
            let ready = state
                .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                .unwrap();
            assert_eq!(bridge.captured_through_for_test(), accepted);
            assert!(!matches!(
                bridge.admission_dispatch(),
                NativeDispatch::Cancel { .. }
            ));
            let resident = state.owned_input_capacity_bytes().unwrap();
            let reserved = state.input_storage_reserved_bytes().unwrap();
            assert!(resident <= reserved && reserved <= storage.byte_limit());
            if turn < 2 {
                assert!(!ready);
                assert!(budget.is_exhausted());
                assert_eq!(budget.remaining_records(), 0);
                assert!(bridge.input_backlog_for_test().0 > 0);
            }
            if budget.is_exhausted() {
                exhausted_turns += 1;
                let before = (
                    bridge.input_backlog_for_test(),
                    state.coordinator.session().native_input_recorded_count(),
                    state.coordinator.session().version(),
                    state.coordinator.deferred_viewport_specs(),
                );
                assert!(
                    !state
                        .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                        .unwrap()
                );
                assert_eq!(
                    (
                        bridge.input_backlog_for_test(),
                        state.coordinator.session().native_input_recorded_count(),
                        state.coordinator.session().version(),
                        state.coordinator.deferred_viewport_specs(),
                    ),
                    before,
                    "the same exhausted turn cannot claim a successor or publish an empty frame"
                );
            }
            let (policy_unsubmitted, decisions, policy_runnable) =
                bridge.with_close_control(|control| {
                    assert_eq!(control.request_for_binding(root), Some(close));
                    assert!(control.held_request(close).is_none());
                    (
                        control
                            .unsubmitted_requests()
                            .any(|request| request == close),
                        control.decision_count(),
                        control.has_runnable_policy_work(),
                    )
                });
            // Surface-close approval does not remove content before exact OS
            // destruction. This tokenless model must only drain policy work;
            // its Allow dispatch fails explicitly and preserves the source.
            assert!(state.coordinator.session().view().item(FIRST).is_some());
            if !policy_runnable {
                close_progress_turn.get_or_insert(turn);
                assert!(!policy_unsubmitted);
                assert_eq!(decisions, 0);
            }
            let specs = state.coordinator.deferred_viewport_specs();
            assert!(specs.len() <= 1);
            if let Some(spec) = specs.first() {
                assert_eq!(spec.binding().surface(), OTHER_SURFACE);
                assert_eq!(spec.role(), NativeSurfaceRole::Child);
                assert_eq!(*child.get_or_insert(spec.binding()), spec.binding());
                create_progress_turn.get_or_insert(turn);
            }
            assert!(state.shutdown.is_none());
        }
        assert!(exhausted_turns >= 2);
        assert!(close_progress_turn.is_some_and(|turn| turn < TURNS - 1));
        assert!(create_progress_turn.is_some_and(|turn| turn < TURNS - 1));
        assert_eq!(state.close_progress().requests_retained(), 1);
        assert_eq!(bridge.input_backlog_for_test(), (0, None));
        assert!(!state.coordinator.session().has_pending_native_input());
        assert!(!bridge.callback_boundary_pending());
        let version = state.coordinator.session().version();
        let recorded = state.coordinator.session().native_input_recorded_count();
        for _ in 0..2 {
            // The emitted create is now awaiting an external OS fact. Pending
            // ownership alone must not cause an empty-frame or service-wake loop.
            assert_eq!(
                state.service_native_events(&context).unwrap(),
                NativeHostWake::Wait
            );
            assert_eq!(state.coordinator.session().version(), version);
            assert_eq!(
                state.coordinator.session().native_input_recorded_count(),
                recorded
            );
            assert_eq!(state.coordinator.deferred_viewport_specs().len(), 1);
        }
        // This is controlled schedule time with dispatch time frozen. It does
        // not qualify elapsed-time budget expiry or active-release latency.
        let controlled_elapsed = clock.now() - controlled_started_at;
        assert!(
            controlled_elapsed <= Duration::from_millis(100),
            "the frozen-per-dispatch record-budget profile took {controlled_elapsed:?}"
        );
        eprintln!(
            "service-refill: exhausted_turns={exhausted_turns} close_turn={close_progress_turn:?} create_turn={create_progress_turn:?} controlled_elapsed={controlled_elapsed:?} frozen_per_dispatch=true"
        );
    }

    #[test]
    fn shutdown_or_quarantined_service_does_not_start_a_queued_child_with_fresh_facts() {
        for fatal in [false, true] {
            let mut state = queued_child_restore();
            assert!(
                state
                    .coordinator
                    .try_report_retirement_quiescence()
                    .unwrap()
            );
            record_fresh_root_facts(&mut state);
            let mut snapshot = commit_native_prefix(&mut state);
            assert!(snapshot.take_native_effects().is_empty());
            if fatal {
                state.stop_current_output(
                    NativeHostProtocolError::RootSurfaceUnavailable(SURFACE).into(),
                );
            } else {
                state.coordinator.dispatch_bridge().enter_quarantine();
            }
            let context = egui::Context::default();
            let before = state.coordinator.session().version();
            let clock = NativeMonotonicClock::new(|| std::time::Duration::ZERO);
            let mut budget = NativeInputBudget::new(&clock);
            assert!(
                state
                    .service_pending_prefixes(&context, None, SURFACE, &mut budget)
                    .unwrap()
            );
            if fatal {
                assert_eq!(
                    state.service_native_events(&context).unwrap(),
                    NativeHostWake::RepaintRootAfterCurrent
                );
            }
            assert_eq!(budget.remaining_records(), 256);
            assert_eq!(state.coordinator.session().version(), before);
            assert!(state.coordinator.deferred_viewport_specs().is_empty());
            assert!(state.coordinator.session().has_pending_native_create());
            assert!(!state.coordinator.dispatch_bridge().accepts_new_work());
        }
    }

    #[test]
    fn locked_restore_rejects_without_occupying_slot_and_unlock_allows_restore_and_save() {
        let mut state = persistent_state();
        let panes = state.panes.0.clone();
        let saved = state.save_document_json().unwrap();
        state.request_set_arrangement_locked(true).unwrap();
        commit_queued(&mut state, None);
        assert!(state.arrangement_locked());
        assert_queue_error(
            state
                .request_document_restore_json(&saved, |_, _| {
                    panic!("the unconsumed result must take precedence over arrangement lock")
                })
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::ArrangementLockChanged { locked: true, .. }
        ));

        let before = state.coordinator.session().version();
        let generation = state.coordinator.session().next_document_generation();
        let error: NativeLayoutRequestError = state
            .request_document_restore_json(&saved, recognize)
            .unwrap_err();
        assert_eq!(error.kind(), NativeRuntimeErrorKind::OperationConflict);
        assert!(error.source().is_none());
        assert!(!state.application_actions.is_occupied());
        assert!(state.take_action_status().is_none());
        assert!(state.shutdown.is_none());
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(
            state.coordinator.session().next_document_generation(),
            generation
        );
        assert_eq!(state.panes.0, panes);

        state.request_set_arrangement_locked(false).unwrap();
        assert_queue_error(
            state
                .request_document_restore_json(&saved, |_, _| {
                    panic!("the queued unlock must take precedence over arrangement lock")
                })
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );
        commit_queued(&mut state, None);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::ArrangementLockChanged { locked: false, .. }
        ));
        assert!(!state.arrangement_locked());

        state
            .request_document_restore_json(&saved, recognize)
            .unwrap();
        let report = commit_queued(&mut state, None);
        assert_eq!(
            take(&mut state),
            NativeActionStatus::DocumentRestored {
                previous: report.before(),
                current: report.after(),
            }
        );
        let saved_again = state.save_document_json().unwrap();
        let restored =
            DockspaceSession::from_document_json(&saved_again, DockPolicy::default(), recognize)
                .unwrap();
        assert_eq!(restored.document_id(), Some(DOCUMENT));
        assert_eq!(restored.external_key_for_item(FIRST), Some("pane:first"));
        assert_eq!(restored.external_key_for_item(SECOND), Some("pane:second"));
        assert!(restored.view().item(FIRST).unwrap().is_selected());
        assert!(!state.application_actions.is_occupied());
        assert!(state.shutdown.is_none());
        assert_eq!(state.panes.0, panes);

        state.request_set_arrangement_locked(true).unwrap();
        commit_queued(&mut state, None);
        assert!(state.arrangement_locked());
        state.stop_current_output(NativeHostProtocolError::RootSurfaceUnavailable(SURFACE).into());
        assert_queue_error(
            state
                .request_document_restore_json(&saved_again, |_, _| {
                    panic!("shutdown must take precedence over arrangement lock and the unconsumed result")
                })
                .unwrap_err(),
            NativeActionRequestError::Stopped,
        );
    }

    #[test]
    fn capture_and_external_restore_publish_known_and_new_keys_without_replacing_panes() {
        let mut state = persistent_state();
        let panes = state.panes.0.clone();
        let captured = state.save_document_json().unwrap();
        let generation = state.coordinator.session().next_document_generation();
        let mut external =
            DockspaceSession::from_document_json(&captured, DockPolicy::default(), recognize)
                .unwrap();
        assert_eq!(external.ensure_external_item("pane:new").unwrap(), THIRD);
        let reset = external
            .prepare_layout_reset(layout(PRESET_ROOT, &[FIRST, THIRD], THIRD))
            .unwrap();
        let mut frame = external.begin_layout_reset_frame(&reset).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        let saved = external.save_document_json().unwrap();

        let recognized = RefCell::new(Vec::new());
        state
            .request_document_restore_json(&saved, |document, key| {
                recognized.borrow_mut().push(key.to_owned());
                recognize(document, key)
            })
            .unwrap();
        assert_eq!(
            state
                .coordinator
                .session()
                .item_id_for_external_key("pane:new"),
            None
        );
        assert_queue_error(
            state.save_document_json().unwrap_err(),
            NativeActionRequestError::Busy,
        );
        assert_eq!(
            state.coordinator.session().next_document_generation(),
            generation
        );
        assert_queue_error(
            state
                .request_layout_reset(layout(PRESET_ROOT, &[FIRST], FIRST))
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );

        let report = commit_queued(&mut state, Some(SECOND));
        assert_queue_error(
            state.save_document_json().unwrap_err(),
            NativeActionRequestError::Busy,
        );
        let NativeActionStatus::DocumentRestored { previous, current } = take(&mut state) else {
            panic!(
                "the exact document terminal must settle despite native registration and local selection"
            );
        };
        assert_ne!(previous, report.before());
        assert_eq!(current, report.after());
        assert_eq!(
            report.inputs().last(),
            Some(&HostInputOutcome::DocumentRestored { previous, current })
        );
        assert!(
            report
                .inputs()
                .iter()
                .any(|input| matches!(input, HostInputOutcome::NativeSurfaceRegistered { .. }))
        );
        assert!(
            !report
                .inputs()
                .iter()
                .any(|input| matches!(input, HostInputOutcome::ProductActionApplied(_)))
        );
        let mut recognized = recognized.into_inner();
        recognized.sort();
        assert_eq!(recognized, ["pane:first", "pane:new", "pane:second"]);
        assert_eq!(
            state
                .coordinator
                .session()
                .item_id_for_external_key("pane:first"),
            Some(FIRST)
        );
        assert_eq!(
            state
                .coordinator
                .session()
                .item_id_for_external_key("pane:new"),
            Some(THIRD)
        );
        assert!(
            state
                .coordinator
                .session()
                .view()
                .item(THIRD)
                .unwrap()
                .is_selected()
        );
        assert!(state.coordinator.session().view().item(SECOND).is_none());
        assert_eq!(state.panes.0, panes);
        assert!(state.shutdown.is_none());

        let saved_again = state.save_document_json().unwrap();
        let restored =
            DockspaceSession::from_document_json(&saved_again, DockPolicy::default(), recognize)
                .unwrap();
        assert_eq!(restored.document_id(), Some(DOCUMENT));
        assert_eq!(restored.external_key_for_item(SECOND), Some("pane:second"));
        assert_eq!(restored.external_key_for_item(THIRD), Some("pane:new"));
        assert!(restored.view().item(THIRD).unwrap().is_selected());
    }

    #[test]
    fn malformed_restore_keeps_typed_source_and_busy_rejects_before_decoding() {
        let mut state = persistent_state();
        let panes = state.panes.0.clone();
        let before = state.coordinator.session().version();
        let generation = state.coordinator.session().next_document_generation();
        let error = state
            .request_document_restore_json(b"{", |_, _| {
                panic!("malformed JSON must fail before identity recognition");
            })
            .unwrap_err();
        assert_eq!(error.kind(), NativeRuntimeErrorKind::Persistence);
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<DockspacePersistenceError>()
                .unwrap()
                .kind(),
            DockspacePersistenceErrorKind::InvalidDocument
        );
        assert!(!state.application_actions.is_occupied());
        assert!(state.take_action_status().is_none());
        assert!(state.shutdown.is_none());
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(
            state.coordinator.session().next_document_generation(),
            generation
        );

        state.request_select_item(SECOND).unwrap();
        assert_queue_error(
            state
                .request_document_restore_json(b"{", |_, _| {
                    panic!("the occupied slot must be checked first")
                })
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );
        assert_queue_error(
            state.save_document_json().unwrap_err(),
            NativeActionRequestError::Busy,
        );
        commit_queued(&mut state, None);
        assert_queue_error(
            state
                .request_document_restore_json(b"{", |_, _| {
                    panic!("the unconsumed result owns the slot")
                })
                .unwrap_err(),
            NativeActionRequestError::Busy,
        );
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::Dock(egui_dockspace::DockspaceActionStatus::Applied(
                dockspace::model::DockspaceActionOutcome::Selected { item: SECOND, .. }
            ))
        ));
        assert_eq!(state.panes.0, panes);
        assert!(state.shutdown.is_none());
        state.save_document_json().unwrap();
    }
}
