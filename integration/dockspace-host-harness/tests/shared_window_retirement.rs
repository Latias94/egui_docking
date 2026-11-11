//! A shared window keeps one physical callback lane until all retired placements quiesce.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativeHostErrorKind, NativeInputStorageLimit, NativePointerRoster,
    NativeProducerStorageLimit, NativeReceiverAnswer, NativeSurfaceBinding, NativeSurfacePlacement,
    NativeWindowBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, SurfaceUnavailableReason,
};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const INCOMING: SurfaceId = SurfaceId::new(3);
const TOKEN: HostWindowToken = HostWindowToken::new(7);

fn layout(surfaces: &[SurfaceId]) -> DockspaceLayout {
    DockspaceLayout::new(surfaces.iter().copied().map(|surface| {
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(surface.get()),
                DockspaceNode::central_tabs([ItemId::new(surface.get())]),
            ),
        )
    }))
    .unwrap()
}

fn facts() -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn native_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
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

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(native_frame(session))
}

fn shared_session(
    surfaces: &[SurfaceId],
) -> (
    DockspaceSession,
    NativeWindowBinding,
    Vec<NativeSurfaceBinding>,
) {
    shared_session_with_capacity(layout(surfaces), 2)
}

fn shared_session_with_capacity(
    layout: DockspaceLayout,
    slots: usize,
) -> (
    DockspaceSession,
    NativeWindowBinding,
    Vec<NativeSurfaceBinding>,
) {
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let producer = NativeProducerStorageLimit::new(
        NonZeroUsize::new(slots).unwrap(),
        NonZeroUsize::MIN,
        1 << 20,
    );
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(8).unwrap(), 1 << 20)
                .with_producer_storage_limit(producer),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();
    session.register_native_root(A, TOKEN).unwrap();
    let registered = idle(&mut session);
    let [transport] = registered.native_bindings() else {
        panic!("one root must establish the physical callback lane");
    };
    let transport = *transport;
    let window = transport.window();
    session
        .report_managed_native_snapshot([(window, facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    idle(&mut session);

    let placements = [A, B].map(|surface| {
        let x = if surface == A { 0.0 } else { 400.0 };
        NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(400.0, 600.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 400.0, 600.0).unwrap(),
        )
    });
    let mut frame = native_frame(&mut session);
    frame
        .set_native_surface_placements(window, &placements)
        .unwrap();
    let bindings = frame
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 2);
    assert!(bindings.iter().all(|binding| *binding != transport));
    assert_eq!(bindings[1].surface(), B);
    let mut composed = finish(frame);
    assert!(composed.take_native_effects().is_empty());
    (session, window, bindings)
}

fn retire(
    session: &mut DockspaceSession,
    window: NativeWindowBinding,
    bindings: &[NativeSurfaceBinding],
) {
    let reset = session.prepare_layout_reset(layout(&[INCOMING])).unwrap();
    let frame = session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut report = finish(frame);
    assert!(report.workspace_changed());
    assert!(report.native_placement_detachments().is_empty());
    assert!(
        report.take_native_effects().is_empty(),
        "an external root is never destroyed by retirement"
    );
    assert!(session.native_window_bindings().next().is_none());
    assert!(
        session
            .native_surface_bindings_for_window(window)
            .next()
            .is_none()
    );
    for &binding in bindings {
        assert!(!session.is_current_native_binding(binding));
        assert!(session.recognizes_native_binding(binding));
    }
    assert!(session.view().surface(A).is_none() && session.view().surface(B).is_none());
    assert!(session.view().surface(INCOMING).is_some());
}

