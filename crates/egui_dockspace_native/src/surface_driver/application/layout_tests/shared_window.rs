//! A shared Root replaces logical areas without synthesizing more window routes.

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::runtime::{
    NativeCloseState, NativeSurfaceBinding, NativeSurfacePlacement, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
};

use super::*;

fn commit_prefix(state: &mut NativeRuntimeState<Panes>) -> HostFrameReport {
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    report
}

fn current_roster(state: &NativeRuntimeState<Panes>) -> Vec<NativeSurfaceBinding> {
    let window = state
        .coordinator
        .session()
        .native_window_bindings()
        .next()
        .unwrap();
    state
        .coordinator
        .session()
        .native_surface_bindings_for_window(window)
        .collect()
}

fn shared_state() -> NativeRuntimeState<Panes> {
    shared_state_from_session(
        DockspaceSession::from_layout(shared_layout(), DockPolicy::default()).unwrap(),
    )
}

fn shared_layout() -> DockspaceLayout {
    DockspaceLayout::new([
        surface(SURFACE, ROOT, &[FIRST], FIRST),
        surface(OTHER_SURFACE, RootId::new(11), &[SECOND], SECOND),
    ])
    .unwrap()
}

fn shared_state_from_session(session: DockspaceSession) -> NativeRuntimeState<Panes> {
    let mut state = state_from_session(session);
    let registration = commit_prefix(&mut state);
    let original = registration.native_bindings()[0];
    state
        .coordinator
        .bind_viewport(
            egui::ViewportId::ROOT,
            winit::window::WindowId::from(31),
            original,
        )
        .unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    state
        .coordinator
        .report_snapshot(
            [(
                original,
                NativeWindowFacts::live()
                    .with_content_bounds(PhysicalRect::new(0.0, 0.0, 700.0, 480.0).unwrap())
                    .with_outer_bounds(PhysicalRect::new(0.0, 0.0, 700.0, 480.0).unwrap())
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible)
                    .with_close(NativeCloseState::Clear, None),
            )],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows),
        )
        .unwrap();
    commit_prefix(&mut state);
    let placements = [SURFACE, OTHER_SURFACE].map(|surface| {
        let x = if surface == SURFACE { 0.0 } else { 360.0 };
        NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(320.0, 440.0).unwrap(),
            PhysicalPoint::new(x, 20.0).unwrap(),
            scale,
            PhysicalRect::new(x, 20.0, 320.0, 440.0).unwrap(),
        )
    });
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    assert_eq!(current_roster(&state).len(), 2);
    state
}

#[test]
fn reset_replaces_the_whole_shared_root_roster_only_after_commit() {
    let mut state = shared_state();
    let candidate = DockspaceLayout::new([
        surface(SURFACE, PRESET_ROOT, &[FIRST], FIRST),
        surface(OTHER_SURFACE, RootId::new(12), &[SECOND, THIRD], THIRD),
    ])
    .unwrap();
    let before = state.coordinator.session().version();
    let predecessors = current_roster(&state);

    state.request_layout_reset(candidate.clone()).unwrap();
    let reset = state
        .coordinator
        .session()
        .prepare_layout_reset(candidate)
        .unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    let proposals = frame
        .frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert_eq!(proposals.len(), 2);
    assert!(
        proposals
            .iter()
            .all(|continuation| predecessors.contains(&continuation.predecessor()))
    );
    drop(frame);
    assert_eq!(state.coordinator.session().version(), before);
    assert_eq!(current_roster(&state), predecessors);
    assert!(state.coordinator.session().view().item(THIRD).is_none());

    let report = commit_queued(&mut state, None);
    assert_eq!(report.native_root_continuations().len(), 2);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::LayoutReset { .. }
    ));
    let successors = current_roster(&state);
    assert_eq!(successors.len(), 2);
    assert_eq!(successors[0].window(), successors[1].window());
    assert!(
        successors
            .iter()
            .all(|binding| !predecessors.contains(binding))
    );
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .item(THIRD)
            .unwrap()
            .surface(),
        OTHER_SURFACE
    );
    assert!(
        state
            .coordinator
            .dispatch_bridge()
            .with_viewports(|routes| {
                routes.window(egui::ViewportId::ROOT) == Some(winit::window::WindowId::from(31))
            })
    );
    assert!(state.shutdown.is_none());
}

#[test]
fn shared_replacement_does_not_treat_an_unplaced_surface_as_another_root() {
    let mut state = shared_state();
    let unknown = SurfaceId::new(3);
    let candidate = DockspaceLayout::new([
        surface(SURFACE, ROOT, &[FIRST], FIRST),
        surface(OTHER_SURFACE, RootId::new(11), &[SECOND], SECOND),
        surface(unknown, RootId::new(12), &[THIRD], THIRD),
    ])
    .unwrap();
    let before = state.coordinator.session().version();
    assert_eq!(
        state.request_layout_reset(candidate).unwrap_err().kind(),
        NativeRuntimeErrorKind::Unsupported
    );
    assert_eq!(state.coordinator.session().version(), before);
    assert!(!state.application_actions.is_occupied());
}

