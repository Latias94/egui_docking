#![cfg(feature = "serde")]

use std::cell::Cell;

use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceErrorKind,
    DockspaceRuntimeErrorKind, DockspaceSession, HostInputOutcome, HostWindowToken,
    NativeHostErrorKind, NativePointerRoster, NativeReceiverAnswer, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerInput, SurfacePointerPosition,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x73; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const SECOND_ROOT: RootId = RootId::new(2);

fn layout(items: impl IntoIterator<Item = ItemId>) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs(items)),
    )])
    .expect("the persistence fixture layout is valid")
}

fn two_surface_layout(first: ItemId, second: ItemId) -> DockspaceLayout {
    DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([first])),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(SECOND_ROOT, DockspaceNode::central_tabs([second])),
        ),
    ])
    .expect("the two-surface persistence layout is valid")
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 960.0, 640.0).expect("test bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("test minimum validates"),
        96.0,
    )
    .expect("test metrics validate")
}

fn saved_document_with_two_items() -> (Vec<u8>, ItemId, ItemId) {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let mut source = DockspaceSession::from_persistent_layout(
        layout([first, second]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the source session builds");
    let bytes = source
        .save_document_json()
        .expect("the source document encodes");
    (bytes, first, second)
}

#[test]
fn malformed_json_preserves_parser_diagnostics_in_the_public_error_chain() {
    use std::error::Error;

    let error = DockspaceSession::from_document_json(
        b"[1,\n{\"workspace\": }\n]",
        DockPolicy::default(),
        |_, _| true,
    )
    .unwrap_err();
    assert_eq!(error.kind(), DockspacePersistenceErrorKind::InvalidDocument);
    let parser = error
        .source()
        .and_then(|source| source.downcast_ref::<serde_json::Error>())
        .expect("applications must retain the parser's typed position diagnostics");
    assert_eq!(parser.classify(), serde_json::error::Category::Syntax);
    assert_eq!(parser.line(), 2);
    assert!(parser.column() > 0);
}

#[test]
fn document_uses_one_schema_and_requires_explicit_null_fields_before_recognition() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let item = bootstrap.ensure_item("pane:first").unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([item])),
    )])
    .unwrap();
    let mut session =
        DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap();
    let saved = session.save_document_json().unwrap();
    let mut wire: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(wire[0], 1);
    for component in ["workspace", "external_item_keys", "viewport_placements"] {
        assert!(wire[1][component].is_object());
    }
    assert_eq!(
        wire[1]["workspace"]["roots"][0]["central"],
        serde_json::Value::Null
    );
    let restored =
        DockspaceSession::from_document_json(&saved, DockPolicy::default(), |document, key| {
            document == DOCUMENT && key == "pane:first"
        })
        .unwrap();
    assert!(restored.view().item(item).is_some());

    wire[1]["workspace"]["roots"][0]
        .as_object_mut()
        .unwrap()
        .remove("central")
        .unwrap();
    let incomplete = serde_json::to_vec(&wire).unwrap();
    let before = session.version();
    let generation = session.next_document_generation();
    let recognitions = Cell::new(0);
    let recognize = |_: DockspaceDocumentId, _: &str| {
        recognitions.set(recognitions.get() + 1);
        true
    };
    let error = DockspaceSession::from_document_json(&incomplete, DockPolicy::default(), recognize)
        .expect_err("a missing null field cannot be normalized into a complete document");
    assert_eq!(error.kind(), DockspacePersistenceErrorKind::InvalidDocument);
    let error = session
        .prepare_document_restore_json(&incomplete, recognize)
        .expect_err("an incomplete document must fail before live restore preparation");
    assert_eq!(error.kind(), DockspacePersistenceErrorKind::InvalidDocument);
    assert_eq!(recognitions.get(), 0);
    assert_eq!(session.version(), before);
    assert_eq!(session.next_document_generation(), generation);
    assert!(session.view().item(item).is_some());
}

