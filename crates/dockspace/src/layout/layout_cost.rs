//! Opt-in layout attribution using the existing private solver, without a renderer.

use std::hint::black_box;
use std::io::{BufWriter, Write};
use std::time::Instant;

use super::*;
use crate::graph::{RootRecord, SurfacePresentation, WorkspaceBuilder};
use crate::ids::{ItemId, SurfaceId};

const ROOT: RootId = RootId::new(1);
const WIDTHS: [f64; 2] = [16_380.0, 16_444.0];
const HEIGHT: f64 = 16_380.0;
const SEPARATOR: f64 = 4.0;

fn tree(builder: &mut WorkspaceBuilder, first: usize, count: usize, axis: Axis) -> NodeId {
    if count == 1 {
        let first = u64::try_from(first).unwrap() * 2 + 1;
        return builder.insert_node(Node::tabs([ItemId::new(first), ItemId::new(first + 1)]));
    }
    let next = match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    };
    let half = count / 2;
    let left = tree(builder, first, half, next);
    let right = tree(builder, first + half, half, next);
    builder.insert_node(Node::split(axis, [left, right], [0.5, 0.5]).unwrap())
}

fn preorder(workspace: &Workspace) -> Vec<NodeId> {
    let mut pending = vec![workspace.root(ROOT).unwrap().node];
    let mut rows = Vec::new();
    while let Some(node) = pending.pop() {
        rows.push(node);
        if let Node::Split { children, .. } = workspace.node(node).unwrap() {
            pending.extend(children.iter().rev().copied());
        }
    }
    rows
}

fn expected_rects(count: usize, bounds: LogicalRect, axis: Axis, rows: &mut Vec<LogicalRect>) {
    rows.push(bounds);
    if count == 1 {
        return;
    }
    let (first, second, next) = match axis {
        Axis::Horizontal => {
            let half = (bounds.width() - SEPARATOR) * 0.5;
            (
                LogicalRect::new(bounds.x(), bounds.y(), half, bounds.height()).unwrap(),
                LogicalRect::new(
                    bounds.x() + half + SEPARATOR,
                    bounds.y(),
                    half,
                    bounds.height(),
                )
                .unwrap(),
                Axis::Vertical,
            )
        }
        Axis::Vertical => {
            let half = (bounds.height() - SEPARATOR) * 0.5;
            (
                LogicalRect::new(bounds.x(), bounds.y(), bounds.width(), half).unwrap(),
                LogicalRect::new(
                    bounds.x(),
                    bounds.y() + half + SEPARATOR,
                    bounds.width(),
                    half,
                )
                .unwrap(),
                Axis::Horizontal,
            )
        }
    };
    expected_rects(count / 2, first, next, rows);
    expected_rects(count / 2, second, next, rows);
}

fn validate(projection: &LayoutProjection, nodes: &[NodeId], expected: &[LogicalRect]) -> u64 {
    assert_eq!(projection.nodes.len(), nodes.len());
    assert_eq!(projection.splits.len(), nodes.len() / 2);
    let mut digest = 0xcbf2_9ce4_8422_2325_u64;
    for (&node, &expected) in nodes.iter().zip(expected) {
        let rect = projection.nodes[&node].rect;
        assert_eq!(rect, expected);
        for value in [rect.x(), rect.y(), rect.width(), rect.height()] {
            assert!(value >= 0.0 && value.fract() == 0.0 && value <= 65_536.0);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let integer = value as u64;
            for byte in integer.to_le_bytes() {
                digest = (digest ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    digest
}

fn setting(name: &str, default: usize) -> usize {
    std::env::var(name).map_or(default, |value| {
        value.parse().expect("invalid benchmark setting")
    })
}

#[test]
#[ignore = "opt-in attribution; run scripts/compare_layouts.py"]
fn layout_cost() {
    let path = std::env::var_os("DOCKSPACE_LAYOUT_COST_OUT")
        .expect("the layout comparison runner supplies the output path");
    let samples = setting("DOCKSPACE_LAYOUT_COST_SAMPLES", 96);
    let warmup = setting("DOCKSPACE_LAYOUT_COST_WARMUP", 16);
    let panes =
        std::env::var("DOCKSPACE_LAYOUT_COST_PANES").unwrap_or_else(|_| "16,128,1024".into());
    let mut output = BufWriter::new(std::fs::File::create_new(path).unwrap());
    writeln!(output, "product,panes,case,sample,preparation_ns,elapsed_ns,nodes,leaves,splits,width,height,geometry_digest").unwrap();
    assert!(samples > 0);
    for count in panes
        .split(',')
        .map(|value| value.parse::<usize>().unwrap())
    {
        assert!([16, 128, 1024].contains(&count));
        let mut builder = Workspace::builder();
        let node = tree(&mut builder, 0, count, Axis::Horizontal);
        builder.set_root(ROOT, RootRecord::new(node));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().unwrap();
        let nodes = preorder(&workspace);
        let constraints = nodes
            .iter()
            .filter(|&&node| matches!(workspace.node(node), Some(Node::Tabs { .. })))
            .map(|&node| {
                (
                    node,
                    Constraints::new(
                        LogicalSize::new(0.0, 0.0).unwrap(),
                        LogicalSize::new(65_536.0, 65_536.0).unwrap(),
                    )
                    .unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(nodes.len(), count * 2 - 1);
        assert_eq!(constraints.len(), count);
        let metrics = LayoutMetrics::new(SEPARATOR).unwrap();
        let bounds = WIDTHS.map(|width| LogicalRect::new(0.0, 0.0, width, HEIGHT).unwrap());
        let expected = bounds.map(|bounds| {
            let mut rows = Vec::new();
            expected_rects(count, bounds, Axis::Horizontal, &mut rows);
            assert_eq!(rows.len(), nodes.len());
            rows
        });
        for case in ["project_full", "project_prepared"] {
            for iteration in 0..warmup.checked_add(samples).unwrap() {
                let variant = iteration % 2;
                let bounds = black_box(bounds[variant]);
                let prepare = Instant::now();
                let measured = (case == "project_prepared").then(|| {
                    measure_root(
                        &workspace,
                        ROOT,
                        LeafMeasurements::Constraints(&constraints),
                        metrics,
                    )
                    .unwrap()
                });
                let preparation_ns = if measured.is_some() {
                    prepare.elapsed().as_nanos()
                } else {
                    0
                };
                let start = Instant::now();
                let projection = match measured {
                    Some(measured) => black_box(measured).project(bounds, &[]).unwrap(),
                    None => project_root(
                        black_box(&workspace),
                        ROOT,
                        bounds,
                        |node| constraints.get(&node).copied(),
                        metrics,
                    )
                    .unwrap(),
                };
                black_box(&projection);
                let elapsed_ns = start.elapsed().as_nanos();
                let digest = validate(&projection, &nodes, &expected[variant]);
                if iteration >= warmup {
                    writeln!(output, "dockspace,{count},{case},{},{preparation_ns},{elapsed_ns},{},{},{},{},{},{digest:016x}",
                        iteration - warmup, nodes.len(), count, count - 1, bounds.width(), bounds.height()).unwrap();
                }
                // Destruction is outside timing, as in the existing product comparison.
                drop(projection);
            }
        }
    }
    output.flush().unwrap();
}
