//! Opt-in checked docking and layout attribution without a host or renderer.

use std::hint::black_box;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

use crate::geometry::{Constraints, LogicalRect, LogicalSize};
use crate::graph::command::{
    CommandOutcome, DockFraction, DockTarget, Edge, MovePayload, WorkspaceCommand,
};
use crate::graph::transaction::{TransactionReport, WorkspaceTransaction};
use crate::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder};
use crate::ids::{ItemId, NodeId, RootId, SurfaceId};
use crate::policy::DockPolicySnapshot;
use crate::presentation::layout::{LayoutMetrics, LayoutProjection, project_root};

const WIDTHS: [u64; 2] = [16_380, 16_444];
const HEIGHT: u64 = 16_380;
const SEPARATOR: u64 = 4;
const SOURCE_ROOT: RootId = RootId::new(1);
const MOVED_ITEM: ItemId = ItemId::new(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Resize,
    CenterMove,
    EdgeSplit,
    CrossViewMove,
}

impl Case {
    const ALL: [Self; 4] = [
        Self::Resize,
        Self::CenterMove,
        Self::EdgeSplit,
        Self::CrossViewMove,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Resize => "resize",
            Self::CenterMove => "center_move",
            Self::EdgeSplit => "edge_split",
            Self::CrossViewMove => "cross_view_move",
        }
    }

    fn target_root(self, views: usize) -> RootId {
        RootId::new(if self == Self::CrossViewMove {
            u64::try_from(views).unwrap()
        } else {
            1
        })
    }
}

struct Fixture {
    workspace: Workspace,
    bounds: LogicalRect,
    policy: DockPolicySnapshot,
    metrics: LayoutMetrics,
}

fn opposite(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

fn tree(builder: &mut WorkspaceBuilder, first: usize, count: usize, axis: Axis) -> NodeId {
    if count == 1 {
        let first = u64::try_from(first).unwrap() * 2 + 1;
        return builder.insert_node(Node::tabs([ItemId::new(first), ItemId::new(first + 1)]));
    }
    let half = count / 2;
    let left = tree(builder, first, half, opposite(axis));
    let right = tree(builder, first + half, half, opposite(axis));
    builder.insert_node(Node::equal_split(axis, [left, right]).unwrap())
}

fn bounds(width: u64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, f64::from(u32::try_from(width).unwrap()), 16_380.0).unwrap()
}

fn fixture(panes: usize, views: usize, width: u64) -> Fixture {
    let mut builder = Workspace::builder();
    let per_view = panes / views;
    for view in 0..views {
        let ordinal = u64::try_from(view + 1).unwrap();
        let node = tree(&mut builder, view * per_view, per_view, Axis::Horizontal);
        builder.set_root(RootId::new(ordinal), RootRecord::new(node));
        builder.set_surface(
            SurfaceId::new(ordinal),
            SurfacePresentation::with_main(RootId::new(ordinal)),
        );
    }
    let fixture = Fixture {
        workspace: builder.build().unwrap(),
        bounds: bounds(width),
        policy: DockPolicySnapshot::default(),
        metrics: LayoutMetrics::new(4.0).unwrap(),
    };
    let initial = project_all_roots(&fixture);
    assert_eq!(
        export_state(&fixture.workspace, &initial, panes),
        expected_state(panes, views, width, Case::Resize)
    );
    fixture
}

fn extreme_leaf(workspace: &Workspace, root: RootId, last: bool) -> (NodeId, Axis) {
    let mut node = workspace.root(root).unwrap().node;
    let mut parent_axis = None;
    loop {
        match workspace.node(node).unwrap() {
            Node::Tabs { .. } => return (node, parent_axis.expect("fixture has multiple panes")),
            Node::Split { axis, children, .. } => {
                parent_axis = Some(*axis);
                node = *if last {
                    children.last().unwrap()
                } else {
                    children.first().unwrap()
                };
            }
        }
    }
}

enum Request {
    Resize(LogicalRect),
    Command(WorkspaceTransaction),
}

