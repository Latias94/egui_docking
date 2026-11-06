use super::*;
use crate::drop_resolver::structural_work;

fn fixture() -> (DockEngine, PresentationHostLease, crate::ids::NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
    let host = engine.create_presentation_host().unwrap();
    (engine, host, tabs)
}

#[test]
fn local_selection_and_reselection_preserve_revision_events_and_focus() {
    for product_action in [false, true] {
        let (mut engine, host, tabs) = fixture();
        for changed in [true, false] {
            let before = engine.version();
            let source = engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, tabs, ItemId::new(2))
                .unwrap();
            structural_work::reset();
            let input = if product_action {
                EngineInput::SelectItem {
                    expected: before,
                    item: ItemId::new(2),
                }
            } else {
                EngineInput::WorkspaceCommand {
                    expected: before,
                    command: WorkspaceCommand::Select { source },
                }
            };
            let transition = submit_test_input(&mut engine, host, input).unwrap();
            assert_eq!(structural_work::snapshot().item_multiset_scans, 0);
            if !product_action {
                assert!(matches!(transition.reduced_inputs()[0].outcome(),
                InputOutcome::CommandProcessed { changed: actual, .. } if *actual == changed));
            } else {
                assert!(matches!(
                    transition.reduced_inputs()[0].outcome(),
                    InputOutcome::ProductActionProcessed { .. }
                ));
            }
            assert_eq!(engine.version() != before, changed);
            assert_eq!(
                transition
                    .events()
                    .iter()
                    .filter(|event| matches!(event.kind(), WorkspaceEventKind::CommandCommitted(_)))
                    .count(),
                usize::from(changed)
            );
            assert_eq!(
                engine.workspace().tab_mru(tabs),
                Some([ItemId::new(2), ItemId::new(1)].as_slice())
            );
            assert_eq!(
                engine.viewport_focus().published_pane_intent().is_some(),
                product_action
            );
            if let Some(intent) = engine.viewport_focus().published_pane_intent() {
                assert_eq!(intent.item(), Some(ItemId::new(2)));
            }
            assert_eq!(
                transition.focus_delta().pane_intent().is_some(),
                product_action
            );
        }
    }
}

#[test]
fn journal_selection_keeps_the_checked_result_and_discarded_frame_is_atomic() {
    let (mut engine, host, tabs) = fixture();
    let source = engine
        .workspace()
        .capture_item_source(SOURCE_ROOT, tabs, ItemId::new(2))
        .unwrap();
    let command = WorkspaceCommand::Select {
        source: source.clone(),
    };
    structural_work::reset();
    let staged = engine
        .stage_journal_workspace_command_with_authority(
            tab_strip_test_cause(),
            &command,
            &engine.policy,
            WorkspacePublicationAuthority::Ordinary,
            JournalDeliveryActivation::None,
        )
        .unwrap()
        .unwrap();
    assert_eq!(structural_work::snapshot().item_multiset_scans, 0);
    assert!(staged.changed);
    drop(staged);

    let original = engine.workspace.clone();
    let version = engine.version();
    let mut stream = TestInputStream::resume(&engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(&engine, host);
    stream
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: version,
                command: command.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        frame.candidate.workspace().tab_mru(tabs),
        Some([ItemId::new(2), ItemId::new(1)].as_slice())
    );
    let mut invalid = engine.workspace().clone().into_builder();
    invalid.set_tab_mru(tabs, []).unwrap();
    assert!(invalid.build().is_err());
    drop(frame);
    assert_eq!(engine.workspace, original);
    assert_eq!(engine.version(), version);
    assert!(engine.viewport_focus().published_pane_intent().is_none());

    submit_test_input(
        &mut engine,
        host,
        EngineInput::WorkspaceCommand {
            expected: version,
            command,
        },
    )
    .unwrap();
    let stale = WorkspaceCommand::Select { source };
    let (_, application) = engine
        .stage_workspace_command(engine.last_input, &engine.policy, &stale, None)
        .unwrap();
    assert!(matches!(
        application,
        CommandApplication::Rejected(CommandError::StaleNode { .. })
    ));
}
