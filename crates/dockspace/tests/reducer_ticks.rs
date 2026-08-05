mod support;

use support::TestPresentationHost;

use dockspace::command::{DockTarget, WorkspaceCommand};
use dockspace::engine::{
    CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput,
    HostPresentationUnavailableReason,
};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    ItemId, ReducerTickId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{InteractionCancelReason, InteractionOutcome, InteractionStatus};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::transition::{InputOutcome, InputPriority};

const SOURCE_A: StableInputSourceId = StableInputSourceId::new(10);
const SOURCE_B: StableInputSourceId = StableInputSourceId::new(20);
const RESIZE_SURFACE: SurfaceId = SurfaceId::new(1);
const RESIZE_POINTER: PointerId = PointerId::new(1);

fn workspace(item: u64) -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(item)]));
    builder.set_root(RootId::new(item), RootRecord::new(tabs));
    builder.set_surface(
        SurfaceId::new(item),
        SurfacePresentation::with_main(RootId::new(item)),
    );
    builder.build().expect("test workspace must be valid")
}

fn engine() -> DockEngine {
    DockEngine::new(workspace(1), DockPolicy::default()).expect("engine must be valid")
}

fn two_surface_engine() -> DockEngine {
    let mut builder = Workspace::builder();
    for value in [1, 2] {
        let tabs = builder.insert_node(Node::tabs([ItemId::new(value)]));
        let root = RootId::new(value);
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(SurfaceId::new(value), SurfacePresentation::with_main(root));
    }
    DockEngine::new(
        builder
            .build()
            .expect("two-surface workspace must be valid"),
        DockPolicy::default(),
    )
    .expect("two-surface engine must be valid")
}

fn append(
    frame: &mut CoreHostFrame,
    source: StableInputSourceId,
    sequence: u64,
    input: EngineInput,
) {
    support::append_host_input(frame, source, SourceSequence::new(sequence), input)
        .expect("test input must fit its core host-frame phase");
}

fn append_surface_contribution(engine: &DockEngine, frame: &mut CoreHostFrame, surface: SurfaceId) {
    let token = engine
        .begin_surface_contribution(surface)
        .expect("surface must have a measurement ticket");
    let measurements = support::measurements(
        engine,
        surface,
        LogicalRect::new(0.0, 0.0, 240.0, 160.0).expect("bounds must be valid"),
        support::MeasurementProfile::default(),
    );
    let contribution = engine
        .prepare_surface_contribution(token, measurements)
        .expect("complete surface measurements prepare successfully");
    frame
        .push_surface_contribution(contribution)
        .expect("each logical surface contributes at most once");
}

fn complete_host_frame(engine: &DockEngine, frame: &mut CoreHostFrame) {
    support::complete_host_frame_with_retained_or_unavailable(engine, frame);
}

#[test]
fn host_frame_preserves_provider_append_order_without_priority_sorting() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut frame = host.begin(&engine);
    append(&mut frame, SOURCE_B, 1, EngineInput::ValidateWorkspace);
    append(
        &mut frame,
        SOURCE_A,
        2,
        EngineInput::ReplaceWorkspace(workspace(2)),
    );
    complete_host_frame(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    let reduced = transition.reduced_inputs();
    assert_eq!(reduced.len(), 2);
    assert_eq!(reduced[0].source(), SOURCE_B);
    assert_eq!(reduced[1].source(), SOURCE_A);
    assert_eq!(reduced[0].priority(), InputPriority::Maintenance);
    assert_eq!(reduced[1].priority(), InputPriority::LifecycleControl);
    assert!(matches!(
        reduced[0].outcome(),
        InputOutcome::WorkspaceValidated { .. }
    ));
    assert!(matches!(
        reduced[1].outcome(),
        InputOutcome::WorkspaceReplaced { .. }
    ));
    assert_eq!(
        engine.semantic_input_watermark(),
        Some(SourceSequence::new(2))
    );
}

