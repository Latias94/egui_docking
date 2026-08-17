use super::*;

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
    ready.install_layout_facts(config.clone(), measurements);
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
            let operable = candidate.id().splitters().into_iter().all(|splitter| {
                ready
                    .splitter_record(splitter)
                    .is_some_and(SplitterRecord::operable)
            });
            ready.push_splitter_junction_record(SplitterJunctionRecord::new(
                candidate.id(),
                HitRegion::new(candidate.hit),
                candidate.layer,
                operable,
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
    let pane_minimums = root_pane_minimum_measurements(requirements, measurements, root)?;
    let leaf_constraints = root_leaf_constraints(
        workspace,
        config,
        requirements,
        surface,
        root,
        bounds,
        &pane_minimums,
    )?;
    ready.push_root_layout_facts(RootLayoutFacts::new(root, bounds, pane_minimums));
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
pub(super) struct TabStripControlLayout {
    pub(super) rects: [Option<LogicalRect>; 3],
    pub(super) reserved_leading: f64,
    pub(super) reserved_trailing: f64,
}

pub(super) fn tab_strip_control_layout(
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
    surface: SurfaceId,
    root: RootId,
    bounds: LogicalRect,
    pane_minimums: &BTreeMap<PaneMinimumKey, LogicalSize>,
) -> Result<BTreeMap<NodeId, Constraints>, SceneCompilationError> {
    let minimums = root_leaf_minimums(
        workspace,
        config,
        requirements,
        surface,
        root,
        pane_minimums,
    )?;
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
    surface: SurfaceId,
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

pub(super) fn pane_outer_minimum(
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
    let pane_minimums = root_pane_minimum_measurements(requirements, measurements, root)?;
    let minimums = root_leaf_minimums(
        workspace,
        config,
        requirements,
        surface,
        root,
        &pane_minimums,
    )?;
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

pub(super) fn subtree_minimum(
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

pub(super) fn edge_preview(
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
