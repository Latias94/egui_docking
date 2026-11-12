//! Shared tab-flow geometry and presentation-menu host selection.

use std::collections::BTreeSet;

use super::SceneCompilationError;
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{Node, Workspace};
use crate::ids::{NodeId, RootId, SurfaceId};
use crate::presentation_config::DockPresentationConfig;

pub(crate) fn reveal_tab_bar_bounds(
    pane: LogicalRect,
    config: &DockPresentationConfig,
) -> Result<Option<LogicalRect>, GeometryError> {
    let extent = (config.tab_bar_height() * 0.55)
        .min(pane.width())
        .min(pane.height());
    let bounds = LogicalRect::new(pane.x(), pane.y(), extent, extent)?;
    Ok((bounds.width() > 0.0 && bounds.height() > 0.0).then_some(bounds))
}

pub(super) fn desired_tab_width(
    content_width: f64,
    close_allowed: bool,
    config: &DockPresentationConfig,
) -> f64 {
    let close = if close_allowed {
        config.tab_close_extent() + config.tab_horizontal_padding()
    } else {
        0.0
    };
    (content_width + 2.0 * config.tab_horizontal_padding() + close)
        .clamp(config.tab_min_width(), config.tab_max_width())
}

pub(super) struct TabStripFlow {
    pub(super) widths: Vec<f64>,
    pub(super) after_tabs_width: f64,
    pub(super) total_width: f64,
}

impl TabStripFlow {
    pub(super) fn allocate(
        desired_widths: &[f64],
        viewport_width: f64,
        after_tabs_width: f64,
        minimum_width: f64,
    ) -> Result<Self, SceneCompilationError> {
        let widths = allocate_tab_widths(
            desired_widths,
            (viewport_width - after_tabs_width).max(0.0),
            minimum_width,
        )?;
        let total_width = checked_width_sum(
            widths
                .iter()
                .copied()
                .chain(std::iter::once(after_tabs_width)),
            "tab strip width",
        )?;
        Ok(Self {
            widths,
            after_tabs_width,
            total_width,
        })
    }

    pub(super) fn maximum_scroll_offset(&self, viewport_width: f64) -> f64 {
        (self.total_width - viewport_width).max(0.0)
    }

    pub(super) fn tab_range(&self, ordinal: usize) -> Option<(f64, f64)> {
        tab_local_range(self.widths.iter().copied(), ordinal)
    }

    pub(super) fn scroll_offset(
        &self,
        requested: f64,
        viewport_width: f64,
        selected: Option<usize>,
        reveal_selected: bool,
    ) -> f64 {
        let maximum = self.maximum_scroll_offset(viewport_width);
        let requested = requested.clamp(0.0, maximum);
        if reveal_selected
            && let Some((start, end)) = selected.and_then(|index| self.tab_range(index))
        {
            reveal_tab_range(requested, maximum, viewport_width, start, end)
        } else {
            requested
        }
    }
}

pub(super) fn allocate_tab_widths(
    desired: &[f64],
    available: f64,
    minimum: f64,
) -> Result<Vec<f64>, SceneCompilationError> {
    if desired.is_empty() {
        return Ok(Vec::new());
    }
    let count =
        u32::try_from(desired.len()).map_err(|_| SceneCompilationError::GeometryCountOverflow)?;
    let minimum_total = minimum * f64::from(count);
    if minimum_total >= available {
        return Ok(vec![minimum; desired.len()]);
    }
    let desired_total = checked_width_sum(desired.iter().copied(), "desired tab widths")?;
    if desired_total <= available {
        return Ok(desired.to_vec());
    }
    let shrinkable = checked_width_sum(
        desired.iter().map(|width| *width - minimum),
        "shrinkable tab widths",
    )?;
    let excess = desired_total - available;
    if shrinkable <= 0.0 || excess <= 0.0 {
        return Err(SceneCompilationError::InvalidTabWidthAllocation { available, minimum });
    }
    desired
        .iter()
        .map(|width| {
            let allocated = *width - excess * ((*width - minimum) / shrinkable);
            if allocated.is_finite() && allocated >= minimum {
                Ok(allocated)
            } else {
                Err(SceneCompilationError::InvalidTabWidthAllocation { available, minimum })
            }
        })
        .collect()
}

fn checked_width_sum(
    values: impl IntoIterator<Item = f64>,
    aggregate: &'static str,
) -> Result<f64, SceneCompilationError> {
    let sum = values.into_iter().sum::<f64>();
    if sum.is_finite() {
        Ok(sum)
    } else {
        Err(SceneCompilationError::NonFiniteAggregate { aggregate })
    }
}

pub(super) fn reveal_tab_range(
    requested: f64,
    maximum: f64,
    viewport_extent: f64,
    start: f64,
    end: f64,
) -> f64 {
    let (minimum, maximum_for_item) = if end - start <= viewport_extent {
        (
            (end - viewport_extent).clamp(0.0, maximum),
            start.clamp(0.0, maximum),
        )
    } else {
        let aligned = start.clamp(0.0, maximum);
        (aligned, aligned)
    };
    requested.clamp(minimum, maximum_for_item)
}

pub(super) fn tab_local_range(
    widths: impl Iterator<Item = f64>,
    ordinal: usize,
) -> Option<(f64, f64)> {
    let mut widths = widths;
    let start = widths.by_ref().take(ordinal).sum::<f64>();
    Some((start, start + widths.next()?))
}

/// `None` excludes a bar; `Some(false)` retains it as a compacted-menu fallback.
pub(super) fn select_menu_host<E: From<SceneCompilationError>>(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    central: Option<NodeId>,
    mut visible_menu_available: impl FnMut(NodeId) -> Result<Option<bool>, E>,
) -> Result<Option<NodeId>, E> {
    let mut compacted_fallback = None;
    if let Some(central) = central
        && let Some(menu_available) = visible_menu_available(central)?
    {
        compacted_fallback = Some(central);
        if menu_available {
            return Ok(Some(central));
        }
    }

    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            return Err(SceneCompilationError::RepeatedNode {
                surface,
                root,
                node,
            }
            .into());
        }
        match workspace
            .node(node)
            .ok_or(SceneCompilationError::MissingNode {
                surface,
                root,
                node,
            })? {
            Node::Tabs { .. } => {
                let Some(menu_available) = visible_menu_available(node)? else {
                    continue;
                };
                compacted_fallback.get_or_insert(node);
                if menu_available {
                    return Ok(Some(node));
                }
            }
            Node::Split { children, .. } => pending.extend(children.iter().rev().copied()),
        }
    }
    Ok(compacted_fallback)
}
