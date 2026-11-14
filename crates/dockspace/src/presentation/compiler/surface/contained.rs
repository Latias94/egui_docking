//! Contained-window chrome and resize geometry.

use std::collections::BTreeSet;

use super::{SceneCompilationError, clip_rect, rect_has_area};
use crate::geometry::{GeometryError, LogicalRect, LogicalSize};
use crate::graph::{Node, Workspace};
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::targeting::targets::SceneLayerKey;
use crate::policy::DockPolicySnapshot;
use crate::presentation::config::DockPresentationConfig;
use crate::presentation::hit::region::HitRegion;
use crate::presentation::scene::{
    ContainedRecord, ContainedResizeDirection, ContainedResizeRecord, PresentationMenuAnchorHost,
    PresentationMenuAnchorRecord,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_contained_record(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    root: RootId,
    ordinal: usize,
    frontmost: bool,
    durable_outer: LogicalRect,
    surface_bounds: LogicalRect,
    minimum_size: LogicalSize,
    layer: SceneLayerKey,
) -> Result<(ContainedRecord, PresentationMenuAnchorRecord), SceneCompilationError> {
    let border = config
        .floating_border_width()
        .min(durable_outer.width() * 0.5)
        .min(durable_outer.height() * 0.5);
    let durable_inner = LogicalRect::new(
        durable_outer.x() + border,
        durable_outer.y() + border,
        (durable_outer.width() - 2.0 * border).max(0.0),
        (durable_outer.height() - 2.0 * border).max(0.0),
    )?;
    let title_height = config.floating_title_height().min(durable_inner.height());
    let durable_title = LogicalRect::new(
        durable_inner.x(),
        durable_inner.y(),
        durable_inner.width(),
        title_height,
    )?;
    let durable_content = LogicalRect::new(
        durable_inner.x(),
        durable_inner.y() + title_height,
        durable_inner.width(),
        (durable_inner.height() - title_height).max(0.0),
    )?;
    let title_inset_x = config
        .floating_resize_extent()
        .min(durable_title.width() * 0.5);
    let title_top_inset = config
        .floating_resize_extent()
        .min(durable_title.height() * 0.5);
    let inner_title = LogicalRect::new(
        durable_title.x() + title_inset_x,
        durable_title.y() + title_top_inset,
        (durable_title.width() - 2.0 * title_inset_x).max(0.0),
        (durable_title.height() - title_top_inset).max(0.0),
    )?;
    let has_content = root_has_content(workspace, surface, root)?;
    let durable_close = has_content
        .then(|| contained_close_rect(inner_title, config))
        .transpose()?
        .filter(|rect| rect_has_area(*rect));
    let durable_presentation_menu =
        contained_presentation_menu_rect(inner_title, durable_close, config)?;
    let durable_title_drag = if let Some(control) = durable_presentation_menu.or(durable_close) {
        LogicalRect::new(
            inner_title.x(),
            inner_title.y(),
            (control.x() - config.tab_horizontal_padding() - inner_title.x()).max(0.0),
            inner_title.height(),
        )?
    } else {
        inner_title
    };
    let outer = clip_rect(durable_outer, surface_bounds)?;
    let inner = clip_rect(durable_inner, surface_bounds)?;
    let title = clip_rect(durable_title, surface_bounds)?;
    let content = clip_rect(durable_content, surface_bounds)?;
    let close = durable_close
        .map(|close| clip_rect(close, surface_bounds))
        .transpose()?
        .filter(|close| rect_has_area(*close));
    let presentation_menu = durable_presentation_menu
        .map(|anchor| clip_rect(anchor, surface_bounds))
        .transpose()?
        .filter(|anchor| rect_has_area(*anchor));
    let title_drag = clip_rect(durable_title_drag, surface_bounds)?;
    let transform_operable =
        crate::graph::operation::contained_transform_operable(workspace, policy, surface, root)?;
    let contained = ContainedRecord::new(
        floating,
        root,
        ordinal,
        frontmost,
        outer,
        inner,
        title,
        HitRegion::new(title_drag),
        content,
        close,
        transform_operable,
        contained_resize_records(
            durable_outer,
            surface_bounds,
            config.floating_resize_extent(),
        )?,
        minimum_size,
        layer,
    );
    let host = PresentationMenuAnchorHost::ContainedTitle(floating);
    let presentation_menu = match presentation_menu {
        Some(bounds) => PresentationMenuAnchorRecord::ready(root, host, bounds, layer),
        None => PresentationMenuAnchorRecord::compacted(root, host, layer),
    };
    Ok((contained, presentation_menu))
}

fn root_has_content(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
) -> Result<bool, SceneCompilationError> {
    let root_node = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?
        .node;
    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
    let mut has_items = false;
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            return Err(SceneCompilationError::RepeatedNode {
                surface,
                root,
                node,
            });
        }
        match workspace
            .node(node)
            .ok_or(SceneCompilationError::MissingNode {
                surface,
                root,
                node,
            })? {
            Node::Tabs { items, .. } => {
                has_items |= !items.is_empty();
            }
            Node::Split { children, .. } => pending.extend(children.iter().copied()),
        }
    }
    Ok(has_items)
}