#[test]
fn representative_root_rebinding_continues_its_shared_sibling_in_the_same_batch() {
    let mut state = shared_state();
    let predecessors = current_roster(&state);
    let incoming = SurfaceId::new(3);
    let candidate = DockspaceLayout::new([
        surface(incoming, PRESET_ROOT, &[FIRST], FIRST),
        surface(OTHER_SURFACE, RootId::new(12), &[SECOND], SECOND),
    ])
    .unwrap();
    state
        .request_layout_reset_rebinding_root(candidate, incoming)
        .unwrap();
    let report = commit_queued(&mut state, None);
    assert_eq!(state.root_surface, incoming);
    assert_eq!(report.native_root_continuations().len(), 2);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::LayoutReset { .. }
    ));
    let successors = current_roster(&state);
    assert_eq!(successors.len(), 2);
    assert!(
        successors
            .iter()
            .all(|binding| !predecessors.contains(binding))
    );
    assert_eq!(
        successors
            .iter()
            .map(|binding| binding.surface())
            .collect::<std::collections::BTreeSet<_>>(),
        [incoming, OTHER_SURFACE].into_iter().collect()
    );
    assert!(state.shutdown.is_none());
}

#[cfg(feature = "serde")]
mod document {
    use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};

    use super::*;

    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x73; 16]);

    fn persistent_session(layout: DockspaceLayout) -> DockspaceSession {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        for (key, item) in [
            ("pane:first", FIRST),
            ("pane:second", SECOND),
            ("pane:third", THIRD),
        ] {
            assert_eq!(bootstrap.ensure_item(key).unwrap(), item);
        }
        DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap()
    }

    fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
        document == DOCUMENT && matches!(key, "pane:first" | "pane:second" | "pane:third")
    }

    #[test]
    fn restore_prepares_and_publishes_every_shared_root_together_after_a_dropped_attempt() {
        let mut state = shared_state_from_session(persistent_session(shared_layout()));
        let saved = state.save_document_json().unwrap();
        let before = state.coordinator.session().version();
        let predecessors = current_roster(&state);

        state
            .request_document_restore_json(&saved, recognize)
            .unwrap();
        let restore = state
            .coordinator
            .session()
            .prepare_document_restore_json(&saved, recognize)
            .unwrap();
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .submit_native_document_restore_with_roots(
                &restore,
                predecessors
                    .iter()
                    .map(|binding| (*binding, binding.surface())),
            )
            .unwrap();
        assert_eq!(frame.frame.proposed_native_root_continuations().count(), 2);
        drop(frame);
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(current_roster(&state), predecessors);

        let mut report = commit_queued(&mut state, None);
        assert_eq!(report.native_root_continuations().len(), 2);
        assert!(report.take_native_effects().is_empty());
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        let successors = current_roster(&state);
        assert_eq!(successors.len(), 2);
        assert_eq!(successors[0].window(), successors[1].window());
        assert!(
            successors
                .iter()
                .all(|binding| !predecessors.contains(binding))
        );
        for binding in successors {
            assert!(
                state
                    .coordinator
                    .session()
                    .presented_surface(binding.surface())
                    .is_none()
            );
        }
        assert!(state.shutdown.is_none());
    }

    #[test]
    fn restore_rebinding_the_representative_keeps_the_shared_sibling_on_its_real_window() {
        let mut state = shared_state_from_session(persistent_session(shared_layout()));
        let incoming = SurfaceId::new(3);
        let candidate = DockspaceLayout::new([
            surface(incoming, PRESET_ROOT, &[FIRST], FIRST),
            surface(OTHER_SURFACE, RootId::new(12), &[SECOND, THIRD], THIRD),
        ])
        .unwrap();
        let saved = persistent_session(candidate).save_document_json().unwrap();
        let predecessors = current_roster(&state);
        let representative = predecessors
            .iter()
            .copied()
            .find(|binding| binding.surface() == SURFACE)
            .unwrap();
        state
            .request_document_restore_json_rebinding_root(&saved, incoming, recognize)
            .unwrap();
        let mut report = commit_queued(&mut state, None);
        assert_eq!(report.native_root_continuations().len(), 2);
        let continued = report
            .native_root_continuations()
            .iter()
            .find(|continuation| continuation.predecessor() == representative)
            .unwrap();
        assert_eq!(state.root_surface, continued.successor().surface());
        assert_ne!(state.root_surface, SURFACE);
        assert_eq!(
            state
                .coordinator
                .session()
                .view()
                .item(FIRST)
                .unwrap()
                .surface(),
            state.root_surface
        );
        assert_eq!(
            state
                .coordinator
                .session()
                .view()
                .item(THIRD)
                .unwrap()
                .surface(),
            current_roster(&state)
                .iter()
                .find(|binding| binding.surface() != state.root_surface)
                .unwrap()
                .surface()
        );
        assert!(report.take_native_effects().is_empty());
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        assert!(
            state
                .coordinator
                .dispatch_bridge()
                .with_viewports(|routes| {
                    routes.window(egui::ViewportId::ROOT) == Some(winit::window::WindowId::from(31))
                })
        );
        assert!(state.shutdown.is_none());
    }

    #[test]
    fn restore_before_the_first_root_registration_keeps_the_singleton_submission_path() {
        let mut state = state_from_session(persistent_session(layout(ROOT, &[FIRST], FIRST)));
        let saved = persistent_session(layout(PRESET_ROOT, &[FIRST, SECOND], SECOND))
            .save_document_json()
            .unwrap();
        state
            .request_document_restore_json(&saved, recognize)
            .unwrap();
        assert!(
            state
                .coordinator
                .session()
                .native_window_bindings()
                .next()
                .is_none()
        );
        commit_queued(&mut state, None);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        assert!(
            state
                .coordinator
                .session()
                .view()
                .item(SECOND)
                .unwrap()
                .is_selected()
        );
        assert!(state.shutdown.is_none());
    }
}
