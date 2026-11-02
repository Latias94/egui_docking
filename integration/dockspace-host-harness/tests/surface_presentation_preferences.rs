//! Durable surface preferences remain separate from live native authority.

#![cfg(feature = "serde")]

use std::cell::Cell;

use dockspace::geometry::{LogicalRect, PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceView, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceHostFrame,
    DockspaceNativeSurfaceHosting, DockspacePersistenceErrorKind, DockspaceSession,
    DockspaceSurfacePresentationPreference, DockspaceWindowPresentationPreference, HostFrameReport,
    HostInputOutcome, HostWindowToken, NativeCloseState, NativeCreateStrategy,
    NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, SurfaceUnavailableReason,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x70; 16]);
// The recovery host deliberately sorts after its child.
const HOST: SurfaceId = SurfaceId::new(9);
const CHILD: SurfaceId = SurfaceId::new(2);
const HOST_ROOT: RootId = RootId::new(90);
const CHILD_ROOT: RootId = RootId::new(20);
const BACK_ROOT: RootId = RootId::new(30);
const FRONT_ROOT: RootId = RootId::new(40);
const BACK_FLOATING: FloatingPresentationId = FloatingPresentationId::new(3);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(8);
const HOST_ITEM: ItemId = ItemId::new(1);
const HOST_DETAIL: ItemId = ItemId::new(2);
const MAIN_ITEM: ItemId = ItemId::new(3);
const MAIN_DETAIL: ItemId = ItemId::new(4);
const BACK_ITEM: ItemId = ItemId::new(5);
const BACK_DETAIL: ItemId = ItemId::new(6);
const FRONT_ITEM: ItemId = ItemId::new(7);
const FRONT_DETAIL: ItemId = ItemId::new(8);
const HISTORICAL: ItemId = ItemId::new(9);
const KEYS: [(&str, ItemId); 9] = [
    ("pane:host", HOST_ITEM),
    ("pane:host-detail", HOST_DETAIL),
    ("pane:main", MAIN_ITEM),
    ("pane:main-detail", MAIN_DETAIL),
    ("pane:back", BACK_ITEM),
    ("pane:back-detail", BACK_DETAIL),
    ("pane:front", FRONT_ITEM),
    ("pane:front-detail", FRONT_DETAIL),
    ("pane:historical", HISTORICAL),
];

fn back_rect() -> LogicalRect {
    LogicalRect::new(45.0, 60.0, 320.0, 240.0).unwrap()
}

fn front_rect() -> LogicalRect {
    LogicalRect::new(155.0, 175.0, 420.0, 280.0).unwrap()
}

