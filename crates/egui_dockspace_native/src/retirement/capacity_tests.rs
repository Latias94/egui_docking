use super::*;
use crate::mailbox::{HostRecord, NativeHostBridge, journal::HostIngressJournal};
use crate::viewport_callback::NativeViewportCreateFailureRecord;
use crate::viewport_map::{CreateAdmission, CreateTerminal, NativeViewportMap};
use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken,
    NativeCloseState, NativeEffectAcknowledgement, NativePointerRoster, NativeSurfaceCloseAction,
    NativeSurfacePlacement, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, SurfaceUnavailableReason,
};
use eframe::NativeViewportCreateFailureKind;

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn close_acknowledgement() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    NativeCloseEffectAcknowledgement,
) {
    let (mut session, binding) = super::tests::registered_session();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .unwrap();
    let request = commit(&mut session)
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                close_requests.first().copied()
            }
            _ => None,
        })
        .unwrap();
    session.cancel_native_surface_close(request).unwrap();
    let mut effects = commit(&mut session).take_native_effects().into_iter();
    let effect = effects.next().unwrap();
    assert!(effects.next().is_none());
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
        panic!("the real core cancellation supplies a close acknowledgement")
    };
    (session, binding, acknowledgement)
}

fn window_receipt(
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
) -> NativeWindowReceipt {
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, window, binding).unwrap();
    routes.take_window_receipt(viewport, binding).unwrap()
}

fn failed_create(
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
) -> (NativeCreateReceipt, u64) {
    let mut journal = HostIngressJournal::new(dockspace::runtime::NativeMonotonicClock::system());
    failed_create_in(&mut journal, viewport, binding)
}

fn failed_create_in(
    journal: &mut HostIngressJournal,
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
) -> (NativeCreateReceipt, u64) {
    let mut receipt = NativeCreateReceipt::new(None);
    let failure = NativeViewportCreateFailureKind::WindowUnavailable;
    assert_eq!(
        receipt.receive(None, CreateTerminal::Failed(failure), || {
            journal.reserve(HostRecord::ViewportCreateFailed(
                NativeViewportCreateFailureRecord::new(viewport, binding, failure),
            ))
        }),
        CreateAdmission::First,
    );
    let ordinal = receipt.ingress().unwrap().ordinal();
    assert!(receipt.transfer(None, CreateTerminal::Failed(failure)));
    (receipt, ordinal)
}

fn commit_failed_create(
    state: &mut NativeRetirementState,
    binding: NativeSurfaceBinding,
    ordinal: u64,
) {
    state.mark_create_failure_recorded(binding);
    state.commit_create_failures();
    assert!(!state.is_quiescence_candidate(binding));
    let ingress = state.take_create_ingress(ordinal).unwrap();
    assert_eq!(ingress.ordinal(), ordinal);
    assert!(state.take_create_ingress(ordinal).is_none());
    assert!(!state.is_quiescence_candidate(binding));
    state.commit_create_failures();
    assert!(state.is_quiescence_candidate(binding));
}

fn shared_layout() -> DockspaceLayout {
    DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap()
}

fn shared_placements() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    [NativeSurfaceBinding; 2],
) {
    let mut session =
        DockspaceSession::from_layout(shared_layout(), dockspace::policy::DockPolicy::default())
            .unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let original = commit(&mut session).native_bindings()[0];
    let scale = ScaleFactor::new(1.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                original.window(),
                NativeWindowFacts::live()
                    .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    let placements = [1, 2].map(|id| {
        let x = if id == 1 { 0.0 } else { 400.0 };
        NativeSurfacePlacement::new(
            SurfaceId::new(id),
            LogicalSize::new(400.0, 600.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            scale,
            PhysicalRect::new(x, 0.0, 400.0, 600.0).unwrap(),
        )
    });
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let bindings: [NativeSurfaceBinding; 2] = session
        .native_surface_bindings_for_window(original.window())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert_ne!(bindings[0], bindings[1]);
    assert_eq!(bindings[0].window(), bindings[1].window());
    (session, original, bindings)
}

fn shared_replacement<'a>(
    session: &'a mut DockspaceSession,
    bindings: &[NativeSurfaceBinding; 2],
) -> DockspaceHostFrame<'a> {
    let reset = session.prepare_layout_reset(shared_layout()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_native_layout_reset_with_roots(
            &reset,
            bindings.iter().map(|&binding| (binding, binding.surface())),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
}

fn shrinking_replacement(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    let reset = session
        .prepare_layout_reset(
            DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SurfaceId::new(1),
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::central_tabs([ItemId::new(1)]),
                ),
            )])
            .unwrap(),
        )
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
}