fn persistent_session_with_first_item() -> (DockspaceSession, ItemId) {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let session =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the target session builds");
    (session, first)
}

fn present_surface_for_pointer(session: &mut DockspaceSession) {
    let mut measure = session
        .begin_host_frame()
        .expect("the measurement frame begins");
    measure
        .measure_surface(SURFACE, metrics())
        .expect("the pointer fixture measures");
    measure.commit().expect("the measurement frame commits");

    let mut paint = session.begin_host_frame().expect("the paint frame begins");
    let receipt = paint
        .paint_plan(SURFACE)
        .expect("the paint phase opens")
        .expect("the pointer fixture surface is paintable")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("the pointer fixture executes the complete surface program");
    paint
        .confirm_surface_painted(receipt)
        .expect("the exact surface output was painted");
    let mut report = paint.commit().expect("the paint frame commits");
    let mut outputs = report.take_painted_outputs();
    assert_eq!(
        outputs.len(),
        1,
        "the pointer fixture must produce one output"
    );
    let output = outputs.pop().expect("one output was asserted");
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the output presentation is recorded");
    let mut settle = session
        .begin_host_frame()
        .expect("the presentation settlement frame begins");
    settle
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the settlement frame retains the ready surface");
    settle.commit().expect("the presentation settles");
}

#[test]
fn candidate_frame_resolves_session_owned_external_keys() {
    let (mut session, first) = persistent_session_with_first_item();
    let published = session.version();
    let frame = session
        .begin_host_frame()
        .expect("the candidate frame begins");

    assert_eq!(frame.version(), published);
    assert_eq!(frame.external_key_for_item(first), Some("pane:first"));
}

#[test]
fn constructible_contained_rects_round_trip_without_non_finite_wire_values() {
    use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId};

    let mut restored_rects = 0;
    for (minimum, maximum) in [
        ((-f64::MAX, 0.0), (f64::MAX, 1.0)),
        ((0.0, -f64::MAX), (1.0, f64::MAX)),
        ((-f64::MAX, -f64::MAX), (0.0, 0.0)),
    ] {
        let Ok(rect) = LogicalRect::from_min_max(
            LogicalPoint::new(minimum.0, minimum.1).unwrap(),
            LogicalPoint::new(maximum.0, maximum.1).unwrap(),
        ) else {
            continue;
        };
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        let item = bootstrap.ensure_item("pane:first").unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::rootless(SURFACE)
            .with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(1),
                DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([item])),
                rect,
            ))])
        .unwrap();
        let mut session =
            DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap)
                .unwrap();
        let encoded = session.save_document_json().unwrap();
        let restored = DockspaceSession::from_document_json(
            &encoded,
            DockPolicy::default(),
            |document, key| document == DOCUMENT && key == "pane:first",
        )
        .expect("a successfully saved validated layout must remain restorable");
        assert_eq!(restored.view().item(item).unwrap().surface(), SURFACE);
        restored_rects += 1;
    }
    assert_eq!(
        restored_rects, 1,
        "the largest finite span remains supported"
    );
}

#[test]
fn restore_keeps_document_owned_item_ids_when_the_application_recognizes_keys() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let second = bootstrap.ensure_item("pane:second").expect("second item");
    let historical = bootstrap
        .ensure_item("pane:historical")
        .expect("historical item");
    let mut source = DockspaceSession::from_persistent_layout(
        layout([first, second]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the persistent session builds");
    let bytes = source
        .save_document_json()
        .expect("the complete document encodes");

    let restored =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second" | "pane:historical")
        })
        .expect("recognized application keys restore");

    assert_eq!(restored.item_id_for_external_key("pane:first"), Some(first));
    assert_eq!(
        restored.item_id_for_external_key("pane:second"),
        Some(second)
    );
    assert_eq!(
        restored.item_id_for_external_key("pane:historical"),
        Some(historical)
    );
}

