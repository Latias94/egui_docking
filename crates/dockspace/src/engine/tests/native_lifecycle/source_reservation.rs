//! Frozen source proofs for partial payloads awaiting native admission.

use super::*;

use crate::frame::{NativeCreatePhase, NativeCreateRequest};
use crate::geometry::PhysicalPoint;
use crate::interaction::{InteractionEventKind, PreviewVisual};
use crate::model::LogicalSurfaceRejection;
use crate::platform::{
    InputEffectAcknowledgement, NativeDragSourceInput, WindowInputObservation, WindowInputState,
};
use crate::pointer_journal::{
    DesktopRouteFact, DesktopWorkAreaRoute, PointerEventDeliveryOwner, PointerInputLease,
};
use crate::pointer_receiver::PointerReceiverProbeRequest;
use crate::viewport::InputObservationGeneration;

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

fn submit_native_reservation_edge(
    fixture: &mut BackgroundFixture,
    provider: PointerInputLease,
    previous: u64,
    kind: PointerEdgeKind,
    route: DesktopRouteFact,
) -> EngineTransition {
    let binding = background_source_binding(fixture);
    let capture = if kind == PointerEdgeKind::ButtonReleased(PointerButton::Primary) {
        PointerCaptureOwner::None
    } else {
        PointerCaptureOwner::Native(binding)
    };
    let sequence = PointerEdgeSequence::new(previous + 1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new_with_delivery(
            sequence,
            TEST_POINTER,
            kind,
            PointerEdgeLocation::Desktop { route },
            Authority::Known(PointerEventDeliveryOwner::Native(binding)),
            Authority::Known(capture),
        )],
    )
    .expect("the native reservation gesture must have contiguous edges");
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("the native reservation gesture must stage");
    let [candidate] = frame
        .pointer_receiver_candidates()
        .expect("the native edge must freeze its exact receiver roster")
        .candidates()
    else {
        panic!("one native edge must freeze one receiver candidate");
    };
    let observation = match candidate.probes() {
        PointerReceiverProbeRequest::Delivery => {
            assert_eq!(kind, PointerEdgeKind::ButtonPressed(PointerButton::Primary));
            let projection = frame
                .view()
                .interaction_projection(SOURCE_SURFACE)
                .expect("the press must use the source's presented projection");
            let region = projection
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| {
                    matches!(region.id().kind(), PresentationHitRegionKind::TabBody(tab)
                        if tab.item == ItemId::new(1))
                })
                .expect("the selected source tab must have an exact receiver")
                .id();
            let delivery = PointerReceiverDelivery::new(
                projection,
                PointerReceiverDeliveryDisposition::Dock(region),
            )
            .expect("the native press must bind its actual source receiver");
            PointerReceiverObservation::Presented(
                PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                    delivery,
                )])
                .expect("the native press must answer its delivery probe"),
            )
        }
        PointerReceiverProbeRequest::NotApplicable => PointerReceiverObservation::NotApplicable,
        PointerReceiverProbeRequest::HoverHit => {
            panic!("an explicit outside-all route must not request a surface hover receiver")
        }
    };
    let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
        .expect("the native gesture must answer exactly its frozen receiver roster");
    frame
        .submit_pointer_receiver_receipts(receipts)
        .expect("the native gesture receiver receipt must reduce");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("the real native gesture must commit")
}