#[test]
fn shrinking_shared_root_prepays_and_drains_both_exact_predecessors() {
    let (mut session, original, bindings) = shared_placements();
    let bridge =
        NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1).unwrap();
    bridge.configure_lifecycle_storage(2, 1).unwrap();
    bridge.configure_window_resource_storage(2, 0).unwrap();
    bridge
        .bind_viewport_route(ViewportId::ROOT, WindowId::from(95), original)
        .unwrap();
    let version = session.version();
    drop(
        shrinking_replacement(&mut session)
            .prepare_commit()
            .unwrap(),
    );
    assert_eq!(session.version(), version);
    let (mut report, _) = bridge
        .commit_host_frame_with_staging(shrinking_replacement(&mut session), true, None)
        .unwrap();
    assert_eq!(report.native_root_continuations().len(), 1);
    assert_eq!(report.native_placement_detachments(), &bindings[1..]);
    assert!(report.take_native_effects().is_empty());
    assert_eq!(bridge.retirement_quiescence_candidates(), bindings);
    let successor = report.native_root_continuations()[0].successor();
    assert_eq!(
        bridge.with_viewports(|routes| routes.window(ViewportId::ROOT)),
        Some(WindowId::from(95))
    );
    assert_eq!(
        session
            .native_surface_bindings_for_window(successor.window())
            .count(),
        1
    );
    for binding in bindings.into_iter().rev() {
        session.report_native_binding_quiescence(binding).unwrap();
        commit(&mut session);
        session.prepare_native_pointer_capacity().unwrap();
        assert!(bridge.retirement_finish_quiescence(binding));
        assert!(!session.recognizes_native_binding(binding));
    }
    assert!(session.is_current_native_binding(successor));
}

#[test]
fn shrinking_shared_root_refuses_a_missing_detachment_slot_before_publication() {
    let (mut session, original, bindings) = shared_placements();
    let bridge =
        NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1).unwrap();
    bridge.configure_lifecycle_storage(1, 1).unwrap();
    bridge.configure_window_resource_storage(2, 0).unwrap();
    bridge
        .bind_viewport_route(ViewportId::ROOT, WindowId::from(96), original)
        .unwrap();
    let version = session.version();
    assert!(
        bridge
            .commit_host_frame_with_staging(shrinking_replacement(&mut session), true, None)
            .is_err()
    );
    assert_eq!(session.version(), version);
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(ViewportId::ROOT)),
        Some(original)
    );
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    assert!(
        bindings
            .into_iter()
            .all(|binding| session.is_current_native_binding(binding))
    );
}

fn shared_window_waiting_for_destruction(
    receipt_owner_is_sibling: bool,
) -> (
    DockspaceSession,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
    NativeCloseEffectAcknowledgement,
) {
    let layout = DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                if id == 1 {
                    DockspaceNode::central_tabs([ItemId::new(id)])
                } else {
                    DockspaceNode::tabs([ItemId::new(id)])
                },
            ),
        )
    }))
    .unwrap();
    let mut session =
        DockspaceSession::from_layout(layout, dockspace::policy::DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    let owner_surface = SurfaceId::new(if receipt_owner_is_sibling { 2 } else { 1 });
    session
        .register_native_root(owner_surface, HostWindowToken::new(1))
        .unwrap();
    let owner = commit(&mut session).native_bindings()[0];
    let scale = ScaleFactor::new(1.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                owner.window(),
                NativeWindowFacts::live()
                    .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .set_native_surface_placements(
            owner.window(),
            &[1, 2].map(|id| {
                NativeSurfacePlacement::new(
                    SurfaceId::new(id),
                    LogicalSize::new(800.0, 600.0).unwrap(),
                    PhysicalPoint::new(0.0, 0.0).unwrap(),
                    scale,
                    PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                )
            }),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let sibling = session
        .native_surface_bindings_for_window(owner.window())
        .find(|binding| binding.surface() == SurfaceId::new(2))
        .unwrap();
    assert!(
        session.is_current_native_binding(owner),
        "the equivalent declaration preserves the physical receipt's exact tag"
    );
    assert_eq!(owner == sibling, receipt_owner_is_sibling);
    let acknowledgement = approve_shared_retention(&mut session, owner);
    (session, owner, sibling, acknowledgement)
}

fn approve_shared_retention(
    session: &mut DockspaceSession,
    owner: NativeSurfaceBinding,
) -> NativeCloseEffectAcknowledgement {
    session
        .publish_native_close(owner, NativeCloseState::Requested, None)
        .unwrap();
    let observed = commit(session);
    let close = observed
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                close_requests.first().copied()
            }
            _ => None,
        })
        .unwrap();
    session
        .request_native_surface_close(close, NativeSurfaceCloseAction::RetainLayout)
        .unwrap();
    let requested = commit(session);
    let plan = requested
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceCloseRequested { plan, .. } => Some(plan.clone()),
            _ => None,
        })
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    for item in plan.items() {
        frame
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .unwrap();
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let [effect] = frame
        .commit()
        .unwrap()
        .take_native_effects()
        .try_into()
        .unwrap();
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
        panic!("the real close plan must freeze this shared window pending destruction");
    };
    acknowledgement
}

