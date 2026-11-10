//! Terminal document restoration through ordinary and budgeted public host frames.

#![cfg(feature = "serde")]

use std::cell::Cell;
use std::time::Duration;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceHostFrame,
    DockspaceRuntimeErrorKind, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostWindowToken, NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputBudget,
    NativeMonotonicClock, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x74; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(10);
const FIRST: ItemId = ItemId::new(1);
const HISTORICAL: ItemId = ItemId::new(2);
const MISSING: ItemId = ItemId::new(3);

fn layout(root: RootId, items: &[ItemId], selected: ItemId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            root,
            DockspaceNode::central_tabs_with_selection(items.iter().copied(), Some(selected))
                .unwrap(),
        ),
    )])
    .unwrap()
}

fn bootstrap() -> DockspaceDocumentBootstrap {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), FIRST);
    assert_eq!(
        bootstrap.ensure_item("pane:historical").unwrap(),
        HISTORICAL
    );
    bootstrap
}

fn target() -> DockspaceSession {
    let mut session = DockspaceSession::from_persistent_layout(
        layout(ROOT, &[FIRST], FIRST),
        DockPolicy::default(),
        bootstrap(),
    )
    .unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    session
}

fn saved_document() -> Vec<u8> {
    let mut bootstrap = bootstrap();
    assert_eq!(bootstrap.ensure_item("pane:missing").unwrap(), MISSING);
    let mut source = DockspaceSession::from_persistent_layout(
        layout(RootId::new(1), &[FIRST, MISSING], MISSING),
        DockPolicy::default(),
        bootstrap,
    )
    .unwrap();
    source.save_document_json().unwrap();
    let bytes = source.save_document_json().unwrap();
    assert_eq!(source.next_document_generation(), Some(2));
    bytes
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && matches!(key, "pane:first" | "pane:historical" | "pane:missing")
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
        LogicalSize::new(48.0, 32.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    frame.commit().unwrap()
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn assert_original(session: &DockspaceSession) {
    assert_eq!(session.document_id(), Some(DOCUMENT));
    assert_eq!(session.view().surfaces().count(), 1);
    let first = session.view().item(FIRST).unwrap();
    assert_eq!((first.surface(), first.root()), (SURFACE, ROOT));
    assert_eq!(first.tabs().items(), &[FIRST]);
    assert!(first.is_selected());
    assert!(session.view().item(HISTORICAL).is_none());
    assert!(session.view().item(MISSING).is_none());
    assert_eq!(session.item_id_for_external_key("pane:first"), Some(FIRST));
    assert_eq!(
        session.item_id_for_external_key("pane:historical"),
        Some(HISTORICAL)
    );
    assert_eq!(session.item_id_for_external_key("pane:missing"), None);
    assert_eq!(session.external_key_for_item(MISSING), None);
}

#[test]
fn ordinary_terminal_restore_retries_atomically_and_recognizes_unloaded_content() {
    let bytes = saved_document();
    let mut session = target();
    let before = session.version();
    let recognitions = Cell::new(0);
    let prepared = session
        .prepare_document_restore_json(&bytes, |document, key| {
            recognitions.set(recognitions.get() + 1);
            recognize(document, key)
        })
        .unwrap();
    let recognition_count = recognitions.get();
    assert_eq!(recognition_count, 3);
    assert_original(&session);

    // Incoming identity authority must not reach the ordinary input prefix.
    {
        let mut prefix = session.begin_host_frame().unwrap();
        assert_eq!(prefix.external_key_for_item(MISSING), None);
        assert_eq!(
            prefix
                .open_item_current(MISSING, DockPlacement::After(FIRST))
                .unwrap_err()
                .kind(),
            DockspaceRuntimeErrorKind::HostProtocol
        );
    }
    assert_eq!(session.version(), before);
    assert_eq!(session.next_document_generation(), Some(0));
    assert_original(&session);

    let mut candidate_root = None;
    for attempt in 0..3 {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .open_item_current(HISTORICAL, DockPlacement::After(FIRST))
            .unwrap();
        assert!(frame.view().item(HISTORICAL).unwrap().is_selected());
        assert_eq!(frame.external_key_for_item(MISSING), None);
        let prefix_version = frame.version();
        assert_ne!(prefix_version, before);
        frame.submit_document_restore(&prepared).unwrap();
        let restored = frame.view().item(MISSING).unwrap();
        assert_eq!(restored.surface(), SURFACE);
        assert_eq!(restored.root(), RootId::new(1));
        assert_eq!(restored.tabs().items(), &[FIRST, MISSING]);
        assert!(restored.is_selected());
        assert!(frame.view().item(HISTORICAL).is_none());
        assert_eq!(frame.external_key_for_item(MISSING), Some("pane:missing"));
        assert_eq!(
            frame.external_key_for_item(HISTORICAL),
            Some("pane:historical")
        );
        assert_eq!(
            *candidate_root.get_or_insert(restored.root()),
            restored.root()
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
                    .expect_err("a duplicate contribution must reject the whole publication");
            }
            _ => {
                // Identity recognition needs no application content object or pane UI.
                let report = commit_measured(frame);
                assert!(report.workspace_changed());
                assert_eq!(report.before(), before);
                assert_eq!(report.after(), session.version());
                assert_eq!(
                    report.inputs(),
                    &[HostInputOutcome::DocumentRestored {
                        previous: prefix_version,
                        current: session.version(),
                    }]
                );
            }
        }
        assert_eq!(recognitions.get(), recognition_count);
        if attempt < 2 {
            assert_eq!(session.version(), before);
            assert_eq!(session.next_document_generation(), Some(0));
            assert_original(&session);
        }
    }
    assert_eq!(session.document_id(), Some(DOCUMENT));
    assert_eq!(session.next_document_generation(), Some(2));
    assert_eq!(
        session.item_id_for_external_key("pane:missing"),
        Some(MISSING)
    );
    assert_eq!(session.external_key_for_item(MISSING), Some("pane:missing"));
    assert_eq!(
        session.view().item(MISSING).unwrap().root(),
        candidate_root.unwrap()
    );

    // A later restore must retain keys and generation history absent from its saved document.
    let local = session.ensure_external_item("pane:local").unwrap();
    assert_eq!(local.get(), MISSING.get() + 1);
    session.save_document_json().unwrap();
    let prepared = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(local, DockPlacement::After(FIRST))
        .unwrap();
    frame.submit_document_restore(&prepared).unwrap();
    assert!(frame.view().item(local).is_none());
    assert_eq!(frame.external_key_for_item(local), Some("pane:local"));
    commit_measured(frame);
    assert_eq!(session.next_document_generation(), Some(3));
    assert_eq!(session.item_id_for_external_key("pane:local"), Some(local));
    assert_eq!(session.external_key_for_item(local), Some("pane:local"));
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(local, DockPlacement::After(FIRST))
        .unwrap();
    assert!(frame.view().item(local).unwrap().is_selected());
    drop(frame);
    assert!(session.view().item(local).is_none());
    assert_eq!(
        session.ensure_external_item("pane:next").unwrap().get(),
        local.get() + 1
    );
}

#[test]
fn bounded_native_prefix_and_terminal_restore_share_retry_watermarks_and_root_successor() {
    let bytes = saved_document();
    let mut session = target();
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
        panic!("the public native root registration must publish its binding");
    };
    let old = *old;
    let prepared = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let before = session.version();
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
        .poll_native_host_frame(&mut budget, |_| unreachable!())
        .unwrap()
        .expect("one complete independent record must seal within its record budget");
    assert_eq!(budget.remaining_records(), 0);
    assert!(prefix.native_input_pending());
    assert_eq!(prefix.external_key_for_item(MISSING), None);
    assert_eq!(
        prefix
            .submit_document_restore(&prepared)
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert!(commit_deferred(prefix).native_input_pending());
    assert_eq!(session.version(), before);
    assert!(session.is_current_native_binding(old));
    assert_eq!(session.next_document_generation(), Some(0));
    assert_original(&session);

    session.record_native_pointer_batch([movement; 2]).unwrap();
    let recorded = session.native_input_recorded_count();
    // Pause after the independent tail and the first edge of the atomic pair.
    let mut budget = NativeInputBudget::new(&clock);
    budget.complete_unsequenced_records(254);
    assert!(
        session
            .poll_native_host_frame(&mut budget, |_| unreachable!())
            .unwrap()
            .is_none()
    );
    assert_eq!(budget.remaining_records(), 0);
    assert_eq!(session.version(), before);
    assert!(session.has_pending_native_input());
    assert_original(&session);

    let mut candidate_root = None;
    let mut successor = None;
    for attempt in 0..3 {
        let mut budget = NativeInputBudget::new(&clock);
        let mut frame = session
            .poll_native_host_frame(&mut budget, |_| unreachable!())
            .unwrap()
            .expect("the complete native suffix fits in one record budget");
        // Resume only the last edge; drop and failed commit must replay all three.
        assert_eq!(
            budget.remaining_records(),
            if attempt == 0 { 255 } else { 253 }
        );
        assert!(!frame.native_input_pending());
        assert_eq!(frame.external_key_for_item(MISSING), None);
        frame
            .open_item_current(HISTORICAL, DockPlacement::After(FIRST))
            .unwrap();
        let prefix_version = frame.version();
        frame.submit_document_restore(&prepared).unwrap();
        let root = frame.view().item(MISSING).unwrap().root();
        assert_eq!(*candidate_root.get_or_insert(root), root);
        assert_eq!(frame.external_key_for_item(MISSING), Some("pane:missing"));
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
                    .expect_err("a failed restore commit must retain the native input suffix");
            }
            _ => {
                let report = commit_measured(frame);
                assert!(!report.native_input_pending());
                assert_eq!(report.before(), before);
                assert_eq!(report.after(), session.version());
                assert_eq!(
                    report.inputs(),
                    &[HostInputOutcome::DocumentRestored {
                        previous: prefix_version,
                        current: session.version(),
                    }]
                );
                let [binding] = report.native_bindings() else {
                    panic!("restore must publish the complete native root successor roster");
                };
                assert_eq!(binding.surface(), SURFACE);
                assert_ne!(*binding, old);
                successor = Some(*binding);
            }
        }
        assert_eq!(session.native_input_recorded_count(), recorded);
        if attempt < 2 {
            assert!(session.has_pending_native_input());
            assert!(session.is_current_native_binding(old));
            assert_eq!(session.version(), before);
            assert_eq!(session.next_document_generation(), Some(0));
            assert_original(&session);
        }
    }
    let successor = successor.unwrap();
    assert!(!session.has_pending_native_input());
    assert!(!session.is_current_native_binding(old));
    assert!(session.is_current_native_binding(successor));
    assert_eq!(session.document_id(), Some(DOCUMENT));
    assert_eq!(session.next_document_generation(), Some(2));
    assert_eq!(
        session.item_id_for_external_key("pane:missing"),
        Some(MISSING)
    );
    assert_eq!(session.external_key_for_item(MISSING), Some("pane:missing"));
    assert_eq!(
        session.view().item(MISSING).unwrap().root(),
        candidate_root.unwrap()
    );
    assert!(session.view().item(MISSING).unwrap().is_selected());
    let idle = commit_deferred(session.begin_native_host_frame(|_| unreachable!()).unwrap());
    assert!(idle.inputs().is_empty());
    assert!(!idle.native_input_pending());
    assert_eq!(idle.native_bindings(), &[successor]);
    assert_eq!(session.native_input_recorded_count(), recorded);
}

