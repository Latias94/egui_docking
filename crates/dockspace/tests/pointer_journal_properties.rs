mod support;

use dockspace::command::WorkspaceCommand;
use dockspace::engine::{CoreHostFrame, CoreHostFrameError, DockEngine, EngineError, EngineInput};
use dockspace::geometry::LogicalPoint;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SourceSequence, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, PointerId};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerStreamCancelReason, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerProvider, SurfaceLocalPointerProviderError, SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverObservation, PointerReceiverReceiptBatch, PointerReceiverReceiptValidationError,
    PointerReceiverUnknownReason,
};
use dockspace::policy::DockPolicy;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;
use support::{TestPresentationHost, complete_host_frame_with_retained_or_unavailable};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const SEMANTIC_SOURCE: StableInputSourceId = StableInputSourceId::new(401);
const PROPERTY_SEED: u64 = 0xD0C5_5A5A_2026_0726;

fn workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("property workspace is valid"), tabs)
}

fn create_provider(
    engine: &mut DockEngine,
    host: &TestPresentationHost,
    committed_through: u64,
) -> SurfaceLocalPointerProvider {
    engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            ),
            PointerEdgeSequence::new(committed_through),
        )
        .expect("property provider is admitted")
}

fn terminal_journal(previous: u64, pointers: &[u8]) -> PointerEdgeJournal {
    assert!(!pointers.is_empty(), "property segments are non-empty");
    let edges = pointers
        .iter()
        .copied()
        .enumerate()
        .map(|(index, pointer)| {
            let sequence = previous
                .checked_add(u64::try_from(index).expect("property index fits u64") + 1)
                .expect("property sequence does not overflow");
            PointerEdge::new(
                PointerEdgeSequence::new(sequence),
                PointerId::new(u64::from(pointer)),
                PointerEdgeKind::StreamCancelled(
                    PointerStreamCancelReason::ExplicitPlatformCancellation,
                ),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(
                        LogicalPoint::new(f64::from(pointer), 8.0)
                            .expect("property point is finite"),
                    ),
                },
                Authority::Known(PointerCaptureOwner::None),
            )
        })
        .collect::<Vec<_>>();
    let through = previous
        .checked_add(u64::try_from(edges.len()).expect("property length fits u64"))
        .expect("property watermark does not overflow");
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        PointerEdgeSequence::new(through),
        edges,
    )
    .expect("property journal is contiguous")
}

fn submit_terminal_segment(
    frame: &mut CoreHostFrame,
    provider: &SurfaceLocalPointerProvider,
    previous: u64,
    pointers: &[u8],
    invalid_receipt: bool,
) -> Result<(), CoreHostFrameError> {
    let journal = terminal_journal(previous, pointers);
    let mut previous = journal.previous();
    for (index, edge) in journal.edges().iter().cloned().enumerate() {
        let through = edge.sequence();
        let segment = PointerEdgeJournal::new(previous, through, vec![edge])
            .expect("single property edge segment is contiguous");
        frame.submit_surface_pointer_journal(provider, segment)?;
        let candidate = frame
            .pointer_receiver_candidates()
            .expect("property segment freezes candidates")
            .candidates()[0]
            .clone();
        let observation = if invalid_receipt && index == 0 {
            PointerReceiverObservation::Unknown(PointerReceiverUnknownReason::NotReported)
        } else {
            PointerReceiverObservation::NotApplicable
        };
        frame.submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("property receipt forms an exact candidate set"),
        )?;
        previous = through;
    }
    Ok(())
}

fn finish(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    mut frame: CoreHostFrame,
) -> dockspace::transition::EngineTransition {
    complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    host.finish(frame, engine)
}

fn selected_item(engine: &DockEngine, tabs: NodeId) -> Option<ItemId> {
    match engine.workspace().node(tabs) {
        Some(Node::Tabs { selected, .. }) => *selected,
        other => panic!("property tabs node is missing: {other:?}"),
    }
}

fn rollback_cases() -> impl Strategy<Value = (Vec<u8>, Vec<Vec<u8>>, usize)> {
    (
        prop::collection::vec(1_u8..5, 0..6),
        prop::collection::vec(prop::collection::vec(1_u8..5, 1..4), 2..6),
    )
        .prop_flat_map(|(prefix, segments)| {
            let semantic_slots = 0..segments.len();
            (Just(prefix), Just(segments), semantic_slots)
        })
}