fn vacate_closing_sibling(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(
            ItemId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
}

#[test]
fn closing_shared_window_accepts_detachment_before_or_after_its_physical_receipt() {
    for receipt_owner_is_sibling in [false, true] {
        for physical_first in [false, true] {
            let (mut session, owner, sibling, acknowledgement) =
                shared_window_waiting_for_destruction(receipt_owner_is_sibling);
            let slots = if receipt_owner_is_sibling { 1 } else { 2 };
            let bridge =
                NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1)
                    .unwrap();
            bridge.configure_lifecycle_storage(slots, 1).unwrap();
            bridge
                .bind_viewport_route(ViewportId::ROOT, WindowId::from(98), owner)
                .unwrap();
            let backing = bridge.owned_input_capacity_bytes(None);
            if physical_first {
                assert!(bridge.retirement_begin_release(ViewportId::ROOT, owner, acknowledgement));
            }
            let (mut report, _) = bridge
                .commit_host_frame_with_staging(vacate_closing_sibling(&mut session), true, None)
                .unwrap();
            assert_eq!(report.native_placement_detachments(), &[sibling]);
            assert!(report.native_root_continuations().is_empty());
            assert!(report.take_native_effects().is_empty());
            if !physical_first {
                assert!(bridge.retirement_begin_release(ViewportId::ROOT, owner, acknowledgement));
            }
            assert_eq!(bridge.owned_input_capacity_bytes(None), backing);
            assert!(!bridge.retirement_can_begin_release(ViewportId::ROOT, owner));
            assert!(!bridge.retirement_is_quiescence_candidate(owner));
            assert!(!bridge.retirement_finish_quiescence(owner));
            assert_eq!(
                bridge.retirement_is_detached_placement(sibling),
                !receipt_owner_is_sibling
            );
            assert!(session.view().surface(SurfaceId::new(2)).is_none());
            assert_eq!(
                session
                    .native_surface_bindings_for_window(owner.window())
                    .count(),
                1
            );
            assert_eq!(
                bridge.with_viewports(|routes| routes.window(ViewportId::ROOT)),
                Some(WindowId::from(98))
            );
        }
    }
}

#[test]
fn closing_sibling_detachment_refuses_only_a_genuinely_new_missing_exact_slot() {
    let (mut session, owner, sibling, acknowledgement) =
        shared_window_waiting_for_destruction(false);
    let bridge =
        NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1).unwrap();
    bridge.configure_lifecycle_storage(1, 1).unwrap();
    bridge
        .bind_viewport_route(ViewportId::ROOT, WindowId::from(99), owner)
        .unwrap();
    assert!(bridge.retirement_begin_release(ViewportId::ROOT, owner, acknowledgement));
    let version = session.version();
    let backing = bridge.owned_input_capacity_bytes(None);
    for _ in 0..2 {
        let error = bridge
            .commit_host_frame_with_staging(vacate_closing_sibling(&mut session), true, None)
            .unwrap_err();
        assert!(error.is_input_capacity());
        assert_eq!(session.version(), version);
        assert!(session.is_current_native_binding(sibling));
        assert_eq!(bridge.owned_input_capacity_bytes(None), backing);
        assert!(!bridge.retirement_is_quiescence_candidate(owner));
        assert!(!bridge.retirement_finish_quiescence(owner));
        assert!(!bridge.retirement_is_detached_placement(sibling));
    }
}

