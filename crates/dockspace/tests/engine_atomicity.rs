use dockspace::command::{CommandOutcome, WorkspaceCommand};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::error::CommandError;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId, WorkspaceEpoch, WorkspaceRevision};
use dockspace::policy::DockPolicy;
use dockspace::transition::{InputOutcome, InputPriority, WorkspaceVersion};

fn workspace_with_roots(roots: &[(u64, u64, &[u64])]) -> (Workspace, Vec<dockspace::ids::NodeId>) {
    let mut builder = Workspace::builder();
    let mut tabs = Vec::with_capacity(roots.len());

    for &(root, surface, items) in roots {
        let tabs_node = builder.insert_node(Node::tabs(items.iter().copied().map(ItemId::new)));
        builder.set_root(RootId::new(root), RootRecord::new(tabs_node));
        builder.set_surface(
            SurfaceId::new(surface),
            SurfacePresentation::new(RootId::new(root)),
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
fn reduction_uses_source_priority_then_writer_sequence() {
    let (initial, tabs) = workspace_with_roots(&[(1, 1, &[1, 2])]);
    let mut engine = DockEngine::new(initial.clone(), DockPolicy::default())
        .expect("initial workspace must be valid");
    let old_version = engine.version();
    let command = select_command(&initial, 1, tabs[0], 2);

    let maintenance = engine
        .enqueue(EngineInput::ValidateWorkspace)
        .expect("sequence is available");
    let application = engine
        .enqueue(EngineInput::WorkspaceCommand {
            expected: old_version,
            command,
        })
        .expect("sequence is available");
    let lifecycle = engine
        .enqueue_workspace_replacement(initial.clone())
        .expect("sequence is available");

    let transition = engine.reduce_pending().expect("reduction must commit");
    let reduced = transition.reduced_inputs();
    assert_eq!(reduced.len(), 3);
    assert_eq!(reduced[0].sequence(), lifecycle);
    assert_eq!(reduced[0].priority(), InputPriority::LifecycleControl);
    assert_eq!(reduced[1].sequence(), application);
    assert_eq!(reduced[1].priority(), InputPriority::ApplicationCommand);
    assert_eq!(reduced[2].sequence(), maintenance);
    assert_eq!(reduced[2].priority(), InputPriority::Maintenance);
    assert!(matches!(
        reduced[1].outcome(),
        InputOutcome::StaleRejected {
            expected,
            accepted_base,
        } if *expected == old_version && *accepted_base == engine.version()
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
    let version_before = engine.version();
    let command = select_command(&workspace, 1, tabs[0], 2);

    let command_sequence = engine
        .enqueue_command(command)
        .expect("sequence is available");
    let replacement_sequence = engine
        .enqueue_workspace_replacement(workspace.clone())
        .expect("sequence is available");

    let transition = engine.reduce_pending().expect("replacement must commit");
    assert_eq!(
        transition.reduced_inputs()[0].sequence(),
        replacement_sequence
    );
    assert_eq!(transition.reduced_inputs()[1].sequence(), command_sequence);
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

    engine
        .enqueue_command(first)
        .expect("sequence is available");
    engine
        .enqueue_command(second)
        .expect("sequence is available");

    let transition = engine.reduce_pending().expect("both commands must commit");
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

    engine
        .enqueue_command(command)
        .expect("sequence is available");
    let transition = engine.reduce_pending().expect("no-op must be valid");

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
    engine
        .enqueue_command(WorkspaceCommand::Open {
            item: ItemId::new(1),
            target: dockspace::command::DockTarget::Center(target),
        })
        .expect("sequence is available");
    let before = engine.workspace().clone();
    let before_version = engine.version();

    let transition = engine
        .reduce_pending()
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
    assert!(engine.pending_inputs().is_empty());
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
    engine
        .enqueue_command(WorkspaceCommand::Open {
            item: ItemId::new(1),
            target: dockspace::command::DockTarget::Center(target),
        })
        .expect("sequence is available");
    engine
        .enqueue_command(select)
        .expect("sequence is available");

    let transition = engine
        .reduce_pending()
        .expect("expected rejection must not block the valid command");
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
    let expected = engine.version();
    let mut changed_policy = engine.policy().clone();
    changed_policy.set_allow_native_surfaces(true);

    engine
        .enqueue(EngineInput::ReplacePolicy {
            expected,
            policy: changed_policy,
        })
        .expect("sequence is available");
    engine
        .enqueue(EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Open {
                item: ItemId::new(1),
                target: dockspace::command::DockTarget::Center(target),
            },
        })
        .expect("sequence is available");
    let transition = engine
        .reduce_pending()
        .expect("policy change and expected rejection must publish");
    assert!(engine.policy().allows_native_surfaces());
    assert_eq!(engine.version().revision().get(), 1);
    assert_eq!(transition.events().len(), 1);
    assert!(matches!(
        transition.reduced_inputs()[1].outcome(),
        InputOutcome::CommandRejected {
            error: CommandError::ItemAlreadyOpen { item },
            ..
        } if *item == ItemId::new(1)
    ));
    assert!(engine.pending_inputs().is_empty());
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
    engine
        .enqueue_workspace_replacement(replacement)
        .expect("sequence is available");
    engine
        .enqueue(EngineInput::WorkspaceCommand {
            expected: WorkspaceVersion::new(WorkspaceEpoch::new(1), WorkspaceRevision::default()),
            command: WorkspaceCommand::Open {
                item: ItemId::new(2),
                target: dockspace::command::DockTarget::Center(target),
            },
        })
        .expect("sequence is available");
    let transition = engine
        .reduce_pending()
        .expect("replacement and expected rejection must publish");
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
    assert!(engine.pending_inputs().is_empty());
}

#[test]
fn same_root_commands_from_one_version_commit_then_reject_stale_source() {
    let (workspace, tabs) = workspace_with_roots(&[(1, 1, &[1, 2, 3])]);
    let select_two = select_command(&workspace, 1, tabs[0], 2);
    let select_three = select_command(&workspace, 1, tabs[0], 3);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");

    engine
        .enqueue_command(select_two)
        .expect("sequence is available");
    engine
        .enqueue_command(select_three)
        .expect("sequence is available");
    let transition = engine
        .reduce_pending()
        .expect("stale second command must not poison the boundary");

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
    assert!(engine.pending_inputs().is_empty());
}

#[test]
fn newly_enqueued_input_is_deferred_to_the_next_boundary() {
    let (workspace, _) = workspace_with_roots(&[(1, 1, &[1])]);
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("initial workspace must be valid");

    let first = engine
        .enqueue(EngineInput::ValidateWorkspace)
        .expect("sequence is available");
    let first_transition = engine.reduce_pending().expect("first boundary must commit");
    assert_eq!(first_transition.reduced_inputs()[0].sequence(), first);

    let second = engine
        .enqueue(EngineInput::ValidateWorkspace)
        .expect("sequence is available");
    assert!(
        engine
            .pending_inputs()
            .iter()
            .any(|input| input.sequence() == second)
    );
    let second_transition = engine
        .reduce_pending()
        .expect("second boundary must commit");
    assert_eq!(second_transition.reduced_inputs().len(), 1);
    assert_eq!(second_transition.reduced_inputs()[0].sequence(), second);
}