#[test]
fn restore_rejects_an_unrecognized_application_key() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let mut source =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the persistent session builds");
    let bytes = source
        .save_document_json()
        .expect("the complete document encodes");

    let error =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), |_document, _key| {
            false
        })
        .expect_err("an unrecognized key must reject the complete document");

    assert_eq!(
        error.kind(),
        DockspacePersistenceErrorKind::IdentityConflict
    );
}

#[test]
fn live_restore_atomically_publishes_document_state_and_stales_prior_actions() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);
    let stale = target.prepare_select_item(first);
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the same-lineage document prepares");

    let mut restore = target
        .begin_document_restore_frame(&prepared)
        .expect("the same-lineage restore frame begins");
    assert!(restore.view().item(second).is_some());
    assert_eq!(restore.external_key_for_item(second), Some("pane:second"));
    let error = restore
        .select_item_current(first)
        .expect_err("document replacement must remain the final semantic mutation");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored surface measures");
    let report = restore.commit().expect("the complete restore publishes");
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::DocumentRestored { .. }]
    ));

    assert!(target.view().item(second).is_some());
    assert_eq!(target.item_id_for_external_key("pane:second"), Some(second));
    assert_eq!(target.next_document_generation(), Some(1));

    let mut frame = target.begin_host_frame().expect("the next frame begins");
    frame
        .submit_prepared_action(stale)
        .expect("the same-session stale action is structurally accepted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the restored ready candidate is retained");
    let report = frame.commit().expect("the stale-action frame commits");
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
}

#[test]
fn dropping_live_restore_frame_preserves_published_document_state() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);
    let recognitions = Cell::new(0);
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            recognitions.set(recognitions.get() + 1);
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the rollbackable restore prepares once");
    let recognition_count = recognitions.get();

    {
        let restore = target
            .begin_document_restore_frame(&prepared)
            .expect("the rollbackable restore frame begins");
        assert!(restore.view().item(second).is_some());
    }

    assert!(target.view().item(first).is_some());
    assert!(target.view().item(second).is_none());
    assert_eq!(target.item_id_for_external_key("pane:second"), None);
    assert_eq!(target.next_document_generation(), Some(0));
    assert_eq!(recognitions.get(), recognition_count);

    let mut retry = target
        .begin_document_restore_frame(&prepared)
        .expect("the exact prepared candidate remains retryable");
    retry
        .measure_surface(SURFACE, metrics())
        .expect("the retried surface measures");
    retry.commit().expect("the exact retry commits");
    assert_eq!(recognitions.get(), recognition_count);
    assert!(target.view().item(second).is_some());
}

#[test]
fn prepared_restore_stales_when_document_sidecars_change() {
    let (bytes, _, _) = saved_document_with_two_items();
    let (mut target, _) = persistent_session_with_first_item();
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the document restore prepares");
    target
        .ensure_external_item("pane:later")
        .expect("the live document sidecar advances");

    let error = match target.begin_document_restore_frame(&prepared) {
        Ok(_) => panic!("a prepared restore must not overwrite newer key history"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
}

#[test]
fn live_restore_rejects_non_empty_surface_pointer_input_before_advancing_it() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);
    present_surface_for_pointer(&mut target);
    target
        .enable_surface_pointer(SURFACE)
        .expect("the presented surface enables pointer input");
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the document restore prepares");

    let mut restore = target
        .begin_document_restore_frame(&prepared)
        .expect("the restore frame begins");
    let error = restore
        .submit_surface_pointer(SurfacePointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::Moved,
            SurfacePointerPosition::Known(
                LogicalPoint::new(16.0, 16.0).expect("the pointer position validates"),
            ),
            SurfacePointerCapture::Unknown,
            SurfacePointerReceiverFacts::unknown(),
        ))
        .expect_err("document replacement must reject later physical input");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restore remains committable after the rejected edge");
    restore.commit().expect("the guarded restore commits");
    assert!(target.view().item(second).is_some());
}