fn persistent_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(
                HOST_ROOT,
                DockspaceNode::central_tabs_with_selection(
                    [HOST_ITEM, HOST_DETAIL],
                    Some(HOST_DETAIL),
                )
                .unwrap(),
            ),
        ),
        DockspaceSurfaceLayout::new(
            CHILD,
            DockspaceRootLayout::new(
                CHILD_ROOT,
                DockspaceNode::central_tabs_with_selection(
                    [MAIN_ITEM, MAIN_DETAIL],
                    Some(MAIN_DETAIL),
                )
                .unwrap(),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            BACK_FLOATING,
            DockspaceRootLayout::new(
                BACK_ROOT,
                DockspaceNode::tabs_with_selection([BACK_ITEM, BACK_DETAIL], Some(BACK_ITEM))
                    .unwrap(),
            ),
            back_rect(),
        ))
        .with_contained(DockspaceContainedLayout::new(
            FRONT_FLOATING,
            DockspaceRootLayout::new(
                FRONT_ROOT,
                DockspaceNode::tabs_with_selection([FRONT_ITEM, FRONT_DETAIL], Some(FRONT_DETAIL))
                    .unwrap(),
            ),
            front_rect(),
        )),
    ])
    .unwrap();
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    for (key, item) in KEYS {
        assert_eq!(bootstrap.ensure_item(key).unwrap(), item);
    }
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    DockspaceSession::from_persistent_layout(layout, policy, bootstrap).unwrap()
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && KEYS.iter().any(|(known, _)| *known == key)
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn register_windows(session: &mut DockspaceSession) -> [NativeSurfaceBinding; 2] {
    assert!(
        session
            .enable_managed_native_host(
                NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap()
            .is_empty()
    );
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::GlobalWindowPlacement)
                .with(NativeHostCapability::NativeWindowLifecycle),
        )
        .unwrap();
    session
        .register_native_root(HOST, HostWindowToken::new(91))
        .unwrap();
    let root_report = commit_deferred(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let host = root_report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } if binding.surface() == HOST => {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the recovery host must be registered before its child");
    session
        .register_owned_native_child(CHILD, HostWindowToken::new(22), HOST)
        .unwrap();
    let child_report = commit_deferred(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let child = child_report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } if binding.surface() == CHILD => {
                Some(*binding)
            }
            _ => None,
        })
        .expect("the complete child surface must bootstrap as an owned child");
    assert_eq!(child_report.native_bindings().len(), 2);
    assert!(session.is_current_native_binding(host));
    assert!(session.is_current_native_binding(child));
    [host, child]
}

fn geometry(surface: SurfaceId) -> (PhysicalRect, PhysicalRect, ScaleFactor, ScaleFactor) {
    match surface {
        HOST => (
            PhysicalRect::new(-1536.0, 80.0, 1296.0, 960.0).unwrap(),
            PhysicalRect::new(-1528.0, 110.0, 1280.0, 912.0).unwrap(),
            ScaleFactor::new(1.25).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
        ),
        CHILD => (
            PhysicalRect::new(240.0, 128.0, 1016.0, 792.0).unwrap(),
            PhysicalRect::new(248.0, 160.0, 1000.0, 752.0).unwrap(),
            ScaleFactor::new(2.0).unwrap(),
            ScaleFactor::new(1.5).unwrap(),
        ),
        _ => panic!("only the registered host and child have observed native geometry"),
    }
}

