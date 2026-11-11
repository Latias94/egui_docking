//! Vacancies retain published producer lanes and exact physical cleanup owners.

use super::*;
use crate::geometry::PhysicalPoint;
use crate::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout,
};
use crate::runtime::DockspaceHostFrame;
use std::num::NonZeroUsize;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const INCOMING: SurfaceId = SurfaceId::new(3);
const B_ITEM: ItemId = ItemId::new(2);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([A, B, INCOMING].map(|surface| {
        let items = [ItemId::new(surface.get())];
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(surface.get()),
                if surface == B {
                    DockspaceNode::tabs(items)
                } else {
                    DockspaceNode::central_tabs(items)
                },
            ),
        )
    }))
    .unwrap();
    managed_session(layout, 2)
}

fn managed_session(layout: DockspaceLayout, binding_slots: usize) -> DockspaceSession {
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(8).unwrap(), 1 << 20)
                .with_producer_storage_limit(NativeProducerStorageLimit::new(
                    NonZeroUsize::new(binding_slots).unwrap(),
                    NonZeroUsize::MIN,
                    1 << 20,
                )),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();
    session
}

fn frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn placements() -> [NativeSurfacePlacement; 2] {
    [A, B].map(|surface| {
        let x = if surface == A { 0.0 } else { 400.0 };
        NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(400.0, 600.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 400.0, 600.0).unwrap(),
        )
    })
}

