#![cfg(feature = "serde")]

mod support;

use std::cell::RefCell;

use dockspace::command::{DockTarget, RootContent, WorkspaceCommand};
use dockspace::document::{
    DOCKSPACE_DOCUMENT_VERSION, DockspaceDocument, DockspaceDocumentBootstrap,
    DockspaceDocumentDecodeError, DockspaceDocumentEnvelope, DockspaceDocumentId,
    DockspaceDocumentRestore, DockspaceDocumentRestoreError, DockspaceDocumentSession,
    DockspaceDocumentSessionError,
};
use dockspace::engine::{
    CoreHostFrameError, DockEngine, EngineError, EngineInput, HostPresentationUnavailableReason,
};
use dockspace::external_item_key::{ExternalItemKeyMap, ExternalItemKeyReconcileError};
use dockspace::geometry::{LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::Authority;
use dockspace::persistence::WorkspaceSnapshot;
use dockspace::platform::{
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCoordinateObservation, WindowInputState,
    WindowPresentationObservation, WindowPresentationState,
};
use dockspace::pointer_journal::PointerEdgeSequence;
use dockspace::policy::DockPolicy;
use dockspace::presentation_observation::{HostPresentationObservation, PresentationHostLease};
use dockspace::surface_recovery::{ConvertedMainRecovery, SurfaceRecoveryTarget};
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{
    CoordinateObservationGeneration, PlatformSnapshotGeneration, PresentationObservationGeneration,
    ViewportBinding, ViewportRole, WindowToken, WorkAreaToken,
};
use dockspace::viewport_focus::{
    FocusObservationEnvelope, FocusObservationGeneration, GlobalFocusedWindow,
};
use dockspace::viewport_persistence::{
    ViewportPlacementPreference, ViewportPlacementPreferences, WindowPresentationPreference,
};

const DOCUMENT_ID: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"document-lineage");
const OTHER_DOCUMENT_ID: DockspaceDocumentId =
    DockspaceDocumentId::from_bytes(*b"other-document!!");
const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);
const WINDOW: WindowToken = WindowToken::new(1);
const RUNTIME_SOURCE: StableInputSourceId = StableInputSourceId::new(0x5045_5253_4953_5401);

fn workspace(items: impl IntoIterator<Item = ItemId>) -> Workspace {
    let items = items.into_iter().collect::<Vec<_>>();
    let selected = items.last().copied();
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(items.clone(), selected));
    builder
        .set_tab_mru(tabs, items.iter().rev().copied())
        .expect("fixture tab MRU must be valid");
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("fixture workspace must be valid")
}

fn swapped_item_keys() -> ExternalItemKeyMap {
    let mut keys = ExternalItemKeyMap::new();
    keys.ensure_all(["pane/two", "pane/one", "pane/tombstone"])
        .expect("fixture key map must fit");
    keys
}

fn canonical_item_keys() -> ExternalItemKeyMap {
    let mut keys = ExternalItemKeyMap::new();
    keys.ensure_all(["pane/one", "pane/two", "pane/tombstone"])
        .expect("fixture key map must fit");
    keys
}

fn placements() -> ViewportPlacementPreferences {
    let mut placements = ViewportPlacementPreferences::new();
    let rect = PhysicalRect::new(50.0, 75.0, 900.0, 700.0)
        .expect("fixture placement rectangle must be valid");
    placements.set(
        ViewportPlacementPreference::new(SURFACE, rect)
            .expect("fixture placement must be non-empty")
            .with_work_area(WorkAreaToken::new(17))
            .with_scale_factor(ScaleFactor::new(1.5).expect("fixture scale must be valid"))
            .with_presentation(WindowPresentationPreference::Maximized),
    );
    placements
}

fn session_with(
    document_id: DockspaceDocumentId,
    next_generation: u64,
) -> DockspaceDocumentSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(document_id, next_generation);
    bootstrap
        .ensure_external_item_keys(["pane/one", "pane/two", "pane/tombstone"])
        .expect("fixture identities must fit");
    let one = bootstrap
        .item_id("pane/one")
        .expect("first fixture identity exists");
    let two = bootstrap
        .item_id("pane/two")
        .expect("second fixture identity exists");
    bootstrap.set_viewport_placement(
        placements()
            .get(SURFACE)
            .copied()
            .expect("fixture placement exists"),
    );
    let engine = DockEngine::new(workspace([one, two]), DockPolicy::default())
        .expect("fixture engine must initialize");
    DockspaceDocumentSession::bind(engine, bootstrap).expect("fixture document session must bind")
}

fn session_with_untrusted_map(keys: ExternalItemKeyMap) -> DockspaceDocumentSession {
    let engine = DockEngine::new(
        workspace([ItemId::new(1), ItemId::new(2)]),
        DockPolicy::default(),
    )
    .expect("fixture engine must initialize");
    let mut session =
        DockspaceDocumentSession::unbound(engine).expect("fixture session identity must fit");
    session
        .bind_untrusted_import(DOCUMENT_ID, 0, keys, placements(), |_, _| true)
        .expect("explicit fixture proof accepts the imported association");
    session
}

