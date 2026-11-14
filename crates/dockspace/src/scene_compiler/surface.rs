use super::*;
use crate::geometry::LogicalPoint;
use crate::graph::SplitWeight;
use crate::layout::ChildProjection;

mod contained;
mod error;
mod guides;
mod measurement;
#[cfg(feature = "motion")]
mod motion;
mod tabs;

use contained::compile_contained_record;
pub use error::{PresentationCompilationError, SceneCompilationError};
#[cfg(test)]
pub(super) use guides::edge_preview;
use guides::{
    compile_outer_targets_and_guide, compile_outer_targets_without_guide, outer_guide_geometry,
};
use measurement::{RootLayoutMeasurement, measure_root_layout};
pub(crate) use measurement::{
    contained_minimum_size, pane_outer_minimum, selection_geometry_unchanged,
};
#[cfg(feature = "motion")]
pub(crate) use motion::materialize_presentation_sample;
use tabs::{compile_tabs_leaf, select_presentation_menu_tab_bar};
pub(super) use tabs::{operable_tab_chrome, tab_strip_optional_chrome_layout};
#[cfg(test)]
pub(super) use tabs::{reveal_tab_range, tab_strip_control_layout};

#[cfg(test)]
mod junction_compile_tests;

#[cfg(test)]
mod shared_splitter_tests;

