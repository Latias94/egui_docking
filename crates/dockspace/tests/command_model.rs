use std::collections::BTreeSet;

use dockspace::command::{
    DockFraction, DockTarget, Edge, MovePayload, SplitResize, WorkspaceCommand,
};
use dockspace::graph::{Axis, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicySnapshot;
use dockspace::transaction::WorkspaceTransaction;

const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ITEM_COUNT: u64 = 12;
const STEPS: usize = 5_000;

#[derive(Clone, Copy)]
enum ModelDelta {
    None,
    Open(ItemId),
}

struct DeterministicRng(u64);

impl DeterministicRng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn index(&mut self, len: usize) -> usize {
        let len = u64::try_from(len).expect("test collection length must fit u64");
        usize::try_from(self.next() % len).expect("bounded test index must fit usize")
    }
}

fn initial_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let central = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let side = builder.insert_node(Node::tabs((3..=ITEM_COUNT).map(ItemId::new)));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [central, side]).expect("valid split"));
    builder.set_root(ROOT, RootRecord::new(split).with_central(central));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("model workspace must be valid")
}

fn tabs(workspace: &Workspace) -> Vec<(NodeId, Vec<ItemId>)> {
    workspace
        .nodes()
        .filter_map(|(node, value)| match value {
            Node::Tabs { items, .. } => Some((node, items.clone())),
            Node::Split { .. } => None,
        })
        .collect()
}

fn splits(workspace: &Workspace) -> Vec<(NodeId, usize)> {
    workspace
        .nodes()
        .filter_map(|(node, value)| match value {
            Node::Split { children, .. } => Some((node, children.len())),
            Node::Tabs { .. } => None,
        })
        .collect()
}

fn open_items(tab_nodes: &[(NodeId, Vec<ItemId>)]) -> Vec<(NodeId, ItemId)> {
    tab_nodes
        .iter()
        .flat_map(|(tabs, items)| items.iter().map(|item| (*tabs, *item)))
        .collect()
}

fn command_for_step(
    workspace: &Workspace,
    open: &BTreeSet<ItemId>,
    rng: &mut DeterministicRng,
) -> (WorkspaceCommand, ModelDelta) {
    let tab_nodes = tabs(workspace);
    let items = open_items(&tab_nodes);
    let operation = rng.index(6);

    if items.is_empty() {
        return open_command(workspace, open, rng, &tab_nodes);
    }

    match operation {
        0 => select_command(workspace, rng, &items),
        1 => reorder_command(workspace, rng, &tab_nodes, &items),
        2 => center_move_command(workspace, rng, &tab_nodes, &items),
        3 => edge_move_command(workspace, rng, &tab_nodes, &items),
        4 if open.len() < usize::try_from(ITEM_COUNT).expect("item count fits usize") => {
            open_command(workspace, open, rng, &tab_nodes)
        }
        4 => select_command(workspace, rng, &items),
        _ => resize_or_toggle_command(workspace, open, rng, &tab_nodes, &items),
    }
}

fn select_command(
    workspace: &Workspace,
    rng: &mut DeterministicRng,
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let (tabs, item) = items[rng.index(items.len())];
    let source = workspace
        .capture_item_source(ROOT, tabs, item)
        .expect("fresh selection source must be valid");
    (WorkspaceCommand::Select { source }, ModelDelta::None)
}

fn reorder_command(
    workspace: &Workspace,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let (tabs, item) = items[rng.index(items.len())];
    let len = tab_nodes
        .iter()
        .find_map(|(candidate, items)| (*candidate == tabs).then_some(items.len()))
        .expect("selected tabs must still exist");
    let source = workspace
        .capture_item_source(ROOT, tabs, item)
        .expect("fresh reorder source must be valid");
    (
        WorkspaceCommand::Reorder {
            source,
            insertion_index: rng.index(len + 1),
        },
        ModelDelta::None,
    )
}

fn center_move_command(
    workspace: &Workspace,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let (source_tabs, item) = items[rng.index(items.len())];
    let nonempty_targets: Vec<_> = tab_nodes
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .collect();
    let (target_tabs, _) = nonempty_targets[rng.index(nonempty_targets.len())];
    let source = workspace
        .capture_item_source(ROOT, source_tabs, item)
        .expect("fresh move source must be valid");
    let target = workspace
        .capture_tab_target(ROOT, *target_tabs)
        .expect("fresh center target must be valid");
    (
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::Center(target),
        },
        ModelDelta::None,
    )
}