fn contained_close_rect(
    title: LogicalRect,
    config: &DockPresentationConfig,
) -> Result<LogicalRect, GeometryError> {
    let size = config
        .tab_close_extent()
        .min(title.width())
        .min(title.height());
    let padding = config
        .tab_horizontal_padding()
        .min((title.width() - size).max(0.0));
    LogicalRect::new(
        title.max().x() - padding - size,
        title.y() + (title.height() - size) * 0.5,
        size,
        size,
    )
}

fn contained_presentation_menu_rect(
    title: LogicalRect,
    close: Option<LogicalRect>,
    config: &DockPresentationConfig,
) -> Result<Option<LogicalRect>, GeometryError> {
    let minimum = config.tab_close_extent().min(title.height());
    if minimum <= 0.0 {
        return Ok(None);
    }
    let desired = minimum.min(title.width());
    let padding = config.tab_horizontal_padding();
    let (right, size) = if let Some(close) = close {
        let available = (close.x() - title.x()).max(0.0);
        let size = desired.min(available);
        let gap = padding.min((available - size).max(0.0));
        (close.x() - gap, size)
    } else {
        let outer = padding.min((title.width() - desired).max(0.0));
        (title.max().x() - outer, desired)
    };
    (size >= minimum)
        .then(|| {
            LogicalRect::new(
                right - size,
                title.y() + (title.height() - size) * 0.5,
                size,
                size,
            )
        })
        .transpose()
}

fn contained_resize_records(
    rect: LogicalRect,
    surface_bounds: LogicalRect,
    configured_extent: f64,
) -> Result<[ContainedResizeRecord; 8], GeometryError> {
    let x = configured_extent.min(rect.width() * 0.5);
    let y = configured_extent.min(rect.height() * 0.5);
    let left = rect.x() + x;
    let right = rect.max().x() - x;
    let top = rect.y() + y;
    let bottom = rect.max().y() - y;
    let record = |direction, x, y, width, height| {
        LogicalRect::new(x, y, width, height)
            .and_then(|rect| clip_rect(rect, surface_bounds))
            .map(HitRegion::new)
            .map(|hit| ContainedResizeRecord::new(direction, hit))
    };
    Ok([
        record(
            ContainedResizeDirection::NorthWest,
            rect.x(),
            rect.y(),
            x,
            y,
        )?,
        record(
            ContainedResizeDirection::North,
            left,
            rect.y(),
            (right - left).max(0.0),
            y,
        )?,
        record(ContainedResizeDirection::NorthEast, right, rect.y(), x, y)?,
        record(
            ContainedResizeDirection::East,
            right,
            top,
            x,
            (bottom - top).max(0.0),
        )?,
        record(ContainedResizeDirection::SouthEast, right, bottom, x, y)?,
        record(
            ContainedResizeDirection::South,
            left,
            bottom,
            (right - left).max(0.0),
            y,
        )?,
        record(ContainedResizeDirection::SouthWest, rect.x(), bottom, x, y)?,
        record(
            ContainedResizeDirection::West,
            rect.x(),
            top,
            x,
            (bottom - top).max(0.0),
        )?,
    ])
}