fn shared_session() -> (
    DockspaceSession,
    NativeWindowBinding,
    [NativeSurfaceBinding; 2],
) {
    let mut session = session();
    session.register_native_root(A, WINDOW).unwrap();
    let [registered]: [NativeSurfaceBinding; 1] = finish(frame(&mut session))
        .native_bindings()
        .try_into()
        .unwrap();
    let window = registered.window();
    session
        .report_managed_native_snapshot(
            [(
                window,
                NativeWindowFacts::live()
                    .with_content_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
                    .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                    .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut composed = frame(&mut session);
    composed
        .set_native_surface_placements(window, &placements())
        .unwrap();
    let bindings = composed
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    finish(composed);
    (session, window, bindings.try_into().unwrap())
}

fn dock_b(frame: &mut DockspaceHostFrame<'_>) {
    frame
        .dock_item_current(
            B_ITEM,
            DockPlacement::Center(DockAnchor::Central(RootId::new(A.get()))),
        )
        .unwrap();
    assert!(frame.view().surface(B).is_none());
}

fn assert_no_retirement(session: &DockspaceSession, binding: NativeSurfaceBinding) {
    assert!(
        !session
            .engine
            .viewport()
            .has_terminal_native_binding_guard(binding.core_binding()),
        "an unpublished placement cannot retain a terminal guard without a producer lane"
    );
    assert!(
        !session
            .engine
            .viewport()
            .recognizes_input_binding(binding.core_binding())
    );
    assert!(!session.recognizes_native_binding(binding));
}

#[test]
fn unpublished_replacement_then_shared_vacancy_does_not_retain_an_unquiesceable_guard() {
    let (mut session, window, before) = shared_session();
    let version = session.version();
    let mut geometry = placements();
    geometry[1] = NativeSurfacePlacement::new(
        B,
        geometry[1].size(),
        PhysicalPoint::new(450.0, 0.0).unwrap(),
        geometry[1].scale_factor(),
        geometry[1].clip(),
    );
    let mut expected = None;
    for commit in [false, true] {
        let mut candidate = frame(&mut session);
        candidate
            .set_native_surface_placements(window, &geometry)
            .unwrap();
        let transient = candidate
            .native_surface_bindings_for_window(window)
            .find(|binding| binding.surface() == B)
            .unwrap();
        assert_ne!(transient, before[1]);
        dock_b(&mut candidate);
        candidate
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let prepared = candidate.prepare_commit().unwrap();
        assert!(prepared.native_placement_detachments().is_empty());
        assert_eq!(
            prepared
                .native_surface_bindings_for_window(window)
                .collect::<Vec<_>>(),
            [before[0]]
        );
        if commit {
            assert_eq!(expected, Some(transient));
            let report = prepared.commit().unwrap();
            assert!(report.workspace_changed());
            assert!(report.native_placement_detachments().is_empty());
            assert_no_retirement(&session, transient);
            assert_no_retirement(&session, before[1]);
        } else {
            expected = Some(transient);
            drop(prepared);
            assert_eq!(session.version(), version);
            assert!(session.is_current_native_binding(before[1]));
            assert_no_retirement(&session, transient);
        }
    }
    let b = geometry[1];
    let incoming =
        NativeSurfacePlacement::new(INCOMING, b.size(), b.origin(), b.scale_factor(), b.clip());
    let mut reuse = frame(&mut session);
    reuse
        .set_native_surface_placements(window, &[geometry[0], incoming])
        .unwrap();
    assert!(finish(reuse).native_placement_detachments().is_empty());
    assert_eq!(session.native.as_ref().unwrap().producer.bindings.len(), 2);
}

#[test]
fn bootstrap_registration_composition_and_shared_vacancy_have_no_unpublished_retirement() {
    let mut session = session();
    session.register_native_root(A, WINDOW).unwrap();
    let version = session.version();
    let mut expected = None;
    for commit in [false, true] {
        let mut candidate = frame(&mut session);
        let core = candidate
            .frame
            .view()
            .viewport()
            .viewport(A)
            .unwrap()
            .binding();
        let root = NativeSurfaceBinding::from_binding(
            candidate.session.native.as_ref().unwrap().provider(),
            core,
        );
        let window = root.window();
        candidate
            .set_native_surface_placements(window, &placements())
            .unwrap();
        let composed = candidate
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>();
        let transient = composed
            .iter()
            .copied()
            .find(|binding| binding.surface() == B)
            .unwrap();
        let survivor = composed
            .iter()
            .copied()
            .find(|binding| binding.surface() == A)
            .unwrap();
        dock_b(&mut candidate);
        candidate
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let prepared = candidate.prepare_commit().unwrap();
        assert!(prepared.native_placement_detachments().is_empty());
        assert_eq!(
            prepared
                .native_surface_bindings_for_window(window)
                .collect::<Vec<_>>(),
            [survivor]
        );
        if commit {
            assert_eq!(expected, Some((root, transient, survivor)));
            let report = prepared.commit().unwrap();
            assert!(report.native_placement_detachments().is_empty());
            assert_eq!(report.native_bindings(), [survivor]);
            assert_no_retirement(&session, root);
            assert_no_retirement(&session, transient);
            assert!(session.is_current_native_binding(survivor));
            assert_eq!(session.native.as_ref().unwrap().producer.bindings.len(), 1);
        } else {
            expected = Some((root, transient, survivor));
            drop(prepared);
            assert_eq!(session.version(), version);
            assert!(
                session
                    .native
                    .as_ref()
                    .unwrap()
                    .producer
                    .bindings
                    .is_empty()
            );
            assert_no_retirement(&session, root);
            assert_no_retirement(&session, transient);
        }
    }
}

#[test]
fn published_shared_vacancy_retains_its_exact_guard_until_quiescence() {
    let (mut session, window, before) = shared_session();
    for commit in [false, true] {
        let mut candidate = frame(&mut session);
        dock_b(&mut candidate);
        candidate
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let prepared = candidate.prepare_commit().unwrap();
        assert_eq!(prepared.native_placement_detachments(), [before[1]]);
        if commit {
            assert_eq!(
                prepared.commit().unwrap().native_placement_detachments(),
                [before[1]]
            );
        } else {
            drop(prepared);
            assert!(session.is_current_native_binding(before[1]));
            assert!(
                !session
                    .engine
                    .viewport()
                    .has_terminal_native_binding_guard(before[1].core_binding())
            );
        }
    }
    assert!(
        session
            .engine
            .viewport()
            .has_terminal_native_binding_guard(before[1].core_binding())
    );
    assert!(
        session
            .engine
            .viewport()
            .recognizes_input_binding(before[1].core_binding())
    );
    assert!(session.recognizes_native_binding(before[1]));
    assert!(!session.is_current_native_binding(before[1]));
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
    session.report_native_binding_quiescence(before[1]).unwrap();
    finish(frame(&mut session));
    // The next poll reclaims the committed ingress prefix and its exact guard.
    drop(frame(&mut session));
    assert_no_retirement(&session, before[1]);
    assert!(session.is_current_native_binding(before[0]));
}

fn vacancy_session(
    binding_slots: usize,
    shared: bool,
) -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
    let layout = DockspaceLayout::new(
        [A, B, INCOMING]
            .into_iter()
            .filter(|surface| shared || *surface != B)
            .map(|surface| {
                let items = [ItemId::new(surface.get())];
                DockspaceSurfaceLayout::new(
                    surface,
                    DockspaceRootLayout::new(
                        RootId::new(surface.get()),
                        if surface == INCOMING {
                            DockspaceNode::central_tabs(items)
                        } else {
                            DockspaceNode::tabs(items)
                        },
                    ),
                )
            }),
    )
    .unwrap();
    let mut session = managed_session(layout, binding_slots);
    session.register_native_root(A, WINDOW).unwrap();
    session
        .register_native_root(INCOMING, HostWindowToken::new(42))
        .unwrap();
    let bindings: [NativeSurfaceBinding; 2] = finish(frame(&mut session))
        .native_bindings()
        .try_into()
        .unwrap();
    session
        .report_managed_native_snapshot(
            bindings.map(|binding| (binding, NativeWindowFacts::live())),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    finish(frame(&mut session));
    if shared {
        let mut composed = frame(&mut session);
        composed
            .set_native_surface_placements(bindings[0].window(), &placements())
            .unwrap();
        finish(composed);
    }
    (session, bindings)
}

fn replace_then_vacate_singleton(
    session: &mut DockspaceSession,
    before: NativeSurfaceBinding,
) -> (DockspaceHostFrame<'_>, NativeSurfaceBinding) {
    let mut candidate = frame(session);
    candidate
        .set_native_surface_placements(before.window(), &placements()[..1])
        .unwrap();
    let transient = candidate
        .native_surface_bindings_for_window(before.window())
        .next()
        .unwrap();
    assert_ne!(transient, before);
    candidate
        .dock_item_current(
            ItemId::new(A.get()),
            DockPlacement::Center(DockAnchor::Central(RootId::new(INCOMING.get()))),
        )
        .unwrap();
    assert!(candidate.view().surface(A).is_none());
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    (candidate, transient)
}

fn destroy_then_quiesce_cleanup(
    session: &mut DockspaceSession,
    cleanup: NativeSurfaceBinding,
    other: NativeSurfaceBinding,
) {
    let retention = session.engine.runtime_retention_manifest().bindings();
    assert_eq!(retention.active_cleanup_obligations(), 1);
    assert_eq!(retention.terminal_binding_guards(), 0);
    session
        .report_managed_native_snapshot(
            [
                (cleanup.window(), NativeWindowFacts::destroyed()),
                (other.window(), NativeWindowFacts::live()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    finish(frame(session));
    let retention = session.engine.runtime_retention_manifest().bindings();
    assert_eq!(retention.active_cleanup_obligations(), 0);
    assert_eq!(retention.terminal_binding_guards(), 1);
    session
        .report_native_binding_quiescence(cleanup)
        .expect("real destruction leaves an exact producer lane which can quiesce");
    finish(frame(session));
    drop(frame(session));
    assert_no_retirement(session, cleanup);
    assert_eq!(
        session
            .retained_native_surface_bindings_for_window(cleanup.window())
            .count(),
        0
    );
    assert!(!session.recognizes_native_window_binding(cleanup.window()));
    assert!(session.is_current_native_binding(other));
}

#[test]
fn unpublished_last_placement_cleanup_retains_a_producer_until_real_destroyed_and_quiescence() {
    // Two slots are also sufficient: the obsolete A has no exact obligation,
    // and the physical window keeps its own independent observation row.
    for binding_slots in [2, 3] {
        let (mut session, [before, other]) = vacancy_session(binding_slots, false);
        let version = session.version();
        let mut expected = None;
        for commit in [false, true] {
            let (candidate, transient) = replace_then_vacate_singleton(&mut session, before);
            let prepared = candidate.prepare_commit().unwrap();
            assert!(prepared.native_placement_detachments().is_empty());
            assert!(
                prepared
                    .native_surface_bindings_for_window(before.window())
                    .next()
                    .is_none()
            );
            if commit {
                assert_eq!(expected, Some(transient));
                let report = prepared.commit().unwrap();
                assert!(report.native_placement_detachments().is_empty());
                assert_eq!(report.native_bindings(), [other]);
            } else {
                expected = Some(transient);
                drop(prepared);
                assert_eq!(session.version(), version);
                assert!(session.is_current_native_binding(before));
                assert_no_retirement(&session, transient);
            }
        }
        let transient = expected.unwrap();
        assert_eq!(
            session
                .retained_native_surface_bindings_for_window(before.window())
                .collect::<Vec<_>>(),
            [transient],
            "the unpublished physical cleanup needs its own retired producer lane"
        );
        assert!(session.recognizes_native_binding(transient));
        assert!(!session.is_current_native_binding(transient));
        assert!(!session.recognizes_native_binding(before));
        destroy_then_quiesce_cleanup(&mut session, transient, other);
    }
}

#[test]
fn unpublished_physical_cleanup_refuses_an_insufficient_prepared_producer_atomically() {
    let (mut session, [before, other]) = vacancy_session(2, false);
    let version = session.version();
    let watermark = session.engine.backend_ingress_committed_through();
    let recorded = session.native_input_recorded_count();
    let backing = session.native_input_producer_storage_capacity_bytes();
    let producer_bindings = session.native.as_ref().unwrap().producer.bindings.clone();
    let producer_windows = session.native.as_ref().unwrap().producer.windows.clone();
    for _ in 0..2 {
        let (candidate, transient) = replace_then_vacate_singleton(&mut session, before);
        let DockspaceHostFrame { session, frame, .. } = candidate;
        let mut presentation = frame.into_presentation().unwrap();
        presentation
            .resolve_all_presentation_obligations_unavailable(
                crate::engine::HostPresentationUnavailableReason::OutputNotProduced,
            )
            .unwrap();
        let prepared = presentation.prepare_owned(&session.engine).unwrap();
        // A fresh producer must pay both the remaining C and the A' cleanup,
        // including its detached physical window. No live limit is replaced.
        let insufficient =
            NativeProducerStorageLimit::new(NonZeroUsize::MIN, NonZeroUsize::MIN, 1 << 20);
        assert!(matches!(
            NativeProducerState::prepare(prepared.candidate_engine(), None, Some(insufficient),),
            Err(NativePlatformError::InputCapacity)
        ));
        let sufficient = NativeProducerStorageLimit::new(
            NonZeroUsize::new(2).unwrap(),
            NonZeroUsize::MIN,
            1 << 20,
        );
        let next =
            NativeProducerState::prepare(prepared.candidate_engine(), None, Some(sufficient))
                .unwrap();
        assert_eq!(
            next.binding(transient.core_binding()).unwrap().admission,
            None
        );
        assert_eq!(next.bindings.len(), 2);
        assert_eq!(next.windows.len(), 2);
        let window = next.window(before.window().core()).unwrap();
        assert!(!window.current);
        assert!(window.cleanup_observation);
        drop(next);
        drop(prepared);
        assert_eq!(session.version(), version);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            watermark
        );
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(
            session.native_input_producer_storage_capacity_bytes(),
            backing
        );
        assert_eq!(
            session.native.as_ref().unwrap().producer.bindings,
            producer_bindings
        );
        assert_eq!(
            session.native.as_ref().unwrap().producer.windows,
            producer_windows
        );
        assert!(session.is_current_native_binding(before));
        assert!(session.is_current_native_binding(other));
        assert!(session.view().surface(A).is_some());
        assert_no_retirement(session, transient);
        assert_eq!(session.engine.viewport().binding_retirements().count(), 0);
    }
}

#[test]
fn changing_and_vacating_all_shared_placements_retains_only_the_physical_cleanup() {
    let (mut session, [source, other]) = vacancy_session(3, true);
    let before = session
        .native_surface_bindings_for_window(source.window())
        .collect::<Vec<_>>();
    let geometry = placements().map(|placement| {
        NativeSurfacePlacement::new(
            placement.surface(),
            placement.size(),
            PhysicalPoint::new(placement.origin().x() + 20.0, 0.0).unwrap(),
            placement.scale_factor(),
            placement.clip(),
        )
    });
    let mut candidate = frame(&mut session);
    candidate
        .set_native_surface_placements(source.window(), &geometry)
        .unwrap();
    let transient = candidate
        .native_surface_bindings_for_window(source.window())
        .collect::<Vec<_>>();
    assert_eq!(transient.len(), 2);
    assert!(transient.iter().all(|binding| !before.contains(binding)));
    for surface in [A, B] {
        candidate
            .dock_item_current(
                ItemId::new(surface.get()),
                DockPlacement::Center(DockAnchor::Central(RootId::new(INCOMING.get()))),
            )
            .unwrap();
        assert!(candidate.view().surface(surface).is_none());
    }
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = candidate.commit().unwrap();
    assert!(report.native_placement_detachments().is_empty());
    assert_eq!(report.native_bindings(), [other]);
    let cleanup = transient
        .iter()
        .copied()
        .find(|binding| binding.surface() == B)
        .unwrap();
    for binding in before.into_iter().chain(transient) {
        if binding != cleanup {
            assert_no_retirement(&session, binding);
        }
    }
    assert_eq!(
        session
            .retained_native_surface_bindings_for_window(source.window())
            .collect::<Vec<_>>(),
        [cleanup]
    );
    destroy_then_quiesce_cleanup(&mut session, cleanup, other);
}