fn capture_request(fixture: &Fixture, case: Case, views: usize, width: u64) -> Request {
    if case == Case::Resize {
        return Request::Resize(bounds(width));
    }
    let workspace = &fixture.workspace;
    let (source_tabs, _) = extreme_leaf(workspace, SOURCE_ROOT, false);
    let target_root = case.target_root(views);
    let (target_tabs, parent_axis) = extreme_leaf(workspace, target_root, true);
    let source = workspace
        .capture_item_source(SOURCE_ROOT, source_tabs, MOVED_ITEM)
        .unwrap();
    let target = if case == Case::EdgeSplit {
        let edge = match parent_axis {
            Axis::Vertical => Edge::Left,
            Axis::Horizontal => Edge::Top,
        };
        DockTarget::InnerEdge(
            workspace
                .capture_inner_edge_target(
                    target_root,
                    target_tabs,
                    edge,
                    DockFraction::new(0.5).unwrap(),
                )
                .unwrap(),
        )
    } else {
        DockTarget::Center(
            workspace
                .capture_tab_target(target_root, target_tabs)
                .unwrap(),
        )
    };
    Request::Command(WorkspaceTransaction::from_commands([
        WorkspaceCommand::Move {
            payload: MovePayload::Item(source),
            target,
        },
    ]))
}

fn apply_request(fixture: &mut Fixture, request: Request) -> Option<TransactionReport> {
    match request {
        Request::Resize(bounds) => {
            fixture.bounds = bounds;
            None
        }
        Request::Command(transaction) => Some(
            transaction
                .apply(&mut fixture.workspace, &fixture.policy)
                .unwrap(),
        ),
    }
}

struct RootProjection {
    ordinal: u64,
    root: RootId,
    layout: LayoutProjection,
}

fn collect_constraints(
    workspace: &Workspace,
    node: NodeId,
    leaf: Constraints,
    constraints: &mut Vec<(NodeId, Constraints)>,
) {
    match workspace.node(node).unwrap() {
        Node::Tabs { .. } => {
            constraints.push((node, leaf));
        }
        Node::Split { children, .. } => {
            for &child in children {
                collect_constraints(workspace, child, leaf, constraints);
            }
        }
    }
}

fn project_all_roots(fixture: &Fixture) -> Vec<RootProjection> {
    let leaf = Constraints::new(
        LogicalSize::new(0.0, 0.0).unwrap(),
        LogicalSize::new(65_536.0, 65_536.0).unwrap(),
    )
    .unwrap();
    fixture
        .workspace
        .surfaces()
        .enumerate()
        .map(|(view, (_, presentation))| {
            assert!(
                presentation.contained.is_empty(),
                "primary matrix has no floating roots"
            );
            let root = presentation.main_root.unwrap();
            let mut constraints = Vec::new();
            collect_constraints(
                &fixture.workspace,
                fixture.workspace.root(root).unwrap().node,
                leaf,
                &mut constraints,
            );
            let mut constraints = constraints.into_iter().peekable();
            RootProjection {
                ordinal: u64::try_from(view + 1).unwrap(),
                root,
                layout: project_root(
                    &fixture.workspace,
                    root,
                    fixture.bounds,
                    |node| {
                        constraints
                            .next_if(|(id, _)| *id == node)
                            .map(|(_, constraints)| constraints)
                    },
                    fixture.metrics,
                )
                .unwrap(),
            }
        })
        .collect()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn integral_word(value: f64) -> u64 {
    assert!(value.is_finite() && (0.0..=65_536.0).contains(&value) && value.fract() == 0.0);
    value as u64
}

fn append_header(
    words: &mut Vec<u64>,
    identity: [u64; 3],
    rect: [u64; 4],
    item_count: usize,
    selected: u64,
) {
    words.extend(identity);
    words.extend(rect);
    words.extend([u64::try_from(item_count).unwrap(), selected]);
}

fn append_node<'layout>(
    workspace: &Workspace,
    projection: &RootProjection,
    nodes: &mut impl Iterator<Item = (&'layout NodeId, &'layout LogicalRect)>,
    node: NodeId,
    path: u64,
    words: &mut Vec<u64>,
) {
    let (&projected_node, &rect) = nodes
        .next()
        .expect("projection must contain every DFS node");
    assert_eq!(
        projected_node, node,
        "projection must match the actual DFS tree"
    );
    let record = workspace.node(node).unwrap();
    let (kind, items, selected) = match record {
        Node::Tabs {
            items, selected, ..
        } => (0, items.as_slice(), selected.map_or(0, ItemId::get)),
        Node::Split { axis, children, .. } => {
            assert_eq!(children.len(), 2, "primary matrix requires binary topology");
            (if *axis == Axis::Horizontal { 1 } else { 2 }, &[][..], 0)
        }
    };
    append_header(
        words,
        [projection.ordinal, path, kind],
        [
            integral_word(rect.x()),
            integral_word(rect.y()),
            integral_word(rect.width()),
            integral_word(rect.height()),
        ],
        items.len(),
        selected,
    );
    words.extend(items.iter().map(|item| item.get()));
    if let Node::Split { children, .. } = record {
        for (child, &node) in children.iter().enumerate() {
            append_node(
                workspace,
                projection,
                nodes,
                node,
                path.checked_mul(2).unwrap() + u64::try_from(child).unwrap(),
                words,
            );
        }
    }
}