fn observe_windows(session: &mut DockspaceSession, bindings: [NativeSurfaceBinding; 2]) {
    let observations = bindings.map(|binding| {
        let (outer, content, native_scale, presentation_scale) = geometry(binding.surface());
        (
            binding,
            NativeWindowFacts::live()
                .with_outer_bounds(outer)
                .with_content_bounds(content)
                .with_content_size(content.size())
                .with_native_scale_factor(native_scale)
                .with_presentation_scale_factor(presentation_scale)
                .with_input(NativeWindowInputState::ReceivesInput, None)
                .with_presentation(NativeWindowPresentationState::Visible, None)
                .with_close(NativeCloseState::Clear, None),
        )
    });
    session
        .report_managed_native_snapshot(observations, NativeWorkAreaRoster::Unknown)
        .unwrap();
    let report = commit_deferred(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(!session.has_pending_native_input());
}

fn saved_native_document() -> Vec<u8> {
    let mut session = persistent_session();
    let bindings = register_windows(&mut session);
    observe_windows(&mut session, bindings);
    session.save_document_json().unwrap()
}

fn preferences(
    values: impl Iterator<Item = DockspaceSurfacePresentationPreference>,
) -> Vec<DockspaceSurfacePresentationPreference> {
    let mut values = values.collect::<Vec<_>>();
    values.sort_by_key(|value| value.surface());
    values
}

fn assert_hosting(
    values: &[DockspaceSurfacePresentationPreference],
    host: SurfaceId,
    child: SurfaceId,
) {
    assert_eq!(values.len(), 2);
    for (surface, hosting) in [
        (host, DockspaceNativeSurfaceHosting::Root),
        (
            child,
            DockspaceNativeSurfaceHosting::Child {
                recovery_host: host,
            },
        ),
    ] {
        let value = values
            .iter()
            .copied()
            .find(|value| value.surface() == surface)
            .expect("each explicitly registered native surface must retain its own preference");
        assert!(value.native_hosting() == Some(hosting));
    }
}

fn assert_observed_preferences(
    values: &[DockspaceSurfacePresentationPreference],
    host: SurfaceId,
    child: SurfaceId,
) {
    assert_hosting(values, host, child);
    for (original, surface) in [(HOST, host), (CHILD, child)] {
        let value = values
            .iter()
            .copied()
            .find(|value| value.surface() == surface)
            .unwrap();
        let (outer, content, native_scale, _) = geometry(original);
        assert_eq!(value.outer_rect(), Some(outer));
        let inner: Option<PhysicalSize> = value.inner_size_hint();
        assert_eq!(inner, Some(content.size()));
        assert_eq!(value.scale_factor_hint(), Some(native_scale));
        // Visibility is not a Normal, Maximized, or Fullscreen preference.
        let window_presentation: Option<DockspaceWindowPresentationPreference> =
            value.window_presentation();
        assert!(window_presentation.is_none());
    }
}

fn read_preferences(session: &DockspaceSession) -> Vec<DockspaceSurfacePresentationPreference> {
    let version = session.version();
    let generation = session.next_document_generation();
    let values = preferences(session.surface_presentation_preferences());
    assert!(values == preferences(session.surface_presentation_preferences()));
    assert_eq!(session.version(), version);
    assert_eq!(session.next_document_generation(), generation);
    values
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentIdentities {
    host: SurfaceId,
    child: SurfaceId,
    roots: [RootId; 4],
    contained: [FloatingPresentationId; 2],
}

fn assert_complete_content(view: DockspaceView<'_>) -> ContentIdentities {
    assert_eq!(view.surfaces().count(), 2);
    let host = view.item(HOST_ITEM).unwrap();
    let main = view.item(MAIN_ITEM).unwrap();
    let back = view.item(BACK_ITEM).unwrap();
    let front = view.item(FRONT_ITEM).unwrap();
    let back_floating = back.contained().unwrap();
    let front_floating = front.contained().unwrap();
    let host_surface = view.surface(host.surface()).unwrap();
    let child_surface = view.surface(main.surface()).unwrap();
    assert_ne!(host.surface(), main.surface());
    assert_eq!(host_surface.main_root().unwrap().id(), host.root());
    assert_eq!(host_surface.contained_count(), 0);
    assert_eq!(child_surface.main_root().unwrap().id(), main.root());
    assert_eq!(child_surface.contained_count(), 2);
    assert_eq!(
        child_surface
            .contained()
            .map(|contained| (
                contained.id(),
                contained.root().unwrap().id(),
                contained.rect()
            ))
            .collect::<Vec<_>>(),
        [
            (back_floating, back.root(), back_rect()),
            (front_floating, front.root(), front_rect()),
        ]
    );
    for (items, selected, surface, root, contained, central) in [
        (
            [HOST_ITEM, HOST_DETAIL],
            HOST_DETAIL,
            host.surface(),
            host.root(),
            None,
            true,
        ),
        (
            [MAIN_ITEM, MAIN_DETAIL],
            MAIN_DETAIL,
            main.surface(),
            main.root(),
            None,
            true,
        ),
        (
            [BACK_ITEM, BACK_DETAIL],
            BACK_ITEM,
            main.surface(),
            back.root(),
            Some(back_floating),
            false,
        ),
        (
            [FRONT_ITEM, FRONT_DETAIL],
            FRONT_DETAIL,
            main.surface(),
            front.root(),
            Some(front_floating),
            false,
        ),
    ] {
        for item in items {
            let pane = view.item(item).unwrap();
            assert_eq!(
                (pane.surface(), pane.root(), pane.contained()),
                (surface, root, contained)
            );
            assert_eq!(pane.tabs().items(), &items);
            assert_eq!(pane.tabs().selected(), Some(selected));
            assert_eq!(pane.tabs().is_central(), central);
            assert_eq!(pane.is_selected(), item == selected);
        }
    }
    assert!(view.item(HISTORICAL).is_none());
    ContentIdentities {
        host: host.surface(),
        child: main.surface(),
        roots: [host.root(), main.root(), back.root(), front.root()],
        contained: [back_floating, front_floating],
    }
}

fn assert_keys(session: &DockspaceSession) {
    assert_eq!(session.document_id(), Some(DOCUMENT));
    for (key, item) in KEYS {
        assert_eq!(session.item_id_for_external_key(key), Some(item));
        assert_eq!(session.external_key_for_item(item), Some(key));
    }
}

fn assert_no_native_outputs(report: &mut HostFrameReport) {
    assert!(report.native_bindings().is_empty());
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_native_staging_outputs().is_empty());
}

#[test]
fn capture_preserves_registered_hosting_and_observed_physical_geometry() {
    let mut source = persistent_session();
    let original_content = assert_complete_content(source.view());
    // The same graph alone cannot declare which native window owns a surface.
    for value in read_preferences(&source) {
        assert!(value.native_hosting().is_none());
        assert!(value.outer_rect().is_none());
        assert!(value.inner_size_hint().is_none());
        assert!(value.scale_factor_hint().is_none());
        assert!(value.window_presentation().is_none());
    }
    let bindings = register_windows(&mut source);
    let hosting_bytes = source.save_document_json().unwrap();
    let hosting = read_preferences(&source);
    assert_hosting(&hosting, HOST, CHILD);
    for value in &hosting {
        assert!(value.outer_rect().is_none());
        assert!(value.inner_size_hint().is_none());
        assert!(value.scale_factor_hint().is_none());
        assert!(value.window_presentation().is_none());
    }
    let unobserved =
        DockspaceSession::from_document_json(&hosting_bytes, DockPolicy::default(), recognize)
            .unwrap();
    assert!(read_preferences(&unobserved) == hosting);

    observe_windows(&mut source, bindings);
    assert!(read_preferences(&source) == hosting);
    let version = source.version();
    let generation = source.next_document_generation().unwrap();
    let bytes = source.save_document_json().unwrap();
    assert!(std::str::from_utf8(&bytes).unwrap().starts_with("[2,"));
    assert_eq!(source.version(), version);
    assert_eq!(source.next_document_generation(), Some(generation + 1));
    let captured = read_preferences(&source);
    assert_observed_preferences(&captured, HOST, CHILD);
    assert_eq!(assert_complete_content(source.view()), original_content);

    let mut destination =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    assert!(read_preferences(&destination) == captured);
    assert_eq!(
        assert_complete_content(destination.view()),
        original_content
    );
    assert_keys(&destination);
    assert!(!destination.has_pending_native_create());
    assert!(!destination.has_pending_native_input());
    let version = destination.version();
    let generation = destination.next_document_generation();
    let frame = destination.begin_host_frame().unwrap();
    assert!(preferences(frame.surface_presentation_preferences()) == captured);
    assert_eq!(frame.version(), version);
    let mut report = commit_deferred(frame);
    assert_no_native_outputs(&mut report);
    assert_eq!(destination.version(), version);
    assert_eq!(destination.next_document_generation(), generation);
    assert!(destination.presented_surface(HOST).is_none());
    assert!(destination.presented_surface(CHILD).is_none());
}

#[test]
fn pending_child_without_a_current_window_retains_hosting_on_its_first_document_save() {
    let mut source = persistent_session();
    let original_content = assert_complete_content(source.view());
    let [host, child] = register_windows(&mut source);
    assert!(read_preferences(&source).is_empty());
    source
        .report_managed_native_snapshot(
            [
                (host, NativeWindowFacts::live()),
                (child, NativeWindowFacts::destroyed()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let report = commit_deferred(
        source
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(report.native_bindings(), &[host]);
    assert!(!source.is_current_native_binding(child));
    assert!(source.recognizes_native_binding(child));
    assert_eq!(assert_complete_content(source.view()), original_content);
    let bytes = source.save_document_json().unwrap();
    let captured = read_preferences(&source);
    assert_hosting(&captured, HOST, CHILD);
    assert!(captured.iter().all(|value| value.outer_rect().is_none()));
    let restored =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    assert_eq!(read_preferences(&restored), captured);
    assert_eq!(assert_complete_content(restored.view()), original_content);
    assert!(!restored.has_pending_native_create());
    assert!(!restored.has_pending_native_input());
}

#[test]
fn destroyed_root_and_pending_child_keep_both_host_declarations_before_the_first_save() {
    let mut source = persistent_session();
    let original_content = assert_complete_content(source.view());
    let [host, child] = register_windows(&mut source);
    source
        .report_managed_native_snapshot(
            [
                (host, NativeWindowFacts::destroyed()),
                (child, NativeWindowFacts::destroyed()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let report = commit_deferred(
        source
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(report.native_bindings().is_empty());
    assert_eq!(assert_complete_content(source.view()), original_content);
    let bytes = source.save_document_json().unwrap();
    assert_hosting(&read_preferences(&source), HOST, CHILD);
    let restored =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    assert_hosting(&read_preferences(&restored), HOST, CHILD);
    assert_eq!(assert_complete_content(restored.view()), original_content);
}

#[test]
fn terminal_restore_preserves_preference_and_recovery_host_identities_atomically() {
    let bytes = saved_native_document();
    let mut destination =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    let original_content = assert_complete_content(destination.view());
    let original_preferences = read_preferences(&destination);
    assert_observed_preferences(&original_preferences, HOST, CHILD);
    let generation = destination.next_document_generation();

    let reset = destination
        .prepare_layout_reset(DockspaceLayout::empty())
        .unwrap();
    let mut frame = destination.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let mut reset_report = commit_deferred(frame);
    assert_no_native_outputs(&mut reset_report);
    assert!(destination.view().surfaces().next().is_none());
    assert!(destination.view().surface(HOST).is_none());
    assert!(destination.view().surface(CHILD).is_none());
    let published_preferences = read_preferences(&destination);
    assert_eq!(destination.next_document_generation(), generation);
    assert_keys(&destination);

    let before = destination.version();
    let recognitions = Cell::new(0);
    let prepared = destination
        .prepare_document_restore_json(&bytes, |document, key| {
            recognitions.set(recognitions.get() + 1);
            recognize(document, key)
        })
        .unwrap();
    assert_eq!(recognitions.get(), KEYS.len());
    assert_eq!(assert_complete_content(prepared.view()), original_content);
    assert!(preferences(prepared.surface_presentation_preferences()) == original_preferences);
    assert_eq!(destination.version(), before);
    assert_eq!(destination.next_document_generation(), generation);

    let mut candidate_content = None;
    let mut candidate_preferences = None;
    for attempt in 0..2 {
        let mut frame = destination.begin_host_frame().unwrap();
        assert!(preferences(frame.surface_presentation_preferences()) == published_preferences);
        frame.submit_document_restore(&prepared).unwrap();
        let candidate_version = frame.version();
        assert_ne!(candidate_version, before);
        let content = assert_complete_content(frame.view());
        assert_eq!(content, original_content);
        assert!(frame.proposed_native_bindings().next().is_none());
        assert!(frame.view().surface(HOST).is_some());
        assert!(frame.view().surface(CHILD).is_some());
        for root in original_content.roots {
            assert!(frame.view().root(root).is_some());
        }
        for floating in original_content.contained {
            assert!(frame.view().contained(floating).is_some());
        }
        assert_eq!(*candidate_content.get_or_insert(content), content);
        let values = preferences(frame.surface_presentation_preferences());
        assert!(values == original_preferences);
        assert_observed_preferences(&values, content.host, content.child);
        assert!(values == preferences(frame.surface_presentation_preferences()));
        assert!(values == *candidate_preferences.get_or_insert_with(|| values.clone()));
        assert_eq!(frame.version(), candidate_version);
        for (key, item) in KEYS {
            assert_eq!(frame.external_key_for_item(item), Some(key));
        }
        if attempt == 0 {
            drop(frame);
            assert_eq!(destination.version(), before);
            assert!(destination.view().surfaces().next().is_none());
            assert!(read_preferences(&destination) == published_preferences);
        } else {
            let mut report = commit_deferred(frame);
            assert_eq!(
                report.inputs(),
                &[HostInputOutcome::DocumentRestored {
                    previous: before,
                    current: candidate_version,
                }]
            );
            assert_eq!(report.after(), candidate_version);
            assert_no_native_outputs(&mut report);
            assert_eq!(destination.version(), candidate_version);
            assert_eq!(assert_complete_content(destination.view()), content);
            assert!(read_preferences(&destination) == values);
        }
        assert!(preferences(prepared.surface_presentation_preferences()) == original_preferences);
        assert_eq!(assert_complete_content(prepared.view()), original_content);
        assert_eq!(destination.next_document_generation(), generation);
        assert_eq!(recognitions.get(), KEYS.len());
        assert_keys(&destination);
        assert!(!destination.has_pending_native_create());
        assert!(!destination.has_pending_native_input());
    }

    let content = candidate_content.unwrap();
    let restored = read_preferences(&destination);
    assert!(restored == candidate_preferences.unwrap());
    let frame = destination.begin_host_frame().unwrap();
    assert!(preferences(frame.surface_presentation_preferences()) == restored);
    let mut report = commit_deferred(frame);
    assert_no_native_outputs(&mut report);
    assert!(destination.presented_surface(content.host).is_none());
    assert!(destination.presented_surface(content.child).is_none());
    assert_eq!(destination.next_document_generation(), generation);
}

#[test]
fn v1_documents_reject_before_payload_decoding_or_identity_recognition() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let json = std::str::from_utf8(&bytes).unwrap();
    let payload = json
        .strip_prefix("[2,")
        .expect("new documents must use schema V2");
    let old_header = format!("[1,{payload}");
    let before = session.version();
    let generation = session.next_document_generation();
    let values = read_preferences(&session);
    let content = assert_complete_content(session.view());
    for legacy in [
        old_header.as_bytes(),
        br#"[1,null]"#.as_slice(),
        br#"[1,{"workspace":[999,null],"surface_preferences":"must not decode"}]"#.as_slice(),
    ] {
        let recognitions = Cell::new(0);
        let error =
            DockspaceSession::from_document_json(legacy, DockPolicy::default(), |document, key| {
                recognitions.set(recognitions.get() + 1);
                recognize(document, key)
            })
            .expect_err("schema V1 cannot supply the complete native hosting metadata contract");
        assert_eq!(
            error.kind(),
            DockspacePersistenceErrorKind::UnsupportedVersion
        );
        let error = session
            .prepare_document_restore_json(legacy, |document, key| {
                recognitions.set(recognitions.get() + 1);
                recognize(document, key)
            })
            .expect_err("live restore must reject V1 before decoding its payload");
        assert_eq!(
            error.kind(),
            DockspacePersistenceErrorKind::UnsupportedVersion
        );
        assert_eq!(recognitions.get(), 0);
        assert_eq!(session.version(), before);
        assert_eq!(session.next_document_generation(), generation);
        assert!(read_preferences(&session) == values);
        assert_eq!(assert_complete_content(session.view()), content);
        assert_keys(&session);
    }
}
