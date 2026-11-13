use super::support;

use dockspace::command::{CommandOutcome, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::CommandError;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    ItemId, RootId, SourceSequence, StableInputSourceId, SurfaceId, WorkspaceEpoch,
    WorkspaceRevision,
};
use dockspace::policy::DockPolicy;
use dockspace::transition::{InputOutcome, InputPriority, WorkspaceVersion};
use support::{TestInputStream, TestPresentationHost};

const TEST_SOURCE: StableInputSourceId = StableInputSourceId::new(0xA701);

fn workspace_with_roots(roots: &[(u64, u64, &[u64])]) -> (Workspace, Vec<dockspace::ids::NodeId>) {
    let mut builder = Workspace::builder();
    let mut tabs = Vec::with_capacity(roots.len());

    for &(root, surface, items) in roots {
        let tabs_node = builder.insert_node(Node::tabs(items.iter().copied().map(ItemId::new)));
        builder.set_root(RootId::new(root), RootRecord::new(tabs_node));
        builder.set_surface(
            SurfaceId::new(surface),
            SurfacePresentation::with_main(RootId::new(root)),
        );
        tabs.push(tabs_node);
    }

    (builder.build().expect("test workspace must be valid"), tabs)
}

fn select_command(
    workspace: &Workspace,
    root: u64,
    tabs: dockspace::ids::NodeId,
    item: u64,
) -> WorkspaceCommand {
    WorkspaceCommand::Select {
        source: workspace
            .capture_item_source(RootId::new(root), tabs, ItemId::new(item))
            .expect("test item source must be capturable"),
    }
}

#[test]
fn reduction_preserves_host_append_order_without_priority_sorting() {
    let (initial, tabs) = workspace_with_roots(&[(1, 1, &[1, 2])]);
    let mut engine = DockEngine::new(initial.clone(), DockPolicy::default())
        .expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let old_version = engine.version();
    let command = select_command(&initial, 1, tabs[0], 2);

    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(&mut frame, EngineInput::ValidateWorkspace)
        .expect("maintenance input fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: old_version,
                command,
            },
        )
        .expect("application input fits the semantic host-frame phase");
    inputs
        .append(&mut frame, EngineInput::ReplaceWorkspace(initial.clone()))
        .expect("lifecycle input fits the semantic host-frame phase");

    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    let reduced = transition.reduced_inputs();
    assert_eq!(reduced.len(), 3);
    assert_eq!(reduced[0].source_sequence(), SourceSequence::new(1));
    assert_eq!(reduced[0].priority(), InputPriority::Maintenance);
    assert_eq!(reduced[1].source_sequence(), SourceSequence::new(2));
    assert_eq!(reduced[1].priority(), InputPriority::ApplicationCommand);
    assert_eq!(reduced[2].source_sequence(), SourceSequence::new(3));
    assert_eq!(reduced[2].priority(), InputPriority::LifecycleControl);
    assert!(matches!(
        reduced[1].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert_eq!(engine.version().epoch().get(), 1);
    assert_eq!(engine.version().revision().get(), 0);
    assert_eq!(engine.workspace(), &initial);
}

#[test]
fn replacement_invalidates_old_references_even_when_workspace_is_identical() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2])]);
    let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let version_before = engine.version();
    let command = select_command(&workspace, 1, tabs[0], 2);

    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(&mut frame, EngineInput::ReplaceWorkspace(workspace.clone()))
        .expect("replacement fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: version_before,
                command,
            },
        )
        .expect("command fits the semantic host-frame phase");

    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert_eq!(
        transition.reduced_inputs()[0].source_sequence(),
        SourceSequence::new(1)
    );
    assert_eq!(
        transition.reduced_inputs()[1].source_sequence(),
        SourceSequence::new(2)
    );
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::StaleRejected {
            expected,
            accepted_base,
        } if *expected == version_before && *accepted_base == engine.version()
    ));
    assert_eq!(engine.version().epoch().get(), 1);
    assert_eq!(engine.version().revision().get(), 0);
    assert_eq!(engine.workspace(), &workspace);
    assert_eq!(transition.events().len(), 1);
}

