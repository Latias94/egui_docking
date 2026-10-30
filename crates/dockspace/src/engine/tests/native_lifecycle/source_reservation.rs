//! Frozen source proofs for partial payloads awaiting native admission.

use super::*;

use crate::frame::{NativeCreatePhase, NativeCreateRequest};

#[derive(Clone, Copy)]
enum PartialPayload {
    Item,
    Tabs,
    Subtree,
}

fn reserve_partial_source(
    fixture: &mut BackgroundFixture,
    payload: MovePayload,
    child_id: u64,
) -> NativeCreateRequest {
    assert_eq!(
        fixture
            .engine
            .complete_root_source(&payload)
            .expect("the partial source proof must be current"),
        None,
        "the reservation must exercise partial extraction rather than root rehome"
    );
    let surface = SurfaceId::new(child_id);
    let root = RootId::new(child_id);
    let converted = crate::surface_recovery::ConvertedMainRecovery::new(
        root,
        FloatingPresentationId::new(child_id),
        LogicalSize::new(0.0, 0.0).expect("recovery minimum must be valid"),
    );
    let proposal = crate::frame::NativeCreateProposal::new(
        surface,
        PhysicalRect::new(500.0, 0.0, 300.0, 220.0).expect("child placement must be valid"),
        converted,
    );
    let command = WorkspaceCommand::CreateSurfaceRoot {
        surface,
        root,
        content: RootContent::Move(payload.clone()),
    };
    let source_presentation = fixture
        .engine
        .interaction_authority(SOURCE_SURFACE)
        .expect("the source must have exact presented authority");
    let focus_causal = mint_test_focus_causal(&mut fixture.engine);
    let policy = fixture.engine.policy.clone();
    let request = fixture
        .engine
        .start_native_root_create(
            tab_strip_test_cause(),
            focus_causal,
            source_presentation,
            payload,
            command,
            proposal,
            PaneFocusDisposition::Clear,
            &policy,
        )
        .expect("partial native creation must not fail an engine invariant")
        .expect("the exact partial native create must be accepted");
    let _ = fixture.engine.viewport.take_new_effects();
    request
}

fn partial_source_fixture(kind: PartialPayload) -> (BackgroundFixture, NativeCreateRequest) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let branch_peer = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let branch = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_tabs, branch_peer])
            .expect("the partial subtree must be valid"),
    );
    let source_peer = builder.insert_node(Node::tabs([ItemId::new(5), ItemId::new(6)]));
    let source_node = builder.insert_node(
        Node::equal_split(Axis::Vertical, [branch, source_peer])
            .expect("the source root must retain a peer after partial extraction"),
    );
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(3), ItemId::new(7)]));
    let floating = FloatingPresentationId::new(2);
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_node));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(TARGET_ROOT, background_contained_rect()),
    );
    builder
        .attach_contained(TARGET_SURFACE, floating)
        .expect("the unrelated root must have a presentation owner");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut engine = DockEngine::new(
        builder
            .build()
            .expect("the partial source workspace must be valid"),
        policy,
    )
    .expect("the partial source engine must initialize");
    let presentation_host = engine
        .create_presentation_host()
        .expect("the presentation host must mint");
    let mut fixture = BackgroundFixture {
        engine,
        presentation_host,
        source_tabs,
    };
    publish_background_scene(&mut fixture);
    let _ = publish_native_background_platform(&mut fixture);
    let workspace = fixture.engine.workspace();
    let payload = match kind {
        PartialPayload::Item => MovePayload::Item(
            workspace
                .capture_item_source(SOURCE_ROOT, source_tabs, ItemId::new(1))
                .expect("the partial item source must capture"),
        ),
        PartialPayload::Tabs => MovePayload::Tabs(
            workspace
                .capture_node_source(SOURCE_ROOT, source_tabs)
                .expect("the partial tabs source must capture"),
        ),
        PartialPayload::Subtree => MovePayload::Subtree(
            workspace
                .capture_node_source(SOURCE_ROOT, branch)
                .expect("the partial split subtree source must capture"),
        ),
    };
    let request = reserve_partial_source(&mut fixture, payload, 93);
    (fixture, request)
}

