use super::support;

use std::collections::BTreeSet;

use dockspace::command::{
    CloseCommitOutcome, CommandOutcome, ContentCloseTarget, SplitResize, WorkspaceCommand,
};
use dockspace::engine::{CoreHostFrame, DockEngine, EngineInput, LocalSplitterGesturePhase};
use dockspace::error::{CommandError, ReferenceRole, TransactionError};
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::{
    InteractionCancelReason, InteractionOutcome, InteractionRejection, InteractionStatus,
};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerRetirementDisposition, SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceipt, PointerReceiverReceiptBatch,
    PresentedPointerReceiverObservation,
};
use dockspace::policy::{DockPolicy, PolicyRejection, PolicyRevision};
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::{SplitterRecord, SplitterResizeTarget, SurfaceSceneStamp};
use dockspace::transaction::WorkspaceTransaction;
use dockspace::transition::{
    EngineTransition, InputOutcome, SurfaceContributionOutcome, SurfaceContributionRejection,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const POINTER: PointerId = PointerId::new(1);
const INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0x51_7e_22);

#[derive(Debug)]
struct TestPointerStream {
    provider: SurfaceLocalPointerProvider,
    through: u64,
}

fn rect(width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, height).expect("fixture bounds are valid")
}

fn size(width: f64, height: f64) -> LogicalSize {
    LogicalSize::new(width, height).expect("fixture size is valid")
}

fn center(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("fixture point is valid")
}

fn weights(first: f32, second: f32) -> Vec<SplitWeight> {
    SplitWeight::normalize([first, second]).expect("fixture weights normalize")
}

fn node_weights(workspace: &Workspace, split: NodeId) -> &[SplitWeight] {
    match workspace.node(split) {
        Some(Node::Split { weights, .. }) => weights,
        node => panic!("expected split {split:?}, got {node:?}"),
    }
}

fn simple_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("fixture split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("fixture workspace is valid"), split)
}

fn junction_workspace() -> (Workspace, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let top_right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let bottom_right = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let vertical = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_right, bottom_right])
            .expect("vertical fixture split is valid"),
    );
    let horizontal = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, vertical])
            .expect("horizontal fixture split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(horizontal));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        builder.build().expect("junction workspace is valid"),
        horizontal,
        vertical,
    )
}

fn cross_junction_workspace() -> (Workspace, NodeId, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let top_left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let lower_left_tall_top = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let lower_left_tall_bottom = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let lower_left_peer = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let top_right = builder.insert_node(Node::tabs([ItemId::new(5)]));
    let bottom_right = builder.insert_node(Node::tabs([ItemId::new(6)]));
    let lower_left_tall = builder.insert_node(
        Node::equal_split(
            Axis::Vertical,
            [lower_left_tall_top, lower_left_tall_bottom],
        )
        .expect("nested tall split is valid"),
    );
    let lower_left = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [lower_left_tall, lower_left_peer])
            .expect("lower-left wrapper split is valid"),
    );
    let left = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_left, lower_left])
            .expect("left junction arm is valid"),
    );
    let right = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_right, bottom_right])
            .expect("right junction arm is valid"),
    );
    let root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("cross junction root is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(root));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        builder.build().expect("cross junction workspace is valid"),
        root,
        left,
        right,
    )
}

fn publish(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    profile: support::MeasurementProfile,
) {
    support::publish_surface_with(engine, host, SURFACE, rect(400.0, 240.0), profile);
}

fn splitter(engine: &DockEngine, split: NodeId) -> (SurfaceSceneStamp, SplitterRecord) {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let record = ready
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .cloned()
        .expect("splitter is projected");
    (ready.stamp(), record)
}

fn resize_region(
    engine: &DockEngine,
    kind: PresentationHitRegionKind,
) -> (PresentationHitRegionId, LogicalPoint) {
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("surface has current interaction authority");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| region.id().kind() == kind)
        .expect("resize receiver is present in the authoritative hit manifest");
    (region.id(), center(region.hit().rect()))
}

fn create_pointer_stream(
    engine: &mut DockEngine,
    host: &support::TestPresentationHost,
) -> TestPointerStream {
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider is admitted");
    TestPointerStream {
        provider,
        through: 0,
    }
}

fn empty_pointer_journal(pointer: &TestPointerStream) -> PointerEdgeJournal {
    let watermark = PointerEdgeSequence::new(pointer.through);
    PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("empty journal preserves the provider watermark")
}