#[test]
fn physical_receipt_and_detachment_keep_the_destroyed_gate_in_both_orders() {
    let (_session, _, bindings) = shared_placements();
    for receipt_owner_is_detached in [false, true] {
        for physical_first in [false, true] {
            let slots = if receipt_owner_is_detached { 1 } else { 2 };
            let mut state = NativeRetirementState::with_capacity_limit(slots).unwrap();
            let backing = state.entries.as_ptr();
            let owner = bindings[usize::from(receipt_owner_is_detached)];
            let window = WindowId::from(97);
            if physical_first {
                state
                    .retain_window(window_receipt(ViewportId::ROOT, window, owner))
                    .unwrap();
            }
            assert!(state.can_detach_binding(bindings[1]));
            assert_eq!(
                state.detachment_needs_entry(bindings[1]),
                !physical_first || !receipt_owner_is_detached
            );
            state.commit_detachment(bindings[1]);
            if !physical_first {
                assert!(state.can_retain_window_receipt(owner, window));
                state
                    .retain_window(window_receipt(ViewportId::ROOT, window, owner))
                    .unwrap();
            }
            assert!(state.owns_window(window, Some(ViewportId::ROOT)));
            assert_eq!(state.bindings().count(), slots);
            assert!(!state.is_detached_placement(owner));
            assert!(!state.is_quiescence_candidate(owner));
            assert!(!state.finish_quiescence(owner));
            assert!(!state.can_retain_window_receipt(bindings[0], window));
            assert_eq!(state.entries.as_ptr(), backing);
            assert_eq!(
                state.receive_destroyed(window, Some(ViewportId::ROOT), 5, |viewport, binding| {
                    super::tests::destroyed_ingress(viewport, binding, window, 5)
                }),
                Some((ViewportId::ROOT, owner)),
            );
            let capture = state.window_ingress().next().unwrap().ordinal();
            assert_eq!(
                state.observe_destroyed(ViewportId::ROOT, owner, window, 5),
                DestroyedObservation::Recorded
            );
            assert!(!state.finish_quiescence(owner));
            assert!(state.transfer_destroyed(owner, window, 5));
            state.commit_destroyed_callbacks();
            assert!(!state.requires_snapshot());
            drop(state.take_window_ingress(capture).unwrap());
            state.commit_destroyed_callbacks();
            assert!(state.requires_snapshot());
            state.mark_snapshot_queued();
            state.settle_snapshot(false);
            assert!(!state.finish_quiescence(owner));
            state.mark_snapshot_queued();
            state.settle_snapshot(true);
            assert!(!state.finish_quiescence(owner));
            let committed = state.committed_routes().collect::<Vec<_>>();
            state.commit_routes(&committed);
            assert!(state.finish_quiescence(owner));
            if !receipt_owner_is_detached {
                assert!(state.is_quiescence_candidate(bindings[1]));
                assert!(state.finish_quiescence(bindings[1]));
            }
            assert!(!state.has_pending_work());
        }
    }
}