#[cfg(test)]
mod finalize_tests;

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
/// Compiles one exact, measured surface contribution into core-owned scene facts.
///
/// The adapter supplies only measurements minted by `requirements`. Structural
/// identities, layout, layers, targets, guides, previews, and policy outcomes
/// are derived here from core state.
fn compile_surface_scene(
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
    contained_projection: Option<ContainedLayoutProjection>,
    emit_guides: bool,
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
    match (popup.visual_for_surface(surface), popup_plane_bounds) {
        (None, None) => {}
        (Some(_), Some(bounds)) if rect_has_area(bounds) => {}
        (Some(_), Some(_)) => {
            return Err(SceneCompilationError::EmptyPopupPlaneBounds { surface });
        }
        (None, Some(_)) | (Some(_), None) => {
            return Err(SceneCompilationError::PopupPlaneStateMismatch);
        }
    }
    let presentation = workspace
        .surface(surface)
        .ok_or(SceneCompilationError::MissingSurface { surface })?;
    let fraction = dock_fraction(config)?;
    let mut ready = PresentationPlan::from_measurements(actual, popup, bounds, popup_plane_bounds);
    ready.install_layout_facts(config.clone(), measurements);
    ready.set_contained_projection(contained_projection);
    ready.set_contained_order(&presentation.contained);
    if tab_strip_states.popup_requirement() != popup {
        return Err(SceneCompilationError::PopupPlaneStateMismatch);
    }
    if let PopupPlaneRequirement::Active {
        revision, session, ..
    } = popup
        && popup.visual_for_surface(surface).is_some()
    {
        let popup_plane_bounds =
            popup_plane_bounds.ok_or(SceneCompilationError::PopupPlaneStateMismatch)?;
        ready.push_tab_list_menu_backdrop_record(TabListMenuBackdropRecord::new(
            session,
            revision,
            popup_plane_bounds,
        ));
    }

    let contained_placements = presentation
        .contained
        .iter()
        .copied()
        .enumerate()
        .map(|(index, floating)| {
            let record = workspace
                .contained_floating(floating)
                .ok_or(SceneCompilationError::MissingContainedPresentation { surface, floating })?;
            let outer = contained_projection
                .filter(|projection| {
                    projection.surface == surface && projection.floating == floating
                })
                .map_or(record.rect, |projection| projection.rect);
            let layer = SceneLayerKey::contained(index)
                .ok_or(SceneCompilationError::ContainedLayerExhausted { surface, floating })?;
            Ok((floating, record, outer, layer))
        })
        .collect::<Result<Vec<_>, SceneCompilationError>>()?;
    // Publish every projected occluder before laying out rear roots. Guide
    // placement ignores the whole-window drag source; ordinary hits retain it.
    for &(floating, _, outer, layer) in &contained_placements {
        ready.push_drop_occlusion(DropOcclusionRecord::new(
            floating,
            HitRegion::new(outer),
            layer,
        ));
    }

    if let Some(root) = presentation.main_root {
        if let Some(basis) = workspace_index.root_fingerprint(root) {
            ready.bind_command_basis(root, basis.clone());
        }
        let root_measurement = measure_root_layout(
            workspace,
            config,
            requirements,
            measurements,
            root,
            tab_strip_states,
        )?;
        ready.begin_surface_program_root(root, SurfaceProgramRootPresentation::Main)?;
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
            root_measurement,
            bounds,
            SceneLayerKey::surface_base(),
            fraction,
            tab_strip_states,
            resize_overrides,
            true,
            emit_guides,
            &mut ready,
        )?;
        ready.end_surface_program_root(root)?;
    } else {
        ready.set_surface_background(DropTargetRecord::surface_background(
            SurfaceBackground::new(surface),
            HitRegion::new(bounds),
            DropVisual::new(bounds),
        ))?;
    }

    let frontmost_contained = presentation.contained.last().copied();
    for (index, (floating, record, outer, layer)) in contained_placements.into_iter().enumerate() {
        if let Some(basis) = workspace_index.root_fingerprint(record.root) {
            ready.bind_command_basis(record.root, basis.clone());
        }
        let root_measurement = measure_root_layout(
            workspace,
            config,
            requirements,
            measurements,
            record.root,
            tab_strip_states,
        )?;
        let minimum = contained_minimum_size(config, root_measurement.layout.minimum())?;
        ready.push_contained_minimum(ContainedMinimumMeasurement::new(floating, minimum));

        if intersect_rect(bounds, outer)?.is_none() {
            continue;
        }
        let (contained, presentation_menu_anchor) = compile_contained_record(
            workspace,
            policy,
            config,
            surface,
            floating,
            record.root,
            index,
            Some(floating) == frontmost_contained,
            outer,
            bounds,
            minimum,
            layer,
        )?;
        let content = contained.content_bounds();
        let presentation_menu_ready = presentation_menu_anchor.ready_bounds().is_some();
        ready.push_contained_record(contained);
        ready.push_presentation_menu_anchor_record(presentation_menu_anchor);
        ready.begin_surface_program_root(
            record.root,
            SurfaceProgramRootPresentation::Contained(floating),
        )?;
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
                root_measurement,
                content,
                layer,
                fraction,
                tab_strip_states,
                resize_overrides,
                false,
                emit_guides,
                &mut ready,
            )?;
        }
        ready.push_surface_program_root_operation(
            SurfaceProgramRootOperation::ContainedControls(floating),
        )?;
        if presentation_menu_ready {
            ready.push_surface_program_root_operation(
                SurfaceProgramRootOperation::PresentationMenuAnchor(record.root),
            )?;
        }
        ready.end_surface_program_root(record.root)?;
    }

    // Resize ownership comes from the actual root bases bound above.
    ready.set_resize_projection(resize_overrides);
    ready.derive_splitter_operability();
    compile_splitter_junctions(&mut ready)?;
    ready.finish_surface_program()?;

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

fn compile_splitter_junctions(ready: &mut PresentationPlan) -> Result<(), SceneCompilationError> {
    let candidates = derive_splitter_junction_candidates(ready.splitter_records())?;
    if candidates.is_empty() {
        return Ok(());
    }
    let mut splitter_operability = BTreeMap::new();
    for splitter in ready.splitter_records() {
        splitter_operability
            .entry(*splitter.id())
            .or_insert(splitter.operable());
    }
    for candidate in candidates {
        if ready.region_is_operable(candidate.hit, candidate.layer) {
            let operable = candidate.id().splitters().into_iter().all(|splitter| {
                splitter_operability
                    .get(&splitter)
                    .copied()
                    .unwrap_or(false)
            });
            let id = candidate.id();
            ready.push_splitter_junction_record(SplitterJunctionRecord::new(
                id,
                HitRegion::new(candidate.hit),
                candidate.layer,
                operable,
            ));
            ready.push_surface_program_derived_root_operation(
                id.root(),
                SurfaceProgramRootOperation::SplitterJunction(id),
            )?;
        }
    }
    Ok(())
}

