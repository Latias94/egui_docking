//! Pane minimums and selection-dependent layout measurements.

use std::collections::BTreeMap;

use super::SceneCompilationError;
use super::tabs::{reveal_tab_range, tab_local_range};
use crate::geometry::LogicalSize;
use crate::graph::{Node, Workspace};
use crate::ids::{NodeId, RootId};
use crate::layout::{LayoutError, LayoutMetrics, RootMeasurement, measure_root_minimums};
use crate::policy::TabBarVisibility;
use crate::presentation_config::DockPresentationConfig;
use crate::scene::{PresentationPlan, TabBarSceneId};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, PaneMinimumKey, SurfaceRequirements, TabBarRequirement,
};
use crate::tab_strip::{TabStripStateKey, TabStripStateStore};

pub(super) struct RootLayoutMeasurement<'workspace> {
    pub(super) pane_minimums: BTreeMap<PaneMinimumKey, LogicalSize>,
    pub(super) layout: RootMeasurement<'workspace>,
}

pub(super) fn measure_root_layout<'workspace>(
    workspace: &'workspace Workspace,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    root: RootId,
) -> Result<RootLayoutMeasurement<'workspace>, SceneCompilationError> {
    let surface = requirements.ticket().surface();
    let pane_minimums = root_pane_minimum_measurements(requirements, measurements, root)?;
    let minimums = root_leaf_minimums(workspace, config, requirements, root, &pane_minimums)?;
    let layout = measure_root_minimums(
        workspace,
        root,
        &minimums,
        LayoutMetrics::new(config.splitter_thickness())?,
    )
    .map_err(|error| match error {
        LayoutError::MissingRoot { root } => SceneCompilationError::MissingRoot { surface, root },
        LayoutError::MissingNode { node } => SceneCompilationError::MissingNode {
            surface,
            root,
            node,
        },
        LayoutError::MissingLeafConstraints { node } => {
            SceneCompilationError::MissingLeafMinimum { root, tabs: node }
        }
        LayoutError::Cycle { node } => SceneCompilationError::RepeatedNode {
            surface,
            root,
            node,
        },
        error => error.into(),
    })?;
    Ok(RootLayoutMeasurement {
        pane_minimums,
        layout,
    })
}

fn root_pane_minimum_measurements(
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    root: RootId,
) -> Result<BTreeMap<PaneMinimumKey, LogicalSize>, SceneCompilationError> {
    requirements
        .pane_minimums()
        .filter(|key| key.root() == root)
        .map(|key| {
            measurements
                .pane_minimum(key)
                .map(|minimum| (key, minimum))
                .ok_or(SceneCompilationError::MissingPaneMinimumMeasurement { key })
        })
        .collect()
}

fn root_leaf_minimums(
    workspace: &Workspace,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    root: RootId,
    pane_minimums: &BTreeMap<PaneMinimumKey, LogicalSize>,
) -> Result<BTreeMap<NodeId, LogicalSize>, SceneCompilationError> {
    let mut minimums = BTreeMap::new();
    for (key, measured) in pane_minimums.iter().filter(|(key, _)| key.root() == root) {
        let key = *key;
        let Some(Node::Tabs { selected, .. }) = workspace.node(key.tabs()) else {
            continue;
        };
        if key.selected() != *selected {
            continue;
        }
        let id = TabBarSceneId {
            root: key.root(),
            tabs: key.tabs(),
        };
        let tab_bar = requirements
            .tab_bar(id)
            .ok_or(SceneCompilationError::MissingTabBarRequirement { id })?;
        minimums.insert(
            key.tabs(),
            pane_outer_minimum(*measured, config, tab_bar.policy().visibility())?,
        );
    }
    Ok(minimums)
}

pub(crate) fn pane_outer_minimum(
    measured: LogicalSize,
    config: &DockPresentationConfig,
    tab_bar_visibility: TabBarVisibility,
) -> Result<LogicalSize, SceneCompilationError> {
    let minimum = config.minimum_pane_size();
    let content_min = LogicalSize::new(
        measured.width().max(minimum.width()),
        measured.height().max(minimum.height()),
    )?;
    let tab_height = if tab_bar_visibility == TabBarVisibility::Visible {
        config.tab_bar_height()
    } else {
        0.0
    };
    Ok(LogicalSize::new(
        content_min.width(),
        content_min.height() + tab_height,
    )?)
}

/// Checks the selection-dependent inputs of an otherwise unchanged sealed surface.
pub(crate) fn selection_geometry_unchanged(
    plan: &PresentationPlan,
    config: &DockPresentationConfig,
    requirement: &TabBarRequirement,
    selected: Option<crate::ids::ItemId>,
    tab_strip_states: &TabStripStateStore,
) -> Result<bool, SceneCompilationError> {
    let bar = requirement.id();
    let Some(layout) = plan.layout_facts() else {
        return Ok(false);
    };
    let Some(root) = layout.root(bar.root) else {
        return Ok(false);
    };
    let old_selected = plan.selected_in_tabs(bar.tabs);
    let Some(old_minimum) =
        root.pane_minimum(PaneMinimumKey::new(bar.root, bar.tabs, old_selected))
    else {
        return Ok(false);
    };
    let Some(new_minimum) = root.pane_minimum(PaneMinimumKey::new(bar.root, bar.tabs, selected))
    else {
        return Ok(false);
    };
    let visibility = requirement.policy().visibility();
    if pane_outer_minimum(old_minimum, config, visibility)?
        != pane_outer_minimum(new_minimum, config, visibility)?
    {
        return Ok(false);
    }
    let Some(record) = plan
        .tab_bar_records()
        .iter()
        .find(|record| *record.id() == bar)
    else {
        return Ok(visibility != TabBarVisibility::Visible);
    };
    let state = tab_strip_states.state(TabStripStateKey::new(plan.surface(), bar));
    let requested = state
        .and_then(|state| state.scroll_offset())
        .unwrap_or(0.0)
        .clamp(0.0, record.maximum_scroll_offset());
    let scroll = if state.is_none_or(|state| state.resolved_selection() != selected) {
        selected
            .and_then(|item| {
                record
                    .members()
                    .iter()
                    .position(|member| member.tab().item == item)
            })
            .and_then(|index| {
                tab_local_range(
                    record
                        .members()
                        .iter()
                        .map(|member| member.allocated_width()),
                    index,
                )
            })
            .map_or(requested, |(start, end)| {
                reveal_tab_range(
                    requested,
                    record.maximum_scroll_offset(),
                    record.viewport().width(),
                    start,
                    end,
                )
            })
    } else {
        requested
    };
    Ok(scroll == record.scroll_offset())
}

pub(crate) fn contained_minimum_size(
    config: &DockPresentationConfig,
    root_minimum: LogicalSize,
) -> Result<LogicalSize, SceneCompilationError> {
    let border = 2.0 * config.floating_border_width();
    let measured = LogicalSize::new(
        root_minimum.width() + border,
        root_minimum.height() + config.floating_title_height() + border,
    )?;
    let configured = config.minimum_floating_size();
    Ok(LogicalSize::new(
        measured.width().max(configured.width()),
        measured.height().max(configured.height()),
    )?)
}
