//! A shared Root replaces logical areas without synthesizing more window routes.

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::runtime::{
    NativeCloseState, NativeSurfaceBinding, NativeSurfacePlacement, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
};
use std::sync::Arc;

use super::super::super::{NativeRootPlacementUpdate, apply_root_placements};
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
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
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

fn request_placements(
    state: &mut NativeRuntimeState<Panes>,
    placements: &[NativeSurfacePlacement],
) {
    state
        .root_placements
        .reserve(
            &state.coordinator.dispatch_bridge(),
            size_of_val(placements),
        )
        .unwrap();
    state.root_placements.request = Some(Arc::from(placements));
    state.root_placements.explicit = true;
    state
        .root_placements
        .refresh_storage(&state.coordinator.dispatch_bridge());
}

fn assert_ordinary_root_roster(
    state: &mut NativeRuntimeState<Panes>,
    expected: &[SurfaceId],
) -> Arc<[NativeSurfacePlacement]> {
    // This is the ordinary entry's selection path, including runs without input.
    let placements = state
        .root_placements
        .next(
            &state.coordinator,
            state.application_actions.has_queued_replacement(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.surface())
            .collect::<std::collections::BTreeSet<_>>(),
        expected.iter().copied().collect()
    );
    assert!(crate::surface_driver::RootPlacements::matches_committed(
        &state.coordinator,
        &placements
    ));
    let window = current_roster(state)[0].window();
    let frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(super::super::super::root_placements_match_frame(
        &frame.frame,
        window,
        &placements,
    ));
    drop(frame);
    placements
}

#[test]
fn ordinary_root_entry_uses_the_committed_roster_after_shared_reset_shrinks_it() {
    let mut state = shared_state();
    let old = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
    request_placements(&mut state, &old);
    let old_bytes = state.root_placements.retained_bytes();
    drop(old);

    state
        .request_layout_reset(layout(PRESET_ROOT, &[FIRST], FIRST))
        .unwrap();
    commit_queued(&mut state, None);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::LayoutReset { .. }
    ));
    assert!(state.root_placements.request.is_none());
    assert!(state.root_placements.cache.is_none());
    assert_eq!(state.root_placements.retained_bytes(), 0);

    let current = assert_ordinary_root_roster(&mut state, &[SURFACE]);
    let reused = assert_ordinary_root_roster(&mut state, &[SURFACE]);
    assert!(Arc::ptr_eq(&current, &reused));
    assert!(state.root_placements.retained_bytes() < old_bytes);
    assert!(state.shutdown.is_none());
}

#[test]
fn ordinary_root_entry_uses_the_committed_surface_after_representative_rebinding() {
    let mut state = shared_state();
    let old = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
    request_placements(&mut state, &old);
    drop(old);
    let incoming = SurfaceId::new(3);
    state
        .request_layout_reset_rebinding_root(
            DockspaceLayout::new([
                surface(incoming, PRESET_ROOT, &[FIRST], FIRST),
                surface(OTHER_SURFACE, RootId::new(12), &[SECOND], SECOND),
            ])
            .unwrap(),
            incoming,
        )
        .unwrap();
    commit_queued(&mut state, None);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::LayoutReset { .. }
    ));
    assert_eq!(state.root_surface, incoming);
    assert!(state.root_placements.request.is_none());
    assert_ordinary_root_roster(&mut state, &[incoming, OTHER_SURFACE]);
    assert!(state.shutdown.is_none());
}

#[test]
fn ordinary_root_entry_drops_a_pending_declaration_after_docking_vacates_its_sibling() {
    let initial = DockspaceLayout::new([
        surface(SURFACE, ROOT, &[FIRST], FIRST),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(RootId::new(11), DockspaceNode::tabs([SECOND])),
        ),
    ])
    .unwrap();
    let mut state = shared_state_from_session(
        DockspaceSession::from_layout(initial, DockPolicy::default()).unwrap(),
    );
    let old = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
    request_placements(&mut state, &old);
    drop(old);
    state
        .request_command(NativeCommand::DockItem {
            item: SECOND,
            placement: dockspace::model::DockPlacement::Center(
                dockspace::model::DockAnchor::Central(ROOT),
            ),
        })
        .unwrap();
    let report = commit_queued(&mut state, None);
    assert_eq!(report.native_placement_detachments().len(), 1);
    assert!(matches!(take(&mut state), NativeActionStatus::Dock(_)));
    assert!(state.view().surface(OTHER_SURFACE).is_none());
    assert!(state.root_placements.request.is_none());
    assert_eq!(state.root_placements.retained_bytes(), 0);
    assert_ordinary_root_roster(&mut state, &[SURFACE]);
    assert!(state.shutdown.is_none());
}