fn export_state(workspace: &Workspace, projections: &[RootProjection], panes: usize) -> Vec<u64> {
    let mut words = Vec::with_capacity(panes * 32 + projections.len() * 16);
    for projection in projections {
        let mut nodes = projection
            .layout
            .nodes
            .iter_preorder()
            .map(|(id, result)| (id, &result.rect));
        append_node(
            workspace,
            projection,
            &mut nodes,
            workspace.root(projection.root).unwrap().node,
            1,
            &mut words,
        );
        assert!(
            nodes.next().is_none(),
            "projection must contain only this root"
        );
    }
    words
}

#[test]
fn preorder_export_matches_the_keyed_export_for_all_primary_commands() {
    fn append_keyed(
        workspace: &Workspace,
        projection: &RootProjection,
        node: NodeId,
        path: u64,
        words: &mut Vec<u64>,
    ) {
        let rect = projection.layout.nodes[&node].rect;
        let (kind, items, selected) = match workspace.node(node).unwrap() {
            Node::Tabs {
                items, selected, ..
            } => (0, items.as_slice(), selected.map_or(0, ItemId::get)),
            Node::Split { axis, children, .. } => {
                assert_eq!(children.len(), 2);
                (if *axis == Axis::Horizontal { 1 } else { 2 }, &[][..], 0)
            }
        };
        append_header(
            words,
            [projection.ordinal, path, kind],
            [
                integral_word(rect.x()),
                integral_word(rect.y()),
                integral_word(rect.width()),
                integral_word(rect.height()),
            ],
            items.len(),
            selected,
        );
        words.extend(items.iter().map(|item| item.get()));
        if let Node::Split { children, .. } = workspace.node(node).unwrap() {
            for (child, &node) in children.iter().enumerate() {
                append_keyed(
                    workspace,
                    projection,
                    node,
                    path.checked_mul(2).unwrap() + u64::try_from(child).unwrap(),
                    words,
                );
            }
        }
    }

    for views in [1, 4] {
        for case in Case::ALL {
            if case == Case::CrossViewMove && views == 1 {
                continue;
            }
            let mut fixture = fixture(16, views, WIDTHS[0]);
            let request = capture_request(&fixture, case, views, WIDTHS[1]);
            let _ = apply_request(&mut fixture, request);
            let projections = project_all_roots(&fixture);
            let mut expected = Vec::new();
            for projection in &projections {
                append_keyed(
                    &fixture.workspace,
                    projection,
                    fixture.workspace.root(projection.root).unwrap().node,
                    1,
                    &mut expected,
                );
            }
            assert_eq!(export_state(&fixture.workspace, &projections, 16), expected);
            assert_eq!(
                expected,
                expected_state(16, views, integral_word(fixture.bounds.width()), case)
            );
        }
    }
}