#[test]
fn shared_placement_detachment_keeps_exact_quiescence_and_physical_owner_exclusion() {
    let (_session, _, bindings) = shared_placements();
    let mut state = NativeRetirementState::with_capacity_limit(2).unwrap();
    let backing = state.entries.as_ptr();
    let bytes = state.owned_data_capacity_bytes();
    assert!(state.can_detach_binding(bindings[0]));
    state.commit_detachment(bindings[0]);
    assert!(!state.can_detach_binding(bindings[0]));
    assert!(state.can_detach_binding(bindings[1]));
    assert!(state.has_quiescence_owner(bindings[1]));
    assert!(
        state
            .begin_unmaterialized_quiescence(bindings[1], NativeCreateReceipt::new(None))
            .is_err()
    );
    assert!(!state.can_retain_window(bindings[1], WindowId::from(91)));
    state.commit_detachment(bindings[1]);
    assert_eq!(state.bindings().collect::<Vec<_>>(), bindings);
    assert!(state.finish_quiescence(bindings[0]));
    assert!(!state.finish_quiescence(bindings[0]));
    assert!(state.is_quiescence_candidate(bindings[1]));
    assert!(state.finish_quiescence(bindings[1]));
    assert!(!state.has_pending_work());
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), bytes);

    let mut physical = NativeRetirementState::with_capacity_limit(2).unwrap();
    physical
        .retain_window(window_receipt(
            ViewportId::ROOT,
            WindowId::from(92),
            bindings[0],
        ))
        .unwrap();
    assert!(physical.can_detach_binding(bindings[0]));
    assert!(!physical.detachment_needs_entry(bindings[0]));
    assert!(physical.can_detach_binding(bindings[1]));
    assert!(physical.detachment_needs_entry(bindings[1]));
    let mut unmaterialized = NativeRetirementState::with_capacity_limit(2).unwrap();
    unmaterialized
        .begin_unmaterialized_quiescence(bindings[0], NativeCreateReceipt::new(None))
        .unwrap();
    assert!(!unmaterialized.can_detach_binding(bindings[1]));
}

#[test]
fn shared_replacement_prepays_every_exact_placement_and_drains_real_core_guards_independently() {
    for reverse in [false, true] {
        let (mut session, original, bindings) = shared_placements();
        let bridge =
            NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1).unwrap();
        bridge.configure_lifecycle_storage(2, 1).unwrap();
        bridge.configure_window_resource_storage(2, 0).unwrap();
        bridge
            .bind_viewport_route(ViewportId::ROOT, WindowId::from(93), original)
            .unwrap();
        let capacity = bridge.owned_input_capacity_bytes(None);
        let frame = shared_replacement(&mut session, &bindings);
        let proposed = frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>();
        assert_eq!(proposed.len(), 2);
        let (mut report, _) = bridge
            .commit_host_frame_with_staging(frame, true, None)
            .unwrap();
        assert_eq!(report.native_root_continuations(), proposed);
        assert!(report.take_native_effects().is_empty());
        assert!(report.take_painted_outputs().is_empty());
        assert_eq!(bridge.retirement_quiescence_candidates(), bindings);
        let successor_window = proposed[0].successor().window();
        assert_eq!(proposed[1].successor().window(), successor_window);
        assert_eq!(
            session.native_window_bindings().collect::<Vec<_>>(),
            [successor_window]
        );
        let order = if reverse {
            [bindings[1], bindings[0]]
        } else {
            bindings
        };
        for (index, binding) in order.into_iter().enumerate() {
            assert!(session.recognizes_native_binding(binding));
            session.report_native_binding_quiescence(binding).unwrap();
            commit(&mut session);
            session.prepare_native_pointer_capacity().expect(
                "the real core must retain an independent terminal guard for each placement",
            );
            assert!(bridge.retirement_finish_quiescence(binding));
            assert!(!bridge.retirement_finish_quiescence(binding));
            assert!(!session.recognizes_native_binding(binding));
            if index == 0 {
                assert!(session.recognizes_native_binding(order[1]));
                assert!(bridge.retirement_is_quiescence_candidate(order[1]));
            }
            assert!(session.is_current_native_binding(proposed[0].successor()));
            assert!(session.is_current_native_binding(proposed[1].successor()));
        }
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        assert_eq!(bridge.owned_input_capacity_bytes(None), capacity);
    }
}