fn stage_pointer_edge(
    frame: &mut CoreHostFrame,
    pointer: &mut TestPointerStream,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    delivered_to: Option<PresentationHitRegionId>,
) {
    let previous = PointerEdgeSequence::new(pointer.through);
    let sequence = PointerEdgeSequence::new(
        pointer
            .through
            .checked_add(1)
            .expect("test pointer sequence does not exhaust"),
    );
    let capture = if matches!(kind, PointerEdgeKind::ButtonReleased(_)) {
        PointerCaptureOwner::None
    } else {
        PointerCaptureOwner::ProviderEndpoint
    };
    let journal = PointerEdgeJournal::new(
        previous,
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(capture),
        )],
    )
    .expect("single pointer edge is contiguous");
    frame
        .submit_surface_pointer_journal(&pointer.provider, journal)
        .expect("pointer edge follows the provider watermark");
    pointer.through = sequence.get();

    let candidate = frame
        .pointer_receiver_candidates()
        .expect("pointer edge freezes one receiver roster")
        .candidates()
        .first()
        .expect("single pointer edge has one receiver candidate")
        .clone();
    let observation = if let Some(region) = delivered_to {
        let projection = frame
            .view()
            .interaction_projection(SURFACE)
            .expect("sealed frame retains current interaction authority");
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(region),
        )
        .expect("resize delivery is bound to the current output");
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
                .expect("pointer receipt batch is exact"),
        )
        .expect("pointer receiver receipt stages");
}

fn submit_pointer_edge(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    pointer: &mut TestPointerStream,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    delivered_to: Option<PresentationHitRegionId>,
) -> EngineTransition {
    let mut frame = host.begin(engine);
    stage_pointer_edge(&mut frame, pointer, kind, position, delivered_to);
    support::complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    let transition = host.finish(frame, engine);
    assert_eq!(
        pointer.provider.committed_through(),
        PointerEdgeSequence::new(pointer.through),
        "committed pointer watermark advances monotonically"
    );
    transition
}

fn pointer_interaction_outcome(transition: &EngineTransition) -> &InteractionOutcome {
    let [edge] = transition.reduced_pointer_edges() else {
        panic!(
            "expected one reduced pointer edge, got {:?}",
            transition.reduced_pointer_edges()
        );
    };
    let [outcome] = edge.interaction_outcomes() else {
        panic!(
            "expected one pointer interaction outcome, got {:?}",
            edge.interaction_outcomes()
        );
    };
    outcome
}

fn submit_input_with_idle_pointer(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    pointer: &mut TestPointerStream,
    input: EngineInput,
) -> EngineTransition {
    let mut stream = support::TestInputStream::resume(engine, INPUT_SOURCE);
    let mut frame = host.begin(engine);
    frame
        .submit_surface_pointer_journal(&pointer.provider, empty_pointer_journal(pointer))
        .expect("semantic frame preserves the pointer watermark");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("empty journal has an exact empty receipt set"),
        )
        .expect("empty pointer receipt set stages");
    stream
        .append(&mut frame, input)
        .expect("semantic input follows pointer authority");
    support::complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    let transition = host.finish(frame, engine);
    assert_eq!(
        pointer.provider.committed_through(),
        PointerEdgeSequence::new(pointer.through),
        "committed pointer watermark advances monotonically"
    );
    transition
}

fn interaction_outcome(transition: &EngineTransition) -> &InteractionOutcome {
    match transition.reduced_inputs() {
        [input] => match input.outcome() {
            InputOutcome::InteractionProcessed { outcome, .. } => outcome,
            outcome => panic!("unexpected semantic input outcome: {outcome:?}"),
        },
        inputs => panic!("expected one semantic input, got {inputs:?}"),
    }
}

fn submit_local_splitter_gesture(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    surface: SurfaceId,
    target: SplitterResizeTarget,
    phase: LocalSplitterGesturePhase,
) -> EngineTransition {
    let expected = engine.version();
    support::submit_input(
        engine,
        host,
        INPUT_SOURCE,
        EngineInput::LocalSplitterGesture {
            expected,
            surface,
            target,
            phase,
        },
    )
    .expect("local splitter gesture reduces")
}

