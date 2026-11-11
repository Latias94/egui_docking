//! Product observations from public topology, actual AccessKit and painted output.

use egui::accesskit::Role;
use egui::{Pos2, Rect};
use egui_dockspace::{Dockspace, DockspaceAxis, ItemId, LogicalRect};

use super::{FIRST, FLOATING, ROOT, SECOND};

pub(super) fn node_center(output: &egui::FullOutput, role: Role, label: &str) -> Pos2 {
    node_rect(output, role, label).center()
}

pub(super) fn node_rect(output: &egui::FullOutput, role: Role, label: &str) -> Rect {
    let (_, node) = accesskit_node(output, role, label);
    accesskit_node_rect(node)
}

pub(super) fn try_node_rect(output: &egui::FullOutput, role: Role, label: &str) -> Option<Rect> {
    let tree = output.platform_output.accesskit_update.as_ref()?;
    tree.nodes.iter().find_map(|(_, node)| {
        (node.role() == role && node.label() == Some(label)).then(|| accesskit_node_rect(node))
    })
}

pub(super) fn accesskit_node_rect(node: &egui::accesskit::Node) -> Rect {
    let bounds = node.bounds().expect("the control exposes bounds");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
}

pub(super) fn accesskit_node<'a>(
    output: &'a egui::FullOutput,
    role: Role,
    label: &str,
) -> (egui::accesskit::NodeId, &'a egui::accesskit::Node) {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    tree.nodes
        .iter()
        .find_map(|(id, node)| {
            (node.role() == role && node.label() == Some(label)).then_some((*id, node))
        })
        .unwrap_or_else(|| {
            let nodes = tree
                .nodes
                .iter()
                .map(|(_, node)| (node.role(), node.label().map(str::to_owned)))
                .collect::<Vec<_>>();
            panic!("missing accessibility node {role:?} {label:?}; nodes={nodes:?}")
        })
}

pub(super) fn tab_center(output: &egui::FullOutput, label: &str) -> Pos2 {
    node_center(output, Role::Tab, label)
}

pub(super) fn selected(dockspace: &Dockspace) -> Option<ItemId> {
    [FIRST, SECOND].into_iter().find(|item| {
        dockspace
            .view()
            .item(*item)
            .is_some_and(|view| view.is_selected())
    })
}

pub(super) fn tab_items(dockspace: &Dockspace) -> Vec<ItemId> {
    dockspace
        .view()
        .root(ROOT)
        .and_then(|root| root.content())
        .and_then(|node| node.tabs())
        .map_or_else(Vec::new, |tabs| tabs.items().to_vec())
}

pub(super) fn root_tabs(dockspace: &Dockspace) -> Option<Vec<ItemId>> {
    Some(
        dockspace
            .view()
            .root(ROOT)?
            .content()?
            .tabs()?
            .items()
            .to_vec(),
    )
}

pub(super) fn root_split(dockspace: &Dockspace) -> Option<(DockspaceAxis, Vec<Vec<ItemId>>)> {
    let split = dockspace.view().root(ROOT)?.content()?.split()?;
    Some((
        split.axis(),
        split
            .children()
            .map(|child| {
                child
                    .tabs()
                    .map_or_else(Vec::new, |tabs| tabs.items().to_vec())
            })
            .collect(),
    ))
}

pub(super) fn root_weights(dockspace: &Dockspace) -> Option<Vec<f32>> {
    Some(
        dockspace
            .view()
            .root(ROOT)?
            .content()?
            .split()?
            .weights()
            .collect(),
    )
}

pub(super) fn splitter_junction_weights(
    dockspace: &Dockspace,
) -> Option<(Vec<f32>, Vec<Vec<f32>>)> {
    let root = dockspace.view().root(ROOT)?.content()?.split()?;
    let root_weights = root.weights().collect();
    let column_weights = root
        .children()
        .map(|child| child.split().map(|split| split.weights().collect()))
        .collect::<Option<Vec<_>>>()?;
    Some((root_weights, column_weights))
}

pub(super) fn contained_title_point(dockspace: &Dockspace) -> Pos2 {
    let rect = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect();
    let style = dockspace.style();
    let title_inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    Pos2::new(
        rect.min().x() as f32
            + style.floating_border_width
            + title_inset
            + style.tab_horizontal_padding,
        rect.min().y() as f32 + style.floating_border_width + style.floating_title_height * 0.5,
    )
}

pub(super) fn contained_rect(dockspace: &Dockspace) -> LogicalRect {
    dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect()
}

pub(super) fn guide_button_count(
    output: &egui::FullOutput,
    passive: egui::Color32,
    active: egui::Color32,
) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect) if rect.fill == passive || rect.fill == active
            )
        })
        .count()
}

pub(super) fn nearest_guide_center(
    output: &egui::FullOutput,
    passive: egui::Color32,
    active: egui::Color32,
    expected: Pos2,
) -> Pos2 {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == passive || rect.fill == active => {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .min_by(|left, right| {
            left.distance_sq(expected)
                .total_cmp(&right.distance_sq(expected))
        })
        .expect("an active drag paints the requested guide cluster")
}

pub(super) fn rect_fill_count(output: &egui::FullOutput, fill: egui::Color32) -> usize {
    fn count(shape: &egui::Shape, fill: egui::Color32) -> usize {
        match shape {
            egui::Shape::Rect(rect) if rect.fill == fill => 1,
            egui::Shape::Vec(shapes) => shapes.iter().map(|shape| count(shape, fill)).sum(),
            _ => 0,
        }
    }

    output
        .shapes
        .iter()
        .map(|shape| count(&shape.shape, fill))
        .sum()
}

pub(super) fn rect_with_fill(output: &egui::FullOutput, fill: egui::Color32) -> Option<Rect> {
    fn find(shape: &egui::Shape, fill: egui::Color32) -> Option<Rect> {
        match shape {
            egui::Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, fill)),
            _ => None,
        }
    }

    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, fill))
}

pub(super) fn rects_with_fill(output: &egui::FullOutput, fills: &[egui::Color32]) -> Vec<Rect> {
    fn collect(shape: &egui::Shape, fills: &[egui::Color32], found: &mut Vec<Rect>) {
        match shape {
            egui::Shape::Rect(rect) if fills.contains(&rect.fill) => found.push(rect.rect),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, fills, found);
                }
            }
            _ => {}
        }
    }

    let mut found = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, fills, &mut found);
    }
    found
}

pub(super) fn rect_fill_count_in(
    output: &egui::FullOutput,
    fill: egui::Color32,
    bounds: Rect,
) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect)
                    if rect.fill == fill && bounds.contains(rect.rect.center())
            )
        })
        .count()
}

pub(super) fn line_segment_count_in(
    output: &egui::FullOutput,
    color: egui::Color32,
    bounds: Rect,
) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == color
                        && points.iter().all(|point| bounds.contains(*point))
            )
        })
        .count()
}

pub(super) fn splitter_rect(
    output: &egui::FullOutput,
    idle: egui::Color32,
    active: egui::Color32,
) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if (rect.fill == idle || rect.fill == active)
                    && (rect.rect.width() <= 12.0 || rect.rect.height() <= 12.0) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the split fixture paints one splitter")
}