fn assert_source_edits_are_frozen(fixture: &mut BackgroundFixture, request: NativeCreateRequest) {
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("the partial native create must remain pending")
        .clone();
    let resources = fixture
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(resources.len(), 1);
    assert!(fixture.engine.native_create_reserves_root(SOURCE_ROOT));
    assert!(fixture.engine.native_create_reserves_root(RootId::new(93)));
    assert!(!fixture.engine.native_create_reserves_root(TARGET_ROOT));

    let select = workspace
        .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(2))
        .expect("the alternate source selection must capture");
    let reorder = workspace
        .capture_item_source(SOURCE_ROOT, fixture.source_tabs, ItemId::new(1))
        .expect("the source reorder must capture");
    let peer_select = workspace
        .capture_item_source_by_id(ItemId::new(6))
        .expect("the peer selection must capture")
        .expect("the peer item must remain in the source root");
    for command in [
        WorkspaceCommand::Select { source: select },
        WorkspaceCommand::Reorder {
            source: reorder,
            insertion_index: 2,
        },
        WorkspaceCommand::Select {
            source: peer_select,
        },
    ] {
        assert!(matches!(
            fixture
                .engine
                .stage_journal_workspace_command(
                    tab_strip_test_cause(),
                    &command,
                    fixture.engine.policy_snapshot(),
                )
                .expect("shared journal staging must return a typed rejection"),
            Err(CommandError::SurfaceLifecycleFrozen {
                surface: SOURCE_SURFACE,
            })
        ));
        let transition = submit_test_input(
            &mut fixture.engine,
            fixture.presentation_host,
            EngineInput::WorkspaceCommand {
                expected: version,
                command,
            },
        )
        .expect("a frozen partial source edit must not fail the host frame invariant");
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::CommandRejected {
                error: CommandError::SurfaceLifecycleFrozen {
                    surface: SOURCE_SURFACE,
                },
                version: current,
            } if *current == version
        ));
        assert!(transition.events().is_empty());
        assert_eq!(fixture.engine.workspace(), &workspace);
        assert_eq!(fixture.engine.version(), version);
        assert_eq!(
            fixture.engine.viewport().native_create_saga(request.saga()),
            Some(&saga)
        );
        assert_eq!(
            fixture
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            resources
        );
    }

    let unrelated = workspace
        .capture_item_source_by_id(ItemId::new(7))
        .expect("the unrelated selection must capture")
        .expect("the unrelated item must exist");
    let target_tabs = unrelated.tabs();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected: version,
            command: WorkspaceCommand::Select { source: unrelated },
        },
    )
    .expect("an unrelated root edit must remain a normal host frame commit");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert_eq!(
        fixture.engine.workspace().node(target_tabs),
        Some(&Node::tabs_with_selection(
            [ItemId::new(3), ItemId::new(7)],
            Some(ItemId::new(7)),
        ))
    );
    assert_eq!(
        fixture.engine.version(),
        WorkspaceVersion::new(
            version.epoch(),
            version
                .revision()
                .checked_next()
                .expect("the revision must advance"),
        )
    );
    assert_eq!(
        fixture.engine.viewport().native_create_saga(request.saga()),
        Some(&saga)
    );
    assert_eq!(
        fixture
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        resources
    );
    fixture
        .engine
        .validate_payload(saga.prepared().payload())
        .expect("the frozen source proof must survive the unrelated root edit");
    fixture
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .expect("partial source rejection must preserve retained resource ownership");
}

#[test]
fn partial_native_source_edits_are_typed_rejections_while_awaiting_hidden() {
    for kind in [
        PartialPayload::Item,
        PartialPayload::Tabs,
        PartialPayload::Subtree,
    ] {
        let (mut fixture, request) = partial_source_fixture(kind);
        assert!(matches!(
            fixture
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .expect("the hidden child must retain its create saga")
                .phase(),
            NativeCreatePhase::AwaitingHidden { .. }
        ));
        assert_source_edits_are_frozen(&mut fixture, request);
    }
}

#[test]
fn partial_native_source_edits_are_typed_rejections_while_awaiting_first_live() {
    for kind in [
        PartialPayload::Item,
        PartialPayload::Tabs,
        PartialPayload::Subtree,
    ] {
        let (mut fixture, request) = partial_source_fixture(kind);
        let _ = advance_pending_native_root_to_ownership_transfer(&mut fixture, request);
        assert!(matches!(
            fixture
                .engine
                .viewport()
                .native_create_saga(request.saga())
                .expect("the first-live child must retain its create saga")
                .phase(),
            NativeCreatePhase::AwaitingFirstLivePresentation { .. }
        ));
        assert_source_edits_are_frozen(&mut fixture, request);
    }
}

#[test]
fn native_commit_exempts_only_its_own_partial_source_reservation() {
    let (mut fixture, request) = partial_source_fixture(PartialPayload::Item);
    let prepared = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .expect("the partial create must retain its frozen command")
        .prepared()
        .clone();
    let (publication, application) = fixture
        .engine
        .stage_workspace_command_for_native_commit(
            fixture.engine.last_input,
            fixture.engine.policy_snapshot(),
            prepared.command(),
            None,
            request.saga(),
        )
        .expect("the owning native commit must stage without an invariant failure");
    assert!(matches!(
        application,
        CommandApplication::Applied { changed: true, .. }
    ));
    let publication = publication.expect("the owning native commit must prepare publication");
    assert_eq!(
        fixture.engine.first_pending_presentation_source_mismatch(
            &publication.workspace,
            WorkspacePublicationAuthority::Ordinary,
        ),
        Some(SOURCE_SURFACE)
    );

    let other_payload = background_payload(&fixture, ItemId::new(2));
    let other = reserve_partial_source(&mut fixture, other_payload, 94);
    assert_ne!(other.saga(), request.saga());
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let viewport = fixture.engine.viewport().clone();
    let (publication, application) = fixture
        .engine
        .stage_workspace_command_for_native_commit(
            fixture.engine.last_input,
            fixture.engine.policy_snapshot(),
            prepared.command(),
            None,
            request.saga(),
        )
        .expect("another pending source must produce a typed native commit rejection");
    assert!(publication.is_none());
    assert!(matches!(
        application,
        CommandApplication::Rejected(CommandError::SurfaceLifecycleFrozen {
            surface: SOURCE_SURFACE,
        })
    ));
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert_eq!(fixture.engine.viewport(), &viewport);
}