#[test]
fn local_splitter_gesture_commits_only_on_release() {
    let (workspace, split) = simple_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );

    let (scene, record) = splitter(&engine, split);
    let target = SplitterResizeTarget::Handle(*record.id());
    let press = center(record.hit().rect());
    let moved =
        LogicalPoint::new(press.x() + 24.0, press.y()).expect("local splitter move point is valid");
    let released = LogicalPoint::new(press.x() + 48.0, press.y())
        .expect("local splitter release point is valid");
    let initial_workspace = engine.workspace().clone();
    let initial_version = engine.version();

    let pressed = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Press {
            scene,
            initial: press,
            current: press,
        },
    );
    assert!(matches!(
        interaction_outcome(&pressed),
        InteractionOutcome::ResizeBegan { .. }
    ));

    let moved_transition = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Move { current: moved },
    );
    assert!(matches!(
        interaction_outcome(&moved_transition),
        InteractionOutcome::ResizeUpdated { .. }
    ));
    assert_eq!(engine.workspace(), &initial_workspace);
    assert_eq!(engine.version(), initial_version);

    let released_transition = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Release { current: released },
    );
    assert!(matches!(
        interaction_outcome(&released_transition),
        InteractionOutcome::ResizeDelivered { changed: true, .. }
    ));
    assert_ne!(node_weights(engine.workspace(), split), weights(0.5, 0.5));
    assert_eq!(
        engine.version().revision().get(),
        initial_version.revision().get() + 1
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn local_splitter_cancel_preserves_the_durable_layout() {
    let (workspace, split) = simple_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );

    let (scene, record) = splitter(&engine, split);
    let target = SplitterResizeTarget::Handle(*record.id());
    let press = center(record.hit().rect());
    let moved =
        LogicalPoint::new(press.x() + 24.0, press.y()).expect("local splitter move point is valid");
    let initial_workspace = engine.workspace().clone();
    let initial_version = engine.version();

    let pressed = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Press {
            scene,
            initial: press,
            current: press,
        },
    );
    assert!(matches!(
        interaction_outcome(&pressed),
        InteractionOutcome::ResizeBegan { .. }
    ));
    let moved_transition = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Move { current: moved },
    );
    assert!(matches!(
        interaction_outcome(&moved_transition),
        InteractionOutcome::ResizeUpdated { .. }
    ));

    let cancelled = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Cancel,
    );
    assert!(matches!(
        interaction_outcome(&cancelled),
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::LocalResponseCancelled,
            ..
        }
    ));
    assert_eq!(engine.workspace(), &initial_workspace);
    assert_eq!(engine.version(), initial_version);
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn local_junction_gesture_commits_one_atomic_resize_batch() {
    let (workspace, horizontal, vertical) = junction_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );

    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let [junction] = ready.plan().splitter_junction_records() else {
        panic!("fixture exposes one splitter junction");
    };
    let scene = ready.stamp();
    let target = SplitterResizeTarget::Junction(junction.id());
    let press = center(junction.hit().rect());
    let released = LogicalPoint::new(press.x() + 32.0, press.y() + 32.0)
        .expect("local junction release point is valid");
    let initial_horizontal = node_weights(engine.workspace(), horizontal).to_vec();
    let initial_vertical = node_weights(engine.workspace(), vertical).to_vec();
    let initial_version = engine.version();

    let pressed = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Press {
            scene,
            initial: press,
            current: press,
        },
    );
    assert!(matches!(
        interaction_outcome(&pressed),
        InteractionOutcome::ResizeBegan { .. }
    ));

    let released_transition = submit_local_splitter_gesture(
        &mut engine,
        &mut host,
        SURFACE,
        target,
        LocalSplitterGesturePhase::Release { current: released },
    );
    let InteractionOutcome::ResizeDelivered {
        changed: true,
        outcome: CommandOutcome::SplitsResized { splits, .. },
        ..
    } = interaction_outcome(&released_transition)
    else {
        panic!("local junction release must commit one split batch: {released_transition:?}");
    };
    assert_eq!(splits.len(), 2);
    assert_ne!(
        node_weights(engine.workspace(), horizontal),
        initial_horizontal
    );
    assert_ne!(node_weights(engine.workspace(), vertical), initial_vertical);
    assert_eq!(
        engine.version().revision().get(),
        initial_version.revision().get() + 1
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

fn adjust_splitter(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    scene: SurfaceSceneStamp,
    splitter: dockspace::scene::SplitterSceneId,
    delta: f64,
) -> InteractionOutcome {
    let expected = engine.version();
    let transition = support::submit_input(
        engine,
        host,
        INPUT_SOURCE,
        EngineInput::AdjustSplitterResize {
            expected,
            scene,
            splitter,
            delta,
        },
    )
    .expect("splitter adjustment reduces");
    interaction_outcome(&transition).clone()
}

fn close_root(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    pointer: TestPointerStream,
    root: RootId,
) -> EngineTransition {
    let mut pointer = pointer;
    let expected = engine.version();
    let request = submit_input_with_idle_pointer(
        engine,
        host,
        &mut pointer,
        EngineInput::RequestContentClose {
            expected,
            target: ContentCloseTarget::Root(root),
        },
    );
    let InputOutcome::ContentCloseRequested { plan, .. } = request.reduced_inputs()[0].outcome()
    else {
        panic!("root close target must open a plan");
    };
    let plan = plan.clone();
    let mut committed = None;
    for requirement in plan.items() {
        committed = Some(submit_input_with_idle_pointer(
            engine,
            host,
            &mut pointer,
            EngineInput::ResolveClose {
                request: plan.request(),
                token: requirement.token(),
                decision: dockspace::CloseDecision::Allow,
            },
        ));
    }
    let committed = committed.expect("non-empty root has a final close decision");
    assert!(matches!(
        committed.reduced_inputs()[0].outcome(),
        InputOutcome::CloseDecisionProcessed {
            application: Some(Ok(CloseCommitOutcome::RootClosed {
                root: actual,
                ..
            })),
            changed: true,
            ..
        } if *actual == root
    ));
    assert!(committed.events().iter().any(|event| matches!(
        event.kind(),
        dockspace::event::WorkspaceEventKind::CloseCommitted(
            CloseCommitOutcome::RootClosed { root: actual, .. }
        ) if *actual == root
    )));
    let mut receipt = pointer
        .provider
        .drain()
        .expect("closed-root pointer producer has no in-flight host frame");
    let retirement = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        .expect("closed-root pointer producer compacts at its committed watermark");
    assert_eq!(
        retirement.disposition(),
        SurfaceLocalPointerRetirementDisposition::CompactedPreviouslyRetired
    );
    assert!(!retirement.repaint_required());
    assert!(!retirement.interaction_changed());
    committed
}

fn begin_handle_resize(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    pointer: &mut TestPointerStream,
    split: NodeId,
) -> (dockspace::interaction::ResizeSessionId, LogicalPoint) {
    let (_, record) = splitter(engine, split);
    let press = center(record.hit().rect());
    let (region, region_press) = resize_region(
        engine,
        PresentationHitRegionKind::SplitterHandle(*record.id()),
    );
    assert_eq!(press, region_press);
    let transition = submit_pointer_edge(
        engine,
        host,
        pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    match pointer_interaction_outcome(&transition) {
        InteractionOutcome::ResizeBegan { session, .. } => (*session, press),
        outcome => panic!("expected resize activation, got {outcome:?}"),
    }
}

fn junction_press(engine: &DockEngine) -> (PresentationHitRegionId, LogicalPoint) {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let [junction] = ready.plan().splitter_junction_records() else {
        panic!("perpendicular fixture exposes one junction");
    };
    let press = center(junction.hit().rect());
    let (region, region_press) = resize_region(
        engine,
        PresentationHitRegionKind::SplitterJunction(junction.id()),
    );
    assert_eq!(press, region_press);
    (region, press)
}

#[test]
fn resize_update_invalidates_an_older_surface_contribution_in_the_same_tick() {
    let (workspace, split) = simple_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );
    let mut pointer = create_pointer_stream(&mut engine, &host);
    let (session, press) = begin_handle_resize(&mut engine, &mut host, &mut pointer, split);

    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("old surface contribution begins");
    let measurements = support::measurements(
        &engine,
        SURFACE,
        rect(400.0, 240.0),
        support::MeasurementProfile::default(),
    );
    let contribution = engine
        .prepare_surface_contribution(token, measurements)
        .expect("complete pre-resize measurements prepare successfully");
    let mut frame = host.begin(&engine);
    stage_pointer_edge(
        &mut frame,
        &mut pointer,
        PointerEdgeKind::Moved,
        LogicalPoint::new(press.x() + 48.0, press.y()).expect("updated pointer is valid"),
        None,
    );
    frame
        .push_surface_contribution(contribution)
        .expect("one contribution fits in the reducer tick");

    let transition = host.finish(frame, &mut engine);

    assert!(matches!(
        pointer_interaction_outcome(&transition),
        InteractionOutcome::ResizeUpdated { session: actual, .. } if *actual == session
    ));
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Rejected {
            surface: SURFACE,
            reason: SurfaceContributionRejection::StaleBase {
                submitted,
                current,
            },
        }] if *submitted == token.base() && *current != Some(token.base())
    ));
    assert_eq!(
        engine
            .interaction()
            .active_resize_view()
            .map(|resize| resize.session()),
        Some(session)
    );
}