fn sealed_session_frame(
    session: &mut DockspaceDocumentSession,
) -> dockspace::engine::CoreHostFrame {
    let host = session
        .adapter_create_presentation_host()
        .expect("fixture presentation host must mint");
    let mut prelude = session
        .adapter_begin_host_frame(host)
        .expect("session host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("empty presentation observation must submit");
    prelude
        .seal(session.engine())
        .expect("session host frame must seal")
}

fn decode_document(value: serde_json::Value) -> DockspaceDocument {
    serde_json::from_value::<DockspaceDocumentEnvelope>(value)
        .expect("document envelope must decode")
        .into_document()
        .expect("supported nested snapshots must decode")
}

fn submit_session_input(
    session: &mut DockspaceDocumentSession,
    host: PresentationHostLease,
    sequence: u64,
    input: EngineInput,
) -> EngineTransition {
    let mut prelude = session
        .adapter_begin_host_frame(host)
        .expect("document fixture host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("document fixture has no pending presentation output");
    let mut frame = prelude
        .seal(session.engine())
        .expect("document fixture host frame must seal");
    frame
        .append_input(RUNTIME_SOURCE, SourceSequence::new(sequence), input)
        .expect("document fixture input must append");
    support::complete_host_frame_with_retained_or_unavailable(session.engine(), &mut frame);
    let mut frame = frame
        .into_presentation()
        .expect("document fixture must enter presentation phase");
    frame
        .resolve_all_presentation_obligations_unavailable(
            HostPresentationUnavailableReason::OutputNotProduced,
        )
        .expect("document fixture must settle every presentation obligation");
    session
        .adapter_prepare_host_presentation_frame(frame)
        .expect("document fixture host frame must prepare")
        .commit()
}

fn submit_restore_input(
    restore: &mut DockspaceDocumentRestore<'_>,
    host: PresentationHostLease,
    sequence: u64,
    input: EngineInput,
) -> EngineTransition {
    let mut prelude = restore
        .adapter_begin_host_frame(host)
        .expect("document restore host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("document restore has no pending presentation output");
    let mut frame = prelude
        .seal(restore.engine())
        .expect("document restore host frame must seal");
    frame
        .append_input(RUNTIME_SOURCE, SourceSequence::new(sequence), input)
        .expect("document restore input must append");
    support::complete_host_frame_with_retained_or_unavailable(restore.engine(), &mut frame);
    let mut frame = frame
        .into_presentation()
        .expect("document restore frame must enter presentation phase");
    frame
        .resolve_all_presentation_obligations_unavailable(
            HostPresentationUnavailableReason::OutputNotProduced,
        )
        .expect("document restore frame must settle every presentation obligation");
    restore
        .adapter_prepare_host_presentation_frame(frame)
        .expect("document restore host frame must prepare")
        .commit()
}

fn runtime_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities
}

fn platform_snapshot(
    generation: u64,
    binding: ViewportBinding,
    outer_rect: PhysicalRect,
    scale_factor: ScaleFactor,
    presentation: WindowPresentationState,
) -> PlatformSnapshot {
    let window = ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(outer_rect),
            Authority::Known(outer_rect),
            Authority::Known(scale_factor),
            Authority::Known(scale_factor),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(presentation),
            PresentationEffectAcknowledgement::known(None),
        ));
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        support::known_capability_observation(generation, runtime_capabilities()),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        support::known_inventory_observation(generation, std::slice::from_ref(&window)),
        vec![window],
        Vec::new(),
        support::unknown_work_area_observation(generation),
    )
    .expect("document fixture platform snapshot must be canonical")
}

fn apply_document(session: &mut DockspaceDocumentSession, document: DockspaceDocument) {
    let host = session
        .adapter_create_presentation_host()
        .expect("document restore presentation host must mint");
    let mut restore = session
        .begin_restore(document, |_, _, _| true)
        .expect("document restore must prepare");
    let input = restore
        .take_engine_input()
        .expect("document restore must carry one exact input");
    let transition = submit_restore_input(&mut restore, host, 1, input);
    restore
        .commit(&transition)
        .expect("exact document transaction must commit into its owner");
}

#[test]
fn round_trip_binds_workspace_keys_placement_and_generation() {
    let mut source = session_with(DOCUMENT_ID, 7);
    let source_workspace =
        WorkspaceSnapshot::capture(source.workspace()).expect("source workspace must capture");
    let document = source.capture().expect("complete fixture must capture");
    assert_eq!(document.document_id(), DOCUMENT_ID);
    assert_eq!(document.generation(), 7);
    assert_eq!(source.next_generation(), Some(8));

    let encoded = serde_json::to_string(&document).expect("document must encode");
    let decoded = serde_json::from_str::<DockspaceDocumentEnvelope>(&encoded)
        .expect("document must decode")
        .into_document()
        .expect("supported document must decode");
    let mut target = session_with(DOCUMENT_ID, 20);
    apply_document(&mut target, decoded);

    assert_eq!(
        WorkspaceSnapshot::capture(target.workspace()).expect("target workspace must capture"),
        source_workspace
    );
    assert_eq!(target.item_id("pane/one"), Some(ItemId::new(1)));
    assert_eq!(target.item_id("pane/two"), Some(ItemId::new(2)));
    assert_eq!(
        target.item_id("pane/tombstone"),
        Some(ItemId::new(3)),
        "unreferenced assignments preserve reopen identity"
    );
    assert_eq!(
        target
            .viewport_placement(SURFACE)
            .expect("placement must publish")
            .work_area(),
        None,
        "provider-local work-area identity must be reacquired after restore",
    );
    assert_eq!(
        target.next_generation(),
        Some(20),
        "loading an older generation must not reuse an issued generation"
    );
}