pub(crate) struct CompiledSurfacePlan<'a> {
    plan: PresentationPlan,
    workspace: &'a Workspace,
    version: WorkspaceVersion,
    policy: &'a DockPolicySnapshot,
    manifest: &'a SceneRequirementManifest,
    contained_projection: Option<ContainedLayoutProjection>,
}

impl<'a> CompiledSurfacePlan<'a> {
    /// Finalizes exactly the immutable inputs used by the sole constructor.
    pub(crate) fn finalize(self) -> Result<PresentationPlan, SceneBuildError> {
        PresentationPlanValidator::finalize_compiled(self)
    }

    pub(crate) fn into_finalization(
        self,
    ) -> Result<(PresentationPlan, PresentationPlanValidator<'a>), SceneBuildError> {
        let validator = PresentationPlanValidator::for_manifest_surface(
            self.workspace,
            self.version,
            self.manifest,
            self.policy,
            self.plan.surface(),
        )?
        .with_contained_projection(self.contained_projection)?;
        Ok((self.plan, validator))
    }
}

/// The host supplies measurements, while this constructor owns the complete
/// record roster, structural identities, junctions and magnetic-field recipe.
/// No mutable plan escapes between construction and geometry finalization.
pub(crate) fn compile_surface_plan<'a>(
    workspace: &'a Workspace,
    workspace_version: WorkspaceVersion,
    policy: &'a DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &'a SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    contained_projection: Option<ContainedLayoutProjection>,
) -> Result<CompiledSurfacePlan<'a>, PresentationCompilationError> {
    let plan = compile_surface_measurements_with_guide_emission(
        workspace,
        workspace_version,
        policy,
        config,
        manifest,
        measurements,
        tab_strip_states,
        resize_overrides,
        contained_projection,
        true,
    )?;
    Ok(CompiledSurfacePlan {
        plan,
        workspace,
        version: workspace_version,
        policy,
        manifest,
        contained_projection,
    })
}

#[cfg(test)]
pub(crate) fn compile_surface_measurements(
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    contained_projection: Option<ContainedLayoutProjection>,
) -> Result<PresentationPlan, PresentationCompilationError> {
    compile_surface_measurements_with_guide_emission(
        workspace,
        workspace_version,
        policy,
        config,
        manifest,
        measurements,
        tab_strip_states,
        resize_overrides,
        contained_projection,
        true,
    )
}