fn native_drag_reservation_fixture() -> (BackgroundFixture, PointerInputLease, DesktopRouteFact) {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut fixture = background_fixture(policy);
    let work_area = publish_native_background_platform(&mut fixture);
    let binding = background_source_binding(&fixture);
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    let observed = ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(2),
            Authority::Known(PhysicalRect::new(0.0, 0.0, 400.0, 300.0).unwrap()),
            Authority::Known(PhysicalRect::new(-8.0, -30.0, 416.0, 338.0).unwrap()),
            Authority::Known(ScaleFactor::new(1.0).unwrap()),
            Authority::Known(ScaleFactor::new(1.0).unwrap()),
        ))
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(2),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            crate::viewport::PresentationObservationGeneration::new(2),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ));
    let snapshot = test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(2),
            Authority::Known(GlobalFocusedWindow::Foreign),
            Authority::Known(None),
        ),
        vec![observed],
        Vec::new(),
        known_work_area_observation(
            2,
            vec![ObservedWorkArea::new(
                work_area,
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
                ScaleFactor::new(1.0).unwrap(),
            )],
        ),
    )
    .expect("the native drag must have authoritative routing, input, and placement facts");
    let platform_provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider: platform_provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("the physical native drag capabilities must publish");
    publish_background_scene(&mut fixture);
    let projection = fixture
        .engine
        .interaction_projection(SOURCE_SURFACE)
        .unwrap();
    let source = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(region.id().kind(), PresentationHitRegionKind::TabBody(tab)
                if tab.item == ItemId::new(1))
        })
        .expect("the partial source tab must have a presented receiver");
    let point = region_center(source);
    let press_route = DesktopRouteFact::dock_from_desktop_position(
        binding,
        Authority::Known(PhysicalPoint::new(point.x(), point.y()).unwrap()),
    );
    let provider = fixture
        .engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("the native drag pointer provider must enroll");
    let pressed = submit_native_reservation_edge(
        &mut fixture,
        provider,
        0,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press_route,
    );
    assert!(matches!(
        pressed.reduced_pointer_edges()[0].interaction_outcomes(),
        [InteractionOutcome::DragArmed { .. }]
    ));
    let outside = DesktopRouteFact::no_window(
        Authority::Known(PhysicalPoint::new(1000.0, 700.0).unwrap()),
        Authority::Known(DesktopWorkAreaRoute::new(
            platform_provider,
            fixture.engine.viewport().work_area_generation(),
            work_area,
        )),
    );
    for previous in [1, 2] {
        let moved = submit_native_reservation_edge(
            &mut fixture,
            provider,
            previous,
            PointerEdgeKind::Moved,
            outside,
        );
        assert!(
            moved.reduced_pointer_edges()[0]
                .interaction_outcomes()
                .iter()
                .any(|outcome| {
                    matches!(outcome, InteractionOutcome::PreviewUpdated {
                        preview: Some(preview), ..
                    } if matches!(preview.visual(), PreviewVisual::Native { .. }))
                })
        );
    }
    assert_eq!(
        fixture
            .engine
            .viewport()
            .confirmed_drag_source(TEST_POINTER),
        Some(binding)
    );
    assert_eq!(
        fixture.engine.viewport().drag_source_input(TEST_POINTER),
        Some(NativeDragSourceInput::Preserve)
    );
    (fixture, provider, outside)
}

fn stage_native_reservation_checkpoint(
    frame: &mut CoreHostFrame,
    provider: PointerInputLease,
    through: u64,
) {
    let through = PointerEdgeSequence::new(through);
    frame
        .submit_pointer_journal(
            provider,
            PointerEdgeJournal::new(through, through, Vec::new()).unwrap(),
        )
        .expect("the native reservation frame must retain its real pointer watermark");
    frame
        .submit_pointer_receiver_receipts(PointerReceiverReceiptBatch::new([]).unwrap())
        .expect("the empty pointer checkpoint has no receiver probes");
}

#[derive(Debug, Clone, Copy)]
enum ReservedDragPhase {
    Active,
    PendingRelease,
}

#[derive(Debug, Clone, Copy)]
enum ReservedDragIdentity {
    Surface,
    Root,
}