#[test]
fn document_commit_rejects_an_ordinary_workspace_replacement_without_restore_proof() {
    let mut source = session_with(DOCUMENT_ID, 0);
    let document = source.capture().expect("source document must capture");
    let mut target = session_with(DOCUMENT_ID, 1);
    let host = target
        .adapter_create_presentation_host()
        .expect("document restore presentation host must mint");
    let mut restore = target
        .begin_restore(document, |_, _, _| true)
        .expect("document restore must prepare");
    let input = restore
        .take_engine_input()
        .expect("document restore must carry one exact input");
    let EngineInput::RestoreWorkspace(validated) = input else {
        panic!("document transaction must carry a validated workspace restore")
    };
    let transition = submit_restore_input(
        &mut restore,
        host,
        1,
        EngineInput::ReplaceWorkspace(validated.workspace().clone()),
    );

    assert!(matches!(
        restore.commit(&transition),
        Err(DockspaceDocumentSessionError::EnginePublicationMismatch)
    ));
}

#[test]
fn restoring_a_closed_child_remaps_retired_surface_root_and_placement_identities() {
    let child_surface = SurfaceId::new(2);
    let child_root = RootId::new(2);
    let remapped_child_surface = SurfaceId::new(3);
    let remapped_child_root = RootId::new(3);
    let child_rect = PhysicalRect::new(120.0, 160.0, 700.0, 500.0)
        .expect("child fixture rectangle must be valid");
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0);
    bootstrap
        .ensure_external_item_keys(["pane/one", "pane/two", "pane/tombstone"])
        .expect("fixture identities must fit");
    let one = bootstrap
        .item_id("pane/one")
        .expect("first fixture identity exists");
    let two = bootstrap
        .item_id("pane/two")
        .expect("second fixture identity exists");
    bootstrap.set_viewport_placement(
        ViewportPlacementPreference::new(child_surface, child_rect)
            .expect("child fixture placement must be non-empty")
            .with_presentation(WindowPresentationPreference::Maximized),
    );

    let mut builder = Workspace::builder();
    let root_tabs = builder.insert_node(Node::tabs([one]));
    let child_tabs = builder.insert_node(Node::tabs([two]));
    builder.set_root(ROOT, RootRecord::new(root_tabs));
    builder.set_root(child_root, RootRecord::new(child_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let engine = DockEngine::new(
        builder.build().expect("two-surface fixture must build"),
        DockPolicy::default(),
    )
    .expect("two-surface fixture engine must initialize");
    let mut session = DockspaceDocumentSession::bind(engine, bootstrap)
        .expect("two-surface fixture document session must bind");
    let saved = session
        .capture()
        .expect("two-surface document must capture");
    let host = session
        .adapter_create_presentation_host()
        .expect("document restore presentation host must mint");

    let replacement = workspace([one, two]);
    submit_session_input(
        &mut session,
        host,
        1,
        EngineInput::ReplaceWorkspace(replacement),
    );
    assert!(session.workspace().surface(child_surface).is_none());
    assert!(session.workspace().root(child_root).is_none());
    assert_eq!(
        session
            .engine()
            .presentation_identity_frontier()
            .last_surface(),
        2
    );
    assert_eq!(
        session
            .engine()
            .presentation_identity_frontier()
            .last_root(),
        2
    );

    let mut restore = session
        .begin_restore(saved, |_, _, _| true)
        .expect("closed child identities must be rebased before publication");
    let input = restore
        .take_engine_input()
        .expect("document restore must carry one exact input");
    let EngineInput::RestoreWorkspace(validated) = &input else {
        panic!("document transaction must carry a validated workspace restore")
    };
    assert!(validated.workspace().surface(child_surface).is_none());
    assert!(validated.workspace().root(child_root).is_none());
    assert!(
        validated
            .workspace()
            .surface(remapped_child_surface)
            .is_some()
    );
    assert!(validated.workspace().root(remapped_child_root).is_some());

    let transition = submit_restore_input(&mut restore, host, 2, input);
    restore
        .commit(&transition)
        .expect("rebased document restore must commit");

    assert!(session.workspace().surface(child_surface).is_none());
    assert!(session.workspace().root(child_root).is_none());
    assert_eq!(
        session
            .workspace()
            .surface(remapped_child_surface)
            .and_then(|surface| surface.main_root),
        Some(remapped_child_root),
    );
    assert_eq!(session.viewport_placement(child_surface), None);
    assert_eq!(
        session
            .viewport_placement(remapped_child_surface)
            .map(|preference| preference.outer_rect()),
        Some(child_rect),
    );
    assert_eq!(
        session
            .engine()
            .presentation_identity_frontier()
            .last_surface(),
        3
    );
    assert_eq!(
        session
            .engine()
            .presentation_identity_frontier()
            .last_root(),
        3
    );
}

#[test]
fn committed_visible_child_coordinates_refresh_placement_while_hidden_facts_preserve_it() {
    let child_surface = SurfaceId::new(2);
    let child_root = RootId::new(2);
    let child_window = WindowToken::new(2);
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0);
    bootstrap
        .ensure_external_item_keys(["pane/one", "pane/two", "pane/tombstone"])
        .expect("fixture identities must fit");
    let one = bootstrap
        .item_id("pane/one")
        .expect("first fixture identity exists");
    let two = bootstrap
        .item_id("pane/two")
        .expect("second fixture identity exists");
    bootstrap.set_viewport_placement(
        placements()
            .get(SURFACE)
            .copied()
            .expect("fixture placement exists"),
    );
    bootstrap.set_viewport_placement(
        ViewportPlacementPreference::new(
            child_surface,
            PhysicalRect::new(20.0, 30.0, 400.0, 300.0)
                .expect("child fixture rectangle must be valid"),
        )
        .expect("child fixture placement must be non-empty")
        .with_presentation(WindowPresentationPreference::Maximized),
    );

    let mut workspace_builder = Workspace::builder();
    let root_tabs = workspace_builder.insert_node(Node::tabs([one]));
    let child_tabs = workspace_builder.insert_node(Node::tabs([two]));
    workspace_builder.set_root(ROOT, RootRecord::new(root_tabs));
    workspace_builder.set_root(child_root, RootRecord::new(child_tabs));
    workspace_builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    workspace_builder.set_surface(child_surface, SurfacePresentation::with_main(child_root));
    let workspace = workspace_builder
        .build()
        .expect("root and child fixture workspace must be valid");
    let mut policy = DockPolicy::new();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(workspace, policy).expect("fixture engine must initialize");
    let provider = engine
        .create_platform_provider()
        .expect("fixture platform provider must mint");
    let host = engine
        .create_presentation_host()
        .expect("fixture presentation host must mint");
    let mut session = DockspaceDocumentSession::bind(engine, bootstrap)
        .expect("fixture document session must bind");

    let expected = session.engine().version();
    let registered = submit_session_input(
        &mut session,
        host,
        1,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SURFACE,
            token: WINDOW,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    );
    let InputOutcome::ViewportRegistered { .. } = registered.reduced_inputs()[0].outcome() else {
        panic!("fixture root viewport must register");
    };
    let anchor = session
        .engine()
        .root_recovery_anchor(SURFACE)
        .expect("registered root must issue a recovery anchor");
    let recovery_target = SurfaceRecoveryTarget::with_converted_main(
        anchor,
        ConvertedMainRecovery::new(
            child_root,
            FloatingPresentationId::new(1),
            LogicalSize::new(0.0, 0.0).expect("zero child recovery minimum must be valid"),
        ),
    );
    let expected = session.engine().version();
    let registered = submit_session_input(
        &mut session,
        host,
        2,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: child_surface,
            token: child_window,
            role: ViewportRole::Child,
            recovery_target: Some(recovery_target),
        },
    );
    let child_binding = match registered.reduced_inputs()[0].outcome() {
        InputOutcome::ViewportRegistered { binding } => *binding,
        outcome => panic!("fixture child viewport must register: {outcome:?}"),
    };
    let visible_rect = PhysicalRect::new(200.0, 250.0, 1600.0, 1200.0)
        .expect("visible fixture rectangle must be valid");
    let visible_scale = ScaleFactor::new(2.0).expect("visible fixture scale must be valid");
    let expected_epoch = session.engine().version().epoch();
    let visible = submit_session_input(
        &mut session,
        host,
        3,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: platform_snapshot(
                1,
                child_binding,
                visible_rect,
                visible_scale,
                WindowPresentationState::Visible,
            ),
        },
    );
    assert!(matches!(
        visible.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));

    session
        .capture()
        .expect("visible authoritative placement must capture");
    let visible_preference = *session
        .viewport_placement(child_surface)
        .expect("visible placement must be retained");
    assert_eq!(
        session.viewport_placement(SURFACE),
        None,
        "the application root belongs to eframe persistence, not the dockspace document",
    );
    assert_eq!(visible_preference.outer_rect(), visible_rect);
    assert_eq!(visible_preference.inner_size(), Some(visible_rect.size()));
    assert_eq!(visible_preference.scale_factor(), Some(visible_scale));
    assert_eq!(
        visible_preference.presentation(),
        Some(WindowPresentationPreference::Maximized),
        "coordinate refresh must not invent a presentation-mode observation",
    );
    assert_eq!(
        visible_preference.work_area(),
        None,
        "provider-local work-area identity must not survive a durable refresh",
    );

    let hidden_rect = PhysicalRect::new(10.0, 20.0, 300.0, 200.0)
        .expect("hidden fixture rectangle must be valid");
    let expected_epoch = session.engine().version().epoch();
    let hidden = submit_session_input(
        &mut session,
        host,
        4,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot: platform_snapshot(
                2,
                child_binding,
                hidden_rect,
                ScaleFactor::new(1.0).expect("hidden fixture scale must be valid"),
                WindowPresentationState::Hidden,
            ),
        },
    );
    assert!(matches!(
        hidden.reduced_inputs()[0].outcome(),
        InputOutcome::PlatformSnapshotPublished { .. }
    ));

    session
        .capture()
        .expect("hidden viewport must preserve the last trusted placement");
    assert_eq!(
        session.viewport_placement(child_surface),
        Some(&visible_preference),
    );
}

