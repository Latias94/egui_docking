use super::*;
use crate::engine::surface_contribution::PreparedSurfacePaintCandidate;

fn fixture() -> (DockEngine, PresentationHostLease, crate::ids::NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
    let host = engine.create_presentation_host().unwrap();
    (engine, host, tabs)
}

fn prepare_measurements(frame: &CoreHostFrame) -> PreparedSurfaceContribution {
    let candidate = frame.candidate_engine();
    let token = frame
        .view()
        .begin_surface_contribution(SOURCE_SURFACE)
        .unwrap();
    frame
        .view()
        .prepare_surface_contribution(
            token,
            test_support::measurements(
                candidate,
                SOURCE_SURFACE,
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                test_support::MeasurementProfile::default(),
            ),
        )
        .unwrap()
}

fn assert_height(plan: &PresentationPlan, height: f64) {
    assert_eq!(plan.tab_bar_records().len(), 1);
    assert_eq!(plan.tab_bar_records()[0].bounds().height(), height);
}

fn configured_prefix(
    engine: &DockEngine,
    host: PresentationHostLease,
    tabs: crate::ids::NodeId,
) -> CoreHostFrame {
    let mut frame = begin_test_host_frame(engine, host);
    let mut stream = TestInputStream::resume(engine, ENGINE_TEST_INPUT_SOURCE);
    let expected = frame.view().version();
    let target = frame
        .view()
        .workspace()
        .capture_tab_target(SOURCE_ROOT, tabs)
        .unwrap();
    stream
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::Open {
                    item: ItemId::new(3),
                    target: crate::command::DockTarget::Center(target),
                },
            },
        )
        .unwrap();
    let mut policy = frame.view().policy().clone();
    policy.set_allow_tab_merge(false);
    stream
        .append(&mut frame, EngineInput::ReplacePolicy { expected, policy })
        .unwrap();
    let config = frame
        .view()
        .presentation_config()
        .to_builder()
        .tab_bar_height(44.0)
        .build()
        .unwrap();
    stream
        .append(
            &mut frame,
            EngineInput::ReplacePresentationConfig { expected, config },
        )
        .unwrap();
    frame
}

#[test]
fn configuration_applies_before_inline_and_deferred_surface_compilation() {
    for compile_inline in [false, true] {
        let (mut engine, host, tabs) = fixture();
        assert_eq!(engine.presentation_config().tab_bar_height(), 28.0);
        let before = engine.version();
        let mut frame = configured_prefix(&engine, host, tabs);
        assert_eq!(frame.view().presentation_config().tab_bar_height(), 44.0);
        assert!(!frame.view().policy().allows_tab_merge());
        assert!(frame.tick_policy.allows_tab_merge());
        assert_eq!(frame.application_base, before);
        assert_eq!(frame.reduced_inputs.len(), 3);
        assert!(
            frame
                .view()
                .workspace()
                .locate_item_by_id(ItemId::new(3))
                .is_some()
        );
        assert_eq!(engine.presentation_config().tab_bar_height(), 28.0);
        assert!(engine.policy().allows_tab_merge());
        assert_eq!(engine.version(), before);

        let contribution = prepare_measurements(&frame);
        let PreparedSurfacePaintCandidate::Ready(ready) = contribution.paint_candidate() else {
            panic!("the new configuration must compile a ready surface");
        };
        assert_height(ready.plan(), 44.0);
        if compile_inline {
            frame.compile_surface_contribution(contribution).unwrap();
            assert!(matches!(
                frame.compiled_surface_outcomes[&SOURCE_SURFACE],
                SurfaceContributionOutcome::Ready { .. }
            ));
            complete_host_frame_with_current_outputs(&engine, &mut frame);
        } else {
            frame.push_surface_contribution(contribution).unwrap();
            complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
        }
        let transition = frame.finish(&mut engine).unwrap();
        assert_eq!(transition.reduced_inputs().len(), 3);
        assert!(matches!(
            transition.reduced_inputs()[0].outcome(),
            InputOutcome::CommandProcessed { changed: true, .. }
        ));
        assert!(matches!(
            transition.reduced_inputs()[1].outcome(),
            InputOutcome::PolicyReplaced { changed: true, .. }
        ));
        assert!(matches!(
            transition.reduced_inputs()[2].outcome(),
            InputOutcome::PresentationConfigReplaced { changed: true, .. }
        ));
        for (offset, input) in transition.reduced_inputs().iter().enumerate() {
            let sequence = u64::try_from(offset + 1).unwrap();
            assert_eq!(input.source(), ENGINE_TEST_INPUT_SOURCE);
            assert_eq!(input.source_sequence(), SourceSequence::new(sequence));
            assert_eq!(input.causal_ordinal().get(), sequence - 1);
        }
        assert!(matches!(
            transition.surface_contributions(),
            [SurfaceContributionOutcome::Ready { surface, .. }] if *surface == SOURCE_SURFACE
        ));
        assert_eq!(
            transition.presentation_emissions().len(),
            usize::from(compile_inline)
        );
        assert_height(
            engine
                .scene()
                .ready_candidate(SOURCE_SURFACE)
                .unwrap()
                .plan(),
            44.0,
        );
        assert_eq!(
            engine.version().revision().get(),
            before.revision().get() + 2
        );
        assert_eq!(
            engine.semantic_input_watermark(),
            Some(SourceSequence::new(3))
        );
    }
}