#[cfg(test)]
pub(crate) fn compile_surface_measurements_without_guides(
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    contained_projection: Option<ContainedLayoutProjection>,
) -> Result<PresentationPlan, PresentationCompilationError> {
    compile_surface_measurements_with_guide_emission(
        workspace,
        workspace_version,
        policy,
        config,
        manifest,
        measurements,
        tab_strip_states,
        resize_overrides,
        contained_projection,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_surface_measurements_with_guide_emission(
    workspace: &Workspace,
    workspace_version: WorkspaceVersion,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    manifest: &SceneRequirementManifest,
    measurements: &SurfaceMeasurements,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    contained_projection: Option<ContainedLayoutProjection>,
    emit_guides: bool,
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
        contained_projection,
        emit_guides,
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
    root_measurement: RootLayoutMeasurement<'_>,
    bounds: LogicalRect,
    layer: SceneLayerKey,
    fraction: DockFraction,
    tab_strip_states: &TabStripStateStore,
    resize_overrides: &[SplitWeightOverride<'_>],
    publish_presentation_menu_in_tab_bar: bool,
    emit_guides: bool,
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let root_record = workspace
        .root(root)
        .ok_or(SceneCompilationError::MissingRoot { surface, root })?;
    let RootLayoutMeasurement {
        pane_minimums,
        layout,
    } = root_measurement;
    let root_overrides = resize_overrides
        .iter()
        .copied()
        .filter(|override_| override_.source().root() == root)
        .collect::<Vec<_>>();
    let projection = Arc::new(layout.project_with_index(
        bounds,
        &root_overrides,
        workspace_index,
        workspace_version,
    )?);
    ready.push_root_layout_facts(RootLayoutFacts::new(
        root,
        bounds,
        pane_minimums,
        Arc::clone(&projection),
    ));
    let mut node_depths = BTreeMap::new();
    let mut pending_depths = vec![(root_record.node, 0_usize)];
    while let Some((node, depth)) = pending_depths.pop() {
        if node_depths.insert(node, depth).is_some() {
            continue;
        }
        match workspace
            .node(node)
            .ok_or(SceneCompilationError::MissingNode {
                surface,
                root,
                node,
            })? {
            Node::Tabs { .. } => {}
            Node::Split { children, .. } => pending_depths.extend(
                children
                    .iter()
                    .rev()
                    .copied()
                    .map(|child| (child, depth + 1)),
            ),
        }
    }
    let presentation_menu_tabs = publish_presentation_menu_in_tab_bar
        .then(|| {
            select_presentation_menu_tab_bar(
                workspace,
                requirements,
                measurements,
                config,
                surface,
                root,
                root_record.node,
                root_record.central,
                bounds,
                &projection.nodes,
                tab_strip_states,
            )
        })
        .transpose()?
        .flatten();
    let outer_guides = if emit_guides {
        outer_guide_geometry(
            config,
            bounds,
            DropGuideClusterId::outer(surface, root),
            layer,
            ready,
        )?
    } else {
        None
    };

    let tab_measurements = measurements.snapshot();
    for (node, projected) in &projection.nodes {
        let rect = clip_rect(projected.rect, bounds)?;
        match workspace
            .node(*node)
            .ok_or(SceneCompilationError::MissingNode {
                surface,
                root,
                node: *node,
            })? {
            Node::Tabs {
                items, selected, ..
            } => {
                let depth = *node_depths
                    .get(node)
                    .ok_or(SceneCompilationError::MissingNode {
                        surface,
                        root,
                        node: *node,
                    })?;
                compile_tabs_leaf(
                    policy,
                    config,
                    requirements,
                    &tab_measurements,
                    surface,
                    root,
                    *node,
                    depth,
                    rect,
                    items,
                    *selected,
                    layer,
                    fraction,
                    tab_strip_states,
                    root_record.node == *node && root_record.central == Some(*node),
                    presentation_menu_tabs == Some(*node),
                    outer_guides,
                    true,
                    ready,
                )?;
            }
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
                    split.children,
                    *axis,
                    config.splitter_hit_extent(),
                    bounds,
                )?;
                let mut resize_facts = None;
                let operable = policy.splitter_resize_is_allowed(*axis, surface);
                for (index, (splitter, hit)) in split.splitter_rects().zip(hit_rects).enumerate() {
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
                    let before = projection
                        .nodes
                        .get(&children[index])
                        .map(|result| &result.rect)
                        .ok_or(SceneCompilationError::MissingNodeProjection {
                            root,
                            node: children[index],
                        })?;
                    let after = projection
                        .nodes
                        .get(&children[index + 1])
                        .map(|result| &result.rect)
                        .ok_or(SceneCompilationError::MissingNodeProjection {
                            root,
                            node: children[index + 1],
                        })?;
                    let before_minimum_extent =
                        split.children.get(index).map(|child| child.minimum).ok_or(
                            SceneCompilationError::MissingSplitMinimum {
                                root,
                                split: *node,
                                child: children[index],
                            },
                        )?;
                    let after_minimum_extent = split
                        .children
                        .get(index + 1)
                        .map(|child| child.minimum)
                        .ok_or(SceneCompilationError::MissingSplitMinimum {
                            root,
                            split: *node,
                            child: children[index + 1],
                        })?;
                    let before_maximum_extent =
                        split.children.get(index).map(|child| child.maximum).ok_or(
                            SceneCompilationError::MissingSplitMaximum {
                                root,
                                split: *node,
                                child: children[index],
                            },
                        )?;
                    let after_maximum_extent = split
                        .children
                        .get(index + 1)
                        .map(|child| child.maximum)
                        .ok_or(SceneCompilationError::MissingSplitMaximum {
                            root,
                            split: *node,
                            child: children[index + 1],
                        })?;
                    let (child_extents, effective_weights) =
                        resize_facts.get_or_insert_with(|| {
                            let effective_weights = root_overrides
                                .iter()
                                .find(|override_| override_.source().node() == *node)
                                .map_or(weights.as_slice(), |override_| override_.weights());
                            (
                                split
                                    .children
                                    .iter()
                                    .map(|child| child.extent)
                                    .collect::<Arc<[f64]>>(),
                                Arc::<[SplitWeight]>::from(effective_weights),
                            )
                        });
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
                        Arc::clone(child_extents),
                        split.central_index,
                        Arc::clone(effective_weights),
                        layer,
                        operable,
                    ));
                    ready.push_surface_program_root_operation(
                        SurfaceProgramRootOperation::Splitter(id),
                    )?;
                }
            }
        }
    }

    if let Some(geometry) = outer_guides {
        let outer_guide = compile_outer_targets_and_guide(
            policy,
            geometry,
            surface,
            root,
            root_record.node,
            bounds,
            layer,
            fraction,
            ready,
        )?;
        ready.push_drop_guide_cluster(outer_guide);
    } else {
        compile_outer_targets_without_guide(
            policy,
            config,
            surface,
            root,
            root_record.node,
            bounds,
            layer,
            fraction,
            ready,
        )?;
    }
    let outer_target = |edge| DropTargetId::OuterEdge {
        surface,
        root,
        node: root_record.node,
        edge,
    };
    ready.push_magnetic_root_field(
        root,
        layer,
        bounds,
        MagneticEdgeTargets::new(
            outer_target(Edge::Left),
            outer_target(Edge::Right),
            outer_target(Edge::Top),
            outer_target(Edge::Bottom),
        ),
        config,
    );
    Ok(())
}