#[test]
fn raw_session_host_frame_cannot_replace_workspace_with_an_unbound_item() {
    let mut session = session_with(DOCUMENT_ID, 0);
    let mut frame = sealed_session_frame(&mut session);

    assert_eq!(
        frame.append_input(
            StableInputSourceId::new(91),
            SourceSequence::new(1),
            EngineInput::ReplaceWorkspace(workspace([ItemId::new(99)])),
        ),
        Err(CoreHostFrameError::ItemIdentityOutsideScope {
            item: ItemId::new(99),
        })
    );
    assert_eq!(
        session.engine().workspace(),
        &workspace([ItemId::new(1), ItemId::new(2)]),
        "rejected raw input must not publish its speculative replacement"
    );
}

#[test]
fn raw_session_host_frame_cannot_open_an_unbound_item() {
    let mut session = session_with(DOCUMENT_ID, 0);
    let tabs = session
        .engine()
        .workspace()
        .root(ROOT)
        .expect("fixture root exists")
        .node;
    let target = DockTarget::Center(
        session
            .engine()
            .workspace()
            .capture_tab_target(ROOT, tabs)
            .expect("fixture tab target must capture"),
    );
    let mut frame = sealed_session_frame(&mut session);

    assert_eq!(
        frame.append_input(
            StableInputSourceId::new(92),
            SourceSequence::new(1),
            EngineInput::WorkspaceCommand {
                expected: frame.view().version(),
                command: WorkspaceCommand::Open {
                    item: ItemId::new(99),
                    target,
                },
            },
        ),
        Err(CoreHostFrameError::ItemIdentityOutsideScope {
            item: ItemId::new(99),
        })
    );
}