#[test]
fn handle_resize_clamps_both_directions_at_core_derived_root_limits() {
    let (workspace, split) = simple_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );
    let (_, record) = splitter(&engine, split);
    let index = record.id().index;
    let before = record.child_extents()[index];
    let after = record.child_extents()[index + 1];
    let pair_extent = before + after;
    let full_extent = record.child_extents().iter().sum::<f64>();
    assert!(before < record.before_maximum_extent());
    assert!(after < record.after_maximum_extent());
    assert_eq!(record.before_maximum_extent(), 400.0);
    assert_eq!(record.after_maximum_extent(), 400.0);

    let mut pointer = create_pointer_stream(&mut engine, &host);
    let (session, press) = begin_handle_resize(&mut engine, &mut host, &mut pointer, split);
    let positive_point =
        LogicalPoint::new(press.x() + 10_000.0, press.y()).expect("positive update point is valid");
    let positive_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        positive_point,
        None,
    );
    let positive = pointer_interaction_outcome(&positive_transition).clone();
    let InteractionOutcome::ResizeUpdated {
        session: positive_session,
        splits: positive,
        ..
    } = positive
    else {
        panic!("positive resize must publish a clamped preview: {positive:?}");
    };
    assert_eq!(positive_session, session);
    let [positive] = positive.as_slice() else {
        panic!("one handle must update exactly one split: {positive:?}");
    };
    let positive_before = record
        .before_maximum_extent()
        .min(pair_extent - record.after_minimum_extent());
    assert!(
        (f64::from(positive.weights()[index].get()) - positive_before / full_extent).abs()
            <= 1.0e-6
    );

    let negative_point =
        LogicalPoint::new(press.x() - 10_000.0, press.y()).expect("negative update point is valid");
    let negative_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        negative_point,
        None,
    );
    let negative = pointer_interaction_outcome(&negative_transition).clone();
    let InteractionOutcome::ResizeUpdated {
        session: negative_session,
        splits: negative,
        ..
    } = negative
    else {
        panic!("negative resize must publish a clamped preview: {negative:?}");
    };
    assert_eq!(negative_session, session);
    let [negative] = negative.as_slice() else {
        panic!("one handle must update exactly one split: {negative:?}");
    };
    let negative_before = record
        .before_minimum_extent()
        .max(pair_extent - record.after_maximum_extent());
    assert!(
        (f64::from(negative.weights()[index].get()) - negative_before / full_extent).abs()
            <= 1.0e-6
    );

    let delivered_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        negative_point,
        None,
    );
    let delivered = pointer_interaction_outcome(&delivered_transition);
    assert!(matches!(
        delivered,
        InteractionOutcome::ResizeDelivered {
            session: delivered_session,
            changed: true,
            ..
        } if *delivered_session == session
    ));
    assert_eq!(node_weights(engine.workspace(), split), negative.weights());
}