#[test]
fn configuration_phase_runs_after_semantic_inputs_without_reordering() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let root = RootId::new(1);
    let tabs = engine.workspace().root(root).expect("root exists").node;
    let target = engine
        .workspace()
        .capture_tab_target(root, tabs)
        .expect("target is current");
    let mut policy = engine.policy().clone();
    policy.set_allow_tab_merge(false);

    let mut frame = host.begin(&engine);
    append(
        &mut frame,
        SOURCE_B,
        1,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Open {
                item: ItemId::new(2),
                target: DockTarget::Center(target),
            },
        },
    );
    append(
        &mut frame,
        SOURCE_A,
        2,
        EngineInput::ReplacePolicy { expected, policy },
    );
    complete_host_frame(&engine, &mut frame);

    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::PolicyReplaced { changed: true, .. }
    ));
    assert!(!engine.policy().allows_tab_merge());
    assert!(
        engine
            .workspace()
            .item_multiset()
            .contains_key(&ItemId::new(2))
    );
}

#[test]
fn configuration_phase_error_poison_rejects_the_entire_host_frame() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let mut policy = engine.policy().clone();
    policy.set_allow_native_surfaces(true);
    let mut frame = host.begin(&engine);
    frame
        .append_configuration(
            SOURCE_A,
            SourceSequence::new(1),
            EngineInput::ReplacePolicy { expected, policy },
        )
        .expect("configuration input must begin the terminal phase");
    assert_eq!(
        frame.append_input(
            SOURCE_B,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::SemanticAfterConfiguration)
    );
    assert_eq!(
        frame.append_configuration(
            SOURCE_A,
            SourceSequence::new(2),
            EngineInput::ReplacePolicy {
                expected,
                policy: DockPolicy::default(),
            },
        ),
        Err(CoreHostFrameError::SemanticAfterConfiguration),
        "the first construction error remains the batch diagnosis"
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::SemanticAfterConfiguration,
        })
    ));
    assert!(!engine.policy().allows_native_surfaces());
    assert_eq!(engine.last_reducer_tick(), ReducerTickId::default());
    assert_eq!(engine.last_input_sequence().get(), 0);
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn earlier_same_base_host_frame_cannot_finish_after_a_newer_boundary() {
    let mut engine = two_surface_engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut stale = host.begin(&engine);
    assert_eq!(
        stale.surfaces().collect::<Vec<_>>(),
        vec![SurfaceId::new(1), SurfaceId::new(2)]
    );
    append(&mut stale, SOURCE_A, 1, EngineInput::ValidateWorkspace);

    let mut replacement = host.begin(&engine);
    append(
        &mut replacement,
        SOURCE_B,
        1,
        EngineInput::ReplaceWorkspace(workspace(1)),
    );
    complete_host_frame(&engine, &mut replacement);
    host.finish(replacement, &mut engine);

    assert!(matches!(
        stale.finish(&mut engine),
        Err(EngineError::HostFramePredecessorStale {
            submitted,
            current,
        }) if submitted == ReducerTickId::default() && current == ReducerTickId::new(1)
    ));
    let current = host.begin(&engine);
    assert_eq!(
        current.surfaces().collect::<Vec<_>>(),
        vec![SurfaceId::new(1)]
    );
}

#[test]
fn host_frame_rejects_a_foreign_engine_before_state_changes() {
    let mut left = engine();
    let mut right = engine();
    let mut host = TestPresentationHost::new(&mut left);
    let mut foreign = host.begin(&left);
    append(&mut foreign, SOURCE_A, 1, EngineInput::ValidateWorkspace);
    let before_tick = right.last_reducer_tick();
    let before_input = right.last_input_sequence();

    assert!(matches!(
        foreign.finish(&mut right),
        Err(EngineError::HostFrameAuthorityDomainMismatch { .. })
    ));
    assert_eq!(right.last_reducer_tick(), before_tick);
    assert_eq!(right.last_input_sequence(), before_input);
    assert_eq!(right.semantic_input_watermark(), None);
}

#[test]
fn host_frame_rejects_stale_workspace_or_requirements_before_state_changes() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut stale = host.begin(&engine);
    append(&mut stale, SOURCE_A, 1, EngineInput::ValidateWorkspace);

    let mut current = host.begin(&engine);
    append(
        &mut current,
        SOURCE_B,
        1,
        EngineInput::ReplaceWorkspace(workspace(2)),
    );
    complete_host_frame(&engine, &mut current);
    host.finish(current, &mut engine);
    let before_tick = engine.last_reducer_tick();
    let before_input = engine.last_input_sequence();

    assert!(matches!(
        stale.finish(&mut engine),
        Err(EngineError::HostFramePredecessorStale { .. })
    ));
    assert_eq!(engine.last_reducer_tick(), before_tick);
    assert_eq!(engine.last_input_sequence(), before_input);
    assert_eq!(
        engine.semantic_input_watermark(),
        Some(SourceSequence::new(1))
    );
}