#[test]
fn backend_ingress_cannot_replace_a_bound_session_with_an_unbound_item() {
    let mut session = session_with(DOCUMENT_ID, 0);
    let host = session
        .adapter_create_presentation_host()
        .expect("fixture presentation host must mint");
    let mut recorder = session
        .adapter_create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("fixture backend provider must enroll");
    recorder
        .record_semantic_input(EngineInput::ReplaceWorkspace(workspace([ItemId::new(99)])))
        .expect("raw backend recorder accepts semantic workspace input");
    let mut prelude = session
        .adapter_begin_host_frame(host)
        .expect("session host frame must begin");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("empty presentation observation must submit");
    let mut frame = prelude
        .seal(session.engine())
        .expect("session host frame must seal");

    assert_eq!(
        frame.submit_backend_ingress(
            recorder
                .pending_batch()
                .expect("fixture backend batch must freeze"),
        ),
        Err(CoreHostFrameError::ItemIdentityOutsideScope {
            item: ItemId::new(99),
        })
    );
    assert_eq!(
        session.engine().workspace(),
        &workspace([ItemId::new(1), ItemId::new(2)]),
        "rejected backend replay must not publish its speculative replacement"
    );
}

#[test]
fn bound_session_rejects_a_frame_minted_through_its_raw_engine_view() {
    let mut session = session_with(DOCUMENT_ID, 0);
    let host = session
        .adapter_create_presentation_host()
        .expect("fixture presentation host must mint");
    let mut prelude = session
        .engine()
        .begin_host_frame(host)
        .expect("raw engine view can freeze an unrestricted prelude");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("empty presentation observation must submit");
    let mut frame = prelude
        .seal(session.engine())
        .expect("unrestricted raw frame can seal speculatively");
    frame
        .append_input(
            StableInputSourceId::new(93),
            SourceSequence::new(1),
            EngineInput::ReplaceWorkspace(workspace([ItemId::new(99)])),
        )
        .expect("unrestricted raw frame has no session identity scope");
    let frame = frame
        .into_presentation()
        .expect("raw frame can close its semantic prefix");

    assert!(matches!(
        session.adapter_prepare_host_presentation_frame(frame),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::ItemIdentityScopeMismatch,
        })
    ));
    assert_eq!(
        session.engine().workspace(),
        &workspace([ItemId::new(1), ItemId::new(2)]),
        "a frame missing the session scope must never publish"
    );
}

#[test]
fn unbound_session_adopts_a_complete_document_at_one_publish_boundary() {
    let mut source = session_with(DOCUMENT_ID, 7);
    let document = source.capture().expect("source document must capture");
    let engine = DockEngine::new(workspace([ItemId::new(99)]), DockPolicy::default())
        .expect("unbound fixture engine must initialize");
    let mut target =
        DockspaceDocumentSession::unbound(engine).expect("unbound fixture session must initialize");

    apply_document(&mut target, document);

    assert_eq!(target.document_id(), Some(DOCUMENT_ID));
    assert_eq!(target.next_generation(), Some(8));
    assert_eq!(target.item_id("pane/one"), Some(ItemId::new(1)));
    assert!(target.viewport_placement(SURFACE).is_some());
}