#[test]
fn closing_the_active_resize_root_removes_its_surface_and_clears_the_session() {
    let (workspace, split) = simple_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );
    let mut pointer = create_pointer_stream(&mut engine, &host);
    let (session, _) = begin_handle_resize(&mut engine, &mut host, &mut pointer, split);
    let before_revision = engine.version().revision().get();

    let transition = close_root(&mut engine, &mut host, pointer, ROOT);
    assert!(engine.workspace().root(ROOT).is_none());
    assert!(engine.workspace().surface(SURFACE).is_none());
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert!(engine.interaction().active_resize_view().is_none());
    assert_eq!(engine.version().revision().get(), before_revision + 1);
    assert!(transition.interaction_events().iter().any(|event| {
        matches!(
            event.kind(),
            dockspace::interaction::InteractionEventKind::Cancelled {
                status: InteractionStatus::Resizing { session: actual },
                ..
            } if *actual == session
        )
    }));
}

#[test]
fn junction_resize_clamps_one_axis_and_commits_both_axes_in_one_revision() {
    let (workspace, horizontal, vertical) = junction_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile {
            pane_minimum: size(180.0, 20.0),
            ..support::MeasurementProfile::default()
        },
    );
    let (region, press) = junction_press(&engine);
    let (_, horizontal_record) = splitter(&engine, horizontal);
    let (_, vertical_record) = splitter(&engine, vertical);
    let initial_workspace = engine.workspace().clone();
    let initial_version = engine.version();
    let mut pointer = create_pointer_stream(&mut engine, &host);

    let pressed = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    let session = match pointer_interaction_outcome(&pressed) {
        InteractionOutcome::ResizeBegan { session, .. } => session,
        outcome => panic!("expected junction resize activation, got {outcome:?}"),
    };
    let session = *session;
    let vertical_delta = 20.0;
    let update_point = LogicalPoint::new(press.x() + 10_000.0, press.y() + vertical_delta)
        .expect("junction update point is valid");
    let update_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        update_point,
        None,
    );
    let update = pointer_interaction_outcome(&update_transition).clone();
    let InteractionOutcome::ResizeUpdated {
        session: updated_session,
        splits,
        ..
    } = update
    else {
        panic!("junction update must publish one atomic batch: {update:?}");
    };
    assert_eq!(updated_session, session);
    assert_eq!(splits.len(), 2);
    assert_eq!(engine.workspace(), &initial_workspace);
    assert_eq!(engine.version(), initial_version);

    let horizontal_update = splits
        .iter()
        .find(|resize| resize.split().node() == horizontal)
        .expect("batch contains horizontal split");
    let horizontal_index = horizontal_record.id().index;
    let horizontal_available = horizontal_record.child_extents().iter().sum::<f64>();
    let horizontal_pair = horizontal_record.child_extents()[horizontal_index]
        + horizontal_record.child_extents()[horizontal_index + 1];
    let horizontal_expected_before = horizontal_pair - horizontal_record.after_minimum_extent();
    assert!(
        (f64::from(horizontal_update.weights()[horizontal_index].get())
            - horizontal_expected_before / horizontal_available)
            .abs()
            <= 1.0e-6
    );

    let vertical_update = splits
        .iter()
        .find(|resize| resize.split().node() == vertical)
        .expect("batch contains vertical split");
    let vertical_index = vertical_record.id().index;
    let vertical_before = vertical_record.child_extents()[vertical_index];
    let vertical_after = vertical_record.child_extents()[vertical_index + 1];
    assert!(vertical_delta < vertical_after - vertical_record.after_minimum_extent());
    let vertical_available = vertical_record.child_extents().iter().sum::<f64>();
    assert!(
        (f64::from(vertical_update.weights()[vertical_index].get())
            - (vertical_before + vertical_delta) / vertical_available)
            .abs()
            <= 1.0e-6
    );

    let delivered_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        update_point,
        None,
    );
    let delivered = pointer_interaction_outcome(&delivered_transition).clone();
    let InteractionOutcome::ResizeDelivered {
        session: delivered_session,
        outcome:
            CommandOutcome::SplitsResized {
                splits: committed,
                changed: true,
            },
        changed: true,
        ..
    } = delivered
    else {
        panic!("junction release must commit one two-split command: {delivered:?}");
    };
    assert_eq!(delivered_session, session);
    assert_eq!(
        committed.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([horizontal, vertical])
    );
    assert_eq!(
        engine.version().revision().get(),
        initial_version.revision().get() + 1
    );
    assert_eq!(
        node_weights(engine.workspace(), horizontal),
        horizontal_update.weights()
    );
    assert_eq!(
        node_weights(engine.workspace(), vertical),
        vertical_update.weights()
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
}