#[test]
fn source_sequence_replay_rejects_the_entire_host_frame_atomically() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut first = host.begin(&engine);
    append(&mut first, SOURCE_A, 5, EngineInput::ValidateWorkspace);
    complete_host_frame(&engine, &mut first);
    host.finish(first, &mut engine);

    let before_workspace = engine.workspace().clone();
    let before_version = engine.version();
    let before_tick = engine.last_reducer_tick();
    let before_input = engine.last_input_sequence();
    let mut replay = host.begin(&engine);
    append(&mut replay, SOURCE_B, 6, EngineInput::ValidateWorkspace);
    assert_eq!(
        support::append_host_input(
            &mut replay,
            SOURCE_A,
            SourceSequence::new(5),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed)
    );
    assert!(matches!(
        replay.finish(&mut engine),
        Err(EngineError::SourceSequenceNotIncreasing {
            input_source: SOURCE_A,
            previous,
            submitted,
        }) if previous == SourceSequence::new(6) && submitted == SourceSequence::new(5)
    ));
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.last_reducer_tick(), before_tick);
    assert_eq!(engine.last_input_sequence(), before_input);
    assert_eq!(
        engine.semantic_input_watermark(),
        Some(SourceSequence::new(5))
    );
}

#[test]
fn duplicate_source_sequence_inside_one_host_frame_is_atomic() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut frame = host.begin(&engine);
    append(&mut frame, SOURCE_A, 1, EngineInput::ValidateWorkspace);
    assert_eq!(
        support::append_host_input(
            &mut frame,
            SOURCE_A,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed)
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::SourceSequenceNotIncreasing {
            input_source: SOURCE_A,
            previous,
            submitted,
        }) if previous == SourceSequence::new(1) && submitted == SourceSequence::new(1)
    ));
    assert_eq!(engine.last_reducer_tick(), ReducerTickId::default());
    assert_eq!(engine.last_input_sequence().get(), 0);
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn surface_contributions_are_canonical_even_when_callbacks_arrive_reverse() {
    let mut engine = two_surface_engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut frame = host.begin(&engine);
    append_surface_contribution(&engine, &mut frame, SurfaceId::new(2));
    append_surface_contribution(&engine, &mut frame, SurfaceId::new(1));

    let transition = host.finish(frame, &mut engine);
    assert_eq!(
        transition
            .surface_contributions()
            .iter()
            .map(|outcome| outcome.surface())
            .collect::<Vec<_>>(),
        vec![SurfaceId::new(1), SurfaceId::new(2)]
    );
}

#[test]
fn incomplete_contribution_roster_rejects_before_tick_or_watermark_progress() {
    let mut engine = two_surface_engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let before_workspace = engine.workspace().clone();
    let before_version = engine.version();
    let mut frame = host.begin(&engine);
    append(&mut frame, SOURCE_A, 1, EngineInput::ValidateWorkspace);
    append_surface_contribution(&engine, &mut frame, SurfaceId::new(1));

    let mut frame = frame
        .into_presentation()
        .expect("the incomplete measurement frame still enters presentation");
    frame
        .resolve_all_presentation_obligations_unavailable(
            HostPresentationUnavailableReason::OutputNotProduced,
        )
        .expect("the host explicitly reports no physical output");
    let result = frame.finish(&mut engine);
    assert!(
        matches!(
            &result,
            Err(EngineError::HostFrameContributionRosterIncomplete {
                expected,
                submitted,
            }) if expected == &vec![SurfaceId::new(1), SurfaceId::new(2)]
                && submitted == &vec![SurfaceId::new(1)]
        ),
        "unexpected incomplete-roster result: {result:?}"
    );
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.version(), before_version);
    assert_eq!(engine.last_reducer_tick(), ReducerTickId::default());
    assert_eq!(engine.last_input_sequence().get(), 0);
    assert_eq!(engine.semantic_input_watermark(), None);
}