#[test]
fn failed_unbound_adoption_never_partially_binds_document_identity() {
    let mut source = session_with(DOCUMENT_ID, 7);
    let document = source.capture().expect("source document must capture");
    let engine = DockEngine::new(workspace([ItemId::new(99)]), DockPolicy::default())
        .expect("unbound fixture engine must initialize");
    let mut target =
        DockspaceDocumentSession::unbound(engine).expect("unbound fixture session must initialize");
    let before = WorkspaceSnapshot::capture(target.engine().workspace())
        .expect("unbound workspace must capture");

    let mut restore = target
        .begin_restore(document, |_, _, _| true)
        .expect("valid document must reserve a restore transaction");
    restore
        .take_engine_input()
        .expect("restore transaction must carry one exact input");
    restore.abort().expect("unchanged restore must abort");
    assert_eq!(target.document_id(), None);
    assert_eq!(target.next_generation(), None);
    assert_eq!(
        WorkspaceSnapshot::capture(target.engine().workspace())
            .expect("unbound workspace must capture"),
        before
    );
    assert!(matches!(
        target.capture(),
        Err(DockspaceDocumentSessionError::Unbound)
    ));
}

#[test]
fn bootstrap_rejects_workspace_items_without_a_key_binding() {
    let engine = DockEngine::new(
        workspace([ItemId::new(1), ItemId::new(2)]),
        DockPolicy::default(),
    )
    .expect("fixture engine must initialize");
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0);
    bootstrap
        .ensure_external_item_key("pane/one")
        .expect("first fixture key fits");

    assert!(matches!(
        DockspaceDocumentSession::bind(engine, bootstrap),
        Err(DockspaceDocumentSessionError::Capture(
            dockspace::document::DockspaceDocumentCaptureError::MissingExternalItemKey(item)
        )) if item == ItemId::new(2)
    ));
}

#[test]
fn bootstrap_rejects_placement_outside_its_workspace_lineage() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0);
    let item = bootstrap
        .ensure_external_item_key("pane/one")
        .expect("fixture key must fit");
    let unknown_surface = SurfaceId::new(77);
    bootstrap.set_viewport_placement(
        ViewportPlacementPreference::new(
            unknown_surface,
            PhysicalRect::new(0.0, 0.0, 640.0, 480.0)
                .expect("fixture placement rectangle must be valid"),
        )
        .expect("fixture placement must be non-empty"),
    );
    let engine = DockEngine::new(workspace([item]), DockPolicy::default())
        .expect("fixture engine must initialize");

    assert!(matches!(
        DockspaceDocumentSession::bind(engine, bootstrap),
        Err(DockspaceDocumentSessionError::Capture(
            dockspace::document::DockspaceDocumentCaptureError::UnknownPlacementSurface(surface)
        )) if surface == unknown_surface
    ));
}

#[test]
fn bound_map_cannot_be_replaced_and_new_keys_use_the_session_owner() {
    let mut session = session_with(DOCUMENT_ID, 0);
    assert!(matches!(
        session.bind_bootstrap(DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0)),
        Err(DockspaceDocumentSessionError::AlreadyBound)
    ));
    assert_eq!(
        session
            .ensure_external_item_key("pane/later")
            .expect("session-owned allocation must succeed"),
        ItemId::new(4)
    );
    assert_eq!(session.item_id("pane/one"), Some(ItemId::new(1)));
}

#[test]
fn bound_session_rejects_new_workspace_items_without_session_identity() {
    let mut session = session_with(DOCUMENT_ID, 0);
    let unbound_item = ItemId::new(99);
    let open = WorkspaceCommand::CreateSurfaceRoot {
        surface: SurfaceId::new(2),
        root: RootId::new(2),
        content: RootContent::OpenItem(unbound_item),
    };

    assert!(matches!(
        session.validate_workspace_command_identity_bindings(&open),
        Err(DockspaceDocumentSessionError::UnknownExternalItemKey(item)) if item == unbound_item
    ));
    assert!(matches!(
        session.validate_workspace_identity_bindings(&workspace([unbound_item])),
        Err(DockspaceDocumentSessionError::UnknownExternalItemKey(item)) if item == unbound_item
    ));

    let owned_item = session
        .ensure_external_item_key("pane/later")
        .expect("session identity allocation must succeed");
    let owned_open = WorkspaceCommand::CreateSurfaceRoot {
        surface: SurfaceId::new(2),
        root: RootId::new(2),
        content: RootContent::OpenItem(owned_item),
    };
    session
        .validate_workspace_command_identity_bindings(&owned_open)
        .expect("session-owned item must be admissible");
    session
        .validate_workspace_identity_bindings(&workspace([ItemId::new(1), owned_item]))
        .expect("replacement workspace must use session-owned items");
}

#[test]
fn two_bound_sessions_cannot_exchange_swapped_key_histories() {
    let mut canonical = session_with(DOCUMENT_ID, 0);
    let mut swapped = session_with_untrusted_map(swapped_item_keys());
    let swapped_document = swapped.capture().expect("swapped session captures itself");

    assert!(matches!(
        canonical.begin_restore(swapped_document, |_, _, key| key.starts_with("pane/")),
        Err(DockspaceDocumentSessionError::Reconcile(
            ExternalItemKeyReconcileError::ExternalKeyConflict {
                external_key,
                active_item,
                incoming_item,
            }
        )) if external_key == "pane/one"
            && active_item == ItemId::new(1)
            && incoming_item == ItemId::new(2)
    ));
    assert_eq!(canonical.item_id("pane/one"), Some(ItemId::new(1)));
    assert_eq!(canonical.next_generation(), Some(0));
}