#[test]
fn cross_junction_clamps_all_touching_handles_to_one_common_axis_delta() {
    let (workspace, root, left, right) = cross_junction_workspace();
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    support::publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        rect(400.0, 500.0),
        support::MeasurementProfile {
            pane_minimum: size(0.0, 20.0),
            ..support::MeasurementProfile::default()
        },
    );
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let cross_junctions = ready
        .plan()
        .splitter_junction_records()
        .iter()
        .filter(|junction| junction.id().arm_count() == 4)
        .collect::<Vec<_>>();
    let [junction] = cross_junctions.as_slice() else {
        panic!(
            "aligned left and right splitters expose one four-arm junction; splitters={:?}; junctions={:?}",
            ready.plan().splitter_records(),
            ready.plan().splitter_junction_records()
        );
    };
    assert_eq!(junction.id().arm_count(), 4);
    assert_eq!(junction.id().splitters().len(), 3);
    let press = center(junction.hit().rect());
    let (region, region_press) = resize_region(
        &engine,
        PresentationHitRegionKind::SplitterJunction(junction.id()),
    );
    assert_eq!(press, region_press);
    let (_, left_record) = splitter(&engine, left);
    let (_, right_record) = splitter(&engine, right);
    let positive_limit = |record: &SplitterRecord| {
        let index = record.id().index;
        let before = record.child_extents()[index];
        let after = record.child_extents()[index + 1];
        (record.before_maximum_extent() - before).min(after - record.after_minimum_extent())
    };
    assert!(positive_limit(&left_record) < positive_limit(&right_record));
    let initial_version = engine.version();
    let mut pointer = create_pointer_stream(&mut engine, &host);

    let pressed = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    let session = match pointer_interaction_outcome(&pressed) {
        InteractionOutcome::ResizeBegan { session, .. } => session,
        outcome => panic!("expected cross-junction resize activation, got {outcome:?}"),
    };
    let session = *session;
    let update_point = LogicalPoint::new(press.x(), press.y() + 10_000.0)
        .expect("cross-junction update point is valid");
    let update_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        update_point,
        None,
    );
    let update = pointer_interaction_outcome(&update_transition).clone();
    let InteractionOutcome::ResizeUpdated {
        session: updated_session,
        splits,
        ..
    } = update
    else {
        panic!("cross-junction update must publish one atomic batch: {update:?}");
    };
    assert_eq!(updated_session, session);
    assert_eq!(splits.len(), 3);
    let projected_delta = |record: &SplitterRecord, split: NodeId| {
        let update = splits
            .iter()
            .find(|update| update.split().node() == split)
            .expect("touching split has one update");
        let index = record.id().index;
        let available = record.child_extents().iter().sum::<f64>();
        f64::from(update.weights()[index].get()) * available - record.child_extents()[index]
    };
    let left_delta = projected_delta(&left_record, left);
    let right_delta = projected_delta(&right_record, right);
    assert!((left_delta - right_delta).abs() <= 1.0e-4);
    assert!((left_delta - positive_limit(&left_record)).abs() <= 1.0e-4);

    let delivered_transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        update_point,
        None,
    );
    let delivered = pointer_interaction_outcome(&delivered_transition).clone();
    let InteractionOutcome::ResizeDelivered {
        session: delivered_session,
        outcome:
            CommandOutcome::SplitsResized {
                splits: committed,
                changed: true,
            },
        changed: true,
        ..
    } = delivered
    else {
        panic!("cross-junction release must commit one three-split command: {delivered:?}");
    };
    assert_eq!(delivered_session, session);
    assert_eq!(
        committed.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([root, left, right])
    );
    assert_eq!(
        engine.version().revision().get(),
        initial_version.revision().get() + 1
    );
}