#[test]
fn shared_replacement_refuses_an_underpaid_exact_roster_before_core_or_route_publication() {
    for finite_entries in [false, true] {
        let (mut session, original, bindings) = shared_placements();
        let bridge =
            NativeHostBridge::try_new_with_capacity(NativeViewportMap::default(), 2, 1).unwrap();
        if finite_entries {
            bridge.configure_lifecycle_storage(1, 1).unwrap();
        }
        bridge.configure_window_resource_storage(2, 0).unwrap();
        bridge
            .bind_viewport_route(ViewportId::ROOT, WindowId::from(94), original)
            .unwrap();
        if !finite_entries {
            // Physical route admission prepaid one slot. Leave one byte less
            // than the second exact placement requires, with no entry limit.
            assert!(bridge.configure_input_storage_allowance(usize::MAX, 0));
            let baseline = bridge.selected_storage_reserved_bytes().unwrap();
            let growth = NativeRetirementState::reservation_bytes(2).unwrap()
                - NativeRetirementState::reservation_bytes(1).unwrap();
            assert!(bridge.configure_input_storage_allowance(baseline + growth - 1, 0));
        }
        let version = session.version();
        let capacity = bridge.owned_input_capacity_bytes(None);
        let mut expected = None;
        for _ in 0..2 {
            let frame = shared_replacement(&mut session, &bindings);
            let proposed = frame
                .proposed_native_root_continuations()
                .collect::<Vec<_>>();
            assert_eq!(expected.get_or_insert_with(|| proposed.clone()), &proposed);
            assert!(
                bridge
                    .commit_host_frame_with_staging(frame, true, None)
                    .is_err()
            );
            assert_eq!(session.version(), version);
            assert_eq!(
                bridge.with_viewports(|routes| routes.binding(ViewportId::ROOT)),
                Some(original)
            );
            assert!(bridge.retirement_quiescence_candidates().is_empty());
            assert_eq!(bridge.owned_input_capacity_bytes(None), capacity);
            assert!(
                bindings
                    .iter()
                    .all(|binding| session.is_current_native_binding(*binding))
            );
        }
    }
}