struct PhaseTimes {
    request: u128,
    mutation: u128,
    layout: u128,
    export: u128,
    operation_owned: u128,
}

struct Measured {
    times: PhaseTimes,
    report: Option<TransactionReport>,
    projections: Vec<RootProjection>,
    words: Vec<u64>,
}

fn measure(fixture: &mut Fixture, case: Case, panes: usize, views: usize, width: u64) -> Measured {
    let start = Instant::now();
    let request = capture_request(black_box(&*fixture), black_box(case), views, width);
    let captured = Instant::now();
    let report = apply_request(black_box(&mut *fixture), black_box(request));
    let mutated = Instant::now();
    let projections = project_all_roots(black_box(&*fixture));
    let projected = Instant::now();
    let words = export_state(
        black_box(&fixture.workspace),
        black_box(&projections),
        panes,
    );
    black_box(&words);
    black_box(&report);
    let exported = Instant::now();
    Measured {
        times: PhaseTimes {
            request: captured.duration_since(start).as_nanos(),
            mutation: mutated.duration_since(captured).as_nanos(),
            layout: projected.duration_since(mutated).as_nanos(),
            export: exported.duration_since(projected).as_nanos(),
            operation_owned: exported.duration_since(start).as_nanos(),
        },
        report,
        projections,
        words,
    }
}