#[test]
fn prepared_shared_reset_detaches_only_the_removed_published_member_and_rolls_back_on_drop() {
    let (mut session, window, before) = shared_session_with_capacity(layout(&[A, B]), 4);
    let reset = session.prepare_layout_reset(layout(&[A])).unwrap();
    let version = session.version();
    let mut expected = None;
    for commit in [false, true] {
        let mut frame = native_frame(&mut session);
        frame.submit_layout_reset(&reset).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let prepared = frame.prepare_commit().unwrap();
        let pairs: Vec<_> = prepared.native_root_continuations().collect();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].predecessor(), before[0]);
        assert_eq!(prepared.native_placement_detachments(), [before[1]]);
        assert_eq!(
            prepared
                .committed_native_surface_bindings_for_window(window)
                .collect::<Vec<_>>(),
            before
        );
        assert_eq!(
            prepared
                .native_surface_bindings_for_window(pairs[0].successor().window())
                .collect::<Vec<_>>(),
            [pairs[0].successor()]
        );
        if commit {
            assert_eq!(expected, Some(pairs.clone()));
            let report = prepared.commit().unwrap();
            assert_eq!(report.native_root_continuations(), pairs);
            assert_eq!(report.native_placement_detachments(), [before[1]]);
            for binding in &before {
                assert!(session.recognizes_native_binding(*binding));
                session.report_native_binding_quiescence(*binding).unwrap();
            }
            idle(&mut session);
            assert!(
                before
                    .iter()
                    .all(|binding| !session.recognizes_native_binding(*binding))
            );
            assert!(session.is_current_native_binding(pairs[0].successor()));
        } else {
            expected = Some(pairs);
            drop(prepared);
            assert_eq!(session.version(), version);
            assert_eq!(
                session
                    .native_surface_bindings_for_window(window)
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
}

#[test]
fn ordinary_docking_proposes_placement_detachment_after_final_vacancy_reduction() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        ),
        DockspaceSurfaceLayout::new(
            INCOMING,
            DockspaceRootLayout::new(
                RootId::new(3),
                DockspaceNode::central_tabs([ItemId::new(3)]),
            ),
        ),
    ])
    .unwrap();
    let (mut session, window, before) = shared_session_with_capacity(layout, 2);
    let a = session.native_surface_placement(before[0]).unwrap();
    let b = session.native_surface_placement(before[1]).unwrap();
    let incoming =
        NativeSurfacePlacement::new(INCOMING, b.size(), b.origin(), b.scale_factor(), b.clip());
    for commit in [false, true] {
        let mut frame = native_frame(&mut session);
        frame
            .dock_item_current(
                ItemId::new(2),
                DockPlacement::Center(DockAnchor::Central(RootId::new(1))),
            )
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let prepared = frame.prepare_commit().unwrap();
        assert!(prepared.native_root_continuations().next().is_none());
        assert_eq!(prepared.native_placement_detachments(), [before[1]]);
        assert_eq!(
            prepared
                .native_surface_bindings_for_window(window)
                .collect::<Vec<_>>(),
            [before[0]]
        );
        if commit {
            let report = prepared.commit().unwrap();
            assert_eq!(report.native_placement_detachments(), [before[1]]);
        } else {
            drop(prepared);
            assert!(session.view().surface(B).is_some());
            assert!(session.is_current_native_binding(before[1]));
        }
    }
    assert!(session.view().surface(B).is_none());
    let mut frame = native_frame(&mut session);
    assert!(
        frame
            .set_native_surface_placements(window, &[a, incoming])
            .is_err(),
        "an exact terminal guard keeps its slot until quiescence"
    );
    drop(frame);
    session.report_native_binding_quiescence(before[1]).unwrap();
    idle(&mut session);
    let mut frame = native_frame(&mut session);
    frame
        .set_native_surface_placements(window, &[a, incoming])
        .unwrap();
    assert!(finish(frame).native_placement_detachments().is_empty());
    assert!(session.is_current_native_binding(before[0]));
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
}