#[test]
fn live_restore_rejects_conflicting_key_history_atomically() {
    let (bytes, source_first, source_second) = saved_document_with_two_items();
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let target_second = bootstrap.ensure_item("pane:second").expect("second item");
    let target_first = bootstrap.ensure_item("pane:first").expect("first item");
    assert_eq!(source_first, target_second);
    assert_eq!(source_second, target_first);
    let target = DockspaceSession::from_persistent_layout(
        layout([target_first]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the conflicting target session builds");
    let before = target.version();

    let error = match target.prepare_document_restore_json(&bytes, |document, key| {
        document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
    }) {
        Ok(_) => panic!("conflicting append-only key history must reject during preparation"),
        Err(error) => error,
    };

    assert_eq!(
        error.kind(),
        DockspacePersistenceErrorKind::IdentityConflict
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(target_first).is_some());
    assert!(target.view().item(source_first).is_none());
    assert_eq!(target.next_document_generation(), Some(0));
}

#[test]
fn standalone_restore_rejects_a_prepared_candidate_when_managed_native_is_active() {
    let (mut target, first) = persistent_session_with_first_item();
    let (bytes, _, _) = saved_document_with_two_items();
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the document prepares before native enrollment");
    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the native host enrolls");
    let before = target.version();

    let error = match target.begin_document_restore_frame(&prepared) {
        Ok(_) => panic!("native restore must join the native causal frame"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(first).is_some());
}

#[test]
fn observed_native_restore_joins_the_host_frame_without_receiver_queries() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);

    target
        .enable_observed_native_roots()
        .expect("the observed native host enrolls");
    target
        .register_native_root(SURFACE, HostWindowToken::new(8))
        .expect("the root registration records");
    let mut registration = target
        .begin_host_frame()
        .expect("the observed-root registration frame begins");
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the registration frame settles every surface");
    registration
        .commit()
        .expect("the observed native root registration commits");
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the observed-root restore prepares");

    let mut restore = target
        .begin_document_restore_frame(&prepared)
        .expect("the restore joins the observed native host frame");
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored observed-root surface measures");
    restore
        .commit()
        .expect("the observed native document restore commits");

    assert!(target.view().item(second).is_some());
    assert_eq!(target.item_id_for_external_key("pane:second"), Some(second));
}

#[test]
fn managed_native_restore_joins_the_native_host_frame_atomically() {
    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);

    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native host enrolls");
    target
        .register_native_root(SURFACE, HostWindowToken::new(7))
        .expect("the root registration records");
    let mut registration = target
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the registration frame begins");
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the registration frame settles every surface");
    let registration_report = registration
        .commit()
        .expect("the native root registration commits");
    let original_binding = match registration_report.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the managed native restore prepares");

    let mut restore = target
        .begin_native_document_restore_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
        .expect("the document restore joins the managed native frame");
    assert!(restore.view().item(second).is_some());
    assert_eq!(restore.external_key_for_item(second), Some("pane:second"));
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored native surface measures");
    let restore_report = restore
        .commit()
        .expect("the joined document restore publishes atomically");

    let [successor_binding] = restore_report.native_bindings() else {
        panic!(
            "expected one current native binding after restore, got {:?}",
            restore_report.native_bindings()
        );
    };
    assert_ne!(*successor_binding, original_binding);
    assert!(!target.is_current_native_binding(original_binding));
    assert!(target.is_current_native_binding(*successor_binding));

    assert!(target.view().item(second).is_some());
    assert_eq!(target.item_id_for_external_key("pane:second"), Some(second));
    assert_eq!(target.next_document_generation(), Some(1));
}