#[test]
fn unbound_session_rejects_a_self_consistent_but_swapped_item_key_document() {
    let mut swapped = session_with_untrusted_map(swapped_item_keys());
    let swapped_document = swapped
        .capture()
        .expect("swapped document is internally valid and freshly hashed");
    let engine = DockEngine::new(
        workspace([ItemId::new(1), ItemId::new(2)]),
        DockPolicy::default(),
    )
    .expect("target engine must initialize");
    let mut target =
        DockspaceDocumentSession::unbound(engine).expect("target session identity must fit");
    let error = target
        .begin_restore(swapped_document, |document_id, item, key| {
            document_id == DOCUMENT_ID
                && ((item == ItemId::new(1) && key == "pane/one")
                    || (item == ItemId::new(2) && key == "pane/two")
                    || (item == ItemId::new(3) && key == "pane/tombstone"))
        })
        .expect_err("application identity proof must reject swapped assignments");

    assert!(matches!(
        error,
        DockspaceDocumentSessionError::Restore(
            DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { rejected }
        ) if rejected.len() == 2
    ));
    assert_eq!(target.document_id(), None);
}

#[test]
fn untrusted_import_rejects_a_valid_but_swapped_item_key_map() {
    let engine = DockEngine::new(
        workspace([ItemId::new(1), ItemId::new(2)]),
        DockPolicy::default(),
    )
    .expect("fixture engine must initialize");
    let mut session =
        DockspaceDocumentSession::unbound(engine).expect("fixture session identity must fit");

    assert!(matches!(
        session.bind_untrusted_import(
            DOCUMENT_ID,
            0,
            swapped_item_keys(),
            ViewportPlacementPreferences::new(),
            |item, key| {
                (item == ItemId::new(1) && key == "pane/one")
                    || (item == ItemId::new(2) && key == "pane/two")
            },
        ),
        Err(DockspaceDocumentSessionError::ExternalItemAssociationRejected {
            item,
            external_key,
        }) if item == ItemId::new(2) && external_key == "pane/one"
    ));
    assert_eq!(session.document_id(), None);
    assert!(matches!(
        session.capture(),
        Err(DockspaceDocumentSessionError::Unbound)
    ));
}

#[test]
fn untrusted_import_requires_proof_for_closed_pane_history() {
    let engine = DockEngine::new(
        workspace([ItemId::new(1), ItemId::new(2)]),
        DockPolicy::default(),
    )
    .expect("fixture engine must initialize");
    let mut session =
        DockspaceDocumentSession::unbound(engine).expect("fixture session identity must fit");

    assert!(matches!(
        session.bind_untrusted_import(
            DOCUMENT_ID,
            0,
            canonical_item_keys(),
            ViewportPlacementPreferences::new(),
            |_, key| key != "pane/tombstone",
        ),
        Err(DockspaceDocumentSessionError::ExternalItemAssociationRejected {
            item,
            external_key,
        }) if item == ItemId::new(3) && external_key == "pane/tombstone"
    ));
    assert_eq!(session.document_id(), None);
}

#[test]
fn dropped_restore_transaction_is_automatically_aborted_by_the_same_session() {
    let mut source = session_with(DOCUMENT_ID, 0);
    let document = source.capture().expect("source captures");
    let mut target = session_with(DOCUMENT_ID, 1);
    let before = WorkspaceSnapshot::capture(target.workspace()).expect("target captures");

    {
        let mut restore = target
            .begin_restore(document, |_, _, _| true)
            .expect("valid document must reserve a restore transaction");
        restore
            .take_engine_input()
            .expect("restore transaction must carry one exact input");
    }
    assert_eq!(
        WorkspaceSnapshot::capture(target.workspace()).expect("target captures"),
        before
    );
    assert!(
        target.capture().is_ok(),
        "rollback releases the reservation"
    );
}

#[test]
fn wrong_document_lineage_fails_before_registry_or_state_change() {
    let mut source = session_with(OTHER_DOCUMENT_ID, 2);
    let document = source.capture().expect("foreign lineage captures");
    let mut target = session_with(DOCUMENT_ID, 9);
    let resolver_called = std::cell::Cell::new(false);

    assert!(matches!(
        target.begin_restore(document, |_, _, _| {
            resolver_called.set(true);
            true
        }),
        Err(DockspaceDocumentSessionError::WrongLineage {
            expected: DOCUMENT_ID,
            found: OTHER_DOCUMENT_ID,
        })
    ));
    assert!(!resolver_called.get());
    assert_eq!(target.next_generation(), Some(9));
}

