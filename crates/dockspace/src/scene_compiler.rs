//! Core-owned derivation and compilation of semantic presentation scenes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[cfg(test)]
use std::{cell::Cell, thread_local};

use thiserror::Error;

use crate::command::{DockFraction, DockTarget, Edge};
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideEdgeSet, DropGuideTargetRecord,
};
use crate::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey, SurfaceBackground,
};
use crate::error::CommandError;
use crate::geometry::{Constraints, GeometryError, LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{EngineAuthorityDomainId, FloatingPresentationId, NodeId, RootId, SurfaceId};
use crate::layout::{
    LayoutError, LayoutMetrics, LayoutMetricsError, SplitWeightOverride,
    project_root_with_overrides,
};
use crate::policy::{
    DockPolicySnapshot, DockTabBarPolicyRequest, TabBarInteraction, TabBarVisibility,
};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::scene::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
    PaneRecord, PaneSceneId, PresentationPlan, SceneBuildError, SplitterGapPresentation,
    SplitterGapRecord, SplitterJunctionRecord, SplitterRecord, SplitterSceneId, TabBarRecord,
    TabBarSceneId, TabGroupDragRecord, TabListMenuBackdropRecord, TabListMenuGeometryAvailability,
    TabListMenuRecord, TabListMenuRowRecord, TabRecord, TabSceneId, TabStripControlRecord,
    TabStripMemberRecord, TabStripMemberVisibility,
};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, ManifestBuildError, ManifestMeasurementError,
    MeasurementAuthorityError, PaneMinimumKey, PolicyRevision, RequirementBuildError,
    RequirementRevision, SceneRequirementDraft, SceneRequirementManifest, SurfaceMeasurementTicket,
    SurfaceMeasurements, SurfaceRequirementRevision, SurfaceRequirements, TabBarRequirement,
    TabIntrinsicKey, TabStripControlMetrics, TabStripControlPlacement, TabStripKey,
};
use crate::splitter_junction_index::derive_splitter_junction_candidates;
use crate::tab_strip::{
    ActiveTabListMenu, PopupPlaneRequirement, TabStripControlId, TabStripStateKey,
    TabStripStateStore,
};
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

#[cfg(test)]
thread_local! {
    static SURFACE_COMPILATION_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_surface_compilation_count() {
    SURFACE_COMPILATION_COUNT.set(0);
}

#[cfg(test)]
pub(crate) fn surface_compilation_count() -> usize {
    SURFACE_COMPILATION_COUNT.get()
}

/// Derives the complete structural measurement inventory from core-owned topology.
pub(crate) fn derive_scene_requirement_draft(
    authority_domain: EngineAuthorityDomainId,
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    config: PresentationConfigRevision,
    policy: &DockPolicySnapshot,
    revision: RequirementRevision,
    surface_revisions: &BTreeMap<SurfaceId, SurfaceRequirementRevision>,
) -> Result<SceneRequirementDraft, SceneRequirementDerivationError> {
    let workspace_index = Arc::new(
        WorkspaceIndex::build(workspace, workspace_version)
            .map_err(|source| SceneRequirementDerivationError::WorkspaceIndex { source })?,
    );
    derive_scene_requirement_draft_with_index(
        authority_domain,
        workspace,
        workspace_version,
        workspace_index,
        config,
        policy,
        revision,
        surface_revisions,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn derive_scene_requirement_draft_with_index(
    authority_domain: EngineAuthorityDomainId,
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    workspace_index: Arc<WorkspaceIndex>,
    config: PresentationConfigRevision,
    policy: &DockPolicySnapshot,
    revision: RequirementRevision,
    surface_revisions: &BTreeMap<SurfaceId, SurfaceRequirementRevision>,
) -> Result<SceneRequirementDraft, SceneRequirementDerivationError> {
    if workspace_index.version() != workspace_version {
        return Err(
            SceneRequirementDerivationError::WorkspaceIndexVersionMismatch {
                expected: workspace_version,
                actual: workspace_index.version(),
            },
        );
    }
    if let Some((surface, _)) = workspace
        .surfaces()
        .find(|(surface, _)| !surface_revisions.contains_key(surface))
    {
        return Err(SceneRequirementDerivationError::MissingSurfaceRequirementRevision { surface });
    }
    let mut surfaces = BTreeMap::new();
    for (surface, presentation) in workspace.surfaces() {
        let mut pane_minimums = BTreeSet::new();
        let mut tab_intrinsics = BTreeSet::new();
        let mut tab_strips = BTreeSet::new();
        let mut tab_bars = BTreeMap::new();
        let mut roots = Vec::with_capacity(presentation.contained.len().saturating_add(1));
        roots.extend(presentation.main_root);
        for floating in presentation.contained.iter().copied() {
            let root = workspace
                .contained_floating(floating)
                .ok_or(
                    SceneRequirementDerivationError::MissingContainedPresentation {
                        surface,
                        floating,
                    },
                )?
                .root;
            roots.push(root);
        }

        for root in roots {
            derive_root_requirements(
                workspace,
                &workspace_index,
                workspace_version,
                policy,
                surface,
                root,
                &mut pane_minimums,
                &mut tab_intrinsics,
                &mut tab_strips,
                &mut tab_bars,
            )?;
        }

        let surface_revision = *surface_revisions.get(&surface).ok_or(
            SceneRequirementDerivationError::MissingSurfaceRequirementRevision { surface },
        )?;
        let ticket = SurfaceMeasurementTicket::new(
            authority_domain,
            workspace_version.epoch(),
            config,
            policy.revision(),
            surface_revision,
            surface,
        );
        let requirements =
            SurfaceRequirements::new(ticket, pane_minimums, tab_intrinsics, tab_strips, tab_bars)
                .map_err(SceneRequirementDerivationError::Requirement)?;
        surfaces.insert(surface, requirements);
    }

    SceneRequirementDraft::new(
        authority_domain,
        workspace_version,
        workspace_index,
        config,
        policy.revision(),
        revision,
        surfaces,
    )
    .map_err(SceneRequirementDerivationError::Manifest)
}

/// Compiles one exact, measured surface contribution into core-owned scene facts.
///
/// The adapter supplies only measurements minted by `requirements`. Structural
/// identities, layout, layers, targets, guides, previews, and policy outcomes
/// are derived here from core state.
pub(crate) fn compile_surface_scene(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    popup: PopupPlaneRequirement,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
) -> Result<PresentationPlan, SceneCompilationError> {
    let expected = requirements.ticket();
    let actual = measurements.ticket();
    if expected != actual {
        return Err(SceneCompilationError::TicketMismatch { expected, actual });
    }
    if expected.policy() != policy.revision() {
        return Err(SceneCompilationError::PolicyRevisionMismatch {
            ticket: expected.policy(),
            snapshot: policy.revision(),
        });
    }
    let surface = actual.surface();
    let bounds = measurements.bounds();
    if !rect_has_area(bounds) {
        return Err(SceneCompilationError::EmptySurfaceBounds { surface });
    }
    let popup_plane_bounds = measurements.popup_plane_bounds();
    match (popup, popup_plane_bounds) {
        (PopupPlaneRequirement::Inactive { .. }, None) => {}
        (PopupPlaneRequirement::Active { .. }, Some(bounds)) if rect_has_area(bounds) => {}
        (PopupPlaneRequirement::Active { .. }, Some(_)) => {
            return Err(SceneCompilationError::EmptyPopupPlaneBounds { surface });
        }
        (PopupPlaneRequirement::Inactive { .. }, Some(_))
        | (PopupPlaneRequirement::Active { .. }, None) => {
            return Err(SceneCompilationError::PopupPlaneStateMismatch);
        }
    }
    let presentation = workspace
        .surface(surface)
        .ok_or(SceneCompilationError::MissingSurface { surface })?;
    let fraction = dock_fraction(config)?;
    let mut ready = PresentationPlan::from_measurements(actual, popup, bounds, popup_plane_bounds);
    if tab_strip_states.popup_requirement() != popup {
        return Err(SceneCompilationError::PopupPlaneStateMismatch);
    }
    if let PopupPlaneRequirement::Active {
        revision, session, ..
    } = popup
    {
        let popup_plane_bounds =
            popup_plane_bounds.ok_or(SceneCompilationError::PopupPlaneStateMismatch)?;
        ready.push_tab_list_menu_backdrop_record(TabListMenuBackdropRecord::new(
            session,
            revision,
            popup_plane_bounds,
        ));
    }

    if let Some(root) = presentation.main_root {
        compile_root(
            workspace,
            workspace_index,
            workspace_version,
            policy,
            config,
            requirements,
            measurements,
            surface,
            root,
            bounds,
            SceneLayerKey::surface_base(),
            fraction,
            tab_strip_states,
            resize_overrides,
            &mut ready,
        )?;
    } else {
        ready.set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(surface),
            HitRegion::new(bounds),
            DropVisual::new(bounds),
        ))?;
    }

    for (index, floating) in presentation.contained.iter().copied().enumerate() {
        let record = workspace
            .contained_floating(floating)
            .ok_or(SceneCompilationError::MissingContainedPresentation { surface, floating })?;
        let layer = SceneLayerKey::contained(index)
            .ok_or(SceneCompilationError::ContainedLayerExhausted { surface, floating })?;
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            floating,
            HitRegion::new(record.rect),
            layer,
        ));
        let minimum =
            contained_minimum_size(workspace, config, requirements, measurements, record.root)?;
        ready.push_contained_minimum(ContainedMinimumMeasurement::new(floating, minimum));

        if intersect_rect(bounds, record.rect)?.is_none() {
            continue;
        }
        let contained = compile_contained_record(
            workspace,
            policy,
            config,
            surface,
            floating,
            record.root,
            index,
            record.rect,
            bounds,
            minimum,
            layer,
        )?;
        let content = contained.content_bounds();
        ready.push_contained_record(contained);
        if rect_has_area(content) {
            compile_root(
                workspace,
                workspace_index,
                workspace_version,
                policy,
                config,
                requirements,
                measurements,
                surface,
                record.root,
                content,
                layer,
                fraction,
                tab_strip_states,
                resize_overrides,
                &mut ready,
            )?;
        }
    }

    ready.derive_splitter_operability();
    compile_splitter_junctions(&mut ready)?;

    match popup {
        PopupPlaneRequirement::Inactive { .. } => {
            if !ready.tab_list_menu_records().is_empty() {
                return Err(SceneCompilationError::PopupPlaneStateMismatch);
            }
        }
        PopupPlaneRequirement::Active { session, owner, .. } => {
            let exact_menu_count = ready
                .tab_list_menu_records()
                .iter()
                .filter(|record| record.session() == session)
                .count();
            let valid = if owner.surface() == surface {
                exact_menu_count == 1 && ready.tab_list_menu_records().len() == 1
            } else {
                ready.tab_list_menu_records().is_empty()
            };
            if !valid {
                return Err(
                    SceneCompilationError::ActiveTabListMenuProjectionUnavailable { session },
                );
            }
        }
    }

    Ok(ready)
}

fn compile_splitter_junctions(ready: &mut PresentationPlan) -> Result<(), GeometryError> {
    let candidates = derive_splitter_junction_candidates(ready.splitter_records())?;
    for candidate in candidates {
        if ready.region_is_operable(candidate.hit, candidate.layer) {
            ready.push_splitter_junction_record(SplitterJunctionRecord::new(
                candidate.id(),
                HitRegion::new(candidate.hit),
                candidate.layer,
            ));
        }
    }
    Ok(())
}

pub(crate) fn compile_surface_measurements(
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
) -> Result<PresentationPlan, PresentationCompilationError> {
    #[cfg(test)]
    SURFACE_COMPILATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    if manifest.workspace() != workspace_version {
        return Err(PresentationCompilationError::WorkspaceVersionMismatch {
            manifest: manifest.workspace(),
            current: workspace_version,
        });
    }
    let surface = measurements.ticket().surface();
    let validated = manifest.validate_surface(measurements)?;
    let authoritative = validated.require_authoritative()?;
    let requirements = manifest
        .surface(surface)
        .ok_or(PresentationCompilationError::RequirementsVanished { surface })?;
    compile_surface_scene(
        workspace,
        manifest.workspace_index(),
        workspace_version,
        policy,
        config,
        requirements,
        authoritative,
        manifest.popup(),
        tab_strip_states,
        resize_overrides,
    )
    .map_err(PresentationCompilationError::Scene)
}