fn assert_logical_attach_preserves_native_drag_reservation(
    phase: ReservedDragPhase,
    collision: ReservedDragIdentity,
) {
    let (mut fixture, provider, outside) = native_drag_reservation_fixture();
    let InteractionStatus::Dragging { session } = fixture.engine.interaction().status() else {
        panic!("the real press and move must leave an active drag");
    };
    let drag = fixture.engine.interaction.active_drag(session).unwrap();
    let reservation = drag.journal_presentation_reservation.unwrap();
    assert_eq!(drag.complete_root, None, "the drag must reserve a new root");
    let preview = drag.preview().unwrap().public().token();
    assert_eq!(
        drag.preview().unwrap().public().visual().surface(),
        SOURCE_SURFACE
    );
    assert_eq!(
        drag.preview().unwrap().public().visual().target_surface(),
        Some(reservation.surface)
    );
    let through = match phase {
        ReservedDragPhase::Active => 3,
        ReservedDragPhase::PendingRelease => {
            let released = submit_native_reservation_edge(
                &mut fixture,
                provider,
                3,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                outside,
            );
            assert!(
                released.reduced_pointer_edges()[0]
                    .interaction_outcomes()
                    .iter()
                    .any(|outcome| {
                        matches!(outcome, InteractionOutcome::ReleasePending {
                            session: actual, preview: token,
                        } if *actual == session && *token == preview)
                    })
            );
            assert_eq!(
                fixture.engine.interaction().status(),
                InteractionStatus::Idle
            );
            assert_eq!(
                fixture.engine.pending_release_preview(),
                Some((session, preview))
            );
            4
        }
    };
    let (surface, root) = match collision {
        ReservedDragIdentity::Surface => (reservation.surface, RootId::new(1000)),
        ReservedDragIdentity::Root => (SurfaceId::new(1000), reservation.root),
    };
    assert!(fixture.engine.workspace().surface(surface).is_none());
    assert!(fixture.engine.workspace().root(root).is_none());
    assert_ne!(surface, SOURCE_SURFACE);
    assert_ne!(surface, TARGET_SURFACE);
    assert!(!fixture.engine.native_create_reserves_root(root));
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .is_none()
    );
    let workspace = fixture.engine.workspace().clone();
    let version = fixture.engine.version();
    let interaction = fixture.engine.interaction().clone();
    let pending = fixture.engine.pending_mutation.clone();
    let identities = fixture.engine.presentation_identity;
    let mut inputs = TestInputStream::resume(&fixture.engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    stage_native_reservation_checkpoint(&mut frame, provider, through);
    inputs
        .append(
            &mut frame,
            EngineInput::AttachEmptySurface {
                expected: version,
                surface,
                root,
            },
        )
        .expect("a colliding logical attach must enter the existing host-frame transaction");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    let rejected = frame.finish(&mut fixture.engine).unwrap();
    assert!(
        matches!(
            rejected.reduced_inputs()[0].outcome(),
            InputOutcome::LogicalSurfaceRejected {
                surface: actual,
                reason: LogicalSurfaceRejection::RecoveryConflict,
            } if *actual == surface
        ),
        "{phase:?} {collision:?} collision must be a typed refusal: {:?}",
        rejected.reduced_inputs()
    );
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
    assert_eq!(fixture.engine.interaction(), &interaction);
    assert_eq!(fixture.engine.pending_mutation, pending);
    assert_eq!(fixture.engine.presentation_identity, identities);
    assert!(rejected.events().is_empty());
    assert!(rejected.interaction_events().is_empty());
    let retained_reservation = match phase {
        ReservedDragPhase::Active => {
            fixture
                .engine
                .interaction
                .active_drag(session)
                .unwrap()
                .journal_presentation_reservation
        }
        ReservedDragPhase::PendingRelease => {
            fixture
                .engine
                .pending_mutation
                .as_ref()
                .and_then(PendingMutation::drag_release)
                .unwrap()
                .drag
                .journal_presentation_reservation
        }
    };
    assert_eq!(retained_reservation, Some(reservation));

    let mut paint = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    stage_native_reservation_checkpoint(&mut paint, provider, through);
    complete_host_frame_with_current_outputs(&fixture.engine, &mut paint);
    let painted = paint.finish(&mut fixture.engine).unwrap();
    let outputs = painted
        .presentation_emissions()
        .iter()
        .map(|emission| emission.output())
        .collect::<Vec<_>>();
    assert!(outputs.iter().any(|output| {
        output.surface() == SOURCE_SURFACE
            && output
                .payload()
                .interaction()
                .is_some_and(|interaction| interaction.drag_preview() == Some(preview))
    }));
    assert!(
        fixture
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .is_none(),
        "painting the preview must not deliver before its exact presentation"
    );
    let mut prelude = fixture
        .engine
        .begin_host_frame(fixture.presentation_host)
        .unwrap();
    prelude
        .submit_presentation_observation(HostPresentationObservation::Batch(
            outputs
                .into_iter()
                .map(|output| {
                    HostPresentationObservationEntry::new(
                        output.stream(),
                        HostPresentationStreamObservation::Captured {
                            generation: HostPresentationCaptureGeneration::new(
                                output.key().ordinal_for_test(),
                            ),
                            progress: HostPresentationProgress::Retired {
                                settled_through: output.key(),
                                presented: Authority::Known(Some(output.key())),
                            },
                        },
                    )
                })
                .collect(),
        ))
        .expect("only the original preview's exact emitted outputs authorize delivery");
    let mut observed = prelude.seal(&fixture.engine).unwrap();
    stage_native_reservation_checkpoint(&mut observed, provider, through);
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut observed);
    let presented = observed.finish(&mut fixture.engine).unwrap();
    let delivered = match phase {
        ReservedDragPhase::Active => {
            assert!(
                fixture
                    .engine
                    .viewport()
                    .native_create_sagas()
                    .next()
                    .is_none()
            );
            assert_eq!(
                fixture.engine.interaction().status(),
                InteractionStatus::Dragging { session }
            );
            submit_native_reservation_edge(
                &mut fixture,
                provider,
                through,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                outside,
            )
        }
        ReservedDragPhase::PendingRelease => presented,
    };
    let request = delivered
        .interaction_events()
        .iter()
        .find_map(|event| match event.kind() {
            InteractionEventKind::NativePresentationRequested(request) => Some(*request),
            _ => None,
        })
        .expect(
            "the preserved drag must deliver its original native creation after exact presentation",
        );
    assert_eq!(request.binding().surface(), reservation.surface);
    assert_eq!(fixture.engine.viewport().native_create_sagas().count(), 1);
    let saga = fixture
        .engine
        .viewport()
        .native_create_saga(request.saga())
        .unwrap();
    assert_eq!(saga.prepared().proposal().root(), reservation.root);
    assert_eq!(
        saga.prepared().payload(),
        &background_payload(&fixture, ItemId::new(1))
    );
    assert_eq!(
        fixture.engine.interaction().status(),
        InteractionStatus::Idle
    );
    assert!(fixture.engine.pending_mutation.is_none());
    assert_eq!(fixture.engine.workspace(), &workspace);
    assert_eq!(fixture.engine.version(), version);
}

#[test]
fn logical_attach_rejects_active_drag_reserved_surface_with_another_free_root() {
    assert_logical_attach_preserves_native_drag_reservation(
        ReservedDragPhase::Active,
        ReservedDragIdentity::Surface,
    );
}

#[test]
fn logical_attach_rejects_active_drag_reserved_root_with_another_free_surface() {
    assert_logical_attach_preserves_native_drag_reservation(
        ReservedDragPhase::Active,
        ReservedDragIdentity::Root,
    );
}

#[test]
fn logical_attach_rejects_pending_release_reserved_surface_with_another_free_root() {
    assert_logical_attach_preserves_native_drag_reservation(
        ReservedDragPhase::PendingRelease,
        ReservedDragIdentity::Surface,
    );
}

#[test]
fn logical_attach_rejects_pending_release_reserved_root_with_another_free_surface() {
    assert_logical_attach_preserves_native_drag_reservation(
        ReservedDragPhase::PendingRelease,
        ReservedDragIdentity::Root,
    );
}