#[test]
fn explicit_unavailable_roster_frames_are_boundaries_without_input_progress() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let mut first_frame = host.begin(&engine);
    support::complete_host_frame_with_unavailable(&engine, &mut first_frame);
    let first = host.finish(first_frame, &mut engine);
    let mut second_frame = host.begin(&engine);
    support::complete_host_frame_with_unavailable(&engine, &mut second_frame);
    let second = host.finish(second_frame, &mut engine);

    assert_eq!(first.tick(), ReducerTickId::new(1));
    assert_eq!(second.tick(), ReducerTickId::new(2));
    assert!(first.reduced_inputs().is_empty());
    assert!(second.reduced_inputs().is_empty());
    assert!(first.surface_contributions().iter().all(|outcome| matches!(
        outcome,
        dockspace::transition::SurfaceContributionOutcome::Unavailable { .. }
    )));
    assert_eq!(engine.last_input_sequence().get(), 0);
}

fn create_resize_pointer_provider(
    engine: &mut DockEngine,
    host: &TestPresentationHost,
) -> SurfaceLocalPointerProvider {
    engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(RESIZE_SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("resize pointer provider is admitted")
}

fn pointer_edge_journal(
    previous: u64,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    capture: PointerCaptureOwner,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            RESIZE_POINTER,
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(capture),
        )],
    )
    .expect("single resize pointer edge is contiguous")
}

fn stage_pointer_edge(
    frame: &mut CoreHostFrame,
    provider: &SurfaceLocalPointerProvider,
    journal: PointerEdgeJournal,
    delivery: Option<PointerReceiverDeliveryDisposition>,
) {
    frame
        .submit_surface_pointer_journal(provider, journal)
        .expect("resize pointer edge follows the provider watermark");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("resize edge freezes one receiver roster")
        .candidates()
        .first()
        .expect("single resize edge has one receiver candidate")
        .clone();
    let observation = if let Some(disposition) = delivery {
        let projection = frame
            .view()
            .interaction_projection(RESIZE_SURFACE)
            .expect("sealed frame has current resize interaction authority");
        let delivery = PointerReceiverDelivery::new(projection, disposition)
            .expect("resize receiver fact is bound to the current output");
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("resize press answers the exact delivery probe"),
        )
    } else {
        PointerReceiverObservation::NotApplicable
    };
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("resize receipt set is exact"),
        )
        .expect("resize receiver receipt stages");
}

fn active_resize_region(
    engine: &DockEngine,
    splitter: dockspace::scene::SplitterSceneId,
) -> Option<PresentationHitRegionId> {
    engine
        .interaction_projection(RESIZE_SURFACE)?
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            region.id().kind() == PresentationHitRegionKind::SplitterHandle(splitter)
                && !region.is_passive()
        })
        .map(|region| region.id())
}

fn resize_engine() -> (DockEngine, dockspace::ids::NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("split must be valid"),
    );
    builder.set_root(RootId::new(1), RootRecord::new(split));
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(RootId::new(1)),
    );
    let workspace = builder.build().expect("resize workspace must be valid");
    (
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid"),
        split,
    )
}

fn publish_resize_scene(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    split: dockspace::ids::NodeId,
) -> (dockspace::scene::SplitterSceneId, LogicalPoint, f64, f64) {
    let bounds = LogicalRect::new(0.0, 0.0, 400.0, 200.0).expect("bounds are valid");
    support::publish_surface(engine, host, SurfaceId::new(1), bounds);
    let painted = engine
        .scene()
        .ready_surface(SurfaceId::new(1))
        .expect("resize surface is painted");
    let record = painted
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("splitter is painted");
    let press = LogicalPoint::new(
        record.draw_bounds().x() + record.draw_bounds().width() * 0.5,
        record.draw_bounds().y() + record.draw_bounds().height() * 0.5,
    )
    .expect("press point is valid");
    (
        *record.id(),
        press,
        record.child_extents()[0],
        record.child_extents().iter().sum(),
    )
}

