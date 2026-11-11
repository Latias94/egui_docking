//! Durable surface preferences remain separate from live native authority.

#![cfg(feature = "serde")]

use std::cell::Cell;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceView, FloatingPresentationId, ItemId, LogicalSurfaceRejection,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceHostFrame,
    DockspaceNativeSurfaceHosting, DockspacePersistenceErrorKind, DockspaceSession,
    DockspaceSurfacePresentationPreference, DockspaceWindowPresentationPreference, HostFrameReport,
    HostInputOutcome, HostWindowToken, HostWorkAreaToken, NativeCloseState,
    NativeCreateEffectAcknowledgement, NativeCreateReadiness, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeHostCapabilities, NativeHostCapability, NativePointerRoster,
    NativeReceiverAnswer, NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaFacts, NativeWorkAreaRoster, PaintedSurfaceOutput,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
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
    register_windows_with_capabilities(
        session,
        NativeHostCapabilities::none_supported()
            .with(NativeHostCapability::AuthoritativeInventory)
            .with(NativeHostCapability::GlobalWindowPlacement)
            .with(NativeHostCapability::NativeWindowLifecycle),
    )
}

fn register_windows_with_capabilities(
    session: &mut DockspaceSession,
    capabilities: NativeHostCapabilities,
) -> [NativeSurfaceBinding; 2] {
    assert!(
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap()
            .is_empty()
    );
    session
        .configure_managed_native_capabilities(capabilities)
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
                .with_presentation(NativeWindowPresentationState::Visible)
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
    assert_hosting(&read_preferences(&source), HOST, CHILD);
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
    assert!(std::str::from_utf8(&bytes).unwrap().starts_with("[1,"));
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
    assert_hosting(&read_preferences(&source), HOST, CHILD);
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
fn unsupported_versions_reject_before_payload_decoding_or_identity_recognition() {
    let mut session = persistent_session();
    let bytes = session.save_document_json().unwrap();
    let json = std::str::from_utf8(&bytes).unwrap();
    let payload = json
        .strip_prefix("[1,")
        .expect("new documents must use schema V1");
    let unsupported_header = format!("[2,{payload}");
    let before = session.version();
    let generation = session.next_document_generation();
    let values = read_preferences(&session);
    let content = assert_complete_content(session.view());
    for unsupported in [
        unsupported_header.as_bytes(),
        br#"[0,null]"#.as_slice(),
        br#"[2,{"workspace":"must not decode"}]"#.as_slice(),
    ] {
        let recognitions = Cell::new(0);
        let error = DockspaceSession::from_document_json(
            unsupported,
            DockPolicy::default(),
            |document, key| {
                recognitions.set(recognitions.get() + 1);
                recognize(document, key)
            },
        )
        .expect_err("an unsupported schema must not decode the payload");
        assert_eq!(
            error.kind(),
            DockspacePersistenceErrorKind::UnsupportedVersion
        );
        let error = session
            .prepare_document_restore_json(unsupported, |document, key| {
                recognitions.set(recognitions.get() + 1);
                recognize(document, key)
            })
            .expect_err(
                "live restore must reject an unsupported schema before decoding its payload",
            );
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

#[test]
fn review_removing_a_recovery_host_keeps_the_surviving_child_document_saveable() {
    let bytes = saved_native_document();
    let mut session =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    assert!(!session.has_pending_native_create());
    let before = session.version();
    let generation = session.next_document_generation();
    let original_preferences = read_preferences(&session);
    let expected_child = original_preferences
        .iter()
        .find(|preference| preference.surface() == CHILD)
        .copied()
        .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        CHILD,
        DockspaceRootLayout::new(
            CHILD_ROOT,
            DockspaceNode::central_tabs([MAIN_ITEM, MAIN_DETAIL]),
        ),
    )])
    .unwrap();
    let reset = session.prepare_layout_reset(layout).unwrap();
    let mut abandoned = session.begin_host_frame().unwrap();
    abandoned.submit_layout_reset(&reset).unwrap();
    drop(abandoned);
    assert_eq!(session.version(), before);
    assert_eq!(session.next_document_generation(), generation);
    assert_eq!(read_preferences(&session), original_preferences);

    let mut failed = session.begin_host_frame().unwrap();
    failed.submit_layout_reset(&reset).unwrap();
    failed
        .measure_surface(
            CHILD,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(48.0, 32.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(failed.select_item_current(MAIN_ITEM).is_err());
    assert!(
        failed
            .defer_surface(CHILD, SurfaceUnavailableReason::Deferred)
            .is_err()
    );
    assert!(failed.commit().is_err());
    assert_eq!(session.version(), before);
    assert_eq!(session.next_document_generation(), generation);
    assert_eq!(read_preferences(&session), original_preferences);

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let mut report = commit_deferred(frame);
    assert!(report.workspace_changed());
    assert_no_native_outputs(&mut report);
    assert!(session.view().surface(HOST).is_none());
    assert!(session.view().surface(CHILD).is_some());
    assert_eq!(session.view().item(MAIN_ITEM).unwrap().surface(), CHILD);
    assert_eq!(session.view().item(MAIN_DETAIL).unwrap().surface(), CHILD);
    let published = read_preferences(&session);
    assert_eq!(published.len(), 1);
    let child = published[0];
    assert_eq!(child.surface(), CHILD);
    assert!(child.native_hosting().is_none());
    assert_eq!(child.outer_rect(), expected_child.outer_rect());
    assert_eq!(child.inner_size_hint(), expected_child.inner_size_hint());
    assert_eq!(
        child.scale_factor_hint(),
        expected_child.scale_factor_hint()
    );
    assert_eq!(
        child.window_presentation(),
        expected_child.window_presentation()
    );
    assert_eq!(session.next_document_generation(), generation);
    let saved = session
        .save_document_json()
        .expect("removing an unbound recovery host must leave the surviving child saveable");
    let restored =
        DockspaceSession::from_document_json(&saved, DockPolicy::default(), recognize).unwrap();
    assert_eq!(read_preferences(&restored), published);
    assert!(!restored.has_pending_native_create());
}

#[test]
fn retiring_an_unbound_recovery_host_repairs_preferences_before_save_and_does_not_revive_on_attach()
{
    let bytes = saved_native_document();
    let mut session =
        DockspaceSession::from_document_json(&bytes, DockPolicy::default(), recognize).unwrap();
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(HOST_ROOT, DockspaceNode::central_tabs([])),
        ),
        DockspaceSurfaceLayout::new(
            CHILD,
            DockspaceRootLayout::new(
                CHILD_ROOT,
                DockspaceNode::central_tabs([MAIN_ITEM, MAIN_DETAIL]),
            ),
        ),
    ])
    .unwrap();
    let reset = session.prepare_layout_reset(layout).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    commit_deferred(frame);
    assert_hosting(&read_preferences(&session), HOST, CHILD);

    let generation = session.next_document_generation();
    let mut frame = session.begin_host_frame().unwrap();
    let retirement = frame.retire_empty_surface_current(HOST).unwrap();
    let report = commit_deferred(frame);
    assert!(matches!(
        report.submitted_action_outcome(retirement),
        Some(HostInputOutcome::LogicalSurfaceRetired { surface: HOST, .. })
    ));
    let preferences = read_preferences(&session);
    assert_eq!(preferences.len(), 1);
    assert_eq!(preferences[0].surface(), CHILD);
    assert_eq!(preferences[0].native_hosting(), None);
    assert_eq!(preferences[0].outer_rect(), Some(geometry(CHILD).0));
    assert_eq!(session.next_document_generation(), generation);
    session.save_document_json().unwrap();

    let mut frame = session.begin_host_frame().unwrap();
    frame.attach_empty_surface_current(HOST, HOST_ROOT).unwrap();
    commit_deferred(frame);
    assert!(session.view().surface(HOST).is_some());
    assert_eq!(read_preferences(&session), preferences);
    let saved = session.save_document_json().unwrap();
    let restored =
        DockspaceSession::from_document_json(&saved, DockPolicy::default(), recognize).unwrap();
    assert_eq!(read_preferences(&restored), preferences);
    assert!(restored.view().surface(HOST).is_some());
}