fn provider_cycles() -> impl Strategy<Value = Vec<(u8, Vec<u8>)>> {
    prop::collection::vec((0_u8..16, prop::collection::vec(1_u8..5, 1..4)), 2..8)
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        rng_seed: RngSeed::Fixed(PROPERTY_SEED),
        ..ProptestConfig::default()
    })]

    #[test]
    fn failed_late_segment_rolls_back_watermarks_streams_and_semantic_state(
        (prefix, segments, semantic_slot) in rollback_cases(),
    ) {
        let (workspace, tabs) = workspace();
        let mut engine = DockEngine::new(workspace, DockPolicy::default())
            .expect("property engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        let provider = create_provider(&mut engine, &host, 0);

        if !prefix.is_empty() {
            let mut prefix_frame = host.begin(&engine);
            submit_terminal_segment(&mut prefix_frame, &provider, 0, &prefix, false)
                .expect("property prefix stages edgewise");
            let transition = finish(&mut engine, &mut host, prefix_frame);
            for (index, edge) in transition.reduced_pointer_edges().iter().enumerate() {
                prop_assert_eq!(
                    edge.stream().incarnation(),
                    u64::try_from(index).expect("property index fits u64") + 1,
                );
            }
        }

        let prefix_watermark = u64::try_from(prefix.len()).expect("property prefix fits u64");
        let before_workspace = engine.workspace().clone();
        let before_version = engine.version();
        let before_tick = engine.last_reducer_tick();
        let before_interaction = engine.interaction().clone();
        prop_assert_eq!(selected_item(&engine, tabs), Some(ItemId::new(1)));
        let select_second = WorkspaceCommand::Select {
            source: engine
                .workspace()
                .capture_item_source(ROOT, tabs, ItemId::new(2))
                .expect("second item is capturable"),
        };

        let mut failed = host.begin(&engine);
        let mut previous = prefix_watermark;
        for (index, segment) in segments.iter().enumerate() {
            if index == semantic_slot {
                failed
                    .append_input(
                        SEMANTIC_SOURCE,
                        SourceSequence::new(1),
                        EngineInput::WorkspaceCommand {
                            expected: before_version,
                            command: select_second.clone(),
                        },
                    )
                    .expect("semantic mutation stages before the failing segment");
            }
            let result = submit_terminal_segment(
                &mut failed,
                &provider,
                previous,
                segment,
                index + 1 == segments.len(),
            );
            if index + 1 == segments.len() {
                prop_assert_eq!(
                    result,
                    Err(CoreHostFrameError::InputPrefixReductionFailed),
                    "the deliberately invalid terminal receipt must poison the frame"
                );
            } else {
                result.expect("property segment stages edgewise");
            }
            previous += u64::try_from(segment.len()).expect("property segment fits u64");
        }
        prop_assert!(matches!(
            failed.finish(&mut engine),
            Err(EngineError::PointerReceiverReceipt {
                source: PointerReceiverReceiptValidationError::UnknownForNotApplicableCandidate { .. },
            })
        ), "the deliberately invalid terminal receipt must reject the whole frame");
        prop_assert_eq!(engine.workspace(), &before_workspace);
        prop_assert_eq!(engine.version(), before_version);
        prop_assert_eq!(engine.last_reducer_tick(), before_tick);
        prop_assert_eq!(engine.interaction(), &before_interaction);
        prop_assert_eq!(engine.pointer_provider(), Some(provider.lease()));

        let mut retry = host.begin(&engine);
        previous = prefix_watermark;
        for (index, segment) in segments.iter().enumerate() {
            if index == semantic_slot {
                retry
                    .append_input(
                        SEMANTIC_SOURCE,
                        SourceSequence::new(1),
                        EngineInput::WorkspaceCommand {
                            expected: before_version,
                            command: select_second.clone(),
                        },
                    )
                    .expect("failed frame did not consume the semantic source sequence");
            }
            submit_terminal_segment(&mut retry, &provider, previous, segment, false)
                .expect("property retry segment stages edgewise");
            previous += u64::try_from(segment.len()).expect("property segment fits u64");
        }
        let transition = finish(&mut engine, &mut host, retry);

        prop_assert_eq!(selected_item(&engine, tabs), Some(ItemId::new(2)));
        prop_assert_eq!(transition.reduced_inputs().len(), 1);
        let semantic_ordinal = segments[..semantic_slot]
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        prop_assert_eq!(
            transition.reduced_inputs()[0].causal_ordinal().get(),
            u64::try_from(semantic_ordinal).expect("property ordinal fits u64"),
        );
        let reduced = transition.reduced_pointer_edges();
        let expected_edge_count = segments.iter().map(Vec::len).sum::<usize>();
        prop_assert_eq!(reduced.len(), expected_edge_count);
        let mut offset = 0;
        let mut causal_ordinal = 0_u64;
        for (segment_index, segment) in segments.iter().enumerate() {
            if segment_index == semantic_slot {
                causal_ordinal += 1;
            }
            for edge in &reduced[offset..offset + segment.len()] {
                prop_assert_eq!(
                    edge.causal_ordinal().get(),
                    causal_ordinal,
                );
                causal_ordinal += 1;
            }
            offset += segment.len();
        }
        for (index, edge) in reduced.iter().enumerate() {
            let expected = prefix_watermark
                + u64::try_from(index).expect("property index fits u64")
                + 1;
            prop_assert_eq!(edge.edge().sequence(), PointerEdgeSequence::new(expected));
            prop_assert_eq!(edge.stream().incarnation(), expected);
            prop_assert_eq!(edge.stream().lease(), provider.lease());
        }

        let committed_through = previous;
        let tick_after_success = engine.last_reducer_tick();
        let mut replay = host.begin(&engine);
        let replay_source = terminal_journal(prefix_watermark, &segments[0]);
        let replay_edge = replay_source.edges()[0].clone();
        let replay_journal = PointerEdgeJournal::new(
            replay_source.previous(),
            replay_edge.sequence(),
            vec![replay_edge],
        )
        .expect("replay edge segment is contiguous");
        let replay_result = replay.submit_surface_pointer_journal(&provider, replay_journal);
        prop_assert!(matches!(
            replay_result,
            Err(CoreHostFrameError::SurfaceLocalPointerProducerRejected {
                source: SurfaceLocalPointerProviderError::CommittedWatermarkMismatch {
                    lease,
                    committed_through: actual,
                    submitted_previous,
                },
            }) if lease == provider.lease()
                && actual == PointerEdgeSequence::new(committed_through)
                && submitted_previous == PointerEdgeSequence::new(prefix_watermark)
        ), "a successful retry must advance the affine producer watermark exactly once, got {replay_result:?}");
        prop_assert_eq!(engine.last_reducer_tick(), tick_after_success);
        prop_assert_eq!(engine.pointer_provider(), Some(provider.lease()));
    }

    #[test]
    fn quiesced_provider_compaction_prevents_aba_across_successor_incarnations(
        cycles in provider_cycles(),
    ) {
        let (workspace, _) = workspace();
        let mut engine = DockEngine::new(workspace, DockPolicy::default())
            .expect("property engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        let mut retired = Vec::new();
        let mut last_stream_incarnation = 0_u64;

        for (cycle_index, (base, pointers)) in cycles.iter().enumerate() {
            let base = u64::from(*base);
            let provider = create_provider(&mut engine, &host, base);
            let lease = provider.lease();
            prop_assert_eq!(
                lease.incarnation(),
                u64::try_from(cycle_index).expect("property cycle fits u64") + 1,
            );

            for previous_provider in &retired {
                prop_assert_ne!(lease, *previous_provider);
            }
            prop_assert_eq!(engine.pointer_provider(), Some(lease));

            let mut frame = host.begin(&engine);
            submit_terminal_segment(&mut frame, &provider, base, pointers, false)
                .expect("property cycle stages edgewise");
            let transition = finish(&mut engine, &mut host, frame);
            prop_assert_eq!(transition.reduced_pointer_edges().len(), pointers.len());
            for (index, edge) in transition.reduced_pointer_edges().iter().enumerate() {
                last_stream_incarnation += 1;
                prop_assert_eq!(edge.stream().incarnation(), last_stream_incarnation);
                prop_assert_eq!(edge.stream().lease(), lease);
                prop_assert_eq!(edge.stream().pointer(), PointerId::new(u64::from(pointers[index])));
                prop_assert_eq!(
                    edge.edge().sequence(),
                    PointerEdgeSequence::new(
                        base + u64::try_from(index).expect("property index fits u64") + 1,
                    ),
                );
            }

            let committed_through =
                base + u64::try_from(pointers.len()).expect("property segment fits u64");
            let mut receipt = provider
                .drain()
                .expect("a committed property provider has no in-flight host frame");
            prop_assert_eq!(
                receipt.committed_through(),
                PointerEdgeSequence::new(committed_through),
            );
            let retirement = engine
                .retire_quiesced_surface_local_pointer_provider(&mut receipt)
                .expect("live property provider retires and compacts");
            prop_assert!(retirement.repaint_required());
            prop_assert!(receipt.is_consumed());
            prop_assert_eq!(engine.pointer_provider(), None);
            retired.push(lease);

            let retention = engine.runtime_retention_manifest().pointer();
            prop_assert_eq!(retention.retired_lease_guards(), 0);
            prop_assert_eq!(retention.compacted_retirement_ranges(), 1);
            prop_assert_eq!(
                retention.logical_compacted_leases(),
                u64::try_from(cycle_index).expect("property cycle index fits u64") + 1,
            );
        }
    }
}