#[test]
fn stale_second_split_rejects_the_entire_resize_batch_without_mutation() {
    const ROOT_A: RootId = RootId::new(10);
    const ROOT_B: RootId = RootId::new(20);
    const SURFACE_A: SurfaceId = SurfaceId::new(10);
    const SURFACE_B: SurfaceId = SurfaceId::new(20);

    let mut builder = Workspace::builder();
    let a_left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let a_right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let split_a = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [a_left, a_right]).expect("split A is valid"),
    );
    let b_top = builder.insert_node(Node::tabs([ItemId::new(20)]));
    let b_bottom = builder.insert_node(Node::tabs([ItemId::new(21)]));
    let split_b = builder.insert_node(
        Node::equal_split(Axis::Vertical, [b_top, b_bottom]).expect("split B is valid"),
    );
    builder.set_root(ROOT_A, RootRecord::new(split_a));
    builder.set_root(ROOT_B, RootRecord::new(split_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(ROOT_A));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(ROOT_B));
    let mut workspace = builder.build().expect("two-root workspace is valid");

    let source_a = workspace
        .capture_node_source(ROOT_A, split_a)
        .expect("first source is current");
    let stale_source_b = workspace
        .capture_node_source(ROOT_B, split_b)
        .expect("second source is initially current");
    let current_source_b = stale_source_b.clone();
    WorkspaceTransaction::from_commands([WorkspaceCommand::ResizeSplits {
        splits: vec![SplitResize::new(current_source_b, weights(0.4, 0.6))],
    }])
    .apply(
        &mut workspace,
        &DockPolicy::default().snapshot(PolicyRevision::new(1)),
    )
    .expect("intervening resize makes only root B's source stale");
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::ResizeSplits {
        splits: vec![
            SplitResize::new(source_a, weights(0.25, 0.75)),
            SplitResize::new(stale_source_b, weights(0.3, 0.7)),
        ],
    }])
    .apply(
        &mut workspace,
        &DockPolicy::default().snapshot(PolicyRevision::new(2)),
    )
    .expect_err("a stale second entry rejects the complete command");

    assert!(matches!(
        error,
        TransactionError::Command {
            index: 0,
            source: CommandError::StaleNode {
                role: ReferenceRole::Source,
                node,
                ..
            },
        } if node == split_b
    ));
    assert_eq!(workspace, before);
}