#[test]
fn independent_commands_from_one_published_version_commit_in_one_boundary() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2]), (2, 2, &[3, 4])]);
    let first = select_command(&workspace, 1, tabs[0], 2);
    let second = select_command(&workspace, 2, tabs[1], 4);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);

    let expected = engine.version();
    let mut inputs = TestInputStream::new(TEST_SOURCE);

    let mut frame = host.begin(&engine);
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: first,
            },
        )
        .expect("first command fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: second,
            },
        )
        .expect("second command fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert_eq!(engine.version().revision().get(), 2);
    assert_eq!(transition.events().len(), 2);
    for reduced in transition.reduced_inputs() {
        assert!(matches!(
            reduced.outcome(),
            InputOutcome::CommandProcessed {
                changed: true,
                outcome: CommandOutcome::Selected { changed: true, .. },
                ..
            }
        ));
    }
    assert!(matches!(
        engine.workspace().node(tabs[0]),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(2)
    ));
    assert!(matches!(
        engine.workspace().node(tabs[1]),
        Some(Node::Tabs {
            selected: Some(item),
            ..
        }) if *item == ItemId::new(4)
    ));
}

#[test]
fn valid_no_op_does_not_advance_revision_or_emit_event() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2])]);
    let command = select_command(&workspace, 1, tabs[0], 1);
    let mut engine = DockEngine::new(workspace.clone(), DockPolicy::default())
        .expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();

    let transition = inputs_submit(
        &mut engine,
        &mut host,
        EngineInput::WorkspaceCommand { expected, command },
    )
    .expect("no-op must be valid");

    assert_eq!(transition.before(), transition.after());
    assert!(transition.events().is_empty());
    assert_eq!(engine.workspace(), &workspace);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed {
            changed: false,
            outcome: CommandOutcome::Selected { changed: false, .. },
            ..
        }
    ));
}

#[test]
fn expected_command_rejection_is_consumed_without_mutation() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1])]);
    let target = workspace
        .capture_tab_target(RootId::new(1), tabs[0])
        .expect("test target must be capturable");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let before = engine.workspace().clone();
    let before_version = engine.version();

    let transition = inputs_submit(
        &mut engine,
        &mut host,
        EngineInput::WorkspaceCommand {
            expected: before_version,
            command: WorkspaceCommand::Open {
                item: ItemId::new(1),
                target: dockspace::command::DockTarget::Center(target),
            },
        },
    )
    .expect("duplicate ownership is an expected consumed rejection");
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::ItemAlreadyOpen { item },
            version,
        } if *item == ItemId::new(1) && *version == before_version
    ));
    assert_eq!(engine.workspace(), &before);
    assert_eq!(engine.version(), before_version);
    assert!(transition.events().is_empty());
}

#[test]
fn expected_rejection_does_not_block_a_later_valid_command() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2])]);
    let target = workspace
        .capture_tab_target(RootId::new(1), tabs[0])
        .expect("test target must be capturable");
    let select = select_command(&workspace, 1, tabs[0], 2);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::Open {
                    item: ItemId::new(1),
                    target: dockspace::command::DockTarget::Center(target),
                },
            },
        )
        .expect("rejected command fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: select,
            },
        )
        .expect("valid command fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::ItemAlreadyOpen { .. },
            ..
        }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert_eq!(engine.version().revision().get(), 1);
    assert_eq!(transition.events().len(), 1);
    assert!(matches!(
        engine.workspace().node(tabs[0]),
        Some(Node::Tabs {
            selected: Some(selected),
            ..
        }) if *selected == ItemId::new(2)
    ));
}

#[test]
fn later_command_rejection_preserves_an_earlier_policy_change() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1])]);
    let target = workspace
        .capture_tab_target(RootId::new(1), tabs[0])
        .expect("test target must be capturable");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let expected = engine.version();
    let mut changed_policy = engine.policy().clone();
    changed_policy.set_allow_native_surfaces(true);

    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: WorkspaceCommand::Open {
                    item: ItemId::new(1),
                    target: dockspace::command::DockTarget::Center(target),
                },
            },
        )
        .expect("application command fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::ReplacePolicy {
                expected,
                policy: changed_policy,
            },
        )
        .expect("policy replacement fits the terminal configuration phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(engine.policy().allows_native_surfaces());
    assert_eq!(engine.version().revision().get(), 1);
    assert_eq!(transition.events().len(), 1);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::ItemAlreadyOpen { item },
            ..
        } if *item == ItemId::new(1)
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::PolicyReplaced { changed: true, .. }
    ));
}