#[allow(clippy::too_many_arguments)]
fn compile_root(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    bounds: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let root_record = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?;
    let leaf_constraints = root_leaf_constraints(
        workspace,
        config,
        requirements,
        measurements,
        surface,
        root,
        bounds,
    )?;
    let metrics = LayoutMetrics::new(config.splitter_thickness())?;
    let root_overrides = resize_overrides
        .iter()
        .copied()
        .filter(|override_| override_.source().root() == root)
        .collect::<Vec<_>>();
    let projection = project_root_with_overrides(
        workspace,
        root,
        bounds,
        &leaf_constraints,
        metrics,
        &root_overrides,
    )?;

    for (node, projected) in &projection.node_rects {
        let rect = clip_rect(*projected, bounds)?;
        match workspace
            .node(*node)
            .ok_or(SceneCompilationError::MissingNode {
                surface,
                root,
                node: *node,
            })? {
            Node::Tabs { items, selected } => compile_tabs_leaf(
                workspace,
                workspace_index,
                workspace_version,
                policy,
                config,
                requirements,
                measurements,
                surface,
                root,
                *node,
                rect,
                items,
                *selected,
                layer,
                fraction,
                tab_strip_states,
                root_record.node == *node && root_record.central == Some(*node),
                ready,
            )?,
            Node::Split {
                axis,
                children,
                weights,
            } => {
                let split = projection
                    .splits
                    .get(node)
                    .ok_or(SceneCompilationError::MissingSplitProjection { root, split: *node })?;
                let hit_rects = splitter_hit_rects(
                    &split.splitter_rects,
                    *axis,
                    config.splitter_hit_extent(),
                    bounds,
                )?;
                for (index, (splitter, hit)) in split
                    .splitter_rects
                    .iter()
                    .copied()
                    .zip(hit_rects)
                    .enumerate()
                {
                    let id = SplitterSceneId {
                        root,
                        split: *node,
                        index,
                    };
                    let splitter = intersect_rect(bounds, splitter)?;
                    ready.push_splitter_gap_record(SplitterGapRecord::new(
                        id,
                        if splitter.is_some() {
                            SplitterGapPresentation::Rendered
                        } else {
                            SplitterGapPresentation::Collapsed
                        },
                    ));
                    let Some(splitter) = splitter else {
                        continue;
                    };
                    let before = projection.node_rects.get(&children[index]).ok_or(
                        SceneCompilationError::MissingNodeProjection {
                            root,
                            node: children[index],
                        },
                    )?;
                    let after = projection.node_rects.get(&children[index + 1]).ok_or(
                        SceneCompilationError::MissingNodeProjection {
                            root,
                            node: children[index + 1],
                        },
                    )?;
                    let before_minimum_extent =
                        split.child_minimum_extents.get(index).copied().ok_or(
                            SceneCompilationError::MissingSplitMinimum {
                                root,
                                split: *node,
                                child: children[index],
                            },
                        )?;
                    let after_minimum_extent =
                        split.child_minimum_extents.get(index + 1).copied().ok_or(
                            SceneCompilationError::MissingSplitMinimum {
                                root,
                                split: *node,
                                child: children[index + 1],
                            },
                        )?;
                    let before_maximum_extent =
                        split.child_maximum_extents.get(index).copied().ok_or(
                            SceneCompilationError::MissingSplitMaximum {
                                root,
                                split: *node,
                                child: children[index],
                            },
                        )?;
                    let after_maximum_extent =
                        split.child_maximum_extents.get(index + 1).copied().ok_or(
                            SceneCompilationError::MissingSplitMaximum {
                                root,
                                split: *node,
                                child: children[index + 1],
                            },
                        )?;
                    let effective_weights = root_overrides
                        .iter()
                        .find(|override_| override_.source().node() == *node)
                        .map_or_else(|| weights.clone(), |override_| override_.weights().to_vec());
                    let operable = policy.splitter_resize_is_allowed(*axis, surface);
                    ready.push_splitter_record(SplitterRecord::new(
                        id,
                        splitter,
                        HitRegion::new(hit),
                        *axis,
                        clip_rect(*before, bounds)?,
                        clip_rect(*after, bounds)?,
                        before_minimum_extent,
                        after_minimum_extent,
                        before_maximum_extent,
                        after_maximum_extent,
                        split.child_extents.clone(),
                        split.central_index,
                        effective_weights,
                        layer,
                        operable,
                    ));
                }
            }
        }
    }

    push_outer_guide(
        workspace,
        workspace_index,
        workspace_version,
        policy,
        config,
        surface,
        root,
        root_record.node,
        bounds,
        layer,
        fraction,
        ready,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_tabs_leaf(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    bounds: LogicalRect,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    layer: SceneLayerKey,
    fraction: DockFraction,
    tab_strip_states: &TabStripStateStore,
    root_central: bool,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let bar_id = TabBarSceneId { root, tabs };
    let tab_bar_requirement = requirements
        .tab_bar(bar_id)
        .ok_or(SceneCompilationError::MissingTabBarRequirement { id: bar_id })?;
    let tab_height = if tab_bar_requirement.policy().visibility() == TabBarVisibility::Visible {
        config.tab_bar_height().min(bounds.height())
    } else {
        0.0
    };
    let tab_bar = LogicalRect::new(bounds.x(), bounds.y(), bounds.width(), tab_height)?;
    if rect_has_area(tab_bar) {
        compile_tab_strip(
            workspace,
            workspace_index,
            workspace_version,
            policy,
            config,
            tab_bar_requirement,
            measurements,
            surface,
            root,
            tabs,
            tab_bar,
            items,
            selected,
            layer,
            tab_strip_states,
            ready,
        )?;
    }
    let content_height = (bounds.height() - tab_height).max(0.0);
    let content = LogicalRect::new(
        bounds.x(),
        bounds.y() + tab_height,
        bounds.width(),
        content_height,
    )?;
    ready.push_pane_record(PaneRecord::new(
        PaneSceneId { root, tabs },
        bounds,
        content,
        selected,
        layer,
    ));
    if rect_has_area(bounds) {
        push_inner_guide(
            workspace,
            workspace_index,
            workspace_version,
            policy,
            config,
            surface,
            root,
            tabs,
            bounds,
            content,
            layer,
            fraction,
            root_central,
            ready,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_tab_strip(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    requirement: &TabBarRequirement,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    bar: LogicalRect,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    layer: SceneLayerKey,
    tab_strip_states: &TabStripStateStore,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let interaction = requirement.policy().interaction();
    let bar_id = TabBarSceneId { root, tabs };
    let state_key = TabStripStateKey::new(surface, bar_id);
    let state = tab_strip_states.state(state_key);
    let active_menu = tab_strip_states.active_menu_for(state_key);
    let strip_key = TabStripKey::new(surface, bar_id);
    let strip = measurements
        .tab_strip(strip_key)
        .ok_or(SceneCompilationError::MissingTabStripMeasurement { key: strip_key })?;
    let group_extent = if items.is_empty() {
        0.0
    } else {
        config
            .tab_group_grip_extent()
            .min(bar.height())
            .min(bar.width())
    };
    let group_grip_bounds = if group_extent > 0.0 {
        let grip = LogicalRect::new(bar.x(), bar.y(), group_extent, bar.height())?;
        Some(grip)
    } else {
        None
    };
    let group_drag = if interaction == TabBarInteraction::Enabled {
        group_grip_bounds.map(|grip| TabGroupDragRecord::new(grip, HitRegion::new(grip)))
    } else {
        None
    };
    let leading_reserved = strip.leading_reserved();
    let mut desired_widths = Vec::with_capacity(items.len());
    let mut content_widths = Vec::with_capacity(items.len());
    let mut close_allowed = Vec::with_capacity(items.len());
    for item in items.iter().copied() {
        let key = TabIntrinsicKey::new(surface, TabSceneId { root, tabs, item });
        let intrinsic = measurements
            .tab_intrinsic(key)
            .ok_or(SceneCompilationError::MissingTabIntrinsicMeasurement { key })?;
        let item_close_allowed = requirement
            .close_capability(item)
            .ok_or(SceneCompilationError::MissingTabCloseCapability {
                id: TabSceneId { root, tabs, item },
            })?
            .allows_close();
        let close = if item_close_allowed {
            config.tab_close_extent() + config.tab_horizontal_padding()
        } else {
            0.0
        };
        let width = (intrinsic.content_width() + 2.0 * config.tab_horizontal_padding() + close)
            .clamp(config.tab_min_width(), config.tab_max_width());
        content_widths.push(intrinsic.content_width());
        desired_widths.push(width);
        close_allowed.push(item_close_allowed);
    }

    let viewport_x = bar.x() + group_extent + leading_reserved;
    let base_viewport_width =
        (bar.width() - group_extent - leading_reserved - strip.trailing_reserved()).max(0.0);
    let minimum_total = config.tab_min_width()
        * f64::from(
            u32::try_from(items.len()).map_err(|_| SceneCompilationError::GeometryCountOverflow)?,
        );
    let overflowing = !items.is_empty() && minimum_total > base_viewport_width;
    let controls = if overflowing {
        match strip.controls().filter(|metrics| !metrics.is_empty()) {
            Some(metrics) => {
                tab_strip_control_layout(viewport_x, bar, base_viewport_width, metrics)?
            }
            None => None,
        }
    } else {
        None
    };
    let reserved_leading = controls
        .as_ref()
        .map_or(0.0, |layout| layout.reserved_leading);
    let reserved_trailing = controls
        .as_ref()
        .map_or(0.0, |layout| layout.reserved_trailing);
    let viewport_x = viewport_x + reserved_leading;
    let viewport_width = (base_viewport_width - reserved_leading - reserved_trailing).max(0.0);
    let viewport = LogicalRect::new(viewport_x, bar.y(), viewport_width, bar.height())?;
    let control_rects = controls.map(|layout| layout.rects);
    let menu_geometry = if control_rects
        .as_ref()
        .is_some_and(|rects| rects[2].is_some())
        && strip
            .tab_list_menu()
            .is_some_and(crate::scene_manifest::TabListMenuMetrics::can_allocate)
    {
        TabListMenuGeometryAvailability::Available
    } else {
        TabListMenuGeometryAvailability::Unavailable
    };
    let widths = allocate_tab_widths(&desired_widths, viewport_width, config.tab_min_width())?;
    let total_width = checked_sum(widths.iter().copied(), "tab strip width")?;
    let max_scroll = (total_width - viewport.width()).max(0.0);
    let selected_range = selected.and_then(|selected| {
        let index = items.iter().position(|item| *item == selected)?;
        let start = widths[..index].iter().sum::<f64>();
        Some((start, start + widths[index]))
    });
    // Adapter-owned legacy scroll observations are intentionally ignored.
    let requested_scroll = state
        .and_then(|state| state.scroll_offset())
        .unwrap_or(0.0)
        .clamp(0.0, max_scroll);
    let selection_changed = state.is_none_or(|state| state.resolved_selection() != selected);
    let scroll = if selection_changed {
        selected_range.map_or(requested_scroll, |(start, end)| {
            reveal_tab_range(requested_scroll, max_scroll, viewport.width(), start, end)
        })
    } else {
        requested_scroll
    };

    let mut full_tabs = Vec::with_capacity(items.len());
    let mut cursor = viewport.x() - scroll;
    let mut visible_items = BTreeSet::new();
    let mut members = Vec::with_capacity(items.len());
    for (ordinal, ((item, width), close_allowed)) in items
        .iter()
        .copied()
        .zip(widths.iter().copied())
        .zip(close_allowed.iter().copied())
        .enumerate()
    {
        let full = LogicalRect::new(cursor, bar.y(), width, bar.height())?;
        let visible_intersection = intersect_rect(full, viewport)?;
        let visibility = match visible_intersection {
            None => TabStripMemberVisibility::Hidden,
            Some(visible) if visible == full => TabStripMemberVisibility::Visible,
            Some(_) => TabStripMemberVisibility::PartiallyVisible,
        };
        members.push(TabStripMemberRecord::new(
            TabSceneId { root, tabs, item },
            ordinal,
            full,
            visibility,
        ));
        if let Some(chrome) = operable_tab_chrome(full, viewport, close_allowed, config)? {
            let id = TabSceneId { root, tabs, item };
            let drag = if interaction == TabBarInteraction::Enabled {
                chrome.drag
            } else {
                LogicalRect::new(chrome.visible.x(), chrome.visible.y(), 0.0, 0.0)?
            };
            ready.push_tab_record(TabRecord::new(
                id,
                full,
                chrome.visible,
                chrome.text,
                HitRegion::new(drag),
                chrome.close,
                if interaction == TabBarInteraction::Enabled {
                    chrome.close
                } else {
                    None
                },
                selected == Some(item),
                ordinal,
                layer,
            ));
            visible_items.insert(item);
        }
        full_tabs.push(full);
        cursor += width;
    }
    ready.push_tab_bar_record(TabBarRecord::new(
        bar_id,
        bar,
        viewport,
        scroll,
        max_scroll,
        items
            .iter()
            .copied()
            .filter(|item| !visible_items.contains(item))
            .collect(),
        members,
        group_grip_bounds,
        group_drag,
        interaction,
        menu_geometry,
        layer,
    ));

    if let Some([backward, forward, menu]) = control_rects {
        let enabled = interaction == TabBarInteraction::Enabled;
        if let Some(backward) = backward {
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                TabStripControlId::ScrollBackward(bar_id),
                backward,
                enabled && scroll > 0.0,
                layer,
            ));
        }
        if let Some(forward) = forward {
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                TabStripControlId::ScrollForward(bar_id),
                forward,
                enabled && scroll < max_scroll,
                layer,
            ));
        }
        if let Some(menu) = menu {
            let menu_enabled = enabled && menu_geometry.is_available();
            ready.push_tab_strip_control_record(TabStripControlRecord::new(
                TabStripControlId::TabListMenu(bar_id),
                menu,
                menu_enabled,
                layer,
            ));
        }
        if interaction == TabBarInteraction::Enabled
            && let Some(active_menu) = active_menu
            && let Some(menu) = menu
            && let Some(menu_metrics) = strip
                .tab_list_menu()
                .filter(|metrics| metrics.can_allocate())
        {
            compile_tab_list_menu(
                surface,
                bar_id,
                menu,
                layer,
                items,
                selected,
                &content_widths,
                menu_metrics,
                active_menu,
                ready,
            )?;
        }
    }

    if interaction == TabBarInteraction::Enabled {
        let target =
            workspace_index.capture_tab_target(workspace, workspace_version, root, tabs)?;
        let availability = tab_availability(policy);
        for gap in tab_gap_geometries(viewport, &full_tabs, config.splitter_thickness())? {
            ready.push_drop_target(DropTargetRecord::new(
                DropTargetId::TabGap {
                    surface,
                    root,
                    tabs,
                    index: gap.index,
                },
                DockTarget::TabGap {
                    target: target.clone(),
                    index: gap.index,
                },
                availability,
                HitRegion::new(gap.hit),
                layer,
                DropVisual::new(gap.visual),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct TabStripControlLayout {
    rects: [Option<LogicalRect>; 3],
    reserved_leading: f64,
    reserved_trailing: f64,
}

fn tab_strip_control_layout(
    content_x: f64,
    bar: LogicalRect,
    available: f64,
    metrics: TabStripControlMetrics,
) -> Result<Option<TabStripControlLayout>, GeometryError> {
    let entries = [
        metrics.scroll_backward(),
        metrics.scroll_forward(),
        metrics.tab_list_menu(),
    ];
    let group_extent = |placement| {
        let controls = entries
            .iter()
            .flatten()
            .copied()
            .filter(|metric| metric.can_allocate() && metric.placement() == placement)
            .collect::<Vec<_>>();
        controls.iter().map(|metric| metric.extent()).sum::<f64>()
            + metrics.spacing()
                * f64::from(u32::try_from(controls.len().saturating_sub(1)).unwrap_or(u32::MAX))
    };
    let reserved_leading = group_extent(TabStripControlPlacement::ReservedLeading);
    let reserved_trailing = group_extent(TabStripControlPlacement::ReservedTrailing);
    if reserved_leading + reserved_trailing >= available {
        return Ok(None);
    }
    let viewport_x = content_x + reserved_leading;
    let viewport_width = available - reserved_leading - reserved_trailing;
    let viewport_right = viewport_x + viewport_width;
    let overlay_fits = group_extent(TabStripControlPlacement::OverlayLeading)
        + group_extent(TabStripControlPlacement::OverlayTrailing)
        <= viewport_width;
    let mut rects = [None; 3];

    for placement in [
        TabStripControlPlacement::ReservedLeading,
        TabStripControlPlacement::OverlayLeading,
    ] {
        if placement == TabStripControlPlacement::OverlayLeading && !overlay_fits {
            continue;
        }
        let mut cursor = if placement == TabStripControlPlacement::ReservedLeading {
            content_x
        } else {
            viewport_x
        };
        let total = group_extent(placement);
        if total > viewport_width && placement == TabStripControlPlacement::OverlayLeading {
            continue;
        }
        for (index, metric) in entries.iter().copied().enumerate() {
            let Some(metric) =
                metric.filter(|metric| metric.can_allocate() && metric.placement() == placement)
            else {
                continue;
            };
            rects[index] = Some(LogicalRect::new(
                cursor,
                bar.y(),
                metric.extent(),
                bar.height(),
            )?);
            cursor += metric.extent() + metrics.spacing();
        }
    }

    for placement in [
        TabStripControlPlacement::ReservedTrailing,
        TabStripControlPlacement::OverlayTrailing,
    ] {
        if placement == TabStripControlPlacement::OverlayTrailing && !overlay_fits {
            continue;
        }
        let mut cursor = if placement == TabStripControlPlacement::ReservedTrailing {
            content_x + available
        } else {
            viewport_right
        };
        let total = group_extent(placement);
        if total > viewport_width && placement == TabStripControlPlacement::OverlayTrailing {
            continue;
        }
        for (index, metric) in entries.iter().copied().enumerate().rev() {
            let Some(metric) =
                metric.filter(|metric| metric.can_allocate() && metric.placement() == placement)
            else {
                continue;
            };
            cursor -= metric.extent();
            rects[index] = Some(LogicalRect::new(
                cursor,
                bar.y(),
                metric.extent(),
                bar.height(),
            )?);
            cursor -= metrics.spacing();
        }
    }

    Ok(rects
        .iter()
        .any(Option::is_some)
        .then_some(TabStripControlLayout {
            rects,
            reserved_leading,
            reserved_trailing,
        }))
}

#[allow(clippy::too_many_arguments)]
fn compile_tab_list_menu(
    surface: SurfaceId,
    bar: TabBarSceneId,
    anchor: LogicalRect,
    layer: SceneLayerKey,
    items: &[crate::ids::ItemId],
    selected: Option<crate::ids::ItemId>,
    content_widths: &[f64],
    metrics: crate::scene_manifest::TabListMenuMetrics,
    active_menu: &ActiveTabListMenu,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    if items.is_empty() || content_widths.len() != items.len() {
        return Ok(());
    }
    let key = TabStripStateKey::new(surface, bar);
    if active_menu.key() != key || active_menu.items() != items {
        return Err(SceneCompilationError::TabListMenuRosterMismatch { key });
    }
    let bounds = ready
        .popup_plane_bounds()
        .ok_or(SceneCompilationError::PopupPlaneStateMismatch)?;
    if !rect_contains(bounds, anchor) {
        return Err(SceneCompilationError::TabListMenuAnchorOutsidePopupPlane { key });
    }
    let row_gap_count = u32::try_from(items.len().saturating_sub(1))
        .map_err(|_| SceneCompilationError::GeometryCountOverflow)?;
    let row_heights = checked_sum(
        std::iter::repeat_n(metrics.row_height(), items.len()),
        "tab-list row heights",
    )?;
    let row_spacing = checked_sum(
        [metrics.row_spacing() * f64::from(row_gap_count)],
        "tab-list row spacing",
    )?;
    let rows_height = checked_sum([row_heights, row_spacing], "tab-list rows height")?;
    let vertical_padding = checked_sum(
        [metrics.vertical_padding(), metrics.vertical_padding()],
        "tab-list vertical padding",
    )?;
    let natural_height = checked_sum([rows_height, vertical_padding], "tab-list natural height")?;
    let desired_popup_height = natural_height.min(metrics.maximum_height());
    let space_below = (bounds.max().y() - anchor.max().y()).max(0.0);
    let space_above = (anchor.y() - bounds.y()).max(0.0);
    let (opens_below, available_height) = if desired_popup_height <= space_below {
        (true, space_below)
    } else if desired_popup_height <= space_above {
        (false, space_above)
    } else if space_below >= space_above {
        (true, space_below)
    } else {
        (false, space_above)
    };
    let popup_height = desired_popup_height.min(available_height);
    if popup_height <= 0.0 {
        return Err(SceneCompilationError::TabListMenuPopupSpaceUnavailable { key });
    }
    let needs_scrollbar = natural_height > popup_height;
    let horizontal_padding = checked_sum(
        [metrics.horizontal_padding(), metrics.horizontal_padding()],
        "tab-list horizontal padding",
    )?;
    let content_width = checked_sum(
        [
            content_widths.iter().copied().fold(0.0, f64::max),
            horizontal_padding,
        ],
        "tab-list content width",
    )?;
    let popup_width = checked_sum(
        [
            content_width,
            if needs_scrollbar {
                metrics.scrollbar_extent()
            } else {
                0.0
            },
        ],
        "tab-list popup width",
    )?
    .max(anchor.width())
    .min(bounds.width());
    if popup_width <= 0.0 {
        return Ok(());
    }
    let x = (anchor.max().x() - popup_width).clamp(bounds.x(), bounds.max().x() - popup_width);
    let y = if opens_below {
        anchor.max().y()
    } else {
        anchor.y() - popup_height
    };
    let popup = LogicalRect::new(x, y, popup_width, popup_height)?;
    let viewport_width = (popup.width()
        - horizontal_padding
        - if needs_scrollbar {
            metrics.scrollbar_extent()
        } else {
            0.0
        })
    .max(0.0);
    let viewport_height = (popup.height() - vertical_padding).max(0.0);
    if viewport_width <= 0.0 || viewport_height <= 0.0 {
        return Ok(());
    }
    let viewport = LogicalRect::new(
        popup.x() + metrics.horizontal_padding(),
        popup.y() + metrics.vertical_padding(),
        viewport_width,
        viewport_height,
    )?;
    let maximum_scroll = (rows_height - viewport.height()).max(0.0);
    let scroll = active_menu.scroll_offset().clamp(0.0, maximum_scroll);
    let mut cursor = viewport.y() - scroll;
    let mut rows = Vec::with_capacity(items.len());
    for (ordinal, item) in items.iter().copied().enumerate() {
        let row = LogicalRect::new(viewport.x(), cursor, viewport.width(), metrics.row_height())?;
        let hit = intersect_rect(row, viewport)?.map(HitRegion::new);
        rows.push(TabListMenuRowRecord::new(
            TabSceneId {
                root: bar.root,
                tabs: bar.tabs,
                item,
            },
            ordinal,
            row,
            hit,
            selected == Some(item),
            active_menu.focus() == item,
        ));
        cursor += metrics.row_height() + metrics.row_spacing();
    }
    ready.push_tab_list_menu_record(TabListMenuRecord::new(
        active_menu.session(),
        bar,
        popup,
        viewport,
        scroll,
        maximum_scroll,
        rows,
        layer,
    ));
    Ok(())
}

fn allocate_tab_widths(
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
    let desired_total = checked_sum(desired.iter().copied(), "desired tab widths")?;
    if desired_total <= available {
        return Ok(desired.to_vec());
    }
    let shrinkable = checked_sum(
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

fn reveal_tab_range(
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

struct TabChromeGeometry {
    visible: LogicalRect,
    text: LogicalRect,
    drag: LogicalRect,
    close: Option<LogicalRect>,
}

fn operable_tab_chrome(
    full: LogicalRect,
    viewport: LogicalRect,
    close_allowed: bool,
    config: &DockPresentationConfig,
) -> Result<Option<TabChromeGeometry>, GeometryError> {
    let Some(visible) = intersect_rect(full, viewport)? else {
        return Ok(None);
    };
    let operable = config
        .tab_close_extent()
        .min(config.tab_min_width())
        .min(full.width());
    if visible.width() < operable {
        return Ok(None);
    }
    let close = if close_allowed {
        let size = config
            .tab_close_extent()
            .min(full.height())
            .min(full.width());
        if size <= 0.0 {
            return Ok(None);
        }
        let padding = config
            .tab_horizontal_padding()
            .min((full.width() - size).max(0.0));
        let close = LogicalRect::new(
            full.max().x() - padding - size,
            full.y() + (full.height() - size) * 0.5,
            size,
            size,
        )?;
        if !rect_contains(visible, close) {
            return Ok(None);
        }
        Some(close)
    } else {
        None
    };
    let drag = if let Some(close) = close {
        LogicalRect::new(
            visible.x(),
            visible.y(),
            (close.x() - visible.x()).max(0.0),
            visible.height(),
        )?
    } else {
        visible
    };
    if drag.width() < operable || !rect_has_area(drag) {
        return Ok(None);
    }
    let padding = config.tab_horizontal_padding().min(full.width());
    let text_left = (full.x() + padding).min(full.max().x());
    let text_right = close.map_or(full.max().x() - padding, |close| close.x() - padding);
    let text = clip_rect(
        LogicalRect::new(
            text_left,
            full.y(),
            text_right.clamp(text_left, full.max().x()) - text_left,
            full.height(),
        )?,
        visible,
    )?;
    Ok(Some(TabChromeGeometry {
        visible,
        text,
        drag,
        close,
    }))
}

struct TabGapGeometry {
    index: usize,
    hit: LogicalRect,
    visual: LogicalRect,
}

fn tab_gap_geometries(
    viewport: LogicalRect,
    tabs: &[LogicalRect],
    marker_width: f64,
) -> Result<Vec<TabGapGeometry>, GeometryError> {
    if tabs.is_empty() {
        return Ok(Vec::new());
    }
    let centers = tabs
        .iter()
        .map(|tab| tab.x() + tab.width() * 0.5)
        .collect::<Vec<_>>();
    let mut gaps = Vec::with_capacity(tabs.len() + 1);
    for index in 0..=tabs.len() {
        let left = index
            .checked_sub(1)
            .map_or(viewport.x(), |previous| centers[previous]);
        let right = centers.get(index).copied().unwrap_or(viewport.max().x());
        let hit_left = left.max(viewport.x());
        let hit_right = right.min(viewport.max().x());
        if hit_right <= hit_left {
            continue;
        }
        let hit = LogicalRect::new(
            hit_left,
            viewport.y(),
            hit_right - hit_left,
            viewport.height(),
        )?;
        let marker_x = tabs.get(index).map_or(viewport.max().x(), |tab| tab.x());
        let width = marker_width.min(viewport.width());
        if width <= 0.0 {
            continue;
        }
        let marker_min = (marker_x - width * 0.5)
            .clamp(viewport.x(), (viewport.max().x() - width).max(viewport.x()));
        let visual = LogicalRect::new(marker_min, viewport.y(), width, viewport.height())?;
        gaps.push(TabGapGeometry { index, hit, visual });
    }
    Ok(gaps)
}

fn root_leaf_constraints(
    workspace: &Workspace,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
    bounds: LogicalRect,
) -> Result<BTreeMap<NodeId, Constraints>, SceneCompilationError> {
    let minimums =
        root_leaf_minimums(workspace, config, requirements, measurements, surface, root)?;
    let root_node = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?
        .node;
    let root_minimum = subtree_minimum(
        workspace,
        surface,
        root,
        root_node,
        &minimums,
        config.splitter_thickness(),
        &mut BTreeSet::new(),
    )?;
    let maximum = LogicalSize::new(
        bounds.width().max(root_minimum.width()),
        bounds.height().max(root_minimum.height()),
    )?;
    minimums
        .into_iter()
        .map(|(node, minimum)| Ok((node, Constraints::new(minimum, maximum)?)))
        .collect()
}

fn root_leaf_minimums(
    workspace: &Workspace,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    surface: SurfaceId,
    root: RootId,
) -> Result<BTreeMap<NodeId, LogicalSize>, SceneCompilationError> {
    let mut minimums = BTreeMap::new();
    for key in requirements
        .pane_minimums()
        .filter(|key| key.root() == root)
    {
        let measured = measurements
            .pane_minimum(key)
            .ok_or(SceneCompilationError::MissingPaneMinimumMeasurement { key })?;
        let id = TabBarSceneId {
            root: key.root(),
            tabs: key.tabs(),
        };
        let tab_bar = requirements
            .tab_bar(id)
            .ok_or(SceneCompilationError::MissingTabBarRequirement { id })?;
        minimums.insert(
            key.tabs(),
            pane_outer_minimum(measured, config, tab_bar.policy().visibility())?,
        );
    }
    let root_node = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?
        .node;
    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
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
            Node::Tabs { .. } if !minimums.contains_key(&node) => {
                return Err(SceneCompilationError::MissingLeafMinimum { root, tabs: node });
            }
            Node::Tabs { .. } => {}
            Node::Split { children, .. } => {
                pending.extend(children.iter().rev().copied());
            }
        }
    }
    Ok(minimums)
}

fn pane_outer_minimum(
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

fn contained_minimum_size(
    workspace: &Workspace,
    config: &DockPresentationConfig,
    requirements: &SurfaceRequirements,
    measurements: AuthoritativeSurfaceMeasurements<'_>,
    root: RootId,
) -> Result<LogicalSize, SceneCompilationError> {
    let surface = requirements.ticket().surface();
    let minimums =
        root_leaf_minimums(workspace, config, requirements, measurements, surface, root)?;
    let root_record = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?;
    let root_minimum = subtree_minimum(
        workspace,
        surface,
        root,
        root_record.node,
        &minimums,
        config.splitter_thickness(),
        &mut BTreeSet::new(),
    )?;
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

fn subtree_minimum(
    workspace: &Workspace,
    surface: SurfaceId,
    root: RootId,
    node: NodeId,
    minimums: &BTreeMap<NodeId, LogicalSize>,
    splitter: f64,
    visiting: &mut BTreeSet<NodeId>,
) -> Result<LogicalSize, SceneCompilationError> {
    if !visiting.insert(node) {
        return Err(SceneCompilationError::RepeatedNode {
            surface,
            root,
            node,
        });
    }
    let result = match workspace
        .node(node)
        .ok_or(SceneCompilationError::MissingNode {
            surface,
            root,
            node,
        })? {
        Node::Tabs { .. } => minimums
            .get(&node)
            .copied()
            .ok_or(SceneCompilationError::MissingLeafMinimum { root, tabs: node })?,
        Node::Split { axis, children, .. } => {
            let child_minimums = children
                .iter()
                .copied()
                .map(|child| {
                    subtree_minimum(
                        workspace, surface, root, child, minimums, splitter, visiting,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let gap_count = u32::try_from(children.len().saturating_sub(1))
                .map_err(|_| SceneCompilationError::GeometryCountOverflow)?;
            let gaps = splitter * f64::from(gap_count);
            match axis {
                crate::graph::Axis::Horizontal => LogicalSize::new(
                    checked_sum(
                        child_minimums.iter().map(|size| size.width()),
                        "root minimum width",
                    )? + gaps,
                    child_minimums
                        .iter()
                        .map(|size| size.height())
                        .fold(0.0, f64::max),
                )?,
                crate::graph::Axis::Vertical => LogicalSize::new(
                    child_minimums
                        .iter()
                        .map(|size| size.width())
                        .fold(0.0, f64::max),
                    checked_sum(
                        child_minimums.iter().map(|size| size.height()),
                        "root minimum height",
                    )? + gaps,
                )?,
            }
        }
    };
    visiting.remove(&node);
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn compile_contained_record(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    root: RootId,
    ordinal: usize,
    durable_outer: LogicalRect,
    surface_bounds: LogicalRect,
    minimum_size: LogicalSize,
    layer: SceneLayerKey,
) -> Result<ContainedRecord, SceneCompilationError> {
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
    let title_inset_y = config
        .floating_resize_extent()
        .min(durable_title.height() * 0.5);
    let inner_title = LogicalRect::new(
        durable_title.x() + title_inset_x,
        durable_title.y() + title_inset_y,
        (durable_title.width() - 2.0 * title_inset_x).max(0.0),
        (durable_title.height() - 2.0 * title_inset_y).max(0.0),
    )?;
    let close_allowed = root_allows_close(workspace, policy, surface, root)?;
    let durable_close = close_allowed
        .then(|| contained_close_rect(inner_title, config))
        .transpose()?
        .filter(|rect| rect_has_area(*rect));
    let durable_title_drag = if let Some(close) = durable_close {
        LogicalRect::new(
            inner_title.x(),
            inner_title.y(),
            (close.x() - config.tab_horizontal_padding() - inner_title.x()).max(0.0),
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
    let title_drag = clip_rect(durable_title_drag, surface_bounds)?;
    Ok(ContainedRecord::new(
        floating,
        root,
        ordinal,
        outer,
        inner,
        title,
        HitRegion::new(title_drag),
        content,
        close,
        contained_resize_records(
            durable_outer,
            surface_bounds,
            config.floating_resize_extent(),
        )?,
        minimum_size,
        layer,
    ))
}

fn root_allows_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
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
                for item in items.iter().copied() {
                    if !policy.pane_close_capability(item).allows_close() {
                        return Ok(false);
                    }
                }
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

#[derive(Clone, Copy)]
struct GuideButtonGeometry {
    draw: LogicalRect,
    hit: LogicalRect,
}

impl GuideButtonGeometry {
    fn clip_to_hit_cell(&mut self, cell: LogicalRect) -> Result<(), GeometryError> {
        self.hit = clip_rect(self.hit, cell)?;
        self.draw = clip_rect(self.draw, self.hit)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct GuideMetrics {
    half_extent: f64,
    hit_padding: f64,
    inner_offset: f64,
    outer_inset: f64,
}

impl GuideMetrics {
    fn compact(
        config: &DockPresentationConfig,
        bounds: LogicalRect,
        reference_span: f64,
    ) -> Option<Self> {
        // There is no origin-independent logical-pixel cutoff for f64 geometry.
        // Representability is therefore the exact lower bound and becomes a typed
        // compilation rejection instead of silently removing semantic slots.
        let scale = (bounds.width() / reference_span)
            .min(bounds.height() / reference_span)
            .min(1.0);
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        Some(Self {
            half_extent: config.guide_extent() * 0.5 * scale,
            hit_padding: config.guide_hit_padding() * scale,
            inner_offset: (config.guide_extent() + config.guide_gap()) * scale,
            outer_inset: config.guide_outer_inset() * scale,
        })
    }
}

#[derive(Clone, Copy)]
struct GuideEdgeGeometry {
    left: GuideButtonGeometry,
    right: GuideButtonGeometry,
    top: GuideButtonGeometry,
    bottom: GuideButtonGeometry,
}

impl GuideEdgeGeometry {
    const fn get(self, edge: Edge) -> GuideButtonGeometry {
        match edge {
            Edge::Left => self.left,
            Edge::Right => self.right,
            Edge::Top => self.top,
            Edge::Bottom => self.bottom,
        }
    }

    const fn buttons(self) -> [GuideButtonGeometry; 4] {
        [self.left, self.right, self.top, self.bottom]
    }
}

#[allow(clippy::too_many_arguments)]
fn push_inner_guide(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    root: RootId,
    tabs: NodeId,
    node_bounds: LogicalRect,
    content: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    root_central: bool,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let cluster = DropGuideClusterId::inner(surface, root, tabs);
    let placement_bounds = if rect_has_area(content) {
        content
    } else {
        node_bounds
    };
    let metrics = GuideMetrics::compact(
        config,
        placement_bounds,
        config.inner_guide_reference_span(),
    )
    .ok_or(SceneCompilationError::UnrepresentableGuideGeometry {
        cluster,
        bounds: placement_bounds,
    })?;
    let center_x = midpoint(placement_bounds.x(), placement_bounds.max().x());
    let center_y = midpoint(placement_bounds.y(), placement_bounds.max().y());
    let center_geometry = guide_button(
        placement_bounds,
        center_x,
        center_y,
        metrics.half_extent,
        metrics.half_extent,
        metrics.hit_padding,
    )?;
    ensure_complete_guide_geometry(cluster, placement_bounds, &[center_geometry])?;
    let center = guide_target(
        DropTargetId::Center {
            surface,
            root,
            tabs,
        },
        DockTarget::Center(workspace_index.capture_tab_target(
            workspace,
            workspace_version,
            root,
            tabs,
        )?),
        tab_availability(policy),
        center_geometry,
        placement_bounds,
        layer,
    );
    if root_central {
        ready.push_drop_guide_cluster(DropGuideClusterRecord::inner_center(
            surface,
            root,
            tabs,
            HitRegion::new(node_bounds),
            layer,
            center,
        ));
        return Ok(());
    }

    let mut geometry = GuideEdgeGeometry {
        left: guide_button(
            placement_bounds,
            center_x - metrics.inner_offset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        right: guide_button(
            placement_bounds,
            center_x + metrics.inner_offset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        top: guide_button(
            placement_bounds,
            center_x,
            center_y - metrics.inner_offset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        bottom: guide_button(
            placement_bounds,
            center_x,
            center_y + metrics.inner_offset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
    };
    separate_inner_cross_hits(placement_bounds, center_geometry, &mut geometry)?;
    let mut all = Vec::from(geometry.buttons());
    all.push(center_geometry);
    ensure_complete_guide_geometry(cluster, placement_bounds, &all)?;
    let edge_record = |edge| {
        Ok::<_, SceneCompilationError>(guide_target(
            DropTargetId::InnerEdge {
                surface,
                root,
                node: tabs,
                edge,
            },
            DockTarget::InnerEdge(workspace_index.capture_inner_edge_target(
                workspace,
                workspace_version,
                root,
                tabs,
                edge,
                fraction,
            )?),
            edge_availability(policy),
            geometry.get(edge),
            edge_preview(node_bounds, edge, fraction)?,
            layer,
        ))
    };
    let edges = DropGuideEdgeSet::new(
        edge_record(Edge::Left)?,
        edge_record(Edge::Right)?,
        edge_record(Edge::Top)?,
        edge_record(Edge::Bottom)?,
    );
    ready.push_drop_guide_cluster(DropGuideClusterRecord::inner(
        surface,
        root,
        tabs,
        HitRegion::new(node_bounds),
        layer,
        center,
        edges,
    ));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_outer_guide(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    surface: SurfaceId,
    root: RootId,
    root_node: NodeId,
    bounds: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let cluster = DropGuideClusterId::outer(surface, root);
    let metrics = GuideMetrics::compact(config, bounds, config.outer_guide_reference_span())
        .ok_or(SceneCompilationError::UnrepresentableGuideGeometry { cluster, bounds })?;
    let center_x = midpoint(bounds.x(), bounds.max().x());
    let center_y = midpoint(bounds.y(), bounds.max().y());
    let mut geometry = GuideEdgeGeometry {
        left: guide_button(
            bounds,
            bounds.x() + metrics.outer_inset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        right: guide_button(
            bounds,
            bounds.max().x() - metrics.outer_inset,
            center_y,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        top: guide_button(
            bounds,
            center_x,
            bounds.y() + metrics.outer_inset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
        bottom: guide_button(
            bounds,
            center_x,
            bounds.max().y() - metrics.outer_inset,
            metrics.half_extent,
            metrics.half_extent,
            metrics.hit_padding,
        )?,
    };
    separate_outer_cross_hits(bounds, &mut geometry)?;
    ensure_complete_guide_geometry(cluster, bounds, &geometry.buttons())?;
    let edge_record = |edge| {
        Ok::<_, SceneCompilationError>(guide_target(
            DropTargetId::OuterEdge {
                surface,
                root,
                node: root_node,
                edge,
            },
            DockTarget::OuterEdge(workspace_index.capture_outer_edge_target(
                workspace,
                workspace_version,
                root,
                edge,
                fraction,
            )?),
            edge_availability(policy),
            geometry.get(edge),
            edge_preview(bounds, edge, fraction)?,
            layer,
        ))
    };
    ready.push_drop_guide_cluster(DropGuideClusterRecord::outer(
        surface,
        root,
        HitRegion::new(bounds),
        layer,
        DropGuideEdgeSet::new(
            edge_record(Edge::Left)?,
            edge_record(Edge::Right)?,
            edge_record(Edge::Top)?,
            edge_record(Edge::Bottom)?,
        ),
    ));
    Ok(())
}

fn guide_target(
    id: DropTargetId,
    target: DockTarget,
    availability: DropTargetAvailability,
    geometry: GuideButtonGeometry,
    preview: LogicalRect,
    layer: SceneLayerKey,
) -> DropGuideTargetRecord {
    DropGuideTargetRecord::new(
        DropTargetRecord::new(
            id,
            target,
            availability,
            HitRegion::new(geometry.hit),
            layer,
            DropVisual::new(preview),
        ),
        geometry.draw,
    )
}

fn guide_button(
    bounds: LogicalRect,
    center_x: f64,
    center_y: f64,
    half_width: f64,
    half_height: f64,
    hit_padding: f64,
) -> Result<GuideButtonGeometry, GeometryError> {
    let draw = LogicalRect::new(
        center_x - half_width,
        center_y - half_height,
        2.0 * half_width,
        2.0 * half_height,
    )?;
    let expanded = LogicalRect::new(
        draw.x() - hit_padding,
        draw.y() - hit_padding,
        draw.width() + 2.0 * hit_padding,
        draw.height() + 2.0 * hit_padding,
    )?;
    Ok(GuideButtonGeometry {
        draw: clip_rect(draw, bounds)?,
        hit: clip_rect(expanded, bounds)?,
    })
}

fn separate_inner_cross_hits(
    bounds: LogicalRect,
    center: GuideButtonGeometry,
    edges: &mut GuideEdgeGeometry,
) -> Result<(), GeometryError> {
    let left_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        (center.hit.x() - bounds.x()).max(0.0),
        bounds.height(),
    )?;
    let right_cell = LogicalRect::new(
        center.hit.max().x(),
        bounds.y(),
        (bounds.max().x() - center.hit.max().x()).max(0.0),
        bounds.height(),
    )?;
    let top_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        bounds.width(),
        (center.hit.y() - bounds.y()).max(0.0),
    )?;
    let bottom_cell = LogicalRect::new(
        bounds.x(),
        center.hit.max().y(),
        bounds.width(),
        (bounds.max().y() - center.hit.max().y()).max(0.0),
    )?;
    edges.left.clip_to_hit_cell(left_cell)?;
    edges.right.clip_to_hit_cell(right_cell)?;
    edges.top.clip_to_hit_cell(top_cell)?;
    edges.bottom.clip_to_hit_cell(bottom_cell)
}

fn separate_outer_cross_hits(
    bounds: LogicalRect,
    edges: &mut GuideEdgeGeometry,
) -> Result<(), GeometryError> {
    let vertical_min_x = edges.top.hit.x().min(edges.bottom.hit.x());
    let vertical_max_x = edges.top.hit.max().x().max(edges.bottom.hit.max().x());
    let horizontal_min_y = edges.left.hit.y().min(edges.right.hit.y());
    let horizontal_max_y = edges.left.hit.max().y().max(edges.right.hit.max().y());
    let left_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        (vertical_min_x - bounds.x()).max(0.0),
        bounds.height(),
    )?;
    let right_cell = LogicalRect::new(
        vertical_max_x,
        bounds.y(),
        (bounds.max().x() - vertical_max_x).max(0.0),
        bounds.height(),
    )?;
    let top_cell = LogicalRect::new(
        bounds.x(),
        bounds.y(),
        bounds.width(),
        (horizontal_min_y - bounds.y()).max(0.0),
    )?;
    let bottom_cell = LogicalRect::new(
        bounds.x(),
        horizontal_max_y,
        bounds.width(),
        (bounds.max().y() - horizontal_max_y).max(0.0),
    )?;
    edges.left.clip_to_hit_cell(left_cell)?;
    edges.right.clip_to_hit_cell(right_cell)?;
    edges.top.clip_to_hit_cell(top_cell)?;
    edges.bottom.clip_to_hit_cell(bottom_cell)
}

fn ensure_complete_guide_geometry(
    cluster: DropGuideClusterId,
    bounds: LogicalRect,
    buttons: &[GuideButtonGeometry],
) -> Result<(), SceneCompilationError> {
    let complete = buttons.iter().all(|button| {
        rect_has_area(button.draw)
            && rect_has_area(button.hit)
            && rect_contains(bounds, button.hit)
            && rect_contains(button.hit, button.draw)
    });
    if complete && !guide_hits_overlap(buttons) {
        Ok(())
    } else {
        Err(SceneCompilationError::UnrepresentableGuideGeometry { cluster, bounds })
    }
}

fn guide_hits_overlap(buttons: &[GuideButtonGeometry]) -> bool {
    buttons.iter().enumerate().any(|(index, first)| {
        buttons
            .iter()
            .skip(index + 1)
            .any(|second| rects_overlap(first.hit, second.hit))
    })
}

fn edge_preview(
    bounds: LogicalRect,
    edge: Edge,
    fraction: DockFraction,
) -> Result<LogicalRect, GeometryError> {
    let fraction = f64::from(fraction.get());
    let min = bounds.min();
    let max = bounds.max();
    match edge {
        Edge::Left => LogicalRect::from_min_max(
            min,
            LogicalPoint::new(min.x() + bounds.width() * fraction, max.y())?,
        ),
        Edge::Right => {
            let width = bounds.width() * fraction;
            LogicalRect::from_min_max(LogicalPoint::new(max.x() - width, min.y())?, max)
        }
        Edge::Top => LogicalRect::from_min_max(
            min,
            LogicalPoint::new(max.x(), min.y() + bounds.height() * fraction)?,
        ),
        Edge::Bottom => {
            let height = bounds.height() * fraction;
            LogicalRect::from_min_max(LogicalPoint::new(min.x(), max.y() - height)?, max)
        }
    }
}

fn splitter_hit_rects(
    splitters: &[LogicalRect],
    axis: crate::graph::Axis,
    extent: f64,
    bounds: LogicalRect,
) -> Result<Vec<LogicalRect>, GeometryError> {
    splitters
        .iter()
        .enumerate()
        .map(|(index, splitter)| {
            let center = match axis {
                crate::graph::Axis::Horizontal => splitter.x() + splitter.width() * 0.5,
                crate::graph::Axis::Vertical => splitter.y() + splitter.height() * 0.5,
            };
            let coordinate = |rect: &LogicalRect| match axis {
                crate::graph::Axis::Horizontal => rect.x() + rect.width() * 0.5,
                crate::graph::Axis::Vertical => rect.y() + rect.height() * 0.5,
            };
            let axis_min = match axis {
                crate::graph::Axis::Horizontal => bounds.x(),
                crate::graph::Axis::Vertical => bounds.y(),
            };
            let axis_max = match axis {
                crate::graph::Axis::Horizontal => bounds.max().x(),
                crate::graph::Axis::Vertical => bounds.max().y(),
            };
            let cell_min = index.checked_sub(1).map_or(axis_min, |previous| {
                (coordinate(&splitters[previous]) + center) * 0.5
            });
            let cell_max = splitters
                .get(index + 1)
                .map_or(axis_max, |next| (center + coordinate(next)) * 0.5);
            let hit_min = (center - extent * 0.5).max(cell_min);
            let hit_max = (center + extent * 0.5).min(cell_max).max(hit_min);
            let hit = match axis {
                crate::graph::Axis::Horizontal => {
                    LogicalRect::new(hit_min, splitter.y(), hit_max - hit_min, splitter.height())?
                }
                crate::graph::Axis::Vertical => {
                    LogicalRect::new(splitter.x(), hit_min, splitter.width(), hit_max - hit_min)?
                }
            };
            clip_rect(hit, bounds)
        })
        .collect()
}

fn clip_rect(left: LogicalRect, right: LogicalRect) -> Result<LogicalRect, GeometryError> {
    let min_x = left.x().max(right.x()).min(right.max().x());
    let min_y = left.y().max(right.y()).min(right.max().y());
    let max_x = left.max().x().min(right.max().x()).max(min_x);
    let max_y = left.max().y().min(right.max().y()).max(min_y);
    LogicalRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

fn intersect_rect(
    left: LogicalRect,
    right: LogicalRect,
) -> Result<Option<LogicalRect>, GeometryError> {
    let min_x = left.x().max(right.x());
    let min_y = left.y().max(right.y());
    let max_x = left.max().x().min(right.max().x());
    let max_y = left.max().y().min(right.max().y());
    if max_x <= min_x || max_y <= min_y {
        return Ok(None);
    }
    Ok(Some(LogicalRect::new(
        min_x,
        min_y,
        max_x - min_x,
        max_y - min_y,
    )?))
}

fn rect_has_area(rect: LogicalRect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0
}

fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    inner.x() >= outer.x()
        && inner.y() >= outer.y()
        && inner.max().x() <= outer.max().x()
        && inner.max().y() <= outer.max().y()
}

fn rects_overlap(left: LogicalRect, right: LogicalRect) -> bool {
    left.x() < right.max().x()
        && right.x() < left.max().x()
        && left.y() < right.max().y()
        && right.y() < left.max().y()
}

fn midpoint(minimum: f64, maximum: f64) -> f64 {
    minimum * 0.5 + maximum * 0.5
}

fn checked_sum(
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

fn tab_availability(policy: &DockPolicySnapshot) -> DropTargetAvailability {
    if policy.allows_tab_merge() {
        DropTargetAvailability::Available
    } else {
        DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
    }
}

fn edge_availability(policy: &DockPolicySnapshot) -> DropTargetAvailability {
    if policy.allows_edge_split() {
        DropTargetAvailability::Available
    } else {
        DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
    }
}

fn dock_fraction(config: &DockPresentationConfig) -> Result<DockFraction, SceneCompilationError> {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated unit fractions are intentionally represented by DockFraction as f32"
    )]
    let fraction = config.dock_fraction() as f32;
    DockFraction::new(fraction).map_err(|_| SceneCompilationError::InvalidDockFraction)
}

/// Failure to compile authoritative measurements into one semantic surface scene.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneCompilationError {
    /// The validated contribution did not echo the exact requirement ticket.
    #[error("surface measurement ticket {actual:?} does not match requirement {expected:?}")]
    TicketMismatch {
        /// Core-owned requirement ticket.
        expected: SurfaceMeasurementTicket,
        /// Contribution ticket.
        actual: SurfaceMeasurementTicket,
    },
    /// The compiler received a policy snapshot other than the revision frozen by the ticket.
    #[error(
        "surface measurement ticket policy {ticket:?} does not match compiler snapshot {snapshot:?}"
    )]
    PolicyRevisionMismatch {
        /// Exact revision frozen into the core-owned measurement ticket.
        ticket: PolicyRevision,
        /// Revision of the snapshot supplied to compilation.
        snapshot: PolicyRevision,
    },
    /// Measured bounds did not have positive area.
    #[error("surface {surface} measured empty bounds")]
    EmptySurfaceBounds {
        /// Surface whose contribution remains bootstrap.
        surface: SurfaceId,
    },
    /// Measured popup-plane bounds did not have positive area.
    #[error("surface {surface} measured empty popup-plane bounds")]
    EmptyPopupPlaneBounds {
        /// Surface whose active popup cannot become authoritative.
        surface: SurfaceId,
    },
    /// Positive cluster bounds lacked enough representable coordinates for every semantic slot.
    #[error("guide cluster {cluster:?} cannot be represented inside positive bounds {bounds:?}")]
    UnrepresentableGuideGeometry {
        /// Cluster whose complete semantic shape could not be represented.
        cluster: DropGuideClusterId,
        /// Positive placement bounds that failed exact draw and hit construction.
        bounds: LogicalRect,
    },
    /// The ticket named a surface outside the workspace roster.
    #[error("workspace does not contain measured surface {surface}")]
    MissingSurface {
        /// Missing surface.
        surface: SurfaceId,
    },
    /// A surface roster named a missing contained presentation.
    #[error("surface {surface} references missing contained presentation {floating}")]
    MissingContainedPresentation {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing presentation.
        floating: FloatingPresentationId,
    },
    /// A contained roster could not receive a structural layer identity.
    #[error("surface {surface} contained presentation {floating} exhausted structural layers")]
    ContainedLayerExhausted {
        /// Owning surface.
        surface: SurfaceId,
        /// Presentation without a layer.
        floating: FloatingPresentationId,
    },
    /// A surface-owned root record was absent.
    #[error("surface {surface} references missing root {root}")]
    MissingRoot {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing root.
        root: RootId,
    },
    /// A reachable graph node was absent.
    #[error("root {root} on surface {surface} references missing node {node:?}")]
    MissingNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Missing node.
        node: NodeId,
    },
    /// A node was encountered twice while deriving constraints.
    #[error("root {root} on surface {surface} reaches node {node:?} more than once")]
    RepeatedNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Repeated node.
        node: NodeId,
    },
    /// A tabs leaf had no core-minted minimum answer.
    #[error("root {root} tabs node {tabs:?} has no pane minimum")]
    MissingLeafMinimum {
        /// Owning root.
        root: RootId,
        /// Unmeasured leaf.
        tabs: NodeId,
    },
    /// A semantic tabs leaf had no frozen tab-bar policy requirement.
    #[error("tab bar {id:?} has no frozen presentation requirement")]
    MissingTabBarRequirement {
        /// Missing semantic tab-bar identity.
        id: TabBarSceneId,
    },
    /// A tab-bar requirement omitted one item's static close capability.
    #[error("tab {id:?} has no frozen close capability")]
    MissingTabCloseCapability {
        /// Tab whose close policy fact was missing.
        id: TabSceneId,
    },
    /// Exact-set validation succeeded but the authoritative pane minimum was absent.
    #[error("authoritative pane minimum answer is missing for {key:?}")]
    MissingPaneMinimumMeasurement {
        /// Missing exact key.
        key: PaneMinimumKey,
    },
    /// Exact-set validation succeeded but the authoritative tab answer was absent.
    #[error("authoritative tab intrinsic answer is missing for {key:?}")]
    MissingTabIntrinsicMeasurement {
        /// Missing exact key.
        key: TabIntrinsicKey,
    },
    /// Exact-set validation succeeded but the authoritative strip answer was absent.
    #[error("authoritative tab-strip answer is missing for {key:?}")]
    MissingTabStripMeasurement {
        /// Missing exact key.
        key: TabStripKey,
    },
    /// The active core-owned menu disagreed with the exact live tab roster.
    #[error("active tab-list menu roster does not match {key:?}")]
    TabListMenuRosterMismatch {
        /// Exact strip whose live roster changed without reconciliation.
        key: TabStripStateKey,
    },
    /// The menu anchor was not contained by the authoritative popup plane.
    #[error("tab-list menu anchor for {key:?} is outside its popup plane")]
    TabListMenuAnchorOutsidePopupPlane {
        /// Exact strip whose popup anchor cannot be authorized.
        key: TabStripStateKey,
    },
    /// Neither side of the menu anchor had positive authoritative popup space.
    #[error("tab-list menu for {key:?} has no space in its popup plane")]
    TabListMenuPopupSpaceUnavailable {
        /// Exact strip whose popup cannot be placed without covering its anchor.
        key: TabStripStateKey,
    },
    /// Core state owns an active menu but this contribution could not project it.
    #[error("active tab-list menu {session:?} has no exact record in the compiled plan")]
    ActiveTabListMenuProjectionUnavailable {
        /// Exact active menu which must remain authoritative or settle closed.
        session: crate::tab_strip::TabListMenuSessionId,
    },
    /// The transient popup store disagreed with the manifest frozen for compilation.
    #[error("popup-plane state differs from the exact scene requirement manifest")]
    PopupPlaneStateMismatch,
    /// Layout omitted split-specific projection for a reachable split.
    #[error("root {root} has no projection for split {split:?}")]
    MissingSplitProjection {
        /// Owning root.
        root: RootId,
        /// Missing split projection.
        split: NodeId,
    },
    /// Layout omitted one child rectangle needed by a splitter record.
    #[error("root {root} has no projection for node {node:?}")]
    MissingNodeProjection {
        /// Owning root.
        root: RootId,
        /// Missing projected node.
        node: NodeId,
    },
    /// Layout omitted one child minimum required by a splitter record.
    #[error("layout omitted minimum for child {child:?} of split {split:?} in root {root}")]
    MissingSplitMinimum {
        /// Owning root.
        root: RootId,
        /// Split owning the child.
        split: NodeId,
        /// Child whose axis minimum was absent.
        child: NodeId,
    },
    /// Layout omitted one child maximum required by a splitter record.
    #[error("layout omitted maximum for child {child:?} of split {split:?} in root {root}")]
    MissingSplitMaximum {
        /// Owning root.
        root: RootId,
        /// Split owning the child.
        split: NodeId,
        /// Child whose axis maximum was absent.
        child: NodeId,
    },
    /// A collection cardinality could not be represented for geometry arithmetic.
    #[error("presentation geometry count cannot be represented")]
    GeometryCountOverflow,
    /// Finite measurements overflowed while deriving an aggregate.
    #[error("presentation aggregate `{aggregate}` is non-finite")]
    NonFiniteAggregate {
        /// Aggregate which overflowed.
        aggregate: &'static str,
    },
    /// Valid inputs could not produce finite tab widths above the operable minimum.
    #[error("tab widths cannot be allocated within {available} logical units at minimum {minimum}")]
    InvalidTabWidthAllocation {
        /// Width available to the tab sequence before conditional trailing chrome.
        available: f64,
        /// Required operable width of every tab.
        minimum: f64,
    },
    /// A validated core configuration could not be represented by `DockFraction`.
    #[error("presentation dock fraction cannot be represented by DockFraction")]
    InvalidDockFraction,
    /// A current structural target could not be captured.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// Deterministic root layout failed.
    #[error(transparent)]
    Layout(#[from] LayoutError),
    /// Core presentation metrics could not be converted into layout metrics.
    #[error(transparent)]
    LayoutMetrics(#[from] LayoutMetricsError),
    /// Validated logical geometry could not be constructed.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// The compiler produced a scene record that violated construction rules.
    #[error(transparent)]
    Scene(#[from] SceneBuildError),
}

/// Rejection of one exact adapter measurement contribution.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PresentationCompilationError {
    /// The manifest index belongs to another exact workspace revision.
    #[error("manifest workspace {manifest:?} does not match current workspace {current:?}")]
    WorkspaceVersionMismatch {
        /// Version frozen into the manifest and its structural index.
        manifest: WorkspaceVersion,
        /// Version currently published by the engine.
        current: WorkspaceVersion,
    },
    /// The contribution did not match the current core-derived manifest.
    #[error(transparent)]
    Manifest(#[from] ManifestMeasurementError),
    /// At least one exact-set answer explicitly lacked authoritative data.
    #[error(transparent)]
    Authority(#[from] MeasurementAuthorityError),
    /// The manifest entry disappeared after validation, indicating an internal invariant failure.
    #[error("surface {surface} requirements vanished after validation")]
    RequirementsVanished {
        /// Surface whose current requirement entry was missing.
        surface: SurfaceId,
    },
    /// Core-owned semantic compilation failed.
    #[error(transparent)]
    Scene(#[from] SceneCompilationError),
}

fn derive_root_requirements(
    workspace: &Workspace,
    workspace_index: &WorkspaceIndex,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    surface: SurfaceId,
    root: RootId,
    pane_minimums: &mut BTreeSet<PaneMinimumKey>,
    tab_intrinsics: &mut BTreeSet<TabIntrinsicKey>,
    tab_strips: &mut BTreeSet<TabStripKey>,
    tab_bars: &mut BTreeMap<TabBarSceneId, TabBarRequirement>,
) -> Result<(), SceneRequirementDerivationError> {
    let root_node = workspace
        .root(root)
        .ok_or(SceneRequirementDerivationError::MissingRoot { surface, root })?
        .node;
    let mut pending = vec![root_node];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            return Err(SceneRequirementDerivationError::RepeatedNode {
                surface,
                root,
                node,
            });
        }
        match workspace
            .node(node)
            .ok_or(SceneRequirementDerivationError::MissingNode {
                surface,
                root,
                node,
            })? {
            Node::Tabs { items, selected } => {
                pane_minimums.insert(PaneMinimumKey::new(root, node, *selected));
                let target = workspace_index
                    .capture_tab_target(workspace, workspace_version, root, node)
                    .map_err(
                        |_| SceneRequirementDerivationError::TabBarTargetCaptureFailed {
                            surface,
                            root,
                            tabs: node,
                        },
                    )?;
                if target.surface() != surface {
                    return Err(
                        SceneRequirementDerivationError::TabBarTargetSurfaceMismatch {
                            root,
                            tabs: node,
                            expected: surface,
                            actual: target.surface(),
                        },
                    );
                }
                let target_rule = Some(target.rule());
                let id = TabBarSceneId { root, tabs: node };
                let tab_bar_policy =
                    policy.tab_bar_policy(DockTabBarPolicyRequest::new(surface, target_rule));
                tab_bars.insert(
                    id,
                    TabBarRequirement::new(
                        id,
                        tab_bar_policy,
                        items
                            .iter()
                            .copied()
                            .map(|item| (item, policy.pane_close_capability(item)))
                            .collect(),
                    ),
                );
                if tab_bar_policy.visibility() == TabBarVisibility::Visible {
                    tab_strips.insert(TabStripKey::new(surface, id));
                    tab_intrinsics.extend(items.iter().copied().map(|item| {
                        TabIntrinsicKey::new(
                            surface,
                            TabSceneId {
                                root,
                                tabs: node,
                                item,
                            },
                        )
                    }));
                }
            }
            Node::Split { children, .. } => {
                pending.extend(children.iter().rev().copied());
            }
        }
    }
    Ok(())
}

/// Failure to derive an exact semantic measurement inventory.
#[derive(Debug, Clone, PartialEq, Error)]
pub(crate) enum SceneRequirementDerivationError {
    /// A caller attempted to reuse an index outside its exact workspace revision.
    #[error("workspace index version {actual:?} does not match {expected:?}")]
    WorkspaceIndexVersionMismatch {
        /// Exact workspace version being derived.
        expected: WorkspaceVersion,
        /// Exact workspace version frozen into the index.
        actual: WorkspaceVersion,
    },
    /// The exact revision-bound structural index could not be constructed.
    #[error("workspace index construction failed: {source}")]
    WorkspaceIndex {
        /// Structural or presentation invariant rejected by index construction.
        #[source]
        source: CommandError,
    },
    /// A workspace surface had no independently tracked requirement revision.
    #[error("surface {surface} has no requirement revision")]
    MissingSurfaceRequirementRevision {
        /// Surface omitted by the revision inventory.
        surface: SurfaceId,
    },
    /// A surface roster named a missing contained presentation.
    #[error("surface {surface} references missing contained presentation {floating}")]
    MissingContainedPresentation {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing contained presentation.
        floating: FloatingPresentationId,
    },
    /// A surface-owned root record was absent.
    #[error("surface {surface} references missing root {root}")]
    MissingRoot {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing root.
        root: RootId,
    },
    /// A reachable graph node was absent.
    #[error("root {root} on surface {surface} references missing node {node:?}")]
    MissingNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Missing node.
        node: NodeId,
    },
    /// A node was reached twice, indicating a cycle or shared subtree.
    #[error("root {root} on surface {surface} reaches node {node:?} more than once")]
    RepeatedNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Repeated node.
        node: NodeId,
    },
    /// A captured pane-local target disagreed with its roster surface.
    #[error(
        "tab bar for root {root} tabs {tabs:?} belongs to surface {actual}, expected {expected}"
    )]
    TabBarTargetSurfaceMismatch {
        /// Owning root.
        root: RootId,
        /// Tabs leaf whose target facts were incoherent.
        tabs: NodeId,
        /// Surface currently being derived.
        expected: SurfaceId,
        /// Surface frozen by the target capture.
        actual: SurfaceId,
    },
    /// A selected tabs leaf could not produce authoritative target facts.
    #[error(
        "selected tab bar for root {root} tabs {tabs:?} on surface {surface} cannot be captured"
    )]
    TabBarTargetCaptureFailed {
        /// Surface being derived.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Selected tabs leaf.
        tabs: NodeId,
    },
    /// A derived per-surface requirement was internally incoherent.
    #[error(transparent)]
    Requirement(#[from] RequirementBuildError),
    /// The aggregate exact surface inventory was internally incoherent.
    #[error(transparent)]
    Manifest(#[from] ManifestBuildError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{LogicalRect, LogicalSize};
    use crate::graph::{Axis, ContainedFloating, RootRecord, SurfacePresentation};
    use crate::ids::{ItemId, WorkspaceEpoch, WorkspaceRevision};
    use crate::policy::{DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility};
    use crate::presentation_hit::{
        PopupHitRole, PresentationHitManifest, PresentationHitRegionKind, PresentationPlane,
    };
    use crate::presentation_observation::{
        PresentationOutputSerial, SurfacePresentationOutputTicket,
    };
    use crate::scene::{PresentationPlanValidator, SurfaceSceneStamp, TabStripMemberVisibility};
    use crate::scene_manifest::{
        Measurement, SurfaceSceneRevision, TabIntrinsic, TabListMenuMetrics, TabStripControlMetric,
        TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    };

    fn version() -> WorkspaceVersion {
        WorkspaceVersion::new(WorkspaceEpoch::new(3), WorkspaceRevision::new(5))
    }

    fn authority_domain() -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(1)
    }

    fn policy_revision() -> PolicyRevision {
        PolicyRevision::new(8)
    }

    fn policy_snapshot() -> DockPolicySnapshot {
        DockPolicy::default().snapshot(policy_revision())
    }

    #[test]
    fn selected_tab_reveal_preserves_visible_offsets_and_moves_only_as_needed() {
        assert_eq!(reveal_tab_range(40.0, 200.0, 100.0, 50.0, 90.0), 40.0);
        assert_eq!(reveal_tab_range(80.0, 200.0, 100.0, 20.0, 60.0), 20.0);
        assert_eq!(reveal_tab_range(20.0, 200.0, 100.0, 120.0, 160.0), 60.0);
        assert_eq!(
            reveal_tab_range(40.0, 200.0, 100.0, 50.0, 180.0),
            50.0,
            "an oversized selected tab aligns its leading edge"
        );
    }

    fn hit_manifest(
        plan: &PresentationPlan,
        measurements: &SurfaceMeasurements,
    ) -> PresentationHitManifest {
        let scene = SurfaceSceneStamp::new(measurements.ticket(), SurfaceSceneRevision::new(1));
        let output = SurfacePresentationOutputTicket::mint(
            authority_domain(),
            PresentationOutputSerial::new_for_test(1),
            scene,
        );
        PresentationHitManifest::compile(output, plan)
    }

    fn surface_revisions(
        surfaces: impl IntoIterator<Item = SurfaceId>,
    ) -> BTreeMap<SurfaceId, SurfaceRequirementRevision> {
        surfaces
            .into_iter()
            .enumerate()
            .map(|(index, surface)| {
                (
                    surface,
                    SurfaceRequirementRevision::new(
                        u64::try_from(index).expect("fixture index should fit") + 1,
                    ),
                )
            })
            .collect()
    }

    fn balanced_hot_root_fixture(
        leaf_count: usize,
    ) -> (
        Workspace,
        SceneRequirementManifest,
        SurfaceMeasurements,
        usize,
    ) {
        assert!(leaf_count.is_power_of_two());
        let surface = SurfaceId::new(700);
        let root = RootId::new(701);
        let mut builder = Workspace::builder();
        let mut level = Vec::with_capacity(leaf_count);
        let mut central = None;
        for index in 0..leaf_count {
            let item = ItemId::new(
                u64::try_from(index)
                    .expect("fixture index should fit")
                    .saturating_add(1),
            );
            let tabs = builder.insert_node(Node::tabs([item]));
            central.get_or_insert(tabs);
            level.push(tabs);
        }
        let mut axis = Axis::Horizontal;
        while level.len() > 1 {
            let mut parents = Vec::with_capacity(level.len() / 2);
            for children in level.chunks_exact(2) {
                parents.push(
                    builder.insert_node(
                        Node::equal_split(axis, children.iter().copied())
                            .expect("two fixture children form one valid split"),
                    ),
                );
            }
            level = parents;
            axis = match axis {
                Axis::Horizontal => Axis::Vertical,
                Axis::Vertical => Axis::Horizontal,
            };
        }
        builder.set_root(
            root,
            RootRecord::new(level[0]).with_central(central.expect("fixture has one leaf")),
        );
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("balanced fixture should validate");
        let node_count = workspace.nodes().count();
        let draft = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &surface_revisions([surface]),
        )
        .expect("requirements should derive");
        let requirements = draft.surface(surface).expect("surface should exist");
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(
                requirements.bounds(),
                Measurement::Measured(
                    LogicalRect::new(0.0, 0.0, 32_768.0, 32_768.0)
                        .expect("fixture bounds should be valid"),
                ),
            )
            .expect("bounds answer is unique");
        let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(minimum))
                .expect("pane answer is unique");
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(
                    key,
                    Measurement::Measured(
                        TabIntrinsic::new(56.0).expect("intrinsic should be valid"),
                    ),
                )
                .expect("tab answer is unique");
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        TabStripMetrics::new(0.0, 0.0).expect("strip should be valid"),
                    ),
                )
                .expect("strip answer is unique");
        }
        let manifest = draft
            .finalize(PopupPlaneRequirement::default())
            .expect("inactive requirements should finalize");
        (workspace, manifest, measurements, node_count)
    }

    #[test]
    fn scene_target_fingerprints_are_built_once_per_root_at_scale() {
        for leaf_count in [16, 128, 1_024] {
            crate::drop_resolver::structural_work::reset();
            let (workspace, manifest, measurements, node_count) =
                balanced_hot_root_fixture(leaf_count);
            compile_surface_measurements(
                &workspace,
                version(),
                &policy_snapshot(),
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &TabStripStateStore::default(),
                &[],
            )
            .expect("complete measurements should compile");

            let work = crate::drop_resolver::structural_work::snapshot();
            assert_eq!(
                work.root_fingerprint_builds, 1,
                "a {leaf_count}-leaf hot root must build one shared fingerprint"
            );
            assert_eq!(
                work.root_fingerprint_node_visits, node_count,
                "a {leaf_count}-leaf hot root must visit every root node exactly once"
            );
        }
    }

    #[test]
    fn stale_manifest_index_cannot_compile_a_changed_topology_revision() {
        let (workspace, manifest, measurements, _) = balanced_hot_root_fixture(16);
        let mut changed = workspace.clone();
        let root = RootId::new(701);
        let root_node = changed.root(root).expect("fixture root should exist").node;
        let Node::Split {
            children, weights, ..
        } = changed
            .nodes
            .get_mut(root_node)
            .expect("fixture root node should exist")
        else {
            panic!("balanced fixture root must be a split")
        };
        children.reverse();
        weights.reverse();
        changed
            .validate()
            .expect("changed topology should validate");
        let current = WorkspaceVersion::new(version().epoch(), WorkspaceRevision::new(6));

        assert!(matches!(
            compile_surface_measurements(
                &changed,
                current,
                &policy_snapshot(),
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &TabStripStateStore::default(),
                &[],
            ),
            Err(PresentationCompilationError::WorkspaceVersionMismatch {
                manifest: stale,
                current: actual,
            }) if stale == version() && actual == current
        ));
    }

    type OverflowingTabStripFixture = (
        Workspace,
        SceneRequirementManifest,
        SurfaceMeasurements,
        TabStripStateStore,
        SurfaceId,
        TabBarSceneId,
        [ItemId; 4],
    );

    fn overflowing_tab_strip_fixture(legacy_scroll: f64) -> OverflowingTabStripFixture {
        overflowing_tab_strip_fixture_with(
            legacy_scroll,
            &policy_snapshot(),
            Some(
                TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                    .expect("menu metrics should be valid"),
            ),
            true,
        )
    }

    fn overflowing_tab_strip_fixture_with(
        legacy_scroll: f64,
        policy: &DockPolicySnapshot,
        menu: Option<TabListMenuMetrics>,
        open_menu: bool,
    ) -> OverflowingTabStripFixture {
        let controls = TabStripControlMetrics::new(4.0)
            .expect("control metrics should be valid")
            .with_scroll_backward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                    .expect("backward control metric should be valid"),
            )
            .with_scroll_forward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                    .expect("forward control metric should be valid"),
            )
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu control metric should be valid"),
            );
        overflowing_tab_strip_fixture_with_controls(
            legacy_scroll,
            policy,
            menu,
            open_menu,
            controls,
        )
    }

    fn overflowing_tab_strip_fixture_with_controls(
        legacy_scroll: f64,
        policy: &DockPolicySnapshot,
        menu: Option<TabListMenuMetrics>,
        open_menu: bool,
        controls: TabStripControlMetrics,
    ) -> OverflowingTabStripFixture {
        let bounds = LogicalRect::new(0.0, 0.0, 260.0, 180.0)
            .expect("default fixture bounds should be valid");
        overflowing_tab_strip_fixture_with_geometry(
            legacy_scroll,
            policy,
            menu,
            open_menu,
            controls,
            bounds,
            bounds,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn overflowing_tab_strip_fixture_with_geometry(
        legacy_scroll: f64,
        policy: &DockPolicySnapshot,
        menu: Option<TabListMenuMetrics>,
        open_menu: bool,
        controls: TabStripControlMetrics,
        layout_bounds: LogicalRect,
        popup_plane_bounds: LogicalRect,
    ) -> OverflowingTabStripFixture {
        let surface = SurfaceId::new(41);
        let root = RootId::new(42);
        let items = [
            ItemId::new(50),
            ItemId::new(51),
            ItemId::new(52),
            ItemId::new(53),
        ];
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs_with_selection(items, Some(items[3])));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("fixture should validate");
        let draft = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            policy,
            RequirementRevision::new(9),
            &surface_revisions([surface]),
        )
        .expect("requirements should derive");
        let requirements = draft.surface(surface).expect("surface should exist");
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(requirements.bounds(), Measurement::Measured(layout_bounds))
            .expect("bounds answer is unique");
        let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(minimum))
                .expect("pane answer is unique");
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(
                    key,
                    Measurement::Measured(
                        TabIntrinsic::new(88.0).expect("intrinsic should be valid"),
                    ),
                )
                .expect("tab answer is unique");
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        menu.map_or_else(
                            || {
                                TabStripMetrics::new(0.0, 0.0)
                                    .expect("strip should be valid")
                                    .with_controls(controls)
                            },
                            |menu| {
                                TabStripMetrics::new(0.0, 0.0)
                                    .expect("strip should be valid")
                                    .with_controls(controls)
                                    .with_tab_list_menu(menu)
                            },
                        )
                        .with_scroll_offset(legacy_scroll)
                        .expect("legacy observation should be valid"),
                    ),
                )
                .expect("strip answer is unique");
        }
        let bar = TabBarSceneId { root, tabs };
        let mut states = TabStripStateStore::default();
        let state_key = TabStripStateKey::new(surface, bar);
        assert!(
            states
                .reconcile_exact_for_surface_roster([(state_key, items.as_slice())], false)
                .expect("tab-strip roster reconciles")
                .state_changed()
        );
        let state = states.state_mut(state_key);
        state
            .set_scroll_offset(36.0)
            .expect("core scroll should be valid");
        if open_menu {
            let (menu, _) = states
                .open_tab_list_menu(state_key, Some(items[2]))
                .expect("the live strip can open its menu");
            states
                .set_active_menu_scroll_offset(menu, 12.0)
                .expect("menu scroll should be valid");
        }
        let manifest = draft
            .finalize(states.popup_requirement())
            .expect("reconciled popup state must finalize coherently");
        if let Some(key) = manifest
            .surface(surface)
            .expect("final surface requirements should exist")
            .popup_plane_bounds()
        {
            measurements
                .set_popup_plane_bounds(key, Measurement::Measured(popup_plane_bounds))
                .expect("popup-plane bounds answer is unique");
        }
        (
            workspace,
            manifest,
            measurements,
            states,
            surface,
            bar,
            items,
        )
    }

    #[test]
    fn overflow_compiles_stable_controls_membership_and_menu_roster() {
        let (workspace, manifest, measurements, states, surface, bar, items) =
            overflowing_tab_strip_fixture(900.0);
        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        )
        .expect("complete measurements should compile");
        let plan = PresentationPlanValidator::new(&workspace, &policy_snapshot())
            .expect("validator should initialize")
            .validate_and_canonicalize(plan)
            .expect("compiled strip records should be strictly valid");

        assert_eq!(plan.surface(), surface);
        assert_eq!(
            plan.tab_strip_control_records()
                .iter()
                .map(|record| record.id())
                .collect::<Vec<_>>(),
            TabStripControlId::for_bar(bar)
        );
        let members = plan.tab_bar_records()[0].members();
        assert_eq!(
            members
                .iter()
                .map(|member| member.tab().item)
                .collect::<Vec<_>>(),
            items
        );
        let visibilities = members
            .iter()
            .map(|member| member.visibility())
            .collect::<BTreeSet<_>>();
        assert!(visibilities.contains(&TabStripMemberVisibility::Visible));
        assert!(visibilities.contains(&TabStripMemberVisibility::PartiallyVisible));

        let menu = &plan.tab_list_menu_records()[0];
        let [backdrop] = plan.tab_list_menu_backdrop_records() else {
            panic!("an active popup must cover the exact surface")
        };
        assert_eq!(backdrop.session(), menu.session());
        assert_eq!(backdrop.revision(), manifest.popup().revision());
        assert_eq!(backdrop.bounds(), plan.bounds());
        assert_eq!(menu.bar(), bar);
        assert_eq!(menu.session().key(), TabStripStateKey::new(surface, bar));
        assert_eq!(
            menu.rows()
                .iter()
                .map(|row| row.tab().item)
                .collect::<Vec<_>>(),
            items
        );
        assert_eq!(
            menu.rows()
                .iter()
                .filter(|row| row.focused())
                .map(|row| row.tab().item)
                .collect::<Vec<_>>(),
            [items[2]]
        );
        assert!(menu.scroll_offset() <= menu.maximum_scroll_offset());

        let hit_manifest = hit_manifest(&plan, &measurements);
        let ids = hit_manifest
            .regions()
            .iter()
            .map(|region| region.id())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), hit_manifest.regions().len());
        assert_eq!(
            hit_manifest
                .regions()
                .iter()
                .filter(|region| matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabStripControl(_)
                ))
                .count(),
            3
        );
        let blocker = hit_manifest
            .regions()
            .iter()
            .find(|region| {
                region.id().kind() == PresentationHitRegionKind::TabListMenuBlocker(menu.session())
            })
            .expect("the complete popup frame has a blocker");
        assert_eq!(
            blocker.stack().plane(),
            PresentationPlane::Popup(menu.session())
        );
        assert_eq!(
            blocker.stack().popup_role(),
            Some(PopupHitRole::FrameBlocker)
        );
        let backdrop_hit = hit_manifest
            .regions()
            .iter()
            .find(|region| {
                region.id().kind() == PresentationHitRegionKind::TabListMenuBackdrop(menu.session())
            })
            .expect("the popup covers the complete surface");
        assert_eq!(
            backdrop_hit.stack().popup_role(),
            Some(PopupHitRole::Backdrop)
        );
        let row_hits = hit_manifest
            .regions()
            .iter()
            .filter(|region| {
                matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabListMenuRow { menu: actual, .. }
                        if actual == menu.session()
                )
            })
            .count();
        assert_eq!(
            row_hits,
            menu.rows().iter().filter(|row| row.hit().is_some()).count()
        );
    }

    #[test]
    fn bottom_dock_menu_opens_upward_within_the_authoritative_popup_plane() {
        let controls = TabStripControlMetrics::new(4.0)
            .expect("control metrics should be valid")
            .with_scroll_backward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                    .expect("backward control metric should be valid"),
            )
            .with_scroll_forward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                    .expect("forward control metric should be valid"),
            )
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu control metric should be valid"),
            );
        let layout_bounds =
            LogicalRect::new(0.0, 108.0, 180.0, 32.0).expect("bottom dock bounds should be valid");
        let popup_plane_bounds =
            LogicalRect::new(0.0, 0.0, 180.0, 140.0).expect("viewport popup plane should be valid");
        let menu_metrics = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
            .expect("menu metrics should be valid");
        let (workspace, manifest, measurements, states, _, _, _) =
            overflowing_tab_strip_fixture_with_geometry(
                0.0,
                &policy_snapshot(),
                Some(menu_metrics),
                true,
                controls,
                layout_bounds,
                popup_plane_bounds,
            );

        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        )
        .expect("popup-plane measurements should compile");
        let plan = PresentationPlanValidator::new(&workspace, &policy_snapshot())
            .expect("validator should initialize")
            .validate_and_canonicalize(plan)
            .expect("separate layout and popup bounds should validate");
        let [bar] = plan.tab_bar_records() else {
            panic!("fixture should compile one tab bar")
        };
        let [menu] = plan.tab_list_menu_records() else {
            panic!("active fixture should compile one menu")
        };
        let [backdrop] = plan.tab_list_menu_backdrop_records() else {
            panic!("active fixture should compile one backdrop")
        };

        assert_eq!(plan.bounds(), layout_bounds);
        assert_eq!(plan.popup_plane_bounds(), Some(popup_plane_bounds));
        assert_eq!(backdrop.bounds(), popup_plane_bounds);
        assert!(rect_contains(popup_plane_bounds, menu.bounds()));
        assert!(menu.bounds().max().y() <= bar.bounds().y());
    }

    #[test]
    fn active_popup_rejects_an_empty_measured_plane_before_projection() {
        let controls = TabStripControlMetrics::new(4.0)
            .expect("control metrics should be valid")
            .with_scroll_backward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                    .expect("backward control metric should be valid"),
            )
            .with_scroll_forward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                    .expect("forward control metric should be valid"),
            )
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu control metric should be valid"),
            );
        let layout_bounds =
            LogicalRect::new(0.0, 0.0, 180.0, 140.0).expect("layout bounds should be valid");
        let empty_popup_plane = LogicalRect::new(0.0, 0.0, 0.0, 140.0)
            .expect("zero-width popup plane is representable but not authoritative");
        let menu_metrics = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
            .expect("menu metrics should be valid");
        let (workspace, manifest, measurements, states, surface, _, _) =
            overflowing_tab_strip_fixture_with_geometry(
                0.0,
                &policy_snapshot(),
                Some(menu_metrics),
                true,
                controls,
                layout_bounds,
                empty_popup_plane,
            );

        assert_eq!(
            compile_surface_measurements(
                &workspace,
                version(),
                &policy_snapshot(),
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &states,
                &[],
            ),
            Err(PresentationCompilationError::Scene(
                SceneCompilationError::EmptyPopupPlaneBounds { surface },
            ))
        );
    }

    #[test]
    fn menu_only_control_reserves_only_its_trailing_extent() {
        let bar = LogicalRect::new(10.0, 20.0, 200.0, 24.0).expect("bar is valid");
        let controls = TabStripControlMetrics::new(4.0)
            .expect("spacing is valid")
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu extent is valid"),
            );
        let layout = tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
            .expect("control geometry compiles")
            .expect("menu-only roster is allocatable");

        assert_eq!(layout.reserved_leading, 0.0);
        assert_eq!(layout.reserved_trailing, 20.0);
        assert_eq!(layout.rects[0], None);
        assert_eq!(layout.rects[1], None);
        let menu = layout.rects[2].expect("menu control is present");
        assert_eq!(menu.x(), bar.max().x() - 20.0);
        assert_eq!(menu.max().x(), bar.max().x());
    }

    #[test]
    fn menu_only_control_compiles_as_an_exact_scene_subset() {
        let policy = policy_snapshot();
        let menu_metrics = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
            .expect("menu metrics are valid");
        let controls = TabStripControlMetrics::new(4.0)
            .expect("spacing is valid")
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu extent is valid"),
            );
        let (workspace, manifest, measurements, states, _, bar, _) =
            overflowing_tab_strip_fixture_with_controls(
                0.0,
                &policy,
                Some(menu_metrics),
                false,
                controls,
            );
        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        )
        .expect("menu-only measurements compile");
        let plan = PresentationPlanValidator::new(&workspace, &policy)
            .expect("validator initializes")
            .validate_and_canonicalize(plan)
            .expect("optional control subset is valid");

        let [control] = plan.tab_strip_control_records() else {
            panic!("menu-only metrics publish exactly one control")
        };
        assert_eq!(control.id(), TabStripControlId::TabListMenu(bar));
        let bar = plan
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == bar)
            .expect("compiled bar exists");
        assert_eq!(bar.viewport().max().x(), control.bounds().x());
        assert_eq!(control.bounds().max().x(), bar.bounds().max().x());
    }

    #[test]
    fn overlay_controls_never_overlap_when_the_viewport_is_too_narrow() {
        let bar = LogicalRect::new(0.0, 0.0, 40.0, 24.0).expect("bar is valid");
        let controls = TabStripControlMetrics::new(4.0)
            .expect("spacing is valid")
            .with_scroll_backward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                    .expect("backward extent is valid"),
            )
            .with_scroll_forward(
                TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                    .expect("forward extent is valid"),
            )
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu extent is valid"),
            );
        let layout = tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
            .expect("control geometry compiles")
            .expect("reserved menu remains allocatable");

        assert_eq!(layout.rects[0], None);
        assert_eq!(layout.rects[1], None);
        assert!(layout.rects[2].is_some());
        assert_eq!(layout.reserved_trailing, 20.0);
    }

    #[test]
    fn tiny_host_omits_an_unallocatable_reserved_control_roster() {
        let bar = LogicalRect::new(0.0, 0.0, 15.0, 24.0).expect("bar is valid");
        let controls = TabStripControlMetrics::new(0.0)
            .expect("spacing is valid")
            .with_tab_list_menu(
                TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                    .expect("menu extent is valid"),
            );

        assert!(
            tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
                .expect("tiny geometry is a valid unavailable allocation")
                .is_none()
        );
    }

    #[test]
    fn disabled_tab_bar_cannot_publish_an_open_menu_or_row_hits() {
        let mut policy = DockPolicy::default();
        policy.set_tab_bar(TabBarPolicy::new(
            TabBarVisibility::Visible,
            TabBarInteraction::Disabled,
        ));
        let policy = policy.snapshot(policy_revision());
        let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
            .expect("menu metrics should be valid");
        let (workspace, manifest, measurements, states, _, _, _) =
            overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), false);

        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        )
        .expect("disabled strip measurements should compile");
        let plan = PresentationPlanValidator::new(&workspace, &policy)
            .expect("validator should initialize")
            .validate_and_canonicalize(plan)
            .expect("disabled strip output should remain valid");

        assert_eq!(plan.tab_strip_control_records().len(), 3);
        assert!(
            plan.tab_strip_control_records()
                .iter()
                .all(|control| !control.enabled())
        );
        assert!(plan.tab_list_menu_records().is_empty());
        assert!(plan.tab_list_menu_backdrop_records().is_empty());
        let hit_manifest = hit_manifest(&plan, &measurements);
        assert_eq!(
            hit_manifest
                .regions()
                .iter()
                .filter(|region| matches!(
                    region.id().kind(),
                    PresentationHitRegionKind::TabStripControl(_)
                ))
                .count(),
            3,
            "disabled controls remain blockers in the click lane"
        );
        assert!(!hit_manifest.regions().iter().any(|region| matches!(
            region.id().kind(),
            PresentationHitRegionKind::TabListMenuRow { .. }
                | PresentationHitRegionKind::TabListMenuBlocker(_)
        )));
    }

    #[test]
    fn idle_overflow_without_menu_metrics_is_ready_with_a_disabled_menu_receiver() {
        let policy = policy_snapshot();
        let (workspace, manifest, measurements, states, _, bar, _) =
            overflowing_tab_strip_fixture_with(0.0, &policy, None, false);

        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        )
        .expect("optional menu metrics cannot make an idle overflow scene unavailable");
        let plan = PresentationPlanValidator::new(&workspace, &policy)
            .expect("validator should initialize")
            .validate_and_canonicalize(plan)
            .expect("the typed unavailable capability must validate strictly");

        let bar_record = plan
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == bar)
            .expect("overflowing strip has one exact bar record");
        assert_eq!(
            bar_record.menu_geometry_availability(),
            TabListMenuGeometryAvailability::Unavailable
        );
        let menu_control = TabStripControlId::TabListMenu(bar);
        let control = plan
            .tab_strip_control_records()
            .iter()
            .find(|record| record.id() == menu_control)
            .expect("overflow controls retain an exact menu receiver");
        assert!(!control.enabled());
        assert!(plan.tab_list_menu_records().is_empty());
        assert!(plan.tab_list_menu_backdrop_records().is_empty());

        let hit_manifest = hit_manifest(&plan, &measurements);
        assert_eq!(
            hit_manifest
                .regions()
                .iter()
                .filter(|region| {
                    region.id().kind() == PresentationHitRegionKind::TabStripControl(menu_control)
                })
                .count(),
            1,
            "a disabled menu command remains the exclusive click receiver"
        );
    }

    #[test]
    fn active_menu_without_allocatable_geometry_cannot_publish_a_ready_scene() {
        let policy = policy_snapshot();
        let menu = TabListMenuMetrics::new(24.0, 8.0, 20.0, 2.0, 20.0, 10.0)
            .expect("degenerate menu metrics remain a valid measurement");
        let (workspace, manifest, measurements, states, _, _, _) =
            overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), true);

        assert!(matches!(
            compile_surface_measurements(
                &workspace,
                version(),
                &policy,
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &states,
                &[],
            ),
            Err(PresentationCompilationError::Scene(
                SceneCompilationError::ActiveTabListMenuProjectionUnavailable { .. }
            ))
        ));
    }

    #[test]
    fn overflowing_menu_aggregate_is_rejected_before_scene_publication() {
        let policy = policy_snapshot();
        let menu = TabListMenuMetrics::new(f64::MAX, 8.0, 8.0, 2.0, 92.0, 10.0)
            .expect("finite menu metrics should be accepted as measurements");
        let (workspace, manifest, measurements, states, _, _, _) =
            overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), true);

        assert!(matches!(
            compile_surface_measurements(
                &workspace,
                version(),
                &policy,
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &states,
                &[],
            ),
            Err(PresentationCompilationError::Scene(
                SceneCompilationError::NonFiniteAggregate {
                    aggregate: "tab-list row heights"
                }
            ))
        ));
    }

    #[test]
    fn adapter_scroll_observation_does_not_change_core_compilation() {
        let (first_workspace, first_manifest, first, first_states, _, _, _) =
            overflowing_tab_strip_fixture(0.0);
        let (second_workspace, second_manifest, second, second_states, _, _, _) =
            overflowing_tab_strip_fixture(9_000.0);
        let first_plan = compile_surface_measurements(
            &first_workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &first_manifest,
            &first,
            &first_states,
            &[],
        )
        .expect("first measurements should compile");
        let second_plan = compile_surface_measurements(
            &second_workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &second_manifest,
            &second,
            &second_states,
            &[],
        )
        .expect("second measurements should compile");

        assert_eq!(first_plan, second_plan);
    }

    #[test]
    fn edge_previews_reuse_the_exact_activation_boundary() {
        let bounds = LogicalRect::new(0.1, 60.2, 179.7, 159.6).expect("valid bounds");
        let fraction = DockFraction::new(0.3).expect("valid fraction");

        let left = edge_preview(bounds, Edge::Left, fraction).expect("left preview");
        let right = edge_preview(bounds, Edge::Right, fraction).expect("right preview");
        let top = edge_preview(bounds, Edge::Top, fraction).expect("top preview");
        let bottom = edge_preview(bounds, Edge::Bottom, fraction).expect("bottom preview");

        assert_eq!(left.min(), bounds.min());
        assert_eq!(left.max().y(), bounds.max().y());
        assert_eq!(right.max(), bounds.max());
        assert_eq!(right.min().y(), bounds.min().y());
        assert_eq!(top.min(), bounds.min());
        assert_eq!(top.max().x(), bounds.max().x());
        assert_eq!(bottom.max(), bounds.max());
        assert_eq!(bottom.min().x(), bounds.min().x());
    }

    #[test]
    fn single_central_leaf_derives_every_exact_question() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs_with_selection(
            [ItemId::new(10), ItemId::new(11)],
            Some(ItemId::new(11)),
        ));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("fixture should validate");

        let manifest = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &surface_revisions([surface]),
        )
        .expect("requirements should derive")
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");
        let requirements = manifest
            .surface(surface)
            .expect("surface should be present");

        assert_eq!(manifest.surfaces().len(), 1);
        assert_eq!(manifest.authority_domain(), authority_domain());
        assert_eq!(manifest.policy(), policy_revision());
        assert_eq!(requirements.ticket().workspace_epoch(), version().epoch());
        assert_eq!(
            requirements.ticket().surface_requirement(),
            SurfaceRequirementRevision::new(1)
        );
        assert_eq!(
            requirements.pane_minimums().collect::<Vec<_>>(),
            [PaneMinimumKey::new(root, tabs, Some(ItemId::new(11)))]
        );
        assert_eq!(requirements.tab_intrinsics().len(), 2);
        assert_eq!(
            requirements.tab_strips().collect::<Vec<_>>(),
            [TabStripKey::new(surface, TabBarSceneId { root, tabs })]
        );
    }

    #[test]
    fn nested_split_and_rootless_contained_roster_are_complete() {
        let surface = SurfaceId::new(1);
        let root_a = RootId::new(2);
        let root_b = RootId::new(3);
        let floating_a = FloatingPresentationId::new(4);
        let floating_b = FloatingPresentationId::new(5);
        let mut builder = Workspace::builder();
        let a_left = builder.insert_node(Node::tabs([ItemId::new(10)]));
        let a_right = builder.insert_node(Node::tabs([ItemId::new(11)]));
        let a_split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [a_left, a_right]).expect("split should be valid"),
        );
        let b_tabs = builder.insert_node(Node::tabs([ItemId::new(12), ItemId::new(13)]));
        builder.set_root(root_a, RootRecord::new(a_split).with_central(a_right));
        builder.set_root(root_b, RootRecord::new(b_tabs));
        builder.set_surface(surface, SurfacePresentation::rootless());
        builder.set_contained_floating(
            floating_a,
            ContainedFloating::new(
                root_a,
                LogicalRect::new(10.0, 20.0, 300.0, 240.0).expect("valid rect"),
            ),
        );
        builder.set_contained_floating(
            floating_b,
            ContainedFloating::new(
                root_b,
                LogicalRect::new(350.0, 40.0, 280.0, 220.0).expect("valid rect"),
            ),
        );
        builder
            .attach_contained(surface, floating_a)
            .expect("surface should exist");
        builder
            .attach_contained(surface, floating_b)
            .expect("surface should exist");
        let workspace = builder.build().expect("fixture should validate");

        let manifest = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &surface_revisions([surface]),
        )
        .expect("requirements should derive")
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");
        let requirements = manifest
            .surface(surface)
            .expect("surface should be present");

        assert_eq!(requirements.pane_minimums().len(), 3);
        assert_eq!(requirements.tab_intrinsics().len(), 4);
        assert_eq!(requirements.tab_strips().len(), 3);
        assert_eq!(requirements.ticket().surface(), surface);
    }

    #[test]
    fn surface_iteration_and_requirements_are_order_independent() {
        let mut builder = Workspace::builder();
        let tabs_b = builder.insert_node(Node::tabs([ItemId::new(20)]));
        let tabs_a = builder.insert_node(Node::tabs([ItemId::new(10)]));
        builder.set_root(RootId::new(20), RootRecord::new(tabs_b));
        builder.set_root(RootId::new(10), RootRecord::new(tabs_a));
        builder.set_surface(
            SurfaceId::new(20),
            SurfacePresentation::with_main(RootId::new(20)),
        );
        builder.set_surface(
            SurfaceId::new(10),
            SurfacePresentation::with_main(RootId::new(10)),
        );
        let workspace = builder.build().expect("fixture should validate");

        let manifest = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &surface_revisions([SurfaceId::new(10), SurfaceId::new(20)]),
        )
        .expect("requirements should derive")
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");

        assert_eq!(
            manifest
                .surfaces()
                .map(|(surface, _)| surface)
                .collect::<Vec<_>>(),
            [SurfaceId::new(10), SurfaceId::new(20)]
        );
    }

    #[test]
    fn surface_revision_inventory_requires_live_surfaces_and_tolerates_tombstones() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("fixture should validate");

        assert_eq!(
            derive_scene_requirement_draft(
                authority_domain(),
                &workspace,
                version(),
                PresentationConfigRevision::new(7),
                &policy_snapshot(),
                RequirementRevision::new(9),
                &BTreeMap::new(),
            ),
            Err(SceneRequirementDerivationError::MissingSurfaceRequirementRevision { surface })
        );
        let manifest = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &BTreeMap::from([
                (surface, SurfaceRequirementRevision::new(1)),
                (SurfaceId::new(99), SurfaceRequirementRevision::new(2)),
            ]),
        )
        .expect("removed-surface revision tombstones should remain valid")
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");

        assert_eq!(
            manifest
                .surfaces()
                .map(|(surface, _)| surface)
                .collect::<Vec<_>>(),
            [surface]
        );
    }

    #[test]
    fn compilation_rejects_a_policy_snapshot_newer_than_the_exact_ticket() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("fixture should validate");
        let manifest = derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &surface_revisions([surface]),
        )
        .expect("requirements should derive")
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");
        let requirements = manifest
            .surface(surface)
            .expect("surface should be present");
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(
                requirements.bounds(),
                Measurement::Measured(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("bounds should be valid"),
                ),
            )
            .expect("bounds answer is unique");
        let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(minimum))
                .expect("pane answer is unique");
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(
                    key,
                    Measurement::Measured(
                        TabIntrinsic::new(56.0).expect("intrinsic should be valid"),
                    ),
                )
                .expect("tab answer is unique");
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        TabStripMetrics::new(0.0, 0.0).expect("strip should be valid"),
                    ),
                )
                .expect("strip answer is unique");
        }
        let newer = DockPolicy::default().snapshot(
            policy_revision()
                .checked_next()
                .expect("fixture revision should advance"),
        );

        assert!(matches!(
            compile_surface_measurements(
                &workspace,
                version(),
                &newer,
                &DockPresentationConfig::default(),
                &manifest,
                &measurements,
                &TabStripStateStore::default(),
                &[],
            ),
            Err(PresentationCompilationError::Scene(
                SceneCompilationError::PolicyRevisionMismatch { ticket, snapshot },
            )) if ticket == policy_revision() && snapshot == newer.revision()
        ));
    }
}