#[test]
fn a_live_native_owner_cannot_be_retired_by_clearing_its_durable_declaration() {
    let mut session = persistent_session();
    let [host, child] = register_windows(&mut session);
    let before = session.version();
    let preferences = read_preferences(&session);
    assert_hosting(&preferences, HOST, CHILD);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let retirement = frame.retire_empty_surface_current(HOST).unwrap();
    let mut report = commit_deferred(frame);
    assert_eq!(
        report.submitted_action_outcome(retirement),
        Some(&HostInputOutcome::LogicalSurfaceRejected {
            surface: HOST,
            reason: LogicalSurfaceRejection::NativeBindingPresent,
        })
    );
    assert_eq!(session.version(), before);
    assert!(report.take_native_effects().is_empty());
    assert!(session.is_current_native_binding(host));
    assert!(session.is_current_native_binding(child));
    assert_eq!(read_preferences(&session), preferences);
    session.save_document_json().unwrap();
}

fn native_preference_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn native_preference_snapshot(
    session: &mut DockspaceSession,
    bindings: [NativeSurfaceBinding; 2],
    pending: Option<(NativeSurfaceBinding, NativeWindowFacts)>,
) -> HostFrameReport {
    let mut observations = bindings
        .map(|binding| {
            let (outer, content, native_scale, presentation_scale) = geometry(binding.surface());
            (
                binding,
                NativeWindowFacts::live()
                    .with_outer_bounds(outer)
                    .with_content_bounds(content)
                    .with_native_scale_factor(native_scale)
                    .with_presentation_scale_factor(presentation_scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )
        })
        .to_vec();
    observations.extend(pending);
    session
        .report_managed_native_snapshot(
            observations,
            NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
                HostWorkAreaToken::new(1),
                PhysicalRect::new(-1920.0, 0.0, 3840.0, 1080.0).unwrap(),
                ScaleFactor::new(1.0).unwrap(),
            )]),
        )
        .unwrap();
    commit_deferred(native_preference_frame(session))
}