fn edge_move_command(
    workspace: &Workspace,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let (source_tabs, item) = items[rng.index(items.len())];
    let nonempty_targets: Vec<_> = tab_nodes
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .collect();
    let (target_tabs, _) = nonempty_targets[rng.index(nonempty_targets.len())];
    let edges = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
    let fractions = [0.25_f32, 0.5, 0.75];
    let source = workspace
        .capture_item_source(ROOT, source_tabs, item)
        .expect("fresh edge source must be valid");
    let target = workspace
        .capture_inner_edge_target(
            ROOT,
            *target_tabs,
            edges[rng.index(edges.len())],
            DockFraction::new(fractions[rng.index(fractions.len())])
                .expect("model fraction must be valid"),
        )
        .expect("fresh edge target must be valid");
    (
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::InnerEdge(target),
        },
        ModelDelta::None,
    )
}

fn resize_or_toggle_command(
    workspace: &Workspace,
    open: &BTreeSet<ItemId>,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let split_nodes = splits(workspace);
    if split_nodes.is_empty() {
        return open_or_select_command(workspace, open, rng, tab_nodes, items);
    }
    let (split, child_count) = split_nodes[rng.index(split_nodes.len())];
    let source = workspace
        .capture_node_source(ROOT, split)
        .expect("fresh split source must be valid");
    let weights = SplitWeight::normalize(std::iter::repeat_n(1.0, child_count))
        .expect("equal model weights must normalize");
    (
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(source, weights)],
        },
        ModelDelta::None,
    )
}

fn open_or_select_command(
    workspace: &Workspace,
    open: &BTreeSet<ItemId>,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    if open.len() < usize::try_from(ITEM_COUNT).expect("item count fits usize") {
        open_command(workspace, open, rng, tab_nodes)
    } else {
        select_command(workspace, rng, items)
    }
}

fn open_command(
    workspace: &Workspace,
    open: &BTreeSet<ItemId>,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
) -> (WorkspaceCommand, ModelDelta) {
    let closed: Vec<ItemId> = (1..=ITEM_COUNT)
        .map(ItemId::new)
        .filter(|item| !open.contains(item))
        .collect();
    let item = closed[rng.index(closed.len())];
    let nonempty_targets: Vec<_> = tab_nodes
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .collect();
    let (target_tabs, _) = nonempty_targets[rng.index(nonempty_targets.len())];
    let target = workspace
        .capture_tab_target(ROOT, *target_tabs)
        .expect("fresh open target must be valid");
    (
        WorkspaceCommand::Open {
            item,
            target: DockTarget::Center(target),
        },
        ModelDelta::Open(item),
    )
}

#[test]
fn seeded_command_model_preserves_invariants_for_thousands_of_steps() {
    let mut workspace = initial_workspace();
    let policy = DockPolicySnapshot::default();
    let mut expected: BTreeSet<ItemId> = (1..=ITEM_COUNT).map(ItemId::new).collect();
    let mut rng = DeterministicRng(0xd0c5_9ace_5eed_f00d);
    let mut committed = 0_usize;
    let mut rejected = 0_usize;

    for _ in 0..STEPS {
        let (command, delta) = command_for_step(&workspace, &expected, &mut rng);
        let before = workspace.clone();
        let result = WorkspaceTransaction::from_commands([command]).apply(&mut workspace, &policy);
        if result.is_err() {
            rejected += 1;
            assert_eq!(workspace, before);
        } else {
            committed += 1;
            match delta {
                ModelDelta::None => {}
                ModelDelta::Open(item) => assert!(expected.insert(item)),
            }
        }

        workspace
            .validate()
            .expect("every published model step must remain strict");
        assert_eq!(
            workspace.item_multiset(),
            expected.iter().copied().map(|item| (item, 1)).collect()
        );
    }

    assert_eq!(committed + rejected, STEPS);
    assert!(committed > STEPS * 9 / 10);
}
