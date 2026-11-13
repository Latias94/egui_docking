use dockspace::command::{CommandOutcome, ContainedPosition, WorkspaceCommand};
use dockspace::error::{CommandError, TransactionError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicySnapshot;
use dockspace::transaction::WorkspaceTransaction;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const ROOT_A: RootId = RootId::new(2);
const ROOT_B: RootId = RootId::new(3);
const FLOATING_A: FloatingPresentationId = FloatingPresentationId::new(2);
const FLOATING_B: FloatingPresentationId = FloatingPresentationId::new(3);

fn rect(x: f64) -> LogicalRect {
    LogicalRect::new(x, 20.0, 160.0, 120.0).expect("test rectangle must be valid")
}

fn fixture() -> (Workspace, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let node_a = builder.insert_node(Node::tabs([ItemId::new(2), ItemId::new(20)]));
    let node_b = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(ROOT_A, RootRecord::new(node_a));
    builder.set_root(ROOT_B, RootRecord::new(node_b));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(FLOATING_A, ContainedFloating::new(ROOT_A, rect(10.0)));
    builder.set_contained_floating(FLOATING_B, ContainedFloating::new(ROOT_B, rect(200.0)));
    builder
        .attach_contained(SURFACE, FLOATING_A)
        .expect("surface must exist");
    builder
        .attach_contained(SURFACE, FLOATING_B)
        .expect("surface must exist");
    (
        builder.build().expect("fixture must be valid"),
        node_a,
        node_b,
    )
}

fn update_command(
    workspace: &Workspace,
    node: NodeId,
    expected_rect: LogicalRect,
    rect: LogicalRect,
    position: ContainedPosition,
) -> WorkspaceCommand {
    WorkspaceCommand::UpdateContainedPresentation {
        source: workspace
            .capture_node_source(ROOT_A, node)
            .expect("source must be current"),
        floating: FLOATING_A,
        expected_rect,
        expected_roster: workspace
            .capture_contained_roster(SURFACE)
            .expect("roster must be current"),
        rect,
        position,
    }
}

fn command_error(error: TransactionError) -> CommandError {
    match error {
        TransactionError::Command { source, .. } => source,
        other => panic!("expected command rejection, got {other:?}"),
    }
}

#[test]
fn update_atomically_changes_rect_and_raises_to_front() {
    let (mut workspace, node_a, _) = fixture();
    let command = update_command(
        &workspace,
        node_a,
        rect(10.0),
        rect(40.0),
        ContainedPosition::Front,
    );

    let report = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("exact presentation update must apply");

    assert!(matches!(
        report.outcomes(),
        [CommandOutcome::ContainedPresentationUpdated {
            floating: FLOATING_A,
            from: 0,
            to: 1,
            rect_changed: true,
            order_changed: true,
        }]
    ));
    assert_eq!(
        workspace
            .contained_floating(FLOATING_A)
            .expect("floating must remain")
            .rect,
        rect(40.0)
    );
    assert_eq!(
        workspace
            .surface(SURFACE)
            .expect("surface must remain")
            .contained,
        [FLOATING_B, FLOATING_A]
    );
}

#[test]
fn self_anchors_are_typed_rejections_without_partial_rect_updates() {
    for position in [
        ContainedPosition::Before(FLOATING_A),
        ContainedPosition::After(FLOATING_A),
    ] {
        let (mut workspace, node_a, _) = fixture();
        let before = workspace.clone();
        let command = update_command(&workspace, node_a, rect(10.0), rect(40.0), position);

        let error = WorkspaceTransaction::from_commands([command])
            .apply(&mut workspace, &DockPolicySnapshot::default())
            .expect_err("self anchor must reject");

        assert_eq!(
            command_error(error),
            CommandError::ContainedAnchorIsSelf {
                floating: FLOATING_A,
            }
        );
        assert_eq!(workspace, before);
    }
}

#[test]
fn frontmost_same_rect_update_is_a_checked_noop() {
    let (mut workspace, node_a, _) = fixture();
    let source = workspace
        .capture_node_source(ROOT_A, node_a)
        .expect("source must be current");
    let roster = workspace
        .capture_contained_roster(SURFACE)
        .expect("roster must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source,
        floating: FLOATING_A,
        expected_roster: roster,
    }])
    .apply(&mut workspace, &DockPolicySnapshot::default())
    .expect("fixture raise must apply");
    let before = workspace.clone();
    let command = update_command(
        &workspace,
        node_a,
        rect(10.0),
        rect(10.0),
        ContainedPosition::Front,
    );

    let report = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("frontmost exact presentation must be accepted");

    assert!(!report.changed());
    assert!(matches!(
        report.outcomes(),
        [CommandOutcome::ContainedPresentationUpdated {
            floating: FLOATING_A,
            from: 1,
            to: 1,
            rect_changed: false,
            order_changed: false,
        }]
    ));
    assert_eq!(workspace, before);
}