#[test]
fn pointer_interaction_uses_tick_start_policy_before_configuration_phase() {
    let (mut engine, split) = resize_engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let (splitter, press, before, available) = publish_resize_scene(&mut engine, &mut host, split);
    let region = active_resize_region(&engine, splitter)
        .expect("tick-start policy exposes an active splitter receiver");
    let provider = create_resize_pointer_provider(&mut engine, &host);
    let mut begin_frame = host.begin(&engine);
    stage_pointer_edge(
        &mut begin_frame,
        &provider,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            press,
            PointerCaptureOwner::ProviderEndpoint,
        ),
        Some(PointerReceiverDeliveryDisposition::Dock(region)),
    );
    complete_host_frame(&engine, &mut begin_frame);
    let begin = host.finish(begin_frame, &mut engine);
    let session = match begin.reduced_pointer_edges()[0].interaction_outcomes() {
        [InteractionOutcome::ResizeBegan { session, .. }] => *session,
        outcomes => panic!("unexpected resize outcomes: {outcomes:?}"),
    };

    let expected = engine.version();
    let mut replacement = engine.policy().clone();
    replacement.set_allow_splitter_resize(false);
    let update_point = LogicalPoint::new(press.x() + available * 0.25 - before, press.y())
        .expect("update point is valid");
    let mut frame = host.begin(&engine);
    stage_pointer_edge(
        &mut frame,
        &provider,
        pointer_edge_journal(
            1,
            PointerEdgeKind::Moved,
            update_point,
            PointerCaptureOwner::ProviderEndpoint,
        ),
        None,
    );
    append(
        &mut frame,
        SOURCE_A,
        1,
        EngineInput::ReplacePolicy {
            expected,
            policy: replacement,
        },
    );
    complete_host_frame(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    let [move_edge] = transition.reduced_pointer_edges() else {
        panic!("move tick must reduce exactly one pointer edge");
    };
    let [policy_input] = transition.reduced_inputs() else {
        panic!("move tick must reduce exactly one configuration input");
    };
    assert!(matches!(
        move_edge.interaction_outcomes(),
        [InteractionOutcome::ResizeUpdated { session: actual, .. }] if *actual == session
    ));
    assert!(matches!(
        policy_input.outcome(),
        InputOutcome::PolicyReplaced { changed: true, .. }
    ));
    assert_eq!(move_edge.tick(), policy_input.tick());
    assert!(move_edge.causal_ordinal() < policy_input.causal_ordinal());
    assert!(!engine.policy().allows_splitter_resize());
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(transition.interaction_events().iter().any(|event| matches!(
        event.kind(),
        dockspace::interaction::InteractionEventKind::Cancelled {
            status: InteractionStatus::Resizing { session: cancelled },
            reason: InteractionCancelReason::PolicyChanged,
        } if *cancelled == session
    )));

    let mut provider_receipt = provider
        .drain()
        .expect("a committed resize provider has no in-flight host frame");
    assert_eq!(
        provider_receipt.committed_through(),
        PointerEdgeSequence::new(2)
    );
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut provider_receipt)
        .expect("retiring the first provider releases its already-cancelled resize authority");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    let (splitter, press, _, _) = publish_resize_scene(&mut engine, &mut host, split);
    assert_eq!(active_resize_region(&engine, splitter), None);
    let successor = create_resize_pointer_provider(&mut engine, &host);
    let mut rejected_frame = host.begin(&engine);
    stage_pointer_edge(
        &mut rejected_frame,
        &successor,
        pointer_edge_journal(
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            press,
            PointerCaptureOwner::None,
        ),
        Some(PointerReceiverDeliveryDisposition::NoReceiver),
    );
    complete_host_frame(&engine, &mut rejected_frame);
    let rejected = host.finish(rejected_frame, &mut engine);
    assert_eq!(rejected.reduced_pointer_edges().len(), 1);
    assert!(
        rejected.reduced_pointer_edges()[0]
            .interaction_outcomes()
            .is_empty()
    );
    assert!(engine.interaction().active_resize_view().is_none());
    let mut successor_receipt = successor
        .drain()
        .expect("a committed successor has no in-flight host frame");
    assert_eq!(
        successor_receipt.committed_through(),
        PointerEdgeSequence::new(1)
    );
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut successor_receipt)
        .expect("idle successor provider retires at its committed watermark");
    assert!(retirement.repaint_required());
    assert!(!retirement.interaction_changed());
}