#[test]
fn equivalent_explicit_root_declaration_survives_native_resize_on_the_ordinary_entry() {
    let mut state = state_from_session(
        DockspaceSession::from_layout(layout(ROOT, &[FIRST], FIRST), DockPolicy::default())
            .unwrap(),
    );
    let registration = commit_prefix(&mut state);
    let root = registration.native_bindings()[0];
    state
        .coordinator
        .bind_viewport(
            egui::ViewportId::ROOT,
            winit::window::WindowId::from(31),
            root,
        )
        .unwrap();
    let facts = |width, height| {
        let scale = ScaleFactor::new(1.0).unwrap();
        NativeWindowFacts::live()
            .with_content_bounds(PhysicalRect::new(0.0, 0.0, width, height).unwrap())
            .with_outer_bounds(PhysicalRect::new(0.0, 0.0, width, height).unwrap())
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible)
            .with_close(NativeCloseState::Clear, None)
    };
    let capabilities =
        || crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows);
    state
        .coordinator
        .report_snapshot(
            [(root, facts(700.0, 480.0))],
            NativeWorkAreaRoster::Unknown,
            capabilities(),
        )
        .unwrap();
    commit_prefix(&mut state);
    let implicit = state
        .coordinator
        .session()
        .native_surface_placement(root)
        .unwrap();
    let bridge = state.coordinator.dispatch_bridge();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(matches!(
        apply_root_placements(&mut frame.frame, &bridge, root.window(), &[implicit], true).unwrap(),
        NativeRootPlacementUpdate::Unchanged
    ));
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
    state.root_placements.explicit = true;

    state
        .coordinator
        .report_snapshot(
            [(root, facts(900.0, 600.0))],
            NativeWorkAreaRoster::Unknown,
            capabilities(),
        )
        .unwrap();
    commit_prefix(&mut state);
    assert_eq!(
        state.coordinator.session().native_surface_placement(root),
        Some(implicit)
    );
    assert_eq!(
        &*assert_ordinary_root_roster(&mut state, &[SURFACE]),
        &[implicit]
    );
    assert!(state.shutdown.is_none());
}

#[test]
fn tight_root_placement_storage_shares_cached_backing_and_releases_it_before_new_snapshots() {
    let mut state = shared_state();
    let placements = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
    let bytes = size_of_val(&*placements);
    state.root_placements.request = Some(Arc::clone(&placements));
    state
        .root_placements
        .refresh_storage(&state.coordinator.dispatch_bridge());
    drop(placements);
    assert_eq!(state.root_placements.retained_bytes(), bytes);

    let bridge = state.coordinator.dispatch_bridge();
    assert!(bridge.configure_input_storage_allowance(usize::MAX, 0));
    let reserved = bridge.selected_storage_reserved_bytes().unwrap();
    assert!(bridge.configure_input_storage_allowance(reserved, 0));
    let reused = state
        .root_placements
        .next(
            &state.coordinator,
            state.application_actions.has_queued_replacement(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(bridge.selected_storage_reserved_bytes(), Some(reserved));
    assert!(
        state
            .root_placements
            .reserve(&state.coordinator.dispatch_bridge(), bytes)
            .unwrap_err()
            .is_input_capacity()
    );
    drop(reused);

    state.root_placements.request = None;
    state.root_placements.cache = None;
    state
        .root_placements
        .refresh_storage(&state.coordinator.dispatch_bridge());
    assert_eq!(state.root_placements.retained_bytes(), 0);
    assert_eq!(
        bridge.selected_storage_reserved_bytes(),
        Some(reserved - bytes)
    );
    assert!(
        state
            .root_placements
            .next(
                &state.coordinator,
                state.application_actions.has_queued_replacement()
            )
            .unwrap_err()
            .is_input_capacity()
    );
    state
        .root_placements
        .refresh_storage(&state.coordinator.dispatch_bridge());
    assert_eq!(
        bridge.selected_storage_reserved_bytes(),
        Some(reserved - bytes)
    );

    // Vec-to-Arc conversion pays its temporary overlap before allocating.
    assert!(bridge.configure_input_storage_allowance(reserved + bytes, 0));
    state
        .root_placements
        .next(
            &state.coordinator,
            state.application_actions.has_queued_replacement(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(state.root_placements.retained_bytes(), bytes);
    assert_eq!(bridge.selected_storage_reserved_bytes(), Some(reserved));
}

#[test]
fn queued_replacement_uses_committed_placements_and_keeps_the_declaration_out_of_its_frame() {
    let mut state = shared_state();
    let committed = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
    let mut resized = committed.to_vec();
    resized[0] = NativeSurfacePlacement::new(
        SURFACE,
        LogicalSize::new(300.0, 440.0).unwrap(),
        PhysicalPoint::new(5.0, 20.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
        PhysicalRect::new(5.0, 20.0, 300.0, 440.0).unwrap(),
    );
    request_placements(&mut state, &resized);
    state.request_layout_reset(shared_layout()).unwrap();
    let selected = state
        .root_placements
        .next(
            &state.coordinator,
            state.application_actions.has_queued_replacement(),
        )
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&selected, &committed));
    assert_ne!(&*selected, &*resized);
    let window = current_roster(&state)[0].window();
    let bridge = state.coordinator.dispatch_bridge();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(matches!(
        apply_root_placements(&mut frame.frame, &bridge, window, &selected, false).unwrap(),
        NativeRootPlacementUpdate::Unchanged
    ));
    let published = frame.frame.version();
    let action = state
        .application_actions
        .take_after_placements(false)
        .unwrap();
    assert!(state.application_actions.has_pending_replacement());
    let submitted = action
        .submit(&mut frame.frame, published)
        .unwrap()
        .finish(&mut frame.frame)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::LayoutReset { .. }
    ));
    assert!(state.root_placements.request.is_none());
    assert_eq!(
        &*assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]),
        &*committed
    );
    assert!(state.shutdown.is_none());
}