#[test]
fn stale_node_source_rejects_without_touching_rect_or_roster() {
    let (mut workspace, node_a, _) = fixture();
    let command = update_command(
        &workspace,
        node_a,
        rect(10.0),
        rect(40.0),
        ContainedPosition::Front,
    );
    let item = workspace
        .capture_item_source(ROOT_A, node_a, ItemId::new(20))
        .expect("item source must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source: item }])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect("fixture selection must change the source fingerprint");
    let before = workspace.clone();

    let error = WorkspaceTransaction::from_commands([command])
        .apply(&mut workspace, &DockPolicySnapshot::default())
        .expect_err("stale source must reject");

    assert!(matches!(
        command_error(error),
        CommandError::StaleNode {
            role: dockspace::error::ReferenceRole::Source,
            node,
            ..
        } if node == node_a
    ));
    assert_eq!(workspace, before);
}

#[test]
fn stale_rect_or_roster_rejects_the_whole_presentation_update() {
    let (mut stale_rect_workspace, node_a, _) = fixture();
    let stale_rect_command = update_command(
        &stale_rect_workspace,
        node_a,
        rect(10.0),
        rect(40.0),
        ContainedPosition::Front,
    );
    WorkspaceTransaction::from_commands([WorkspaceCommand::UpdateContainedRect {
        surface: SURFACE,
        root: ROOT_A,
        floating: FLOATING_A,
        expected_rect: rect(10.0),
        rect: rect(15.0),
    }])
    .apply(&mut stale_rect_workspace, &DockPolicySnapshot::default())
    .expect("fixture rect change must apply");
    let after_rect_change = stale_rect_workspace.clone();
    let error = WorkspaceTransaction::from_commands([stale_rect_command])
        .apply(&mut stale_rect_workspace, &DockPolicySnapshot::default())
        .expect_err("stale rectangle must reject");
    assert!(matches!(
        command_error(error),
        CommandError::StaleContainedRect {
            floating: FLOATING_A,
            ..
        }
    ));
    assert_eq!(stale_rect_workspace, after_rect_change);

    let (mut stale_roster_workspace, node_a, node_b) = fixture();
    let stale_roster_command = update_command(
        &stale_roster_workspace,
        node_a,
        rect(10.0),
        rect(40.0),
        ContainedPosition::Front,
    );
    let source_b = stale_roster_workspace
        .capture_node_source(ROOT_B, node_b)
        .expect("peer source must be current");
    let roster = stale_roster_workspace
        .capture_contained_roster(SURFACE)
        .expect("peer roster must be current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source: source_b,
        floating: FLOATING_B,
        expected_roster: roster,
    }])
    .apply(&mut stale_roster_workspace, &DockPolicySnapshot::default())
    .expect("frontmost peer raise is a checked noop");
    let source_a = stale_roster_workspace
        .capture_node_source(ROOT_A, node_a)
        .expect("source must remain current");
    let roster = stale_roster_workspace
        .capture_contained_roster(SURFACE)
        .expect("roster must remain current");
    WorkspaceTransaction::from_commands([WorkspaceCommand::RaiseContained {
        source: source_a,
        floating: FLOATING_A,
        expected_roster: roster,
    }])
    .apply(&mut stale_roster_workspace, &DockPolicySnapshot::default())
    .expect("peer order change must apply");
    let after_roster_change = stale_roster_workspace.clone();
    let error = WorkspaceTransaction::from_commands([stale_roster_command])
        .apply(&mut stale_roster_workspace, &DockPolicySnapshot::default())
        .expect_err("stale roster must reject");
    assert!(matches!(
        command_error(error),
        CommandError::StaleContainedRoster {
            surface: SURFACE,
            ..
        }
    ));
    assert_eq!(stale_roster_workspace, after_roster_change);
}