#[test]
fn full_producer_reuses_a_vacated_placement_slot_without_changing_the_window() {
    let (mut session, window, before) = shared_session(&[A, B, INCOMING]);
    let a = session.native_surface_placement(before[0]).unwrap();
    let b = session.native_surface_placement(before[1]).unwrap();
    let incoming =
        NativeSurfacePlacement::new(INCOMING, b.size(), b.origin(), b.scale_factor(), b.clip());
    let mut frame = native_frame(&mut session);
    assert_eq!(
        frame
            .set_native_surface_placements(window, &[a, b, incoming])
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    drop(frame);

    let mut frame = native_frame(&mut session);
    frame
        .set_native_surface_placements(window, &[a, incoming])
        .expect("an immediately obsolete placement must free its producer slot");
    let proposed: Vec<_> = frame.native_surface_bindings_for_window(window).collect();
    assert_eq!(proposed[0], before[0]);
    assert_eq!(proposed[1].surface(), INCOMING);
    drop(frame);

    assert_eq!(
        session
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        before
    );
    let mut frame = native_frame(&mut session);
    frame
        .set_native_surface_placements(window, &[a, incoming])
        .unwrap();
    assert_eq!(
        frame
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        proposed
    );
    assert!(finish(frame).take_native_effects().is_empty());
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
    assert!(!session.recognizes_native_binding(before[1]));
    assert!(session.is_current_native_binding(proposed[1]));
}

#[test]
fn shared_retirement_drains_original_physical_callbacks_after_candidate_drop_and_exact_retry() {
    let (mut session, window, bindings) = shared_session(&[A, B]);
    retire(&mut session, window, &bindings);
    let version = session.version();
    let before = format!("{:?}", session.view());
    session
        .report_managed_native_snapshot([(window, facts())], NativeWorkAreaRoster::Unknown)
        .expect("the original physical callback must still reach the last placement's cleanup");
    let recorded = session.native_input_recorded_count();
    let frame = native_frame(&mut session);
    drop(frame);
    assert!(session.has_pending_native_input());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.version(), version);
    let mut live = idle(&mut session);
    assert!(!live.workspace_changed());
    assert!(live.take_native_effects().is_empty());
    assert_eq!(format!("{:?}", session.view()), before);

    for _ in 0..2 {
        session
            .report_managed_native_snapshot(
                [(window, NativeWindowFacts::destroyed())],
                NativeWorkAreaRoster::Unknown,
            )
            .expect("terminal physical callbacks remain drainable until explicit quiescence");
        let mut destroyed = idle(&mut session);
        assert!(!destroyed.workspace_changed());
        assert!(destroyed.take_native_effects().is_empty());
        assert!(!destroyed.native_input_pending());
        assert_eq!(session.version(), version);
        assert_eq!(format!("{:?}", session.view()), before);
    }
}

#[test]
fn shared_retirement_releases_bounded_lifetimes_only_after_each_exact_lane_quiesces() {
    for reverse in [false, true] {
        let (mut session, window, bindings) = shared_session(&[A, B]);
        retire(&mut session, window, &bindings);
        session
            .report_managed_native_snapshot(
                [(window, NativeWindowFacts::destroyed())],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        idle(&mut session);
        let recorded = session.native_input_recorded_count();
        assert_eq!(
            session
                .register_native_root(INCOMING, TOKEN)
                .unwrap_err()
                .native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert_eq!(session.native_input_recorded_count(), recorded);
        let quiescences = if reverse {
            [bindings[1], bindings[0]]
        } else {
            [bindings[0], bindings[1]]
        };
        for binding in quiescences {
            session
                .report_native_binding_quiescence(binding)
                .expect("physical termination must permit each retired exact placement to drain");
        }
        let mut drained = idle(&mut session);
        assert!(drained.take_native_effects().is_empty());
        for &binding in &bindings {
            assert!(!session.recognizes_native_binding(binding));
        }

        session.register_native_root(INCOMING, TOKEN).unwrap();
        let registered = idle(&mut session);
        let [successor] = registered.native_bindings() else {
            panic!("quiescence must release the prepaid lifetime slot");
        };
        let successor = *successor;
        assert_ne!(successor.window(), window);
        assert_eq!(successor.surface(), INCOMING);
        let recorded = session.native_input_recorded_count();
        assert_eq!(
            session
                .report_managed_native_snapshot(
                    [
                        (successor.window(), facts()),
                        (window, NativeWindowFacts::destroyed()),
                    ],
                    NativeWorkAreaRoster::Unknown,
                )
                .unwrap_err()
                .native_kind(),
            Some(NativeHostErrorKind::StaleBinding)
        );
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(session.is_current_native_binding(successor));
        assert!(session.view().surface(INCOMING).is_some());
        assert!(session.view().item(ItemId::new(INCOMING.get())).is_some());
    }
}