fn splitter_hit_rects(
    children: &[ChildProjection],
    axis: crate::graph::Axis,
    extent: f64,
    bounds: LogicalRect,
) -> Result<Vec<LogicalRect>, GeometryError> {
    children
        .iter()
        .enumerate()
        .filter_map(|(index, child)| child.following_gap.as_ref().map(|gap| (index, gap)))
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
            let cell_min = index
                .checked_sub(1)
                .and_then(|previous| children[previous].following_gap.as_ref())
                .map_or(axis_min, |previous| (coordinate(previous) + center) * 0.5);
            let cell_max = children
                .get(index + 1)
                .and_then(|child| child.following_gap.as_ref())
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

pub(super) fn clip_rect(
    left: LogicalRect,
    right: LogicalRect,
) -> Result<LogicalRect, GeometryError> {
    let min_x = left.x().max(right.x()).min(right.max().x());
    let min_y = left.y().max(right.y()).min(right.max().y());
    let max_x = left.max().x().min(right.max().x()).max(min_x);
    let max_y = left.max().y().min(right.max().y()).max(min_y);
    // Keep shared boundaries exact: rebuilding max from min + size can overlap
    // adjacent hit cells by a rounding unit.
    LogicalRect::from_min_max(
        LogicalPoint::new(min_x, min_y)?,
        LogicalPoint::new(max_x, max_y)?,
    )
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
    Ok(Some(LogicalRect::from_min_max(
        LogicalPoint::new(min_x, min_y)?,
        LogicalPoint::new(max_x, max_y)?,
    )?))
}

fn rect_has_area(rect: LogicalRect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0
}

pub(super) fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
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