#[test]
fn one_shared_slot_keeps_the_failed_create_until_quiescence_then_retries_the_original_window() {
    let failed = super::tests::binding();
    let live = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(31);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    let (receipt, ordinal) = failed_create(viewport, failed);
    state
        .begin_unmaterialized_quiescence(failed, receipt)
        .unwrap();
    let receipt = state
        .retain_window(window_receipt(viewport, window, live))
        .unwrap_err();
    assert_eq!(receipt.binding(), live);
    assert_eq!(receipt.window(), window);
    assert!(!state.can_retain_window(live, window));
    assert_eq!(
        state.prepare_capacity_for_retained_lifetimes(2),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable),
    );
    commit_failed_create(&mut state, failed, ordinal);
    assert_eq!(state.next_quiescence_candidate(None), Some(failed));
    let receipt = state.retain_window(receipt).unwrap_err();
    assert!(state.has_quiescence_owner(failed));
    assert!(state.finish_quiescence(failed));
    assert!(!state.finish_quiescence(failed));
    assert!(!state.has_pending_work());
    assert!(!state.can_configure_capacity_limit());
    state.retain_window(receipt).unwrap();
    assert!(state.has_window(viewport, live));
    assert_eq!(state.bindings().collect::<Vec<_>>(), [live]);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn release_preserves_its_acknowledgement_through_full_commit_and_rejected_snapshot_retry() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let other = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(41);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state
        .begin_release(
            viewport,
            binding,
            acknowledgement,
            Some(window_receipt(viewport, window, binding)),
        )
        .unwrap();
    let (receipt, ordinal) = failed_create(viewport, other);
    let receipt = state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap_err();
    assert_eq!(receipt.ingress().unwrap().ordinal(), ordinal);
    assert_eq!(
        state.receive_destroyed(window, Some(viewport), 5, |viewport, binding| {
            super::tests::destroyed_ingress(viewport, binding, window, 5)
        }),
        Some((viewport, binding)),
    );
    let capture = state.window_ingress().next().unwrap().ordinal();
    assert_eq!(
        state.observe_destroyed(viewport, binding, window, 5),
        DestroyedObservation::Recorded,
    );
    assert!(state.transfer_destroyed(binding, window, 5));
    state.commit_destroyed_callbacks();
    assert!(!state.requires_snapshot());
    let ingress = state.take_window_ingress(capture).unwrap();
    assert_eq!(ingress.window_event().unwrap().ordinal(), 5);
    assert!(!state.requires_snapshot());
    state.commit_destroyed_callbacks();
    assert_eq!(
        state.destroyed_observations().next(),
        Some((binding, NativeWindowFacts::destroyed_after(acknowledgement))),
    );
    for _ in 0..2 {
        state.mark_snapshot_queued();
        assert!(state.committed_routes().next().is_none());
        state.settle_snapshot(false);
        assert!(state.requires_snapshot());
        assert!(!state.finish_quiescence(binding));
        assert_eq!(
            state.destroyed_observations().next(),
            Some((binding, NativeWindowFacts::destroyed_after(acknowledgement))),
        );
    }
    state.mark_snapshot_queued();
    state.settle_snapshot(true);
    let committed = state.committed_routes().next().unwrap();
    assert_eq!(committed.viewport(), viewport);
    assert_eq!(committed.binding(), binding);
    assert!(!state.is_quiescence_candidate(binding));
    assert!(state.can_retire_committed_route_for_replacement(viewport, binding));
    state.commit_routes(&[committed]);
    assert_eq!(state.next_quiescence_candidate(None), Some(binding));
    let receipt = state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap_err();
    assert!(state.finish_quiescence(binding));
    state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap();
    assert_eq!(state.create_ingress().next().unwrap().ordinal(), ordinal);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn capacity_and_identity_refusals_return_the_original_release_capabilities() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let failed = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(51);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let (failed_receipt, ordinal) = failed_create(viewport, failed);
    state
        .begin_unmaterialized_quiescence(failed, failed_receipt)
        .unwrap();
    assert!(!state.can_begin_release(viewport, binding));
    let (retry_ack, retry_window) = state
        .begin_release(
            viewport,
            binding,
            acknowledgement,
            Some(window_receipt(viewport, window, binding)),
        )
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    let retry_window = retry_window.unwrap();
    assert_eq!(retry_window.viewport(), viewport);
    assert_eq!(retry_window.window(), window);
    assert_eq!(retry_window.binding(), binding);
    commit_failed_create(&mut state, failed, ordinal);
    assert!(state.finish_quiescence(failed));
    let foreign_viewport = ViewportId::from_hash_of("foreign-release");
    let (retry_ack, retry_window) = state
        .begin_release(foreign_viewport, binding, retry_ack, Some(retry_window))
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    assert!(!state.has_pending_work());
    state
        .begin_release(viewport, binding, retry_ack, retry_window)
        .unwrap();
    let (retry_ack, retry_window) = state
        .begin_release(viewport, binding, acknowledgement, None)
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    assert!(retry_window.is_none());
    assert_eq!(state.bindings().count(), 1);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn lifetime_exclusivity_rejects_viewport_aliases_and_both_terminal_kinds() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let foreign = super::tests::binding();
    assert_eq!(binding.surface(), foreign.surface());
    assert_eq!(binding.window_token(), foreign.window_token());
    assert!(!binding.same_window_lifetime(foreign));
    let viewport = ViewportId::ROOT;
    let alias = ViewportId::from_hash_of("alias-window");
    let window = WindowId::from(61);
    let mut state = NativeRetirementState::with_capacity_limit(2).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state
        .retain_window(window_receipt(viewport, window, binding))
        .unwrap();
    assert!(state.can_begin_release(viewport, binding));
    assert!(!state.can_begin_release(alias, binding));
    let (returned, receipt) = state
        .begin_release(alias, binding, acknowledgement, None)
        .unwrap_err();
    assert_eq!(returned, acknowledgement);
    assert!(receipt.is_none());
    let receipt = state
        .retain_window(window_receipt(alias, WindowId::from(62), binding))
        .unwrap_err();
    assert_eq!(receipt.binding(), binding);
    assert_eq!(receipt.viewport(), alias);
    let failed = NativeCreateReceipt::new(None);
    assert!(
        state
            .begin_unmaterialized_quiescence(binding, failed)
            .is_err()
    );
    assert!(!state.adopt_successor_binding(binding, foreign));
    assert!(state.has_window(viewport, binding));
    state
        .begin_unmaterialized_quiescence(foreign, NativeCreateReceipt::new(None))
        .unwrap();
    assert!(!state.can_begin_release(alias, foreign));
    assert!(!state.can_retain_window(foreign, WindowId::from(63)));
    state
        .begin_release(viewport, binding, acknowledgement, None)
        .unwrap();
    assert_eq!(state.bindings().count(), 2);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn quiescence_cursor_orders_mixed_owners_and_keeps_a_blocked_candidate() {
    let mut bindings = [
        super::tests::binding(),
        super::tests::binding(),
        super::tests::binding(),
    ];
    bindings.sort_unstable();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(71);
    let mut state = NativeRetirementState::with_capacity_limit(3).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    let mut journal = HostIngressJournal::new(dockspace::runtime::NativeMonotonicClock::system());
    let (last, last_ordinal) = failed_create_in(&mut journal, viewport, bindings[2]);
    state
        .begin_unmaterialized_quiescence(bindings[2], last)
        .unwrap();
    state
        .retain_window(window_receipt(viewport, window, bindings[1]))
        .unwrap();
    let (first, first_ordinal) = failed_create_in(&mut journal, viewport, bindings[0]);
    assert_ne!(first_ordinal, last_ordinal);
    state
        .begin_unmaterialized_quiescence(bindings[0], first)
        .unwrap();
    commit_failed_create(&mut state, bindings[0], first_ordinal);
    commit_failed_create(&mut state, bindings[2], last_ordinal);
    assert_eq!(state.next_quiescence_candidate(None), Some(bindings[0]));
    assert_eq!(
        state.next_quiescence_candidate(Some(bindings[0])),
        Some(bindings[2])
    );
    assert!(state.finish_quiescence(bindings[2]));
    assert_eq!(state.next_quiescence_candidate(Some(bindings[2])), None);
    assert_eq!(state.next_quiescence_candidate(None), Some(bindings[0]));
    assert!(!state.finish_quiescence(bindings[1]));
    assert_eq!(state.bindings().collect::<Vec<_>>(), bindings[..2]);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn reservation_and_unlimited_preparation_preserve_existing_backing_on_overflow() {
    for slots in [0, 1, 7] {
        let mut state = NativeRetirementState::with_capacity_limit(slots).unwrap();
        assert_eq!(state.capacity_limit(), Some(slots));
        assert!(!state.can_configure_capacity_limit());
        assert_eq!(
            Some(state.owned_data_capacity_bytes()),
            NativeRetirementState::reservation_bytes(slots),
        );
        let backing = state.entries.as_ptr();
        assert!(state.prepare_capacity_for_retained_lifetimes(slots).is_ok());
        assert_eq!(
            state.prepare_capacity_for_retained_lifetimes(slots + 1),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
        assert_eq!(state.entries.as_ptr(), backing);
    }
    let mut state = NativeRetirementState::default();
    assert!(state.can_configure_capacity_limit());
    assert_eq!(state.capacity_limit(), None);
    state.prepare_capacity_for_retained_lifetimes(3).unwrap();
    assert!(!state.has_pending_work());
    assert!(state.can_configure_capacity_limit());
    assert_eq!(state.entries.capacity(), 3);
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state.prepare_capacity_for_retained_lifetimes(1).unwrap();
    for required in [usize::MAX, usize::MAX / size_of::<RetirementEntry>() + 1] {
        assert_eq!(NativeRetirementState::reservation_bytes(required), None);
        assert_eq!(
            state.prepare_capacity_for_retained_lifetimes(required),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
        assert_eq!(
            NativeRetirementState::with_capacity_limit(required).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable,
        );
    }
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
    state.prepare_capacity_for_retained_lifetimes(4).unwrap();
    assert_eq!(state.entries.capacity(), 4);
    assert_eq!(
        Some(state.owned_data_capacity_bytes()),
        NativeRetirementState::reservation_bytes(4),
    );
}

#[test]
fn default_unlimited_accepts_more_lifetimes_than_the_selected_finite_bound() {
    let bindings = [super::tests::binding(), super::tests::binding()];
    let mut finite = NativeRetirementState::with_capacity_limit(1).unwrap();
    let mut unlimited = NativeRetirementState::default();
    unlimited
        .prepare_capacity_for_retained_lifetimes(2)
        .unwrap();
    let baseline = unlimited.owned_data_capacity_bytes();
    let backing = unlimited.entries.as_ptr();
    for binding in bindings {
        unlimited
            .begin_unmaterialized_quiescence(binding, NativeCreateReceipt::new(None))
            .unwrap();
    }
    finite
        .begin_unmaterialized_quiescence(bindings[0], NativeCreateReceipt::new(None))
        .unwrap();
    assert!(
        finite
            .begin_unmaterialized_quiescence(bindings[1], NativeCreateReceipt::new(None))
            .is_err()
    );
    assert_eq!(unlimited.bindings().count(), 2);
    assert_eq!(unlimited.entries.as_ptr(), backing);
    assert_eq!(unlimited.owned_data_capacity_bytes(), baseline);
    assert!(!unlimited.can_configure_capacity_limit());
    assert_eq!(unlimited.capacity_limit(), None);
}