#[test]
fn replacement_queued_after_frozen_geometry_waits_and_keeps_explicit_predecessors_exact() {
    for explicit in [false, true] {
        let mut state = shared_state();
        let committed = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
        let mut resized = committed.to_vec();
        resized[0] = NativeSurfacePlacement::new(
            SURFACE,
            LogicalSize::new(300.0, 440.0).unwrap(),
            PhysicalPoint::new(5.0, 20.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(5.0, 20.0, 300.0, 440.0).unwrap(),
        );
        request_placements(&mut state, &resized);
        let frozen = state
            .root_placements
            .next(
                &state.coordinator,
                state.application_actions.has_queued_replacement(),
            )
            .unwrap()
            .unwrap();
        let predecessor = current_roster(&state)
            .into_iter()
            .find(|binding| binding.surface() == SURFACE)
            .unwrap();
        let incoming = SurfaceId::new(3);
        if explicit {
            state
                .request_layout_reset_rebinding_root(
                    DockspaceLayout::new([
                        surface(incoming, PRESET_ROOT, &[FIRST], FIRST),
                        surface(OTHER_SURFACE, RootId::new(12), &[SECOND], SECOND),
                    ])
                    .unwrap(),
                    incoming,
                )
                .unwrap();
        } else {
            state.request_layout_reset(shared_layout()).unwrap();
        }
        let bridge = state.coordinator.dispatch_bridge();
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(matches!(
            apply_root_placements(
                &mut frame.frame,
                &bridge,
                predecessor.window(),
                &frozen,
                true
            )
            .unwrap(),
            NativeRootPlacementUpdate::Applied
        ));
        assert!(
            state
                .application_actions
                .take_after_placements(true)
                .is_none()
        );
        assert!(state.application_actions.has_queued_replacement());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        state.coordinator.settle_host_frame_report(&report).unwrap();
        state
            .root_placements
            .commit(&state.coordinator, state.root_surface, &report);
        assert_ne!(
            current_roster(&state)
                .into_iter()
                .find(|binding| binding.surface() == SURFACE),
            Some(predecessor)
        );
        assert_eq!(state.root_surface, SURFACE);
        assert_eq!(&*frozen, &*resized);
        assert!(crate::surface_driver::RootPlacements::matches_committed(
            &state.coordinator,
            &frozen
        ));

        commit_queued(&mut state, None);
        let status = take(&mut state);
        if explicit {
            assert!(matches!(
                status,
                NativeActionStatus::RootContinuationRejected {
                    predecessor: rejected,
                    incoming_root,
                    ..
                } if rejected == predecessor && incoming_root == incoming
            ));
            assert!(state.view().surface(incoming).is_none());
        } else {
            assert!(matches!(status, NativeActionStatus::LayoutReset { .. }));
        }
        assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
        assert!(state.shutdown.is_none());
    }
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
        let old = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
        request_placements(&mut state, &old);
        drop(old);
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
        assert!(state.root_placements.request.is_none());
        let current_root = state.root_surface;
        assert_ordinary_root_roster(&mut state, &[current_root, OTHER_SURFACE]);
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
    fn ordinary_root_entry_uses_committed_placements_after_restore_removes_a_shared_sibling() {
        let mut state = shared_state_from_session(persistent_session(shared_layout()));
        let old = assert_ordinary_root_roster(&mut state, &[SURFACE, OTHER_SURFACE]);
        request_placements(&mut state, &old);
        drop(old);
        let saved = persistent_session(layout(PRESET_ROOT, &[FIRST], FIRST))
            .save_document_json()
            .unwrap();
        state
            .request_document_restore_json(&saved, recognize)
            .unwrap();
        commit_queued(&mut state, None);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::DocumentRestored { .. }
        ));
        assert!(state.root_placements.request.is_none());
        let current_root = state.root_surface;
        assert_ordinary_root_roster(&mut state, &[current_root]);
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