#[test]
fn foreign_stale_and_changed_sidecar_preparations_reject_without_publishing_the_prefix() {
    enum Invalidation {
        ForeignSession,
        SemanticCommit,
        Save,
        NewKey,
    }

    let bytes = saved_document();
    for invalidation in [
        Invalidation::ForeignSession,
        Invalidation::SemanticCommit,
        Invalidation::Save,
        Invalidation::NewKey,
    ] {
        let mut session = target();
        let prepared = match invalidation {
            Invalidation::ForeignSession => target()
                .prepare_document_restore_json(&bytes, recognize)
                .unwrap(),
            _ => session
                .prepare_document_restore_json(&bytes, recognize)
                .unwrap(),
        };
        let prepared_version = session.version();
        match invalidation {
            Invalidation::ForeignSession => {}
            Invalidation::SemanticCommit => {
                let mut frame = session.begin_host_frame().unwrap();
                frame
                    .open_item_current(HISTORICAL, DockPlacement::After(FIRST))
                    .unwrap();
                commit_measured(frame);
                assert_ne!(session.version(), prepared_version);
            }
            Invalidation::Save => {
                session.save_document_json().unwrap();
                assert_eq!(session.version(), prepared_version);
                assert_eq!(session.next_document_generation(), Some(1));
            }
            Invalidation::NewKey => {
                session.ensure_external_item("pane:later").unwrap();
                assert_eq!(session.version(), prepared_version);
            }
        }
        let before = session.version();
        let generation = session.next_document_generation();
        let later = session.item_id_for_external_key("pane:later");
        let first = session.view().item(FIRST).unwrap();
        let items = first.tabs().items().to_vec();
        let selected = first.tabs().selected();
        {
            let mut frame = session.begin_host_frame().unwrap();
            if selected == Some(FIRST) {
                frame
                    .open_item_current(HISTORICAL, DockPlacement::After(FIRST))
                    .unwrap();
            } else {
                frame.select_item_current(FIRST).unwrap();
            }
            assert_ne!(frame.version(), before);
            assert_eq!(
                frame.submit_document_restore(&prepared).unwrap_err().kind(),
                DockspaceRuntimeErrorKind::OperationConflict
            );
            assert_eq!(frame.view().item(FIRST).unwrap().root(), ROOT);
        }
        assert_eq!(session.version(), before);
        assert_eq!(session.document_id(), Some(DOCUMENT));
        assert_eq!(session.next_document_generation(), generation);
        assert_eq!(session.view().item(FIRST).unwrap().tabs().items(), items);
        assert_eq!(
            session.view().item(FIRST).unwrap().tabs().selected(),
            selected
        );
        assert_eq!(session.view().item(FIRST).unwrap().root(), ROOT);
        assert!(session.view().item(MISSING).is_none());
        assert_eq!(session.item_id_for_external_key("pane:later"), later);
        if let Some(later) = later {
            assert_eq!(session.external_key_for_item(later), Some("pane:later"));
        }
        assert_eq!(session.item_id_for_external_key("pane:missing"), None);
        assert_eq!(session.external_key_for_item(FIRST), Some("pane:first"));
        assert_eq!(
            session.external_key_for_item(HISTORICAL),
            Some("pane:historical")
        );
    }
}