fn native_preference_paint(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> PaintedSurfaceOutput {
    let mut frame = native_preference_frame(session);
    frame
        .measure_surface(
            surface,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    commit_deferred(frame);
    let mut frame = native_preference_frame(session);
    let receipt = frame
        .paint_plan(surface)
        .unwrap()
        .expect("the exact measured native surface must expose its docking program")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = commit_deferred(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    output
}

fn pending_native_preference_tear_off() -> (
    DockspaceSession,
    [NativeSurfaceBinding; 2],
    NativeSurfaceBinding,
    NativeCreateEffectAcknowledgement,
) {
    let mut session = persistent_session();
    let bindings = register_windows_with_capabilities(
        &mut session,
        NativeHostCapabilities::none_supported()
            .with(NativeHostCapability::AuthoritativeInventory)
            .with(NativeHostCapability::GlobalWindowPlacement)
            .with(NativeHostCapability::NativeWindowLifecycle)
            .with(NativeHostCapability::WorkArea)
            .with(NativeHostCapability::PointerHitTestObservation)
            .with(NativeHostCapability::CloseCancellation),
    );
    native_preference_snapshot(&mut session, bindings, None);
    for binding in bindings {
        let output = native_preference_paint(&mut session, binding.surface());
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        commit_deferred(native_preference_frame(&mut session));
    }
    session.save_document_json().unwrap();
    let before = session.version();
    let generation = session.next_document_generation();
    let preferences = read_preferences(&session);
    let placement = NativeWindowPlacement::new(geometry(CHILD).0);
    let mut dropped = native_preference_frame(&mut session);
    dropped
        .move_root_to_new_window_current(BACK_ROOT, placement)
        .unwrap();
    assert_eq!(dropped.view().surfaces().count(), 2);
    assert_eq!(dropped.view().item(BACK_ITEM).unwrap().surface(), CHILD);
    drop(dropped);
    assert_eq!(session.version(), before);
    assert_eq!(session.next_document_generation(), generation);
    assert_eq!(read_preferences(&session), preferences);
    assert!(!session.has_pending_native_create());

    let mut frame = native_preference_frame(&mut session);
    frame
        .move_root_to_new_window_current(BACK_ROOT, placement)
        .unwrap();
    let mut report = commit_deferred(frame);
    assert!(!report.workspace_changed());
    let [create] = report.take_native_effects().try_into().unwrap();
    let binding = match create.operation() {
        NativeEffectOperation::CreateWindow { binding, .. } => *binding,
        operation => panic!("expected the exact CreateWindow request, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("creation must retain its exact readiness acknowledgement");
    };
    assert!(session.view().surface(binding.surface()).is_none());
    assert_eq!(session.view().item(BACK_ITEM).unwrap().surface(), CHILD);
    assert_eq!(read_preferences(&session), preferences);
    let saved = session.save_document_json().unwrap();
    let restored =
        DockspaceSession::from_document_json(&saved, DockPolicy::default(), recognize).unwrap();
    assert!(restored.view().surface(binding.surface()).is_none());
    assert_eq!(read_preferences(&restored), preferences);
    (session, bindings, binding, created)
}

fn native_preference_child_facts() -> NativeWindowFacts {
    let bounds = geometry(CHILD).0;
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_create_readiness(NativeCreateReadiness::Prepared, None)
}

#[test]
fn persistent_native_preparation_can_be_cancelled_without_publishing_a_surface_preference() {
    let (mut session, bindings, pending, created) = pending_native_preference_tear_off();
    let version = session.version();
    let preferences = read_preferences(&session);
    native_preference_snapshot(
        &mut session,
        bindings,
        Some((
            pending,
            native_preference_child_facts()
                .with_presentation(NativeWindowPresentationState::Visible)
                .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
        )),
    );
    let output = native_preference_paint(&mut session, pending.surface());
    session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .unwrap();
    let mut report = commit_deferred(native_preference_frame(&mut session));
    assert!(report.take_native_effects().is_empty());
    assert!(report.native_admissions().is_empty());
    session.save_document_json().unwrap();
    let generation = session.next_document_generation();
    let mut dropped = native_preference_frame(&mut session);
    assert!(dropped.cancel_next_native_create().unwrap());
    drop(dropped);
    assert_eq!(session.version(), version);
    assert_eq!(session.next_document_generation(), generation);
    assert!(session.has_pending_native_create());
    assert_eq!(read_preferences(&session), preferences);

    let mut frame = native_preference_frame(&mut session);
    assert!(frame.cancel_next_native_create().unwrap());
    let mut report = commit_deferred(frame);
    let [close] = report.take_native_effects().try_into().unwrap();
    assert!(matches!(close.operation(),
        NativeEffectOperation::CompensatingClose { binding } if *binding == pending));
    let Some(NativeEffectAcknowledgement::Close(closed)) = close.accepted() else {
        panic!("pending creation cancellation must retain its exact close acknowledgement");
    };
    assert!(!session.has_pending_native_create());
    assert_eq!(session.version(), version);
    assert_eq!(session.view().item(BACK_ITEM).unwrap().surface(), CHILD);
    assert_eq!(read_preferences(&session), preferences);
    session.save_document_json().unwrap();
    native_preference_snapshot(
        &mut session,
        bindings,
        Some((pending, NativeWindowFacts::destroyed_after(closed))),
    );
    session.report_native_binding_quiescence(pending).unwrap();
    commit_deferred(native_preference_frame(&mut session));
    assert_eq!(read_preferences(&session), preferences);
    session.save_document_json().unwrap();
}

#[test]
fn persistent_native_preparation_publishes_hosting_only_with_exact_first_live_output() {
    let (mut session, bindings, pending, created) = pending_native_preference_tear_off();
    let before = session.version();
    let preferences = read_preferences(&session);
    native_preference_snapshot(
        &mut session,
        bindings,
        Some((
            pending,
            native_preference_child_facts()
                .with_presentation(NativeWindowPresentationState::Visible)
                .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
        )),
    );
    native_preference_snapshot(
        &mut session,
        bindings,
        Some((
            pending,
            native_preference_child_facts()
                .with_presentation(NativeWindowPresentationState::Visible),
        )),
    );
    let report = commit_deferred(native_preference_frame(&mut session));
    assert!(report.native_admissions().is_empty());
    assert_eq!(session.version(), before);
    assert_eq!(read_preferences(&session), preferences);
    session.save_document_json().unwrap();

    let dropped = native_preference_paint(&mut session, pending.surface());
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    let report = commit_deferred(native_preference_frame(&mut session));
    assert!(report.native_admissions().is_empty());
    assert_eq!(session.view().item(BACK_ITEM).unwrap().surface(), CHILD);
    assert_eq!(read_preferences(&session), preferences);
    session.save_document_json().unwrap();

    let output = native_preference_paint(&mut session, pending.surface());
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut dropped = native_preference_frame(&mut session);
    assert_eq!(
        dropped.view().item(BACK_ITEM).unwrap().surface(),
        pending.surface()
    );
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(dropped);
    assert_eq!(session.version(), before);
    assert_eq!(read_preferences(&session), preferences);
    assert_eq!(session.view().item(BACK_ITEM).unwrap().surface(), CHILD);
    let report = commit_deferred(native_preference_frame(&mut session));
    assert_eq!(report.native_admissions(), &[pending]);
    assert_eq!(
        session.view().item(BACK_ITEM).unwrap().surface(),
        pending.surface()
    );
    assert_eq!(
        session.view().item(BACK_DETAIL).unwrap().surface(),
        pending.surface()
    );
    assert!(!session.has_pending_native_create());
    let published = read_preferences(&session);
    assert_eq!(published.len(), 3);
    assert_eq!(
        published
            .iter()
            .find(|value| value.surface() == pending.surface())
            .unwrap()
            .native_hosting(),
        Some(DockspaceNativeSurfaceHosting::Child {
            recovery_host: HOST
        }),
    );
    let saved = session.save_document_json().unwrap();
    let restored =
        DockspaceSession::from_document_json(&saved, DockPolicy::default(), recognize).unwrap();
    assert_eq!(
        restored.view().item(BACK_ITEM).unwrap().surface(),
        pending.surface()
    );
    assert_eq!(read_preferences(&restored).len(), 3);
}