#[test]
fn later_command_rejection_preserves_an_earlier_workspace_replacement() {
    let (initial, _) = workspace_with_roots(&[(1, 1, &[1])]);
    let (replacement, replacement_tabs) = workspace_with_roots(&[(2, 2, &[2])]);
    let target = replacement
        .capture_tab_target(RootId::new(2), replacement_tabs[0])
        .expect("replacement target must be capturable");
    let mut engine =
        DockEngine::new(initial, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(&mut frame, EngineInput::ReplaceWorkspace(replacement))
        .expect("replacement fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected: WorkspaceVersion::new(
                    WorkspaceEpoch::new(1),
                    WorkspaceRevision::default(),
                ),
                command: WorkspaceCommand::Open {
                    item: ItemId::new(2),
                    target: dockspace::command::DockTarget::Center(target),
                },
            },
        )
        .expect("application command fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert_eq!(
        engine.version(),
        WorkspaceVersion::new(WorkspaceEpoch::new(1), WorkspaceRevision::default())
    );
    assert_eq!(
        engine.workspace().item_multiset(),
        [(ItemId::new(2), 1)].into()
    );
    assert_eq!(transition.events().len(), 1);
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::ItemAlreadyOpen { item },
            ..
        } if *item == ItemId::new(2)
    ));
}

#[test]
fn same_root_commands_from_one_version_commit_then_reject_stale_source() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2, 3])]);
    let select_two = select_command(&workspace, 1, tabs[0], 2);
    let select_three = select_command(&workspace, 1, tabs[0], 3);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);

    let expected = engine.version();
    let mut inputs = TestInputStream::new(TEST_SOURCE);
    let mut frame = host.begin(&engine);
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: select_two,
            },
        )
        .expect("first command fits the semantic host-frame phase");
    inputs
        .append(
            &mut frame,
            EngineInput::WorkspaceCommand {
                expected,
                command: select_three,
            },
        )
        .expect("second command fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);

    assert_eq!(engine.version().revision().get(), 1);
    assert_eq!(transition.events().len(), 1);
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::CommandProcessed { changed: true, .. }
    ));
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::StaleNode { .. },
            ..
        }
    ));
    assert!(matches!(
        engine.workspace().node(tabs[0]),
        Some(Node::Tabs {
            selected: Some(selected),
            ..
        }) if *selected == ItemId::new(2)
    ));
}

#[test]
fn separately_submitted_inputs_use_distinct_explicit_boundaries() {
    let (workspace, _) = workspace_with_roots(&[(1, 1, &[1])]);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");
    let mut host = TestPresentationHost::new(&mut engine);
    let mut inputs = TestInputStream::new(TEST_SOURCE);

    let mut first_frame = host.begin(&engine);
    inputs
        .append(&mut first_frame, EngineInput::ValidateWorkspace)
        .expect("maintenance input fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut first_frame);
    let first_transition = host.finish(first_frame, &mut engine);
    assert_eq!(
        first_transition.reduced_inputs()[0].source_sequence(),
        SourceSequence::new(1)
    );

    let mut second_frame = host.begin(&engine);
    inputs
        .append(&mut second_frame, EngineInput::ValidateWorkspace)
        .expect("maintenance input fits the semantic host-frame phase");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut second_frame);
    let second_transition = host.finish(second_frame, &mut engine);
    assert_eq!(second_transition.reduced_inputs().len(), 1);
    assert_eq!(
        second_transition.reduced_inputs()[0].source_sequence(),
        SourceSequence::new(2)
    );
}

fn inputs_submit(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    input: EngineInput,
) -> Result<dockspace::transition::EngineTransition, dockspace::engine::EngineError> {
    let mut inputs = TestInputStream::resume(engine, TEST_SOURCE);
    inputs.submit(engine, host, input)
}