#[test]
fn restore_queries_every_historical_key_and_aggregates_rejection() {
    let mut source = session_with(DOCUMENT_ID, 0);
    let document = source.capture().expect("source captures");
    let mut target = session_with(DOCUMENT_ID, 1);
    let observed = RefCell::new(Vec::new());

    let error = target
        .begin_restore(document, |document_id, item, key| {
            observed
                .borrow_mut()
                .push((document_id, item, key.to_owned()));
            false
        })
        .expect_err("rejected pane associations must reject the document");
    assert!(matches!(
        error,
        DockspaceDocumentSessionError::Restore(
            DockspaceDocumentRestoreError::ExternalItemAssociationsRejected { rejected }
        ) if rejected.len() == 3
    ));
    assert_eq!(
        observed.into_inner(),
        vec![
            (DOCUMENT_ID, ItemId::new(1), "pane/one".to_owned()),
            (DOCUMENT_ID, ItemId::new(3), "pane/tombstone".to_owned()),
            (DOCUMENT_ID, ItemId::new(2), "pane/two".to_owned()),
        ]
    );
}

#[test]
fn workspace_key_placement_and_generation_splices_fail_the_same_hash() {
    let mut source = session_with(DOCUMENT_ID, 11);
    let document = source.capture().expect("source captures");
    let encoded = serde_json::to_value(document).expect("document must encode");

    let mut mutations = Vec::new();
    let mut generation = encoded.clone();
    generation[1]["generation"] = serde_json::Value::from(12_u64);
    mutations.push(generation);

    let mut workspace = encoded.clone();
    workspace[1]["workspace"][1]["nodes"][0]["node"]["selected"] = serde_json::Value::from(1_u64);
    mutations.push(workspace);

    let mut keys = encoded.clone();
    keys[1]["external_item_keys"][1]["entries"][0]["external_key"] =
        serde_json::Value::from("pane/rebound");
    mutations.push(keys);

    let mut placement = encoded;
    placement[1]["viewport_placements"][1]["placements"][0]["outer_rect"]["x"] =
        serde_json::Value::from(999.0);
    mutations.push(placement);

    for mutation in mutations {
        let document = decode_document(mutation);
        let mut target = session_with(DOCUMENT_ID, 20);
        assert!(matches!(
            target.begin_restore(document, |_, _, _| true),
            Err(DockspaceDocumentSessionError::Restore(
                DockspaceDocumentRestoreError::BindingHashMismatch
            ))
        ));
        assert_eq!(target.next_generation(), Some(20));
    }
}

#[test]
fn failed_or_aborted_restore_rolls_back_all_owned_state() {
    let mut source = session_with(DOCUMENT_ID, 2);
    let document = source.capture().expect("source captures");
    let mut target = session_with(DOCUMENT_ID, 9);
    let before_workspace =
        WorkspaceSnapshot::capture(target.workspace()).expect("target workspace captures");
    let before_placement = *target
        .viewport_placement(SURFACE)
        .expect("target placement exists");
    let mut restore = target
        .begin_restore(document, |_, _, _| true)
        .expect("valid document must reserve a restore transaction");
    restore
        .take_engine_input()
        .expect("restore transaction must carry one exact input");
    restore.abort().expect("unchanged restore must abort");

    assert_eq!(
        WorkspaceSnapshot::capture(target.workspace()).expect("target workspace captures"),
        before_workspace
    );
    assert_eq!(
        *target
            .viewport_placement(SURFACE)
            .expect("target placement remains"),
        before_placement
    );
    assert_eq!(target.item_id("pane/one"), Some(ItemId::new(1)));
    assert_eq!(target.next_generation(), Some(9));
}

#[test]
fn unsupported_outer_and_nested_versions_remain_typed() {
    let envelope =
        serde_json::from_str::<DockspaceDocumentEnvelope>(r#"[9,{"future":{"shape":[1,2,3]}}]"#)
            .expect("future payload must be skipped");
    assert_eq!(envelope.version(), 9);
    assert!(matches!(
        envelope.into_document(),
        Err(DockspaceDocumentDecodeError::UnsupportedVersion {
            found: 9,
            supported: DOCKSPACE_DOCUMENT_VERSION,
        })
    ));

    let mut source = session_with(DOCUMENT_ID, 7);
    let document = source.capture().expect("source captures");
    let mut workspace_version = serde_json::to_value(&document).expect("document encodes");
    workspace_version[1]["workspace"][0] = serde_json::Value::from(9_u64);
    let envelope = serde_json::from_value::<DockspaceDocumentEnvelope>(workspace_version)
        .expect("outer shape remains valid");
    assert!(matches!(
        envelope.into_document(),
        Err(DockspaceDocumentDecodeError::Workspace(
            dockspace::persistence::SnapshotRestoreError::UnsupportedVersion { found: 9, .. }
        ))
    ));

    let mut placement_version = serde_json::to_value(document).expect("document encodes");
    placement_version[1]["viewport_placements"][0] = serde_json::Value::from(9_u64);
    let envelope = serde_json::from_value::<DockspaceDocumentEnvelope>(placement_version)
        .expect("outer shape remains valid");
    assert!(matches!(
        envelope.into_document(),
        Err(DockspaceDocumentDecodeError::ViewportPlacements(
            dockspace::viewport_persistence::ViewportPlacementRestoreError::UnsupportedVersion {
                found: 9,
                ..
            }
        ))
    ));
}