#[test]
fn locked_restore_rejects_and_successful_restore_closes_semantic_and_configuration_input() {
    let bytes = saved_document();
    let mut session = target();
    let before_lock = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut locked = session.policy().clone();
    locked.set_arrangement_locked(true);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(locked).unwrap();
    commit_deferred(frame);
    let locked_version = session.version();
    let while_locked = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    for prepared in [&before_lock, &while_locked] {
        let mut frame = session.begin_host_frame().unwrap();
        assert_eq!(
            frame.submit_document_restore(prepared).unwrap_err().kind(),
            DockspaceRuntimeErrorKind::OperationConflict
        );
        drop(frame);
        assert!(session.policy().arrangement_locked());
        assert_eq!(session.version(), locked_version);
        assert_eq!(session.next_document_generation(), Some(0));
        assert_original(&session);
    }

    let mut unlocked = session.policy().clone();
    unlocked.set_arrangement_locked(false);
    let mut frame = session.begin_host_frame().unwrap();
    frame.replace_policy(unlocked.clone()).unwrap();
    commit_deferred(frame);
    let prepared = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_document_restore(&prepared).unwrap();
    assert_eq!(
        frame.select_item_current(FIRST).unwrap_err().kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert_eq!(
        frame
            .request_close_item_current(MISSING)
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert_eq!(
        frame.replace_policy(unlocked).unwrap_err().kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert_eq!(
        frame
            .replace_presentation_config(DockPresentationConfig::default())
            .unwrap_err()
            .kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert_eq!(
        frame.submit_document_restore(&prepared).unwrap_err().kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    let report = commit_measured(frame);
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::DocumentRestored { .. }]
    ));
    assert!(!session.policy().arrangement_locked());
    assert!(session.view().item(MISSING).unwrap().is_selected());
    assert_eq!(session.next_document_generation(), Some(2));
}