fn split_rect(rect: [u64; 4], axis: Axis) -> ([u64; 4], [u64; 4]) {
    let [x, y, width, height] = rect;
    match axis {
        Axis::Horizontal => {
            assert_eq!((width - SEPARATOR) % 2, 0);
            let half = (width - SEPARATOR) / 2;
            (
                [x, y, half, height],
                [x + half + SEPARATOR, y, half, height],
            )
        }
        Axis::Vertical => {
            assert_eq!((height - SEPARATOR) % 2, 0);
            let half = (height - SEPARATOR) / 2;
            ([x, y, width, half], [x, y + half + SEPARATOR, width, half])
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn expected_node(
    words: &mut Vec<u64>,
    view: usize,
    path: u64,
    first: usize,
    count: usize,
    axis: Axis,
    rect: [u64; 4],
    panes: usize,
    views: usize,
    case: Case,
) {
    let ordinal = u64::try_from(view + 1).unwrap();
    let split_kind = if axis == Axis::Horizontal { 1 } else { 2 };
    if count > 1 {
        append_header(words, [ordinal, path, split_kind], rect, 0, 0);
        let (left, right) = split_rect(rect, axis);
        let half = count / 2;
        expected_node(
            words,
            view,
            path * 2,
            first,
            half,
            opposite(axis),
            left,
            panes,
            views,
            case,
        );
        expected_node(
            words,
            view,
            path * 2 + 1,
            first + half,
            half,
            opposite(axis),
            right,
            panes,
            views,
            case,
        );
        return;
    }
    let per_view = panes / views;
    let target_view = if case == Case::CrossViewMove {
        views - 1
    } else {
        0
    };
    let target = target_view * per_view + per_view - 1;
    let first_item = u64::try_from(first).unwrap() * 2 + 1;
    if case == Case::EdgeSplit && first == target {
        append_header(words, [ordinal, path, split_kind], rect, 0, 0);
        let (inserted, original) = split_rect(rect, axis);
        append_header(words, [ordinal, path * 2, 0], inserted, 1, 2);
        words.push(2);
        append_header(words, [ordinal, path * 2 + 1, 0], original, 2, first_item);
        words.extend([first_item, first_item + 1]);
    } else if case != Case::Resize && first == 0 {
        append_header(words, [ordinal, path, 0], rect, 1, 1);
        words.push(1);
    } else if matches!(case, Case::CenterMove | Case::CrossViewMove) && first == target {
        append_header(words, [ordinal, path, 0], rect, 3, 2);
        words.extend([first_item, first_item + 1, 2]);
    } else {
        append_header(words, [ordinal, path, 0], rect, 2, first_item);
        words.extend([first_item, first_item + 1]);
    }
}

fn expected_state(panes: usize, views: usize, width: u64, case: Case) -> Vec<u64> {
    let mut words = Vec::new();
    let per_view = panes / views;
    for view in 0..views {
        expected_node(
            &mut words,
            view,
            1,
            view * per_view,
            per_view,
            Axis::Horizontal,
            [0, 0, width, HEIGHT],
            panes,
            views,
            case,
        );
    }
    words
}

struct Counts {
    nodes: usize,
    leaves: usize,
    splits: usize,
    items: usize,
    roots: usize,
}

fn validate(
    fixture: &Fixture,
    measured: &Measured,
    panes: usize,
    views: usize,
    case: Case,
) -> Counts {
    fixture.workspace.validate().unwrap();
    let mut counts = Counts {
        nodes: 0,
        leaves: 0,
        splits: 0,
        items: 0,
        roots: fixture.workspace.roots().count(),
    };
    for (node, record) in fixture.workspace.nodes() {
        counts.nodes += 1;
        match record {
            Node::Tabs {
                items, selected, ..
            } => {
                counts.leaves += 1;
                counts.items += items.len();
                let selected = selected.unwrap();
                let expected_mru: Vec<_> = std::iter::once(selected)
                    .chain(items.iter().copied().filter(|item| *item != selected))
                    .collect();
                assert_eq!(fixture.workspace.tab_mru(node).unwrap(), expected_mru);
            }
            Node::Split {
                children, weights, ..
            } => {
                counts.splits += 1;
                assert_eq!(children.len(), 2);
                assert!(weights.iter().all(|weight| weight.get() == 0.5));
            }
        }
    }
    let edge = usize::from(case == Case::EdgeSplit);
    assert_eq!(counts.nodes, 2 * panes - views + 2 * edge);
    assert_eq!(counts.leaves, panes + edge);
    assert_eq!(counts.splits, panes - views + edge);
    assert_eq!(counts.items, 2 * panes);
    assert_eq!(counts.roots, views);
    assert_eq!(fixture.workspace.surfaces().count(), views);
    assert_eq!(fixture.workspace.contained_floatings().count(), 0);
    let roster = fixture.workspace.item_multiset();
    assert_eq!(roster.len(), 2 * panes);
    for item in 1..=u64::try_from(2 * panes).unwrap() {
        assert_eq!(roster.get(&ItemId::new(item)), Some(&1));
    }
    assert_eq!(measured.projections.len(), views);
    assert_eq!(
        measured
            .projections
            .iter()
            .map(|row| row.layout.nodes.len())
            .sum::<usize>(),
        counts.nodes
    );
    assert_eq!(
        measured
            .projections
            .iter()
            .map(|row| row.layout.splits.len())
            .sum::<usize>(),
        counts.splits
    );
    for projection in &measured.projections {
        assert_eq!(projection.root.get(), projection.ordinal);
        assert_eq!(
            fixture
                .workspace
                .surface(SurfaceId::new(projection.ordinal))
                .unwrap()
                .main_root,
            Some(projection.root)
        );
        let root = fixture.workspace.root(projection.root).unwrap();
        assert_eq!(root.central, None);
        assert_eq!(projection.layout.nodes[&root.node].rect, fixture.bounds);
        for split in projection.layout.splits.values() {
            assert_eq!(split.overflow, 0.0);
            assert_eq!(split.unallocated, 0.0);
        }
    }
    if case == Case::Resize {
        assert!(measured.report.is_none());
    } else {
        let report = measured.report.as_ref().unwrap();
        assert!(report.changed());
        match report.outcomes() {
            [
                CommandOutcome::Moved {
                    items,
                    source_root,
                    target_root,
                    changed,
                },
            ] => {
                assert_eq!(items.as_slice(), &[MOVED_ITEM]);
                assert_eq!(*source_root, SOURCE_ROOT);
                assert_eq!(*target_root, case.target_root(views));
                assert!(*changed);
            }
            other => panic!("unexpected command outcome: {other:?}"),
        }
    }
    counts
}

fn digest(words: &[u64]) -> u64 {
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for word in words {
        for byte in word.to_le_bytes() {
            digest = (digest ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
        }
    }
    digest
}

fn text_setting(name: &str, default: &str) -> String {
    std::env::var_os(name).map_or_else(
        || default.to_owned(),
        |value| {
            value
                .into_string()
                .expect("benchmark setting must be Unicode")
        },
    )
}

fn setting(name: &str, default: usize) -> usize {
    let value = text_setting(name, &default.to_string())
        .parse()
        .expect("invalid benchmark setting");
    assert!(value <= 10_000, "benchmark iteration setting exceeds 10000");
    value
}

fn choices(name: &str, default: &str, allowed: &[usize]) -> Vec<usize> {
    let mut values = Vec::new();
    for value in text_setting(name, default).split(',') {
        let value = value.parse().expect("invalid benchmark size setting");
        assert!(allowed.contains(&value), "unsupported benchmark size");
        assert!(!values.contains(&value), "duplicate benchmark size");
        values.push(value);
    }
    assert!(!values.is_empty());
    values
}

fn case_settings(views: &[usize]) -> Vec<Case> {
    let selected = text_setting("DOCKSPACE_DOCK_COST_CASE", "");
    assert!(
        selected.is_empty() || Case::ALL.iter().any(|case| case.name() == selected),
        "unsupported docking case"
    );
    assert!(selected != Case::CrossViewMove.name() || views.contains(&4));
    Case::ALL
        .into_iter()
        .filter(|case| selected.is_empty() || case.name() == selected)
        .collect()
}

#[test]
#[ignore = "opt-in renderer-free docking comparison; root serializes execution"]
fn docking_cost() {
    assert!(
        option_env!("DOCKSPACE_BENCH_NO_WORK_COUNTERS").is_some(),
        "compile timing probe with DOCKSPACE_BENCH_NO_WORK_COUNTERS=1"
    );
    let samples = setting("DOCKSPACE_DOCK_COST_SAMPLES", 48);
    let warmup = setting("DOCKSPACE_DOCK_COST_WARMUP", 8);
    assert!(samples > 0);
    let panes = choices("DOCKSPACE_DOCK_COST_PANES", "16,128,1024", &[16, 128, 1024]);
    let views = choices("DOCKSPACE_DOCK_COST_VIEWS", "1,4", &[1, 4]);
    let cases = case_settings(&views);
    let state_dir = std::env::var_os("DOCKSPACE_DOCK_COST_STATE_DIR").map(PathBuf::from);
    if let Some(directory) = &state_dir {
        std::fs::create_dir_all(directory).unwrap();
    }
    let output =
        std::env::var_os("DOCKSPACE_DOCK_COST_OUT").expect("runner supplies the Rust CSV path");
    let mut output = BufWriter::new(std::fs::File::create_new(output).unwrap());
    writeln!(output, "product,panes,views,case,sample,request_ns,mutation_ns,layout_ns,export_ns,operation_owned_ns,nodes,leaves,splits,items,roots,width,height,state_digest").unwrap();
    for panes in panes {
        for &views in &views {
            for &case in &cases {
                if case == Case::CrossViewMove && views != 4 {
                    continue;
                }
                for iteration in 0..warmup.checked_add(samples).unwrap() {
                    let variant = iteration % 2;
                    let width = if case == Case::Resize {
                        WIDTHS[variant]
                    } else {
                        WIDTHS[0]
                    };
                    let initial_width = if case == Case::Resize {
                        WIDTHS[1 - variant]
                    } else {
                        WIDTHS[0]
                    };
                    let mut fixture = fixture(panes, views, initial_width);
                    let measured = measure(&mut fixture, case, panes, views, width);
                    let counts = validate(&fixture, &measured, panes, views, case);
                    assert_eq!(fixture.bounds, bounds(width));
                    // Projections and the common output are owned and outlive the workspace.
                    drop(fixture);
                    assert_eq!(measured.words, expected_state(panes, views, width, case));
                    let digest = digest(&measured.words);
                    if iteration < warmup {
                        continue;
                    }
                    let sample = iteration - warmup;
                    if sample == 0
                        && let Some(directory) = &state_dir
                    {
                        let dump = directory
                            .join(format!("dockspace-{panes}-{views}-{}.txt", case.name()));
                        let mut dump = BufWriter::new(std::fs::File::create_new(dump).unwrap());
                        for word in &measured.words {
                            writeln!(dump, "{word}").unwrap();
                        }
                        dump.flush().unwrap();
                    }
                    let times = &measured.times;
                    writeln!(output, "dockspace,{panes},{views},{},{sample},{},{},{},{},{},{},{},{},{},{},{width},{HEIGHT},{digest:016x}", case.name(), times.request, times.mutation, times.layout, times.export, times.operation_owned, counts.nodes, counts.leaves, counts.splits, counts.items, counts.roots).unwrap();
                    // Rich projections, reports, and common output are destroyed after timing.
                    drop(measured);
                }
            }
        }
    }
    output.flush().unwrap();
}

#[test]
#[ignore = "separate counter attribution; never use this build for timing"]
fn docking_work() {
    use crate::interaction::targeting::resolver::structural_work;

    assert!(
        option_env!("DOCKSPACE_BENCH_NO_WORK_COUNTERS").is_none(),
        "compile counter probe without DOCKSPACE_BENCH_NO_WORK_COUNTERS"
    );
    let panes = choices("DOCKSPACE_DOCK_COST_PANES", "16,128,1024", &[16, 128, 1024]);
    let views = choices("DOCKSPACE_DOCK_COST_VIEWS", "1,4", &[1, 4]);
    let cases = case_settings(&views);
    let output =
        std::env::var_os("DOCKSPACE_DOCK_COST_OUT").expect("runner supplies the counter CSV path");
    let mut output = BufWriter::new(std::fs::File::create_new(output).unwrap());
    // Existing cloned_* columns are logical inventory. Appended columns count
    // concrete node-store forks, payload element copies and completed root bodies.
    // Tabs payload copies include their owned MRU elements; no separate MRU store remains.
    writeln!(output, "panes,views,case,fingerprint_builds,fingerprint_node_visits,item_multiset_scans,item_multiset_item_visits,transaction_candidates,cloned_nodes,cloned_tab_items,node_store_forks,node_handle_clones,node_payload_clones,tab_item_copies,split_child_copies,split_weight_copies,mru_item_copies,completed_root_structures,root_structure_rows,root_structure_split_handles").unwrap();
    for panes in panes {
        for &views in &views {
            for &case in &cases {
                if case == Case::CrossViewMove && views != 4 {
                    continue;
                }
                let initial_width = if case == Case::Resize {
                    WIDTHS[1]
                } else {
                    WIDTHS[0]
                };
                let mut fixture = fixture(panes, views, initial_width);
                structural_work::reset();
                let measured = measure(&mut fixture, case, panes, views, WIDTHS[0]);
                let work = structural_work::snapshot();
                validate(&fixture, &measured, panes, views, case);
                drop(fixture);
                assert_eq!(
                    measured.words,
                    expected_state(panes, views, WIDTHS[0], case)
                );
                let clones = work.workspace_deep_clones.transaction_candidates;
                let storage = work.workspace_storage;
                writeln!(
                    output,
                    "{panes},{views},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    case.name(),
                    work.root_fingerprint_builds,
                    work.root_fingerprint_node_visits,
                    work.item_multiset_scans,
                    work.item_multiset_item_visits,
                    clones.calls,
                    clones.volume.nodes,
                    clones.volume.tab_items,
                    storage.node_store_forks,
                    storage.node_handle_clones,
                    storage.node_payload_clones,
                    storage.tab_item_copies,
                    storage.split_child_copies,
                    storage.split_weight_copies,
                    storage.mru_item_copies,
                    work.completed_root_structures,
                    work.root_structure_rows,
                    work.root_structure_split_handles
                )
                .unwrap();
            }
        }
    }
    output.flush().unwrap();
}