#[test]
fn managed_restore_resumes_a_budget_paused_candidate_before_admitting_new_document_items() {
    use dockspace::runtime::{
        NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputBudget,
        NativeMonotonicClock, NativePointerEvent, NativePointerHover, NativePointerId,
        NativePointerInput, NativePointerOwner,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let (bytes, first, second) = saved_document_with_two_items();
    let (mut target, target_first) = persistent_session_with_first_item();
    assert_eq!(first, target_first);
    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the native host must enroll");
    let mut initial = target
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the initial pointer checkpoint must begin");
    initial
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    initial
        .commit()
        .expect("the initial checkpoint must commit");
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the restore with a new item must prepare");
    let before = target.version();
    target
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
        .expect("a native predecessor must record");
    let nanos = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&nanos);
    let clock =
        NativeMonotonicClock::new(move || Duration::from_nanos(source.load(Ordering::SeqCst)));
    let mut budget = NativeInputBudget::new(&clock);
    nanos.store(2_000_000, Ordering::SeqCst);
    assert!(
        target
            .poll_native_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .expect("the expired budget must retain an unpublished candidate")
            .is_none()
    );
    assert_eq!(target.version(), before);
    assert!(target.view().item(second).is_none());

    let mut restore = target
        .begin_native_document_restore_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
        .expect("a restore must resume the paused prefix before admitting its new item scope");
    assert!(restore.view().item(second).is_some());
    assert_eq!(restore.external_key_for_item(second), Some("pane:second"));
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored surface must measure");
    let report = restore
        .commit()
        .expect("the native predecessor and document restore must commit together");
    assert!(!report.native_input_pending());
    assert!(target.view().item(second).is_some());
    assert_eq!(target.item_id_for_external_key("pane:second"), Some(second));
}

#[test]
fn managed_restore_reports_the_complete_multi_binding_roster() {
    let mut source_bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let source_first = source_bootstrap
        .ensure_item("pane:first")
        .expect("the first source item allocates");
    let source_second = source_bootstrap
        .ensure_item("pane:second")
        .expect("the second source item allocates");
    let mut source = DockspaceSession::from_persistent_layout(
        two_surface_layout(source_first, source_second),
        DockPolicy::default(),
        source_bootstrap,
    )
    .expect("the two-surface source builds");
    let bytes = source
        .save_document_json()
        .expect("the two-surface document encodes");

    let mut target_bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = target_bootstrap
        .ensure_item("pane:first")
        .expect("the first target item allocates");
    let second = target_bootstrap
        .ensure_item("pane:second")
        .expect("the second target item allocates");
    assert_eq!((first, second), (source_first, source_second));
    let mut target = DockspaceSession::from_persistent_layout(
        two_surface_layout(first, second),
        DockPolicy::default(),
        target_bootstrap,
    )
    .expect("the two-surface target builds");
    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native host enrolls");
    target
        .register_native_root(SURFACE, HostWindowToken::new(17))
        .expect("the first root registration records");
    target
        .register_native_root(SECOND_SURFACE, HostWindowToken::new(18))
        .expect("the second root registration records");
    let mut registration = target
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the registration frame begins");
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the registration frame settles both surfaces");
    let registration = registration.commit().expect("both registrations commit");
    let predecessors = registration
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(predecessors.len(), 2);

    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the two-surface restore prepares");
    let mut restore = target
        .begin_native_document_restore_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
        .expect("the two-surface restore frame begins");
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the first restored surface measures");
    restore
        .measure_surface(SECOND_SURFACE, metrics())
        .expect("the second restored surface measures");
    let report = restore.commit().expect("the two-surface restore publishes");
    let successors = report.native_bindings();
    assert_eq!(
        successors
            .iter()
            .map(|binding| binding.surface())
            .collect::<Vec<_>>(),
        vec![SURFACE, SECOND_SURFACE]
    );
    for successor in successors {
        let predecessor = predecessors
            .iter()
            .find(|binding| binding.surface() == successor.surface())
            .expect("each successor has one predecessor");
        assert_ne!(successor, predecessor);
        assert!(!target.is_current_native_binding(*predecessor));
        assert!(target.is_current_native_binding(*successor));
    }
}

