use std::collections::BTreeSet;

use dockspace::graph::command::{
    DockFraction, DockTarget, Edge, MovePayload, SplitResize, WorkspaceCommand,
};
use dockspace::graph::transaction::WorkspaceTransaction;
use dockspace::graph::{Axis, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicySnapshot;

const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ITEM_COUNT: u64 = 12;
const INITIAL_OPEN_ITEMS: u64 = 8;
const STEPS: usize = 5_000;

#[derive(Debug, Clone, Copy)]
enum ModelOperation {
    Select,
    Reorder,
    CenterMove,
    EdgeMove,
    Open,
    Resize,
}

impl ModelOperation {
    const ALL: [Self; 6] = [
        Self::Select,
        Self::Reorder,
        Self::CenterMove,
        Self::EdgeMove,
        Self::Open,
        Self::Resize,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Select => 0,
            Self::Reorder => 1,
            Self::CenterMove => 2,
            Self::EdgeMove => 3,
            Self::Open => 4,
            Self::Resize => 5,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct OperationCoverage {
    attempted: usize,
    succeeded: usize,
    changed: usize,
}

#[derive(Debug, Clone)]
enum ModelDelta {
    Select {
        tabs: NodeId,
        item: ItemId,
    },
    Reorder {
        tabs: NodeId,
        item: ItemId,
        from: usize,
        to: usize,
        expected_items: Vec<ItemId>,
    },
    CenterMove {
        target_tabs: NodeId,
        target_root: RootId,
        item: ItemId,
    },
    EdgeMove {
        target_tabs: NodeId,
        target_root: RootId,
        item: ItemId,
        edge: Edge,
        fraction: DockFraction,
    },
    Open {
        target_tabs: NodeId,
        item: ItemId,
    },
    Resize {
        split: NodeId,
        weights: Vec<SplitWeight>,
    },
}

impl ModelDelta {
    const fn operation(&self) -> ModelOperation {
        match self {
            Self::Select { .. } => ModelOperation::Select,
            Self::Reorder { .. } => ModelOperation::Reorder,
            Self::CenterMove { .. } => ModelOperation::CenterMove,
            Self::EdgeMove { .. } => ModelOperation::EdgeMove,
            Self::Open { .. } => ModelOperation::Open,
            Self::Resize { .. } => ModelOperation::Resize,
        }
    }
}

impl OperationCoverage {
    fn record_attempt(&mut self) {
        self.attempted += 1;
    }

    fn record_success(&mut self, changed: bool) {
        self.succeeded += 1;
        if changed {
            self.changed += 1;
        }
    }
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
    let side = builder.insert_node(Node::tabs((3..=INITIAL_OPEN_ITEMS).map(ItemId::new)));
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
    (
        WorkspaceCommand::Select { source },
        ModelDelta::Select { tabs, item },
    )
}

fn reorder_command(
    workspace: &Workspace,
    rng: &mut DeterministicRng,
    tab_nodes: &[(NodeId, Vec<ItemId>)],
    items: &[(NodeId, ItemId)],
) -> (WorkspaceCommand, ModelDelta) {
    let (tabs, item) = items[rng.index(items.len())];
    let current_items = tab_nodes
        .iter()
        .find_map(|(candidate, items)| (*candidate == tabs).then_some(items.clone()))
        .expect("selected tabs must still exist");
    let insertion_index = rng.index(current_items.len() + 1);
    let from = current_items
        .iter()
        .position(|candidate| *candidate == item)
        .expect("selected item must still be in its tabs");
    let to = if insertion_index > from {
        insertion_index - 1
    } else {
        insertion_index
    };
    let mut expected_items = current_items;
    if from != to {
        expected_items.remove(from);
        expected_items.insert(to, item);
    }
    let source = workspace
        .capture_item_source(ROOT, tabs, item)
        .expect("fresh reorder source must be valid");
    (
        WorkspaceCommand::Reorder {
            source,
            insertion_index,
        },
        ModelDelta::Reorder {
            tabs,
            item,
            from,
            to,
            expected_items,
        },
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
        ModelDelta::CenterMove {
            target_tabs: *target_tabs,
            target_root: ROOT,
            item,
        },
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
    let edge = edges[rng.index(edges.len())];
    let fraction = DockFraction::new(fractions[rng.index(fractions.len())])
        .expect("model fraction must be valid");
    let source = workspace
        .capture_item_source(ROOT, source_tabs, item)
        .expect("fresh edge source must be valid");
    let target = workspace
        .capture_inner_edge_target(ROOT, *target_tabs, edge, fraction)
        .expect("fresh edge target must be valid");
    (
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target: DockTarget::InnerEdge(target),
        },
        ModelDelta::EdgeMove {
            target_tabs: *target_tabs,
            target_root: ROOT,
            item,
            edge,
            fraction,
        },
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
    let current_first = match workspace.node(split).expect("selected split must exist") {
        Node::Split { weights, .. } => weights[0].get(),
        Node::Tabs { .. } => unreachable!("selected node came from split_nodes"),
    };
    let first_weight = if current_first > 0.5 { 0.25 } else { 0.75 };
    let remaining_count = u16::try_from(child_count - 1)
        .expect("test split child count must fit a normalized f32 divisor");
    let remaining_weight = (1.0 - first_weight) / f32::from(remaining_count);
    let weights = SplitWeight::normalize(
        std::iter::once(first_weight).chain(std::iter::repeat_n(remaining_weight, child_count - 1)),
    )
    .expect("model resize weights must normalize");
    (
        WorkspaceCommand::ResizeSplits {
            splits: vec![SplitResize::new(source, weights.clone())],
        },
        ModelDelta::Resize { split, weights },
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
        ModelDelta::Open {
            target_tabs: *target_tabs,
            item,
        },
    )
}

fn item_tabs(workspace: &Workspace, item: ItemId) -> Option<NodeId> {
    workspace.nodes().find_map(|(node, value)| match value {
        Node::Tabs { items, .. } if items.contains(&item) => Some(node),
        Node::Tabs { .. } | Node::Split { .. } => None,
    })
}

fn direct_split_children(
    workspace: &Workspace,
    first: NodeId,
    second: NodeId,
) -> Option<(Axis, usize, usize, SplitWeight, SplitWeight)> {
    workspace.nodes().find_map(|(_, node)| {
        let Node::Split {
            axis,
            children,
            weights,
        } = node
        else {
            return None;
        };
        let first_index = children.iter().position(|child| *child == first)?;
        let second_index = children.iter().position(|child| *child == second)?;
        Some((
            *axis,
            first_index,
            second_index,
            *weights.get(first_index)?,
            *weights.get(second_index)?,
        ))
    })
}

fn axis_for_edge(edge: Edge) -> Axis {
    match edge {
        Edge::Left | Edge::Right => Axis::Horizontal,
        Edge::Top | Edge::Bottom => Axis::Vertical,
    }
}

fn assert_success_postcondition(
    delta: &ModelDelta,
    report: &dockspace::graph::transaction::TransactionReport,
    before: &Workspace,
    after: &Workspace,
) {
    assert_eq!(report.changed(), before != after);

    match delta {
        ModelDelta::Select { tabs, item } => {
            assert!(matches!(
                report.outcomes(),
                [dockspace::graph::command::CommandOutcome::Selected {
                    item: actual_item,
                    tabs: actual_tabs,
                    ..
                }] if actual_item == item && actual_tabs == tabs
            ));
            assert!(matches!(
                after.node(*tabs),
                Some(Node::Tabs {
                    selected: Some(selected),
                    ..
                }) if selected == item
            ));
            assert_eq!(after.tab_mru(*tabs).and_then(|mru| mru.first()), Some(item));
        }
        ModelDelta::Reorder {
            tabs,
            item,
            from,
            to,
            expected_items,
        } => {
            assert!(matches!(
                report.outcomes(),
                [dockspace::graph::command::CommandOutcome::Reordered {
                    item: actual_item,
                    tabs: actual_tabs,
                    from: actual_from,
                    to: actual_to,
                    changed,
                }] if actual_item == item
                    && actual_tabs == tabs
                    && actual_from == from
                    && actual_to == to
                    && *changed == (from != to)
            ));
            assert!(matches!(
                after.node(*tabs),
                Some(Node::Tabs { items, .. }) if items == expected_items
            ));
        }
        ModelDelta::CenterMove {
            target_tabs,
            target_root,
            item,
        } => {
            assert!(matches!(
                report.outcomes(),
                [dockspace::graph::command::CommandOutcome::Moved {
                    items,
                    target_root: actual_root,
                    ..
                }] if items.as_slice() == std::slice::from_ref(item)
                    && actual_root == target_root
            ));
            assert!(matches!(
                after.node(*target_tabs),
                Some(Node::Tabs { items, .. }) if items.contains(item)
            ));
            assert!(
                after
                    .capture_item_source(*target_root, *target_tabs, *item)
                    .is_ok()
            );
        }
        ModelDelta::EdgeMove {
            target_tabs,
            target_root,
            item,
            edge,
            fraction,
        } => {
            let [
                dockspace::graph::command::CommandOutcome::Moved {
                    items,
                    target_root: actual_root,
                    changed,
                    ..
                },
            ] = report.outcomes()
            else {
                panic!("edge move must publish one moved outcome");
            };
            assert_eq!(items.as_slice(), std::slice::from_ref(item));
            assert_eq!(*actual_root, *target_root);
            if *changed {
                let moved_tabs = item_tabs(after, *item).expect("moved item must remain open");
                let (axis, target_index, moved_index, target_weight, moved_weight) =
                    direct_split_children(after, *target_tabs, moved_tabs)
                        .expect("edge move must make target and payload direct split children");
                assert_eq!(axis, axis_for_edge(*edge));
                match edge {
                    Edge::Left | Edge::Top => {
                        assert_eq!(moved_index + 1, target_index, "{delta:?}");
                    }
                    Edge::Right | Edge::Bottom => {
                        assert_eq!(target_index + 1, moved_index, "{delta:?}");
                    }
                }
                let payload_weight = moved_weight.get();
                let target_weight = target_weight.get();
                let actual_fraction = payload_weight / (payload_weight + target_weight);
                assert!(
                    (actual_fraction - fraction.get()).abs() <= 1.0e-5,
                    "edge payload share {actual_fraction} must match requested fraction {}",
                    fraction.get()
                );
            } else {
                assert_eq!(after, before);
            }
            let moved_tabs = item_tabs(after, *item).expect("moved item must remain open");
            assert!(
                after
                    .capture_item_source(*target_root, moved_tabs, *item)
                    .is_ok()
            );
        }
        ModelDelta::Open { target_tabs, item } => {
            assert!(matches!(
                report.outcomes(),
                [dockspace::graph::command::CommandOutcome::Opened { item: actual, root }]
                    if actual == item && *root == ROOT
            ));
            assert!(matches!(
                after.node(*target_tabs),
                Some(Node::Tabs { items, .. }) if items.contains(item)
            ));
            assert_eq!(after.item_multiset().get(item), Some(&1));
        }
        ModelDelta::Resize { split, weights } => {
            assert!(matches!(
                report.outcomes(),
                [dockspace::graph::command::CommandOutcome::SplitsResized {
                    splits,
                    changed: true,
                }] if splits == &[*split]
            ));
            assert!(matches!(
                after.node(*split),
                Some(Node::Split { weights: actual, .. }) if actual == weights
            ));
        }
    }
}

#[test]
fn edge_move_model_covers_each_direction_and_fraction_with_a_state_change() {
    let policy = DockPolicySnapshot::default();
    let edges = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
    let fractions = [0.25_f32, 0.5, 0.75];

    for edge in edges {
        for value in fractions {
            let fraction = DockFraction::new(value).expect("target fraction must be valid");
            let mut workspace = initial_workspace();
            let before = workspace.clone();
            let target_tabs =
                item_tabs(&workspace, ItemId::new(1)).expect("initial target item must be open");
            let source_tabs =
                item_tabs(&workspace, ItemId::new(3)).expect("initial source item must be open");
            let source = workspace
                .capture_item_source(ROOT, source_tabs, ItemId::new(3))
                .expect("initial source item must be capturable");
            let target = workspace
                .capture_inner_edge_target(ROOT, target_tabs, edge, fraction)
                .expect("initial edge target must be capturable");
            let command = WorkspaceCommand::Move {
                payload: MovePayload::Item(source),
                target: DockTarget::InnerEdge(target),
            };
            let delta = ModelDelta::EdgeMove {
                target_tabs,
                target_root: ROOT,
                item: ItemId::new(3),
                edge,
                fraction,
            };
            let report = WorkspaceTransaction::from_commands([command])
                .apply(&mut workspace, &policy)
                .expect("targeted edge move must succeed");

            assert!(
                report.changed(),
                "{edge:?} at {value} must change workspace"
            );
            assert_success_postcondition(&delta, &report, &before, &workspace);
        }
    }
}

#[test]
fn seeded_command_model_preserves_invariants_for_thousands_of_steps() {
    let mut workspace = initial_workspace();
    let policy = DockPolicySnapshot::default();
    let mut expected: BTreeSet<ItemId> = (1..=INITIAL_OPEN_ITEMS).map(ItemId::new).collect();
    let mut rng = DeterministicRng(0xd0c5_9ace_5eed_f00d);
    let mut committed = 0_usize;
    let mut rejected = 0_usize;
    let mut coverage = [OperationCoverage::default(); ModelOperation::ALL.len()];

    for _ in 0..STEPS {
        let (command, delta) = command_for_step(&workspace, &expected, &mut rng);
        let operation = delta.operation();
        coverage[operation.index()].record_attempt();
        let before = workspace.clone();
        let result = WorkspaceTransaction::from_commands([command]).apply(&mut workspace, &policy);
        match result {
            Err(_) => {
                rejected += 1;
                assert_eq!(workspace, before);
            }
            Ok(report) => {
                committed += 1;
                assert_success_postcondition(&delta, &report, &before, &workspace);
                coverage[operation.index()].record_success(report.changed());
                if let ModelDelta::Open { item, .. } = delta {
                    assert!(expected.insert(item));
                }
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
    for operation in ModelOperation::ALL {
        let observed = coverage[operation.index()];
        assert!(observed.attempted > 0, "{operation:?} was never attempted");
        assert!(observed.succeeded > 0, "{operation:?} never succeeded");
        assert!(
            observed.changed > 0,
            "{operation:?} never changed workspace state"
        );
    }
}
