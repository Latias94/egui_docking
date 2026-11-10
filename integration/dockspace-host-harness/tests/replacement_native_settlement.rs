//! Workspace replacement preserves native settlement in the public causal report.

use std::time::Duration;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostWindowToken, NativeHostCapabilities, NativeHostCapability,
    NativeInputBudget, NativeMonotonicClock, NativePointerRoster, NativeSurfaceBinding,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const WINDOW: HostWindowToken = HostWindowToken::new(7);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn layout(items: &[ItemId], selected: ItemId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs_with_selection(items.iter().copied(), Some(selected))
                .unwrap(),
        ),
    )])
    .unwrap()
}

fn policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy
}

fn enable_native(session: &mut DockspaceSession) {
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::GlobalFocusObservation),
        )
        .unwrap();
}

fn native_frame(session: &mut DockspaceSession, expected_records: usize) -> DockspaceHostFrame<'_> {
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    let frame = session
        .poll_native_host_frame(&mut budget, |_| {
            panic!("native window facts cannot require a pointer receiver")
        })
        .unwrap()
        .expect("the complete native prefix fits in one deterministic budget");
    assert_eq!(budget.remaining_records(), 256 - expected_records);
    assert!(!frame.native_input_pending());
    frame
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    frame.commit().unwrap()
}