#[test]
fn saving_does_not_stale_a_prepared_product_action() {
    let (mut session, first) = persistent_session_with_first_item();
    let second = session
        .ensure_external_item("pane:second")
        .expect("the second item identity allocates");

    let mut open = session.begin_host_frame().expect("the open frame begins");
    open.open_item_current(second, dockspace::model::DockPlacement::After(first))
        .expect("the second item opens");
    open.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the ready surface is retained");
    open.commit().expect("the open frame commits");

    let prepared = session.prepare_select_item(first);
    let version = session.version();
    session
        .save_document_json()
        .expect("saving the session succeeds");
    assert_eq!(session.version(), version);

    let mut frame = session.begin_host_frame().expect("the action frame begins");
    frame
        .submit_prepared_action(prepared)
        .expect("the pre-save action remains structurally valid");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the ready surface is retained");
    let report = frame.commit().expect("the action frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(_)]
    ));
    assert!(
        session
            .view()
            .item(first)
            .is_some_and(|item| item.is_selected())
    );
}

#[test]
fn restoring_an_older_generation_preserves_append_only_identity_history() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let first = bootstrap.ensure_item("pane:first").expect("first item");
    let mut source =
        DockspaceSession::from_persistent_layout(layout([first]), DockPolicy::default(), bootstrap)
            .expect("the source session builds");
    let older = source
        .save_document_json()
        .expect("the older generation encodes");
    let later = source
        .ensure_external_item("pane:later")
        .expect("the later identity allocates");
    let latest = source
        .save_document_json()
        .expect("the latest generation encodes");

    let mut target =
        DockspaceSession::from_document_json(&latest, DockPolicy::default(), |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:later")
        })
        .expect("the latest generation restores");
    assert_eq!(target.next_document_generation(), Some(2));

    let prepared = target
        .prepare_document_restore_json(&older, |document, key| {
            document == DOCUMENT && key == "pane:first"
        })
        .expect("the older generation prepares");
    let mut restore = target
        .begin_document_restore_frame(&prepared)
        .expect("the older same-lineage generation prepares");
    restore
        .measure_surface(SURFACE, metrics())
        .expect("the restored surface measures");
    restore.commit().expect("the older generation publishes");

    assert_eq!(target.next_document_generation(), Some(2));
    assert_eq!(target.item_id_for_external_key("pane:later"), Some(later));
    let newest = target
        .ensure_external_item("pane:newest")
        .expect("a fresh identity allocates after the restore");
    assert!(newest.get() > later.get());
}

#[test]
fn malformed_unsupported_and_wrong_lineage_documents_reject_atomically() {
    let (target, first) = persistent_session_with_first_item();
    let before = target.version();
    let generation = target.next_document_generation();

    for (bytes, expected_kind) in [
        (
            b"{".as_slice(),
            DockspacePersistenceErrorKind::InvalidDocument,
        ),
        (
            br#"[999,{}]"#.as_slice(),
            DockspacePersistenceErrorKind::UnsupportedVersion,
        ),
    ] {
        let error = match target.prepare_document_restore_json(bytes, |_document, _key| true) {
            Ok(_) => panic!("invalid document bytes must reject"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), expected_kind);
    }

    let foreign_document = DockspaceDocumentId::from_bytes([0x91; 16]);
    let mut bootstrap = DockspaceDocumentBootstrap::new(foreign_document);
    let foreign_item = bootstrap.ensure_item("pane:foreign").expect("foreign item");
    let mut foreign = DockspaceSession::from_persistent_layout(
        layout([foreign_item]),
        DockPolicy::default(),
        bootstrap,
    )
    .expect("the foreign session builds");
    let bytes = foreign
        .save_document_json()
        .expect("the foreign document encodes");
    let resolver_called = Cell::new(false);
    let error = match target.prepare_document_restore_json(&bytes, |_document, _key| {
        resolver_called.set(true);
        true
    }) {
        Ok(_) => panic!("a foreign lineage must reject"),
        Err(error) => error,
    };

    assert_eq!(
        error.kind(),
        DockspacePersistenceErrorKind::IdentityConflict
    );
    assert!(!resolver_called.get());
    assert_eq!(target.version(), before);
    assert_eq!(target.next_document_generation(), generation);
    assert!(target.view().item(first).is_some());
}