#[test]
fn configuration_keeps_source_order_and_workspace_replacement_application_base() {
    let (mut engine, host, _) = fixture();
    let old_version = engine.version();
    let original_config = engine.presentation_config().clone();
    let replacement = engine.workspace().clone().into_builder().build().unwrap();
    let source = StableInputSourceId::new(90);
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .append_input(
            source,
            SourceSequence::new(1),
            EngineInput::ReplaceWorkspace(replacement),
        )
        .unwrap();
    let accepted_base = frame.view().version();
    assert_ne!(accepted_base, old_version);
    frame
        .append_configuration(
            source,
            SourceSequence::new(2),
            EngineInput::ReplacePresentationConfig {
                expected: old_version,
                config: original_config
                    .to_builder()
                    .tab_bar_height(44.0)
                    .build()
                    .unwrap(),
            },
        )
        .unwrap();
    assert_eq!(frame.view().presentation_config(), &original_config);
    frame
        .append_presentation_config_replacement(
            source,
            SourceSequence::new(3),
            original_config
                .to_builder()
                .tab_bar_height(44.0)
                .build()
                .unwrap(),
        )
        .unwrap();
    frame
        .append_presentation_config_replacement(
            source,
            SourceSequence::new(4),
            original_config
                .to_builder()
                .tab_bar_height(44.0)
                .build()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(frame.view().presentation_config().tab_bar_height(), 44.0);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame.finish(&mut engine).unwrap();
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::StaleRejected { expected, accepted_base: actual }
            if *expected == old_version && *actual == accepted_base
    ));
    for (offset, changed) in [(2, true), (3, false)] {
        let input = &transition.reduced_inputs()[offset];
        assert_eq!(input.source(), source);
        assert_eq!(
            input.source_sequence().get(),
            u64::try_from(offset + 1).unwrap()
        );
        assert!(matches!(
            input.outcome(),
            InputOutcome::PresentationConfigReplaced { changed: actual, revision }
                if *actual == changed && revision.get() == 1
        ));
    }
    assert_eq!(engine.version(), accepted_base);
    assert_eq!(
        engine.semantic_input_watermark(),
        Some(SourceSequence::new(4))
    );
}

#[test]
fn configuration_after_encoding_poisons_only_the_private_frame() {
    let (mut engine, host, _) = fixture();
    let before = engine.candidate();
    let mut frame = begin_test_host_frame(&engine, host);
    frame.capture_encoding_checkpoint().unwrap();
    let config = frame
        .view()
        .presentation_config()
        .to_builder()
        .tab_bar_height(44.0)
        .build()
        .unwrap();
    assert_eq!(
        frame.append_presentation_config_replacement(
            ENGINE_TEST_INPUT_SOURCE,
            SourceSequence::new(1),
            config,
        ),
        Err(CoreHostFrameError::InputAfterPresentation)
    );
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::HostFramePoisoned {
            source: CoreHostFrameError::InputAfterPresentation
        })
    ));
    assert_eq!(engine, before);
}

#[test]
fn configuration_encoding_restart_keeps_the_accepted_input_candidate() {
    for publish in [false, true] {
        let (mut engine, host, tabs) = fixture();
        let before = engine.candidate();
        let mut frame = configured_prefix(&engine, host, tabs);
        let workspace = frame.candidate.workspace.clone();
        let version = frame.view().version();
        let checkpoint = frame.capture_encoding_checkpoint().unwrap();
        let first = prepare_measurements(&frame);
        frame.compile_surface_contribution(first).unwrap();
        let discarded = frame
            .view()
            .scene()
            .ready_candidate(SOURCE_SURFACE)
            .unwrap()
            .output_ticket();

        frame.restart_encoding_from(&checkpoint).unwrap();
        assert_eq!(frame.reduced_inputs.len(), 3);
        assert_eq!(frame.candidate.last_input.get(), 3);
        assert_eq!(frame.view().version(), version);
        assert!(Arc::ptr_eq(
            workspace.as_arc(),
            frame.candidate.workspace.as_arc()
        ));
        assert_eq!(frame.view().presentation_config().tab_bar_height(), 44.0);
        assert!(!frame.view().policy().allows_tab_merge());
        assert!(frame.tick_policy.allows_tab_merge());
        assert!(frame.compiled_surface_outcomes.is_empty());
        let second = prepare_measurements(&frame);
        frame.compile_surface_contribution(second).unwrap();
        let replacement = frame
            .view()
            .scene()
            .ready_candidate(SOURCE_SURFACE)
            .unwrap()
            .output_ticket();
        assert_ne!(discarded, replacement);
        if publish {
            complete_host_frame_with_current_outputs(&engine, &mut frame);
            let transition = frame.finish(&mut engine).unwrap();
            assert_eq!(transition.reduced_inputs().len(), 3);
            assert_eq!(transition.presentation_emissions().len(), 1);
            assert_eq!(engine.version(), version);
            assert_eq!(engine.last_input.get(), 3);
            assert_eq!(
                transition
                    .events()
                    .iter()
                    .filter(|event| matches!(event.kind(), WorkspaceEventKind::CommandCommitted(_)))
                    .count(),
                1
            );
        } else {
            drop(frame);
            assert_eq!(engine, before);
        }
    }
}

#[test]
fn configuration_rejects_contributions_prepared_under_the_old_ticket() {
    let (mut engine, host, _) = fixture();
    let mut frame = begin_test_host_frame(&engine, host);
    let contribution = prepare_measurements(&frame);
    let config = frame
        .view()
        .presentation_config()
        .to_builder()
        .tab_bar_height(44.0)
        .build()
        .unwrap();
    frame
        .append_presentation_config_replacement(
            ENGINE_TEST_INPUT_SOURCE,
            SourceSequence::new(1),
            config,
        )
        .unwrap();
    frame.push_surface_contribution(contribution).unwrap();
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame.finish(&mut engine).unwrap();
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }]
    ));
    assert_eq!(engine.presentation_config().tab_bar_height(), 44.0);
    assert!(engine.scene().ready_candidate(SOURCE_SURFACE).is_none());
    assert!(transition.presentation_emissions().is_empty());
}