fn present_successor(session: &mut DockspaceSession) {
    commit_measured(native_frame(session, 0));
    let mut frame = native_frame(session, 0);
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    for output in report.take_painted_outputs() {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let recorded = session.native_input_recorded_count();
    commit_deferred(native_frame(session, 1));
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
}

fn registered(mut session: DockspaceSession) -> (DockspaceSession, NativeSurfaceBinding) {
    enable_native(&mut session);
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let records = usize::try_from(session.native_input_recorded_count()).unwrap();
    let report = commit_deferred(native_frame(&mut session, records));
    let [HostInputOutcome::NativeSurfaceRegistered { binding }] = report.inputs() else {
        panic!("the real managed root registration must publish its exact binding");
    };
    assert_eq!(report.native_bindings(), &[*binding]);
    assert_eq!(binding.surface(), SURFACE);
    assert_eq!(binding.window_token(), WINDOW);
    assert!(session.is_current_native_binding(*binding));
    assert!(!session.has_pending_native_input());
    (session, *binding)
}

fn local_facts() -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(1200.0, 900.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(2.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

#[test]
fn layout_reset_preserves_native_snapshot_settlement_and_retry_watermarks() {
    let defaults = layout(&[FIRST, SECOND], FIRST);
    let (mut session, old) =
        registered(DockspaceSession::from_layout(defaults.clone(), policy()).unwrap());
    let reset = session.prepare_layout_reset(defaults).unwrap();
    let before = session.version();
    let recorded_before = session.native_input_recorded_count();
    session
        .report_managed_native_snapshot([(old, local_facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let recorded = session.native_input_recorded_count();
    assert_eq!(recorded, recorded_before + 1);
    assert!(session.has_pending_native_input());
    assert!(session.native_local_coordinates(old).is_none());

    let mut candidate_version = None;
    let mut successor = None;
    for attempt in 0..3 {
        let mut frame = native_frame(&mut session, 1);
        frame.select_item_current(SECOND).unwrap();
        assert!(frame.view().item(SECOND).unwrap().is_selected());
        let prefix_version = frame.version();
        assert_ne!(prefix_version, before);
        frame.submit_layout_reset(&reset).unwrap();
        assert!(frame.view().item(FIRST).unwrap().is_selected());
        assert_eq!(
            *candidate_version.get_or_insert(frame.version()),
            frame.version()
        );
        match attempt {
            0 => drop(frame),
            1 => {
                frame
                    .defer_surface(SURFACE, SurfaceUnavailableReason::Deferred)
                    .unwrap();
                assert_eq!(
                    frame
                        .defer_surface(SURFACE, SurfaceUnavailableReason::Deferred)
                        .unwrap_err()
                        .kind(),
                    DockspaceRuntimeErrorKind::HostProtocol
                );
                frame
                    .commit()
                    .expect_err("a duplicate contribution must retain the snapshot prefix");
            }
            _ => {
                let report = commit_measured(frame);
                assert!(report.workspace_changed());
                assert_eq!(report.before(), before);
                assert_eq!(report.after(), session.version());
                assert_eq!(
                    report.inputs(),
                    &[
                        HostInputOutcome::NativePlatformSnapshotApplied {
                            close_requests: Vec::new(),
                        },
                        HostInputOutcome::LayoutReset {
                            previous: prefix_version,
                            current: session.version(),
                        },
                    ]
                );
                let [binding] = report.native_bindings() else {
                    panic!("reset must publish the complete native root successor roster");
                };
                assert_eq!(binding.surface(), old.surface());
                assert_eq!(binding.window_token(), old.window_token());
                assert_ne!(*binding, old);
                successor = Some(*binding);
            }
        }
        assert_eq!(session.native_input_recorded_count(), recorded);
        if attempt < 2 {
            assert_eq!(session.version(), before);
            assert!(session.view().item(FIRST).unwrap().is_selected());
            assert!(session.is_current_native_binding(old));
            assert!(session.native_local_coordinates(old).is_none());
            assert!(session.has_pending_native_input());
        }
    }

    let successor = successor.unwrap();
    assert!(!session.is_current_native_binding(old));
    assert!(session.is_current_native_binding(successor));
    assert!(session.native_local_coordinates(old).is_none());
    assert!(session.native_local_coordinates(successor).is_none());
    assert!(!session.has_pending_native_input());
    let idle = commit_deferred(native_frame(&mut session, 0));
    assert!(idle.inputs().is_empty());
    assert_eq!(idle.native_bindings(), &[successor]);
    assert_eq!(session.native_input_recorded_count(), recorded);

    // Only a fresh snapshot for the successor can establish its local authority.
    session
        .report_managed_native_snapshot([(successor, local_facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let report = commit_deferred(native_frame(&mut session, 1));
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::NativePlatformSnapshotApplied {
            close_requests: Vec::new(),
        }]
    );
    assert_eq!(report.native_bindings(), &[successor]);
    assert!(session.native_local_coordinates(successor).is_none());
    present_successor(&mut session);
    assert!(session.native_local_coordinates(successor).is_some());
    assert!(session.native_local_coordinates(old).is_none());
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
    assert!(!session.has_pending_native_input());
}

#[test]
fn native_registration_and_layout_reset_publish_the_original_binding_before_its_successor() {
    let defaults = layout(&[FIRST, SECOND], FIRST);
    let mut session = DockspaceSession::from_layout(defaults.clone(), policy()).unwrap();
    enable_native(&mut session);
    let reset = session.prepare_layout_reset(defaults).unwrap();
    let before = session.version();
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let recorded = session.native_input_recorded_count();
    let mut frame = native_frame(&mut session, usize::try_from(recorded).unwrap());
    frame.select_item_current(SECOND).unwrap();
    let prefix_version = frame.version();
    frame.submit_layout_reset(&reset).unwrap();
    let report = commit_measured(frame);
    let [
        HostInputOutcome::NativeSurfaceRegistered { binding: old },
        HostInputOutcome::LayoutReset { previous, current },
    ] = report.inputs()
    else {
        panic!("registration and replacement must share one report in core causal order");
    };
    assert_eq!(*previous, prefix_version);
    assert_eq!(*current, session.version());
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), session.version());
    let [successor] = report.native_bindings() else {
        panic!("the complete roster must contain the registered root's successor");
    };
    assert_eq!(old.surface(), SURFACE);
    assert_eq!(old.window_token(), WINDOW);
    assert_eq!(successor.surface(), old.surface());
    assert_eq!(successor.window_token(), old.window_token());
    assert_ne!(successor, old);
    assert!(!session.is_current_native_binding(*old));
    assert!(session.is_current_native_binding(*successor));
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(!session.has_pending_native_input());
}

#[test]
fn retained_root_predecessors_quiesce_through_the_original_native_prefix_after_repeated_resets() {
    let defaults = layout(&[FIRST, SECOND], FIRST);
    let (mut session, mut predecessor) =
        registered(DockspaceSession::from_layout(defaults.clone(), policy()).unwrap());
    for _ in 0..3 {
        let reset = session.prepare_layout_reset(defaults.clone()).unwrap();
        let mut frame = native_frame(&mut session, 0);
        frame.submit_layout_reset(&reset).unwrap();
        let report = commit_deferred(frame);
        let [successor] = report.native_bindings() else {
            panic!("one retained root must publish its exact successor");
        };
        let successor = *successor;
        assert_ne!(successor, predecessor);
        assert_eq!(successor.window_token(), predecessor.window_token());
        assert!(session.recognizes_native_binding(predecessor));
        let version = session.version();
        session
            .report_native_binding_quiescence(predecessor)
            .unwrap();
        assert!(!session.recognizes_native_binding(predecessor));
        let recorded = session.native_input_recorded_count();
        drop(native_frame(&mut session, 1));
        assert!(session.has_pending_native_input());
        assert_eq!(session.version(), version);
        commit_deferred(native_frame(&mut session, 1));
        // The next frame retires the exact committed quiescence prefix and guard.
        let idle = commit_deferred(native_frame(&mut session, 0));
        assert!(idle.inputs().is_empty());
        assert_eq!(idle.native_bindings(), &[successor]);
        assert_eq!(session.version(), version);
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(!session.has_pending_native_input());
        assert!(session.is_current_native_binding(successor));
        assert!(!session.recognizes_native_binding(predecessor));
        predecessor = successor;
    }
}

#[cfg(feature = "serde")]
mod document_restore {
    use super::*;
    use dockspace::runtime::{
        DockspaceDocumentBootstrap, DockspaceDocumentId, NativeGlobalFocus, NativeHostErrorKind,
    };

    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x73; 16]);
    const NEW: ItemId = ItemId::new(3);

    fn bootstrap() -> DockspaceDocumentBootstrap {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), FIRST);
        assert_eq!(bootstrap.ensure_item("pane:second").unwrap(), SECOND);
        bootstrap
    }

    fn saved_document() -> Vec<u8> {
        let mut bootstrap = bootstrap();
        assert_eq!(bootstrap.ensure_item("pane:new").unwrap(), NEW);
        let mut source = DockspaceSession::from_persistent_layout(
            layout(&[FIRST, NEW], NEW),
            policy(),
            bootstrap,
        )
        .unwrap();
        source.save_document_json().unwrap()
    }

    #[test]
    fn document_restore_preserves_native_snapshot_and_focus_without_authorizing_late_old_facts() {
        let bytes = saved_document();
        let (mut session, old) = registered(
            DockspaceSession::from_persistent_layout(
                layout(&[FIRST, SECOND], FIRST),
                policy(),
                bootstrap(),
            )
            .unwrap(),
        );
        let restore = session
            .prepare_document_restore_json(&bytes, |document, key| {
                document == DOCUMENT && matches!(key, "pane:first" | "pane:second" | "pane:new")
            })
            .unwrap();
        let before = session.version();
        let generation = session.next_document_generation();
        let recorded_before = session.native_input_recorded_count();
        session
            .report_managed_native_snapshot([(old, local_facts())], NativeWorkAreaRoster::Unknown)
            .unwrap();
        session
            .report_native_global_focus(NativeGlobalFocus::Dock(old))
            .unwrap();
        let recorded = session.native_input_recorded_count();
        assert_eq!(recorded, recorded_before + 2);
        assert_eq!(session.item_id_for_external_key("pane:new"), None);

        let mut successor = None;
        for attempt in 0..2 {
            let mut frame = native_frame(&mut session, 2);
            assert_eq!(frame.external_key_for_item(NEW), None);
            frame.select_item_current(SECOND).unwrap();
            assert!(frame.view().item(SECOND).unwrap().is_selected());
            let prefix_version = frame.version();
            assert_ne!(prefix_version, before);
            frame.submit_document_restore(&restore).unwrap();
            assert_eq!(frame.external_key_for_item(NEW), Some("pane:new"));
            assert!(frame.view().item(NEW).unwrap().is_selected());
            assert!(frame.view().item(SECOND).is_none());
            if attempt == 0 {
                drop(frame);
                assert_eq!(session.version(), before);
                assert_eq!(session.next_document_generation(), generation);
                assert_eq!(session.item_id_for_external_key("pane:new"), None);
                assert!(session.view().item(NEW).is_none());
                assert!(session.view().item(FIRST).unwrap().is_selected());
                assert!(session.is_current_native_binding(old));
                assert!(session.native_local_coordinates(old).is_none());
                assert!(session.has_pending_native_input());
            } else {
                let report = commit_measured(frame);
                assert!(report.workspace_changed());
                assert_eq!(report.before(), before);
                assert_eq!(report.after(), session.version());
                assert_eq!(
                    report.inputs(),
                    &[
                        HostInputOutcome::NativePlatformSnapshotApplied {
                            close_requests: Vec::new(),
                        },
                        HostInputOutcome::NativeFocusObservationApplied,
                        HostInputOutcome::DocumentRestored {
                            previous: prefix_version,
                            current: session.version(),
                        },
                    ]
                );
                let [binding] = report.native_bindings() else {
                    panic!("restore must publish the complete native root successor roster");
                };
                assert_eq!(binding.surface(), old.surface());
                assert_eq!(binding.window_token(), old.window_token());
                assert_ne!(*binding, old);
                successor = Some(*binding);
            }
            assert_eq!(session.native_input_recorded_count(), recorded);
        }

        let successor = successor.unwrap();
        assert_eq!(session.document_id(), Some(DOCUMENT));
        assert_eq!(session.item_id_for_external_key("pane:new"), Some(NEW));
        assert_eq!(session.external_key_for_item(NEW), Some("pane:new"));
        assert!(session.view().item(NEW).unwrap().is_selected());
        assert!(!session.is_current_native_binding(old));
        assert!(session.is_current_native_binding(successor));
        assert!(!session.native_binding_can_report_focus(old));
        assert!(!session.native_binding_can_report_focus(successor));
        assert!(session.native_local_coordinates(successor).is_none());
        assert!(!session.has_pending_native_input());

        // A callback captured before replacement cannot lend authority to its successor.
        let error = session
            .report_native_global_focus(NativeGlobalFocus::Dock(old))
            .unwrap_err();
        assert_eq!(error.native_kind(), Some(NativeHostErrorKind::StaleBinding));
        session
            .report_managed_native_snapshot([(old, local_facts())], NativeWorkAreaRoster::Unknown)
            .expect_err("a late live snapshot for the old binding must be rejected atomically");
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert!(!session.has_pending_native_input());
        let idle = commit_deferred(native_frame(&mut session, 0));
        assert!(idle.inputs().is_empty());
        assert_eq!(idle.native_bindings(), &[successor]);
        assert!(!session.native_binding_can_report_focus(successor));
        assert!(session.native_local_coordinates(successor).is_none());

        session
            .report_managed_native_snapshot(
                [(successor, local_facts())],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        session
            .report_native_global_focus(NativeGlobalFocus::Dock(successor))
            .unwrap();
        let report = commit_deferred(native_frame(&mut session, 2));
        assert_eq!(
            report.inputs(),
            &[
                HostInputOutcome::NativePlatformSnapshotApplied {
                    close_requests: Vec::new(),
                },
                HostInputOutcome::NativeFocusObservationApplied,
            ]
        );
        assert_eq!(report.native_bindings(), &[successor]);
        assert!(session.native_binding_can_report_focus(successor));
        assert!(session.native_local_coordinates(successor).is_none());
        present_successor(&mut session);
        assert!(session.native_local_coordinates(successor).is_some());
        assert!(!session.native_binding_can_report_focus(old));
        assert!(session.native_local_coordinates(old).is_none());
        assert_eq!(session.native_input_recorded_count(), recorded + 3);
        assert!(!session.has_pending_native_input());
    }
}