#[test]
fn disabled_vertical_policy_omits_corner_and_rejects_direct_batch_without_partial_resize() {
    let (workspace, horizontal, vertical) = junction_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(Axis::Vertical, false);

    let mut command_workspace = workspace.clone();
    let horizontal_source = command_workspace
        .capture_node_source(ROOT, horizontal)
        .expect("horizontal source is current");
    let vertical_source = command_workspace
        .capture_node_source(ROOT, vertical)
        .expect("vertical source is current");
    let before_command = command_workspace.clone();
    let error = WorkspaceTransaction::from_commands([WorkspaceCommand::ResizeSplits {
        splits: vec![
            SplitResize::new(horizontal_source, weights(0.25, 0.75)),
            SplitResize::new(vertical_source, weights(0.25, 0.75)),
        ],
    }])
    .apply(
        &mut command_workspace,
        &policy.snapshot(PolicyRevision::new(1)),
    )
    .expect_err("disabled vertical axis rejects the batch");
    assert!(matches!(
        error,
        TransactionError::Command {
            source: CommandError::Policy(PolicyRejection::ResizeAxisDisabled {
                axis: Axis::Vertical,
            }),
            ..
        }
    ));
    assert_eq!(command_workspace, before_command);

    let mut engine = DockEngine::new(workspace, policy).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let horizontal = ready
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == horizontal)
        .expect("horizontal splitter is present");
    let vertical = ready
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == vertical)
        .expect("vertical splitter is present");
    assert!(horizontal.operable());
    assert!(!vertical.operable());
    assert!(ready.plan().splitter_junction_records().is_empty());
}

#[test]
fn fully_occluded_splitter_has_no_pointer_or_scene_bound_action_authority() {
    const FRONT_ROOT: RootId = RootId::new(2);
    const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
    let rear_split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("rear split is valid"),
    );
    let front = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(ROOT, RootRecord::new(rear_split));
    builder.set_root(FRONT_ROOT, RootRecord::new(front));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        FRONT_FLOATING,
        ContainedFloating::new(FRONT_ROOT, rect(400.0, 240.0)),
    );
    builder
        .attach_contained(SURFACE, FRONT_FLOATING)
        .expect("surface accepts front contained presentation");
    let workspace = builder.build().expect("occlusion workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    publish(
        &mut engine,
        &mut host,
        support::MeasurementProfile::default(),
    );

    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface has painted authority");
    let record = ready
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == rear_split)
        .cloned()
        .expect("rear splitter remains paintable");
    assert!(!record.operable());
    let point = center(record.hit().rect());
    assert_eq!(
        ready
            .plan()
            .splitter_resize_target_at(point)
            .expect("occluded hit map is unambiguous"),
        None
    );
    let scene = ready.stamp();
    let splitter = *record.id();
    let before_workspace = engine.workspace().clone();
    let before_version = engine.version();

    let outcome = adjust_splitter(&mut engine, &mut host, scene, splitter, 24.0);

    assert_eq!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::SplitterGestureHitUnavailable {
            surface: SURFACE,
        })
    );
    assert_eq!(engine.workspace(), &before_workspace);
    assert_eq!(engine.version(), before_version);
    assert_eq!(
        node_weights(engine.workspace(), rear_split),
        weights(0.5, 0.5)
    );
}
