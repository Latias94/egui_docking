//! Validation and canonicalization of complete semantic presentation plans.

use super::*;
use crate::workspace::WorkspaceIndexView;

/// Reusable validator for one complete surface presentation plan.
///
/// The validator reuses one workspace-derived index when the caller already has
/// a requirement manifest, while retaining the standalone constructor used by
/// direct scene and drop-resolution tests.
pub(crate) struct PresentationPlanValidator<'a> {
    workspace: &'a Workspace,
    workspace_index: SceneWorkspaceIndex<'a>,
    policy: &'a DockPolicySnapshot,
}

impl<'a> PresentationPlanValidator<'a> {
    pub(crate) fn new(
        workspace: &'a Workspace,
        policy: &'a DockPolicySnapshot,
    ) -> Result<Self, SceneBuildError> {
        Ok(Self {
            workspace,
            workspace_index: SceneWorkspaceIndex::new(workspace)?,
            policy,
        })
    }

    pub(crate) fn for_manifest_surface(
        workspace: &'a Workspace,
        workspace_version: WorkspaceVersion,
        manifest: &'a SceneRequirementManifest,
        policy: &'a DockPolicySnapshot,
        surface: SurfaceId,
    ) -> Result<Self, SceneBuildError> {
        let actual = manifest.workspace_index().version();
        let Some(workspace_index) = manifest.workspace_index().at_version(workspace_version) else {
            return Err(SceneBuildError::WorkspaceIndexVersionMismatch {
                expected: workspace_version,
                actual,
            });
        };
        if manifest.surface(surface).is_none() {
            return Err(SceneBuildError::UnexpectedSceneSurface { surface });
        }
        Ok(Self {
            workspace,
            workspace_index: SceneWorkspaceIndex::from_manifest_surface(
                workspace,
                workspace_index,
                surface,
            )?,
            policy,
        })
    }

    /// Validates and canonicalizes one plan without publishing partial state.
    pub(crate) fn validate_and_canonicalize(
        &self,
        mut ready: PresentationPlan,
    ) -> Result<PresentationPlan, SceneBuildError> {
        let surface = ready.surface;
        if let Some(expected) = self.workspace_index.bound_surface()
            && expected != surface
        {
            return Err(SceneBuildError::PresentationSurfaceMismatch {
                expected,
                actual: surface,
            });
        }
        validate_semantic_uniqueness(&ready)?;
        validate_popup_plane(&ready)?;
        if !rect_has_area(ready.bounds) {
            return Err(SceneBuildError::EmptyReadySurfaceBounds { surface });
        }
        validate_surface_background(&ready, self.workspace, &self.workspace_index)?;
        if ready.measurement_ticket().is_some() {
            validate_ready_semantics(&ready, self.workspace, &self.workspace_index, self.policy)?;
        }
        let published_occlusions: HashSet<_> = ready
            .drop_occlusions
            .iter()
            .map(|occlusion| occlusion.floating())
            .collect();
        for floating in self.workspace_index.contained_on_surface(surface) {
            if !published_occlusions.contains(&floating) {
                return Err(SceneBuildError::MissingDropOcclusion { surface, floating });
            }
        }
        for occlusion in &ready.drop_occlusions {
            let floating = occlusion.floating();
            if !self
                .workspace_index
                .contained_belongs_to_surface(surface, floating)
            {
                return Err(SceneBuildError::InvalidDropOcclusion { surface, floating });
            }
            let expected = self
                .workspace_index
                .contained_layer(floating)
                .ok_or(SceneBuildError::InvalidDropOcclusion { surface, floating })?;
            if occlusion.layer() != expected {
                return Err(SceneBuildError::DropOcclusionLayerMismatch {
                    surface,
                    floating,
                    expected,
                    actual: occlusion.layer(),
                });
            }
            let region = occlusion.region().rect();
            if !rect_has_area(region) {
                return Err(SceneBuildError::EmptyDropOcclusionRegion { surface, floating });
            }
            let expected = self
                .workspace_index
                .contained_rect(floating)
                .ok_or(SceneBuildError::InvalidDropOcclusion { surface, floating })?;
            if region != expected {
                return Err(SceneBuildError::DropOcclusionGeometryMismatch {
                    surface,
                    floating,
                    expected,
                    actual: region,
                });
            }
        }
        for target in &ready.drop_targets {
            validate_drop_target(&ready, target, self.workspace, &self.workspace_index)?;
        }
        for cluster in &ready.drop_guide_clusters {
            validate_drop_guide_cluster(&ready, cluster, self.workspace, &self.workspace_index)?;
        }
        validate_contained_minimums(&ready, &self.workspace_index)?;
        canonicalize_ready(
            &mut ready,
            self.workspace,
            &self.workspace_index,
            self.policy,
        );
        Ok(ready)
    }
}

fn validate_popup_plane(ready: &PresentationPlan) -> Result<(), SceneBuildError> {
    let backdrops = &ready.tab_list_menu_backdrop_records;
    match ready.popup {
        PopupPlaneRequirement::Inactive { .. } => {
            if ready.popup_plane_bounds.is_some()
                || !backdrops.is_empty()
                || !ready.tab_list_menu_records.is_empty()
            {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
        }
        PopupPlaneRequirement::Active {
            revision,
            session,
            owner,
        } => {
            let Some(popup_plane_bounds) = ready.popup_plane_bounds else {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            };
            if backdrops.len() != 1
                || backdrops[0].session() != session
                || backdrops[0].revision() != revision
                || !rect_has_area(popup_plane_bounds)
                || backdrops[0].bounds() != popup_plane_bounds
            {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
            let owner_surface = owner.surface() == ready.surface;
            if owner_surface {
                if ready.tab_list_menu_records.len() != 1
                    || ready.tab_list_menu_records[0].session() != session
                    || ready.tab_list_menu_records[0].bar() != owner.bar()
                {
                    return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                        surface: ready.surface,
                    });
                }
            } else if !ready.tab_list_menu_records.is_empty() {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
        }
    }
    Ok(())
}

fn validate_semantic_uniqueness(ready: &PresentationPlan) -> Result<(), SceneBuildError> {
    fn stable_duplicate<T: Copy + Ord>(values: impl IntoIterator<Item = T>) -> Option<T> {
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for value in values {
            if !seen.insert(value) {
                duplicates.insert(value);
            }
        }
        duplicates.into_iter().next()
    }

    if let Some(id) = stable_duplicate(ready.pane_records.iter().map(PaneRecord::id)) {
        return Err(SceneBuildError::DuplicatePane { id });
    }
    if let Some(id) = stable_duplicate(ready.tab_bar_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateTabBar { id });
    }
    if let Some(id) = stable_duplicate(ready.tab_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateTab { id });
    }
    if let Some(id) = stable_duplicate(ready.splitter_gap_records.iter().map(SplitterGapRecord::id))
    {
        return Err(SceneBuildError::DuplicateSplitterGap { id });
    }
    if let Some(id) = stable_duplicate(ready.splitter_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateSplitter { id });
    }
    if let Some(id) = stable_duplicate(
        ready
            .splitter_junction_records
            .iter()
            .map(SplitterJunctionRecord::id),
    ) {
        return Err(SceneBuildError::DuplicateSplitterJunction { id });
    }
    if let Some(floating) = stable_duplicate(
        ready
            .contained_records
            .iter()
            .map(ContainedRecord::floating),
    ) {
        return Err(SceneBuildError::DuplicateContainedRecord { floating });
    }
    if let Some(floating) = stable_duplicate(
        ready
            .contained_minimums
            .iter()
            .map(|measurement| measurement.floating()),
    ) {
        return Err(SceneBuildError::DuplicateContainedMinimumMeasurement { floating });
    }
    if let Some(floating) =
        stable_duplicate(ready.drop_occlusions.iter().map(|record| record.floating()))
    {
        return Err(SceneBuildError::DuplicateDropOcclusion { floating });
    }
    if let Some(id) = stable_duplicate(
        ready
            .drop_guide_clusters
            .iter()
            .map(DropGuideClusterRecord::id),
    ) {
        return Err(SceneBuildError::DuplicateDropGuideCluster { id });
    }
    let targets = ready
        .surface_background
        .iter()
        .map(DropTargetRecord::id)
        .chain(ready.drop_targets.iter().map(DropTargetRecord::id))
        .chain(
            ready
                .drop_guide_clusters
                .iter()
                .flat_map(|cluster| cluster.targets().map(|(_, guide_target)| guide_target.id())),
        );
    if let Some(id) = stable_duplicate(targets) {
        return Err(SceneBuildError::DuplicateDropTarget { id });
    }
    Ok(())
}

fn validate_contained_minimums(
    ready: &PresentationPlan,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let surface = ready.surface;
    let published: HashSet<_> = ready
        .contained_minimums
        .iter()
        .map(|measurement| measurement.floating())
        .collect();
    if let Some(floating) = ready
        .contained_minimums
        .iter()
        .map(|measurement| measurement.floating())
        .filter(|floating| !workspace_index.contained_belongs_to_surface(surface, *floating))
        .min()
    {
        return Err(SceneBuildError::UnexpectedContainedMinimumMeasurement { surface, floating });
    }
    for floating in workspace_index.contained_on_surface(surface) {
        if !published.contains(&floating) {
            return Err(SceneBuildError::MissingContainedMinimumMeasurement { surface, floating });
        }
    }
    Ok(())
}

fn validate_surface_background(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let Some(presentation) = workspace.surface(ready.surface) else {
        if let Some(background) = &ready.surface_background {
            return Err(SceneBuildError::UnexpectedSurfaceBackground {
                surface: ready.surface,
                target: background.id(),
            });
        }
        return Ok(());
    };

    match (presentation.main_root, &ready.surface_background) {
        (None, None) => {
            return Err(SceneBuildError::MissingSurfaceBackground {
                surface: ready.surface,
            });
        }
        (Some(_), Some(background)) => {
            return Err(SceneBuildError::UnexpectedSurfaceBackground {
                surface: ready.surface,
                target: background.id(),
            });
        }
        (Some(_), None) => return Ok(()),
        (None, Some(_)) => {}
    }

    let background =
        ready
            .surface_background
            .as_ref()
            .ok_or(SceneBuildError::MissingSurfaceBackground {
                surface: ready.surface,
            })?;
    if background.id().surface() != ready.surface
        || !background.semantics_match()
        || background.availability() != DropTargetAvailability::Available
        || !matches!(
            background.destination(),
            DropDestination::SurfaceBackground(destination)
                if destination.surface() == ready.surface
        )
    {
        return Err(SceneBuildError::InvalidSurfaceBackgroundRecord {
            surface: ready.surface,
            target: background.id(),
        });
    }
    if background.layer() != SceneLayerKey::surface_base()
        || workspace_index
            .contained_layers(ready.surface)
            .any(|layer| background.layer() >= layer)
    {
        return Err(SceneBuildError::SurfaceBackgroundLayerMismatch {
            surface: ready.surface,
            actual: background.layer(),
        });
    }
    if !rect_has_area(background.region().rect())
        || !rect_contains(ready.bounds, background.region().rect())
    {
        return Err(SceneBuildError::InvalidSurfaceBackgroundHitRegion {
            surface: ready.surface,
        });
    }
    validate_drop_visual(ready, background)
}

fn validate_ready_semantics(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) -> Result<(), SceneBuildError> {
    validate_contained_records(ready, workspace, workspace_index)?;
    let expected_panes = expected_pane_records(ready, workspace, workspace_index)?;
    validate_pane_and_tab_records(ready, workspace, workspace_index, policy, &expected_panes)?;
    validate_splitter_records(ready, workspace, workspace_index, policy)?;
    Ok(())
}

fn validate_contained_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let expected = workspace
        .surface(ready.surface)
        .into_iter()
        .flat_map(|presentation| presentation.contained.iter().copied().enumerate())
        .filter_map(|(ordinal, floating)| {
            let indexed = workspace_index.contained_records().get(&floating)?;
            rects_overlap_with_area(ready.bounds, indexed.rect).then_some((
                floating,
                ordinal,
                indexed.root,
                indexed.rect,
                indexed.layer,
            ))
        })
        .collect::<Vec<_>>();
    if ready.contained_records.len() != expected.len() {
        return Err(SceneBuildError::ContainedRecordSetMismatch {
            surface: ready.surface,
        });
    }
    let minimums = ready
        .contained_minimums
        .iter()
        .map(|measurement| (measurement.floating(), measurement.minimum_size()))
        .collect::<HashMap<_, _>>();
    const DIRECTIONS: [ContainedResizeDirection; 8] = [
        ContainedResizeDirection::NorthWest,
        ContainedResizeDirection::North,
        ContainedResizeDirection::NorthEast,
        ContainedResizeDirection::East,
        ContainedResizeDirection::SouthEast,
        ContainedResizeDirection::South,
        ContainedResizeDirection::SouthWest,
        ContainedResizeDirection::West,
    ];
    for (record, (floating, ordinal, root, durable, layer)) in
        ready.contained_records.iter().zip(expected)
    {
        let valid_identity = record.floating() == floating
            && record.ordinal() == ordinal
            && record.root() == root
            && record.layer() == layer
            && minimums.get(&floating).copied() == Some(record.minimum_size());
        let valid_partition =
            rect_is_exact_intersection(record.outer_bounds(), ready.bounds, durable)
                && rect_has_area(record.outer_bounds())
                && rect_contains(record.outer_bounds(), record.inner_bounds())
                && rect_contains(record.inner_bounds(), record.title_bounds())
                && rect_contains(record.inner_bounds(), record.content_bounds())
                && rect_contains(record.title_bounds(), record.title_drag_hit().rect())
                && !rects_overlap_with_area(record.title_bounds(), record.content_bounds())
                && record.close_bounds().is_none_or(|close| {
                    rect_has_area(close)
                        && rect_contains(record.title_bounds(), close)
                        && !rects_overlap_with_area(close, record.title_drag_hit().rect())
                });
        let valid_resize =
            record
                .resize()
                .iter()
                .zip(DIRECTIONS)
                .all(|(resize, direction)| {
                    resize.direction() == direction
                        && rect_contains(record.outer_bounds(), resize.hit().rect())
                })
                && record.resize().iter().enumerate().all(|(index, first)| {
                    record.resize().iter().skip(index + 1).all(|second| {
                        !rects_overlap_with_area(first.hit().rect(), second.hit().rect())
                    })
                });
        if !valid_identity || !valid_partition || !valid_resize {
            return Err(SceneBuildError::InvalidContainedRecord {
                surface: ready.surface,
                floating: record.floating(),
            });
        }
    }
    Ok(())
}

fn expected_pane_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<BTreeMap<PaneSceneId, (Option<ItemId>, SceneLayerKey)>, SceneBuildError> {
    let mut roots = Vec::new();
    if let Some(root) = workspace
        .surface(ready.surface)
        .and_then(|presentation| presentation.main_root)
    {
        roots.push(root);
    }
    roots.extend(
        ready
            .contained_records
            .iter()
            .filter(|record| rect_has_area(record.content_bounds()))
            .map(ContainedRecord::root),
    );
    let mut expected = BTreeMap::new();
    for root in roots {
        let layer =
            workspace_index
                .root_layer(root)
                .ok_or(SceneBuildError::CompiledRootUnavailable {
                    surface: ready.surface,
                    root,
                })?;
        let mut pending = vec![workspace_index.root_node(root).ok_or(
            SceneBuildError::CompiledRootUnavailable {
                surface: ready.surface,
                root,
            },
        )?];
        while let Some(node) = pending.pop() {
            match workspace.node(node) {
                Some(Node::Tabs { selected, .. }) => {
                    expected.insert(PaneSceneId { root, tabs: node }, (*selected, layer));
                }
                Some(Node::Split { children, .. }) => {
                    pending.extend(children.iter().rev().copied());
                }
                None => {
                    return Err(SceneBuildError::CompiledRootUnavailable {
                        surface: ready.surface,
                        root,
                    });
                }
            }
        }
    }
    Ok(expected)
}

fn validate_pane_and_tab_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
    expected_panes: &BTreeMap<PaneSceneId, (Option<ItemId>, SceneLayerKey)>,
) -> Result<(), SceneBuildError> {
    let panes = ready
        .pane_records
        .iter()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    if panes.len() != expected_panes.len() {
        return Err(SceneBuildError::PaneRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, (selected, layer)) in expected_panes {
        let Some(record) = panes.get(id).copied() else {
            return Err(SceneBuildError::PaneRecordSetMismatch {
                surface: ready.surface,
            });
        };
        if record.selected() != *selected
            || record.layer() != *layer
            || !rect_contains(ready.bounds, record.bounds())
            || !rect_contains(record.bounds(), record.content_bounds())
        {
            return Err(SceneBuildError::InvalidPaneRecord {
                surface: ready.surface,
                id: *id,
            });
        }
    }

    let pane_policies = expected_panes
        .iter()
        .map(|(id, (selected, _))| {
            let target = selected
                .map(|_| workspace.capture_tab_target(id.root, id.tabs))
                .transpose()
                .map_err(|_| SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                })?;
            if target
                .as_ref()
                .is_some_and(|target| target.surface() != ready.surface)
            {
                return Err(SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                });
            }
            Ok((
                *id,
                policy.tab_bar_policy(DockTabBarPolicyRequest::new(
                    ready.surface,
                    target.as_ref().map(|target| target.rule()),
                )),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, SceneBuildError>>()?;
    let bars = ready
        .tab_bar_records
        .iter()
        .map(|record| (*record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let mut controls_by_bar = BTreeMap::<TabBarSceneId, BTreeMap<TabStripControlId, _>>::new();
    for control in &ready.tab_strip_control_records {
        let bar = control.id().bar();
        if controls_by_bar
            .entry(bar)
            .or_default()
            .insert(control.id(), control)
            .is_some()
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar,
            });
        }
    }
    let mut menus_by_bar = BTreeMap::new();
    for menu in &ready.tab_list_menu_records {
        if menus_by_bar.insert(menu.bar(), menu).is_some() {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: menu.bar(),
            });
        }
    }
    if ready.tab_list_menu_records.len() > 1 {
        let id = ready.tab_list_menu_records[0].bar();
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    if controls_by_bar.keys().any(|id| !bars.contains_key(id))
        || menus_by_bar.keys().any(|id| !bars.contains_key(id))
    {
        let id = controls_by_bar
            .keys()
            .chain(menus_by_bar.keys())
            .find(|id| !bars.contains_key(id))
            .copied()
            .expect("a foreign tab-strip record was detected");
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    let expected_bars = panes
        .iter()
        .filter(|(id, pane)| {
            rect_has_area(pane.bounds())
                && pane_policies
                    .get(id)
                    .is_some_and(|policy| policy.visibility() == TabBarVisibility::Visible)
        })
        .count();
    if bars.len() != expected_bars {
        return Err(SceneBuildError::TabBarRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, pane) in &panes {
        let bar_id = TabBarSceneId {
            root: id.root,
            tabs: id.tabs,
        };
        let tab_bar_policy =
            pane_policies
                .get(id)
                .copied()
                .ok_or(SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                })?;
        if !rect_has_area(pane.bounds()) || tab_bar_policy.visibility() == TabBarVisibility::Hidden
        {
            if bars.contains_key(&bar_id) {
                return Err(SceneBuildError::InvalidTabBarRecord {
                    surface: ready.surface,
                    id: bar_id,
                });
            }
            continue;
        }
        let Some(bar) = bars.get(&bar_id).copied() else {
            return Err(SceneBuildError::TabBarRecordSetMismatch {
                surface: ready.surface,
            });
        };
        if bar.layer() != pane.layer()
            || bar.interaction() != tab_bar_policy.interaction()
            || !rect_has_area(bar.bounds())
            || !rect_contains(pane.bounds(), bar.bounds())
            || !rect_contains(bar.bounds(), bar.viewport())
            || !bar.scroll_offset().is_finite()
            || !bar.maximum_scroll_offset().is_finite()
            || bar.scroll_offset() < 0.0
            || bar.maximum_scroll_offset() < 0.0
            || bar.scroll_offset() > bar.maximum_scroll_offset()
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
    }

    let tabs_by_bar = ready.tab_records.iter().fold(
        BTreeMap::<TabBarSceneId, Vec<&TabRecord>>::new(),
        |mut grouped, record| {
            grouped
                .entry(TabBarSceneId {
                    root: record.id().root,
                    tabs: record.id().tabs,
                })
                .or_default()
                .push(record);
            grouped
        },
    );
    if tabs_by_bar.keys().any(|id| !bars.contains_key(id)) {
        return Err(SceneBuildError::TabRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (bar_id, bar) in bars {
        let Some(Node::Tabs { items, selected }) = workspace.node(bar_id.tabs) else {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        };
        let group_grip_matches = match bar.group_grip_bounds() {
            None => items.is_empty(),
            Some(grip) => {
                !items.is_empty()
                    && rect_has_area(grip)
                    && grip.x() == bar.bounds().x()
                    && grip.y() == bar.bounds().y()
                    && grip.height() == bar.bounds().height()
                    && rect_contains(bar.bounds(), grip)
                    && !rects_overlap_with_area(grip, bar.viewport())
            }
        };
        let group_interaction_matches = match bar.interaction() {
            TabBarInteraction::Disabled => bar.group_drag().is_none(),
            TabBarInteraction::Enabled => match (bar.group_grip_bounds(), bar.group_drag()) {
                (None, None) => items.is_empty(),
                (Some(grip), Some(group)) => {
                    group.grip_bounds() == grip
                        && group.hit().rect() == grip
                        && rect_has_area(group.hit().rect())
                }
                _ => false,
            },
        };
        if !group_grip_matches || !group_interaction_matches {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
        let visible = tabs_by_bar.get(&bar_id).map_or(&[][..], Vec::as_slice);
        let visible_items = visible
            .iter()
            .map(|record| record.id().item)
            .collect::<HashSet<_>>();
        let expected_hidden = items
            .iter()
            .copied()
            .filter(|item| !visible_items.contains(item))
            .collect::<Vec<_>>();
        if bar.hidden_items() != expected_hidden {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
        let members_match = bar.members().len() == items.len()
            && bar.members().iter().enumerate().all(|(ordinal, member)| {
                let tab = member.tab();
                let visible_record = visible.iter().find(|record| *record.id() == tab);
                let visibility_matches = match (member.visibility(), visible_record) {
                    (TabStripMemberVisibility::Visible, Some(record)) => {
                        member.full_bounds() == record.full_bounds()
                            && record.full_bounds() == record.visible_bounds()
                    }
                    (TabStripMemberVisibility::PartiallyVisible, Some(record)) => {
                        member.full_bounds() == record.full_bounds()
                            && record.full_bounds() != record.visible_bounds()
                    }
                    (TabStripMemberVisibility::PartiallyVisible, None) => true,
                    (TabStripMemberVisibility::Hidden, None) => true,
                    (TabStripMemberVisibility::Visible, None)
                    | (TabStripMemberVisibility::Hidden, Some(_)) => false,
                };
                let full = member.full_bounds();
                let preceding_edge_matches = if ordinal == 0 {
                    full.x() == bar.viewport().x() - bar.scroll_offset()
                } else {
                    bar.members()[ordinal - 1].full_bounds().max().x() == full.x()
                };
                member.ordinal() == ordinal
                    && tab.root == bar_id.root
                    && tab.tabs == bar_id.tabs
                    && items.get(ordinal).copied() == Some(tab.item)
                    && rect_has_area(full)
                    && full.y() == bar.bounds().y()
                    && full.height() == bar.bounds().height()
                    && preceding_edge_matches
                    && visibility_matches
            });
        if !members_match {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }

        validate_tab_strip_controls_and_menu(
            ready,
            bar,
            items,
            *selected,
            controls_by_bar.get(&bar_id),
            menus_by_bar.get(&bar_id).copied(),
        )?;
        for record in visible {
            let id = *record.id();
            let ordinal_matches = items.get(record.ordinal()).copied() == Some(id.item);
            let close_allowed = policy.pane_close_capability(id.item).allows_close();
            let close_visual_matches = match (close_allowed, record.close_visual_bounds()) {
                (false, None) => true,
                (true, Some(close)) => {
                    rect_has_area(close) && rect_contains(record.visible_bounds(), close)
                }
                _ => false,
            };
            let interaction_geometry_matches = match bar.interaction() {
                TabBarInteraction::Enabled => {
                    rect_has_area(record.drag_hit().rect())
                        && rect_contains(record.visible_bounds(), record.drag_hit().rect())
                        && record.close_bounds() == record.close_visual_bounds()
                        && record.close_bounds().is_none_or(|close| {
                            !rects_overlap_with_area(close, record.drag_hit().rect())
                        })
                }
                TabBarInteraction::Disabled => {
                    !rect_has_area(record.drag_hit().rect()) && record.close_bounds().is_none()
                }
            };
            let geometry_matches = rect_has_area(record.visible_bounds())
                && rect_is_exact_intersection(
                    record.visible_bounds(),
                    record.full_bounds(),
                    bar.viewport(),
                )
                && rect_contains(record.visible_bounds(), record.text_bounds())
                && close_visual_matches
                && interaction_geometry_matches;
            if id.root != bar_id.root
                || id.tabs != bar_id.tabs
                || !ordinal_matches
                || record.selected() != (*selected == Some(id.item))
                || record.layer() != bar.layer()
                || !geometry_matches
                || !workspace_index.semantic_node_exists(ready.surface, id.root, id.tabs)
            {
                return Err(SceneBuildError::InvalidTabRecord {
                    surface: ready.surface,
                    id,
                });
            }
        }
    }
    Ok(())
}

fn validate_tab_strip_controls_and_menu(
    ready: &PresentationPlan,
    bar: &TabBarRecord,
    items: &[ItemId],
    selected: Option<ItemId>,
    controls: Option<&BTreeMap<TabStripControlId, &TabStripControlRecord>>,
    menu: Option<&TabListMenuRecord>,
) -> Result<(), SceneBuildError> {
    let id = *bar.id();
    let expected_control_ids = TabStripControlId::for_bar(id);
    if let Some(controls) = controls {
        if controls.is_empty()
            || controls.len() > expected_control_ids.len()
            || controls
                .keys()
                .any(|control| !expected_control_ids.contains(control))
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
        let records = controls.values().copied().collect::<Vec<_>>();
        let valid_geometry = records.iter().all(|control| {
            rect_has_area(control.bounds())
                && control.hit().rect() == control.bounds()
                && rect_contains(bar.bounds(), control.bounds())
                && control.layer() == bar.layer()
        }) && records.iter().enumerate().all(|(index, control)| {
            records[index + 1..]
                .iter()
                .all(|other| !rects_overlap_with_area(control.bounds(), other.bounds()))
        });
        let interaction_enabled = bar.interaction() == TabBarInteraction::Enabled;
        let valid_enablement = records.iter().all(|control| {
            let expected = match control.id() {
                TabStripControlId::ScrollBackward(_) => {
                    interaction_enabled && bar.scroll_offset() > 0.0
                }
                TabStripControlId::ScrollForward(_) => {
                    interaction_enabled && bar.scroll_offset() < bar.maximum_scroll_offset()
                }
                TabStripControlId::TabListMenu(_) => {
                    interaction_enabled && bar.menu_geometry_availability().is_available()
                }
            };
            control.enabled() == expected
        });
        let menu_control_present = controls.contains_key(&TabStripControlId::TabListMenu(id));
        if !valid_geometry || !valid_enablement || bar.maximum_scroll_offset() <= 0.0 {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
        if (bar.menu_geometry_availability().is_available() && !menu_control_present)
            || (menu.is_some() && !menu_control_present)
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
    } else if menu.is_some() || bar.menu_geometry_availability().is_available() {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }

    let Some(menu) = menu else {
        return Ok(());
    };
    let Some(popup_plane_bounds) = ready.popup_plane_bounds else {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    };
    let menu_control = controls
        .and_then(|controls| controls.get(&TabStripControlId::TabListMenu(id)))
        .copied();
    let valid_menu = bar.interaction() == TabBarInteraction::Enabled
        && bar.menu_geometry_availability().is_available()
        && menu.layer() == bar.layer()
        && menu.session().key() == TabStripStateKey::new(ready.surface, id)
        && menu_control.is_some_and(|control| rect_contains(popup_plane_bounds, control.bounds()))
        && rect_has_area(menu.bounds())
        && rect_contains(popup_plane_bounds, menu.bounds())
        && rect_has_area(menu.viewport())
        && rect_contains(menu.bounds(), menu.viewport())
        && menu.scroll_offset().is_finite()
        && menu.maximum_scroll_offset().is_finite()
        && menu.scroll_offset() >= 0.0
        && menu.maximum_scroll_offset() >= 0.0
        && menu.scroll_offset() <= menu.maximum_scroll_offset()
        && menu.rows().len() == items.len();
    if !valid_menu {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }

    let mut focused = 0_usize;
    for (ordinal, row) in menu.rows().iter().copied().enumerate() {
        let tab = row.tab();
        focused += usize::from(row.focused());
        let hit_matches = match row.hit() {
            Some(hit) => {
                rect_has_area(hit.rect())
                    && rect_contains(row.bounds(), hit.rect())
                    && rect_contains(menu.viewport(), hit.rect())
                    && rect_is_exact_intersection(hit.rect(), row.bounds(), menu.viewport())
            }
            None => !rects_overlap_with_area(row.bounds(), menu.viewport()),
        };
        if row.ordinal() != ordinal
            || items.get(ordinal).copied() != Some(tab.item)
            || tab.root != id.root
            || tab.tabs != id.tabs
            || !rect_has_area(row.bounds())
            || row.bounds().x() != menu.viewport().x()
            || row.bounds().width() != menu.viewport().width()
            || row.selected() != (selected == Some(tab.item))
            || !hit_matches
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
    }
    if focused != 1 {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    Ok(())
}

pub(super) fn splitter_extent_satisfies_constraints(
    extent: Option<f64>,
    minimum: f64,
    maximum: f64,
) -> bool {
    let Some(extent) = extent else {
        return false;
    };
    if !extent.is_finite()
        || extent < 0.0
        || !minimum.is_finite()
        || minimum < 0.0
        || !maximum.is_finite()
        || maximum < minimum
    {
        return false;
    }
    let tolerance =
        f64::EPSILON * extent.abs().max(minimum.abs()).max(maximum.abs()).max(1.0) * 16.0;
    (extent >= minimum || minimum - extent <= tolerance)
        && (extent <= maximum || extent - maximum <= tolerance)
}

fn validate_splitter_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) -> Result<(), SceneBuildError> {
    let splitters = ready
        .splitter_records
        .iter()
        .map(|record| (*record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let splitter_gaps = ready
        .splitter_gap_records
        .iter()
        .map(|record| (record.id(), record.presentation()))
        .collect::<BTreeMap<_, _>>();
    let compiled_roots = ready
        .pane_records
        .iter()
        .map(|pane| pane.id().root)
        .collect::<BTreeSet<_>>();
    let mut expected_splitters = BTreeSet::new();
    for root in compiled_roots {
        let Some(root_record) = workspace.root(root) else {
            return Err(SceneBuildError::CompiledRootUnavailable {
                surface: ready.surface,
                root,
            });
        };
        let mut pending = vec![root_record.node];
        let mut visited = BTreeSet::new();
        while let Some(node) = pending.pop() {
            if !visited.insert(node) {
                continue;
            }
            match workspace.node(node) {
                Some(Node::Tabs { .. }) => {}
                Some(Node::Split { children, .. }) => {
                    expected_splitters.extend((0..children.len().saturating_sub(1)).map(|index| {
                        SplitterSceneId {
                            root,
                            split: node,
                            index,
                        }
                    }));
                    pending.extend(children.iter().rev().copied());
                }
                None => {
                    return Err(SceneBuildError::CompiledRootUnavailable {
                        surface: ready.surface,
                        root,
                    });
                }
            }
        }
    }
    if splitter_gaps.keys().copied().collect::<BTreeSet<_>>() != expected_splitters {
        return Err(SceneBuildError::SplitterGapRecordSetMismatch {
            surface: ready.surface,
        });
    }
    let rendered_splitters = splitter_gaps
        .iter()
        .filter_map(|(id, presentation)| {
            (*presentation == SplitterGapPresentation::Rendered).then_some(*id)
        })
        .collect::<BTreeSet<_>>();
    if splitters.keys().copied().collect::<BTreeSet<_>>() != rendered_splitters {
        return Err(SceneBuildError::SplitterRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, record) in &splitters {
        let Some(Node::Split { axis, children, .. }) = workspace.node(id.split) else {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        };
        let weight_sum = record
            .weights()
            .iter()
            .map(|weight| f64::from(weight.get()))
            .sum::<f64>();
        let before_extent = record.child_extents().get(id.index).copied();
        let after_extent = record.child_extents().get(id.index + 1).copied();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.split)
            || id.index >= children.len().saturating_sub(1)
            || record.axis() != *axis
            || record.weights().len() != children.len()
            || !weight_sum.is_finite()
            || (weight_sum - 1.0).abs() > 1.0e-5
            || !rect_has_area(record.draw_bounds())
            || !rect_has_area(record.hit().rect())
            || !rect_contains(ready.bounds, record.hit().rect())
            || !rect_contains(record.hit().rect(), record.draw_bounds())
            || !rect_contains(ready.bounds, record.before_bounds())
            || !rect_contains(ready.bounds, record.after_bounds())
            || !record.before_minimum_extent().is_finite()
            || record.before_minimum_extent() < 0.0
            || !record.after_minimum_extent().is_finite()
            || record.after_minimum_extent() < 0.0
            || !splitter_extent_satisfies_constraints(
                before_extent,
                record.before_minimum_extent(),
                record.before_maximum_extent(),
            )
            || !splitter_extent_satisfies_constraints(
                after_extent,
                record.after_minimum_extent(),
                record.after_maximum_extent(),
            )
            || record.child_extents().len() != children.len()
            || record
                .child_extents()
                .iter()
                .any(|extent| !extent.is_finite() || *extent < 0.0)
            || record
                .central_index()
                .is_some_and(|index| index >= children.len())
        {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        }
        validate_semantic_layer(ready.surface, id.root, record.layer(), workspace_index)?;
        if record.operable()
            != (policy.splitter_resize_is_allowed(record.axis(), ready.surface)
                && region_has_authoritative_area(
                    record.hit().rect(),
                    record.layer(),
                    &ready.drop_occlusions,
                ))
        {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        }
    }

    let expected_junctions = derive_splitter_junction_candidates(splitters.values().copied())
        .map_err(|source| SceneBuildError::InvalidSplitterJunctionGeometry {
            surface: ready.surface,
            source,
        })?
        .into_iter()
        .filter(|candidate| {
            region_has_authoritative_area(candidate.hit, candidate.layer, &ready.drop_occlusions)
        })
        .map(|candidate| (candidate.id(), candidate))
        .collect::<BTreeMap<_, _>>();
    if ready.splitter_junction_records.len() != expected_junctions.len() {
        return Err(SceneBuildError::SplitterJunctionRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for junction in &ready.splitter_junction_records {
        let Some(expected) = expected_junctions.get(&junction.id()).copied() else {
            return Err(SceneBuildError::InvalidSplitterJunctionRecord {
                surface: ready.surface,
                id: junction.id(),
            });
        };
        if junction.layer() != expected.layer
            || !rect_has_area(junction.hit().rect())
            || junction.hit().rect() != expected.hit
        {
            return Err(SceneBuildError::InvalidSplitterJunctionRecord {
                surface: ready.surface,
                id: junction.id(),
            });
        }
    }
    Ok(())
}

fn validate_semantic_layer(
    surface: SurfaceId,
    root: RootId,
    actual: SceneLayerKey,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let expected =
        workspace_index
            .root_layer(root)
            .ok_or(SceneBuildError::SemanticLayerMismatch {
                surface,
                root,
                expected: SceneLayerKey::surface_base(),
                actual,
            })?;
    if actual != expected {
        return Err(SceneBuildError::SemanticLayerMismatch {
            surface,
            root,
            expected,
            actual,
        });
    }
    Ok(())
}

fn canonicalize_ready(
    ready: &mut PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) {
    ready.pane_records.sort_unstable_by_key(PaneRecord::id);
    ready
        .tab_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .tab_bar_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .tab_strip_control_records
        .sort_unstable_by_key(|record| record.id());
    ready
        .tab_list_menu_records
        .sort_unstable_by_key(TabListMenuRecord::session);
    ready
        .splitter_gap_records
        .sort_unstable_by_key(SplitterGapRecord::id);
    ready
        .splitter_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .splitter_junction_records
        .sort_unstable_by_key(SplitterJunctionRecord::id);
    ready
        .contained_records
        .sort_unstable_by_key(|record| record.ordinal());
    ready
        .contained_minimums
        .sort_unstable_by_key(|measurement| measurement.floating());
    ready
        .drop_occlusions
        .sort_unstable_by_key(|occlusion| occlusion.floating());
    ready
        .drop_guide_clusters
        .sort_unstable_by_key(DropGuideClusterRecord::id);
    ready
        .drop_targets
        .sort_unstable_by_key(DropTargetRecord::id);

    let surface = ready.surface;
    for target in &mut ready.drop_targets {
        canonicalize_drop_target(target, workspace, workspace_index, surface, policy);
    }
    for cluster in &mut ready.drop_guide_clusters {
        cluster.for_each_target_mut(|_, guide_target| {
            canonicalize_drop_target(
                guide_target.target_mut(),
                workspace,
                workspace_index,
                surface,
                policy,
            );
        });
    }
}

fn canonicalize_drop_target(
    target: &mut DropTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    surface: SurfaceId,
    policy: &DockPolicySnapshot,
) {
    if !target.availability().is_available() {
        return;
    }
    if !target_reference_is_current(workspace, workspace_index, target)
        || !target_structure_is_valid(workspace, workspace_index, surface, target)
    {
        target.set_availability(DropTargetAvailability::Unavailable(
            DropTargetUnavailable::Stale,
        ));
        return;
    }
    let allowed = match target.id().kind() {
        DropTargetKind::TabGap | DropTargetKind::Center => policy.allows_tab_merge(),
        DropTargetKind::InnerEdge | DropTargetKind::OuterEdge => policy.allows_edge_split(),
        DropTargetKind::SurfaceBackground => true,
    };
    if !allowed {
        target.set_availability(DropTargetAvailability::Unavailable(
            DropTargetUnavailable::PolicyDisabled,
        ));
    }
}

fn validate_drop_guide_cluster(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let cluster_id = cluster.id();
    validate_drop_guide_cluster_identity(ready, cluster_id, workspace, workspace_index)?;
    let expected_layer = workspace_index.root_layer(cluster_id.root).ok_or(
        SceneBuildError::InvalidDropGuideCluster {
            surface: ready.surface,
            cluster: cluster_id,
        },
    )?;
    if cluster.layer() != expected_layer {
        return Err(SceneBuildError::DropGuideClusterLayerMismatch {
            surface: ready.surface,
            cluster: cluster_id,
            expected: expected_layer,
            actual: cluster.layer(),
        });
    }
    let activation = validate_drop_guide_activation(ready, cluster)?;
    for (slot, guide_target) in cluster.targets() {
        validate_drop_guide_target(
            ready,
            cluster,
            activation,
            slot,
            guide_target,
            workspace,
            workspace_index,
        )?;
    }
    validate_drop_guide_hit_separation(ready.surface, cluster)
}

fn validate_drop_guide_cluster_identity(
    ready: &PresentationPlan,
    cluster: DropGuideClusterId,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let scope_is_valid = match cluster.scope {
        DropGuideScope::Inner(node) => {
            workspace_index.semantic_node_exists(ready.surface, cluster.root, node)
                && matches!(workspace.node(node), Some(Node::Tabs { .. }))
        }
        DropGuideScope::Outer => {
            workspace_index.root_belongs_to_surface(ready.surface, cluster.root)
                && workspace_index
                    .root_node(cluster.root)
                    .and_then(|node| workspace.node(node))
                    .is_some()
        }
    };
    if cluster.surface != ready.surface || !scope_is_valid {
        return Err(SceneBuildError::InvalidDropGuideCluster {
            surface: ready.surface,
            cluster,
        });
    }
    Ok(())
}

fn validate_drop_guide_activation(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
) -> Result<LogicalRect, SceneBuildError> {
    let cluster_id = cluster.id();
    let activation = cluster.activation().rect();
    if !rect_has_area(activation) {
        return Err(SceneBuildError::EmptyDropGuideActivation {
            surface: ready.surface,
            cluster: cluster_id,
        });
    }
    if !rect_contains(ready.bounds, activation) {
        return Err(SceneBuildError::DropGuideActivationOutsideSurface {
            surface: ready.surface,
            cluster: cluster_id,
        });
    }
    Ok(activation)
}

fn validate_drop_guide_target(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
    activation: LogicalRect,
    slot: DropGuideSlot,
    guide_target: &DropGuideTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let cluster_id = cluster.id();
    let target = guide_target.target();
    if !drop_guide_slot_matches(cluster_id, slot, target.id(), workspace, workspace_index) {
        return Err(SceneBuildError::DropGuideTargetSlotMismatch {
            cluster: cluster_id,
            slot,
            target: target.id(),
        });
    }
    if target.layer() != cluster.layer() {
        return Err(SceneBuildError::DropGuideTargetLayerMismatch {
            cluster: cluster_id,
            target: target.id(),
            cluster_layer: cluster.layer(),
            target_layer: target.layer(),
        });
    }

    let hit = target.region().rect();
    if !rect_has_area(hit) {
        return Err(SceneBuildError::EmptyDropGuideHitRegion {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(ready.bounds, hit) {
        return Err(SceneBuildError::DropGuideHitRegionOutsideSurface {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(activation, hit) {
        return Err(SceneBuildError::DropGuideHitRegionOutsideActivation {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }

    let draw = guide_target.draw();
    if !rect_has_area(draw) {
        return Err(SceneBuildError::EmptyDropGuideDraw {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(hit, draw) {
        return Err(SceneBuildError::DropGuideDrawOutsideHitRegion {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }

    validate_drop_target(ready, target, workspace, workspace_index)?;
    let preview = target.visual().rect();
    if !rect_contains(activation, preview) {
        return Err(SceneBuildError::DropGuidePreviewOutsideActivation {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    Ok(())
}

fn validate_drop_guide_hit_separation(
    surface: SurfaceId,
    cluster: &DropGuideClusterRecord,
) -> Result<(), SceneBuildError> {
    for (index, (first_slot, first)) in cluster.targets().enumerate() {
        for (second_slot, second) in cluster.targets().skip(index + 1) {
            if rects_overlap_with_area(
                first.target().region().rect(),
                second.target().region().rect(),
            ) {
                return Err(SceneBuildError::OverlappingDropGuideHitRegions {
                    surface,
                    cluster: cluster.id(),
                    first: first_slot,
                    second: second_slot,
                });
            }
        }
    }
    Ok(())
}

fn drop_guide_slot_matches(
    cluster: DropGuideClusterId,
    slot: DropGuideSlot,
    target: DropTargetId,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> bool {
    match (cluster.scope, slot) {
        (DropGuideScope::Inner(node), DropGuideSlot::Center) => {
            target
                == (DropTargetId::Center {
                    surface: cluster.surface,
                    root: cluster.root,
                    tabs: node,
                })
        }
        (DropGuideScope::Inner(node), DropGuideSlot::Edge(edge)) => {
            let is_root_central = workspace_index.root_node(cluster.root) == Some(node)
                && workspace
                    .root(cluster.root)
                    .is_some_and(|root| root.central == Some(node));
            !is_root_central
                && target
                    == (DropTargetId::InnerEdge {
                        surface: cluster.surface,
                        root: cluster.root,
                        node,
                        edge,
                    })
        }
        (DropGuideScope::Outer, DropGuideSlot::Center) => false,
        (DropGuideScope::Outer, DropGuideSlot::Edge(edge)) => {
            workspace_index.root_node(cluster.root).is_some_and(|node| {
                target
                    == (DropTargetId::OuterEdge {
                        surface: cluster.surface,
                        root: cluster.root,
                        node,
                        edge,
                    })
            })
        }
    }
}

fn validate_drop_target(
    ready: &PresentationPlan,
    target: &DropTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    if matches!(target.destination(), DropDestination::SurfaceBackground(_)) {
        return Err(SceneBuildError::SurfaceBackgroundInTopologyTargets {
            surface: ready.surface,
        });
    }
    validate_drop_visual(ready, target)?;
    let root =
        target_root(target.destination()).ok_or(SceneBuildError::TargetSemanticMismatch {
            surface: ready.surface,
            target: target.id(),
        })?;
    let expected =
        workspace_index
            .root_layer(root)
            .ok_or(SceneBuildError::TargetSemanticMismatch {
                surface: ready.surface,
                target: target.id(),
            })?;
    if target.layer() != expected {
        return Err(SceneBuildError::DropTargetLayerMismatch {
            surface: ready.surface,
            target: target.id(),
            expected,
            actual: target.layer(),
        });
    }
    if target.id().surface() != ready.surface
        || !target.semantics_match()
        || (target_reference_is_current(workspace, workspace_index, target)
            && !target_structure_is_valid(workspace, workspace_index, ready.surface, target))
    {
        return Err(SceneBuildError::TargetSemanticMismatch {
            surface: ready.surface,
            target: target.id(),
        });
    }
    Ok(())
}

fn rect_has_area(rect: LogicalRect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0
}

fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    let outer_min = outer.min();
    let outer_max = outer.max();
    let inner_min = inner.min();
    let inner_max = inner.max();
    inner_min.x() >= outer_min.x()
        && inner_min.y() >= outer_min.y()
        && inner_max.x() <= outer_max.x()
        && inner_max.y() <= outer_max.y()
}

fn rects_overlap_with_area(left: LogicalRect, right: LogicalRect) -> bool {
    left.min().x().max(right.min().x()) < left.max().x().min(right.max().x())
        && left.min().y().max(right.min().y()) < left.max().y().min(right.max().y())
}

pub(super) fn region_has_authoritative_area(
    region: LogicalRect,
    layer: SceneLayerKey,
    occlusions: &[DropOcclusionRecord],
) -> bool {
    let occluders = occlusions
        .iter()
        .filter(|occlusion| occlusion.layer() > layer)
        .map(|occlusion| occlusion.region().rect())
        .filter(|occlusion| rects_overlap_with_area(region, *occlusion))
        .collect::<Vec<_>>();
    if occluders.is_empty() {
        return true;
    }

    let mut x_edges = vec![region.x(), region.max().x()];
    for occluder in &occluders {
        x_edges.push(occluder.x().max(region.x()));
        x_edges.push(occluder.max().x().min(region.max().x()));
    }
    x_edges.sort_by(|left, right| left.total_cmp(right));
    x_edges.dedup_by(|left, right| left.total_cmp(right).is_eq());

    for strip in x_edges.windows(2) {
        let [strip_min, strip_max] = [strip[0], strip[1]];
        if strip_min >= strip_max {
            continue;
        }
        let mut intervals = occluders
            .iter()
            .filter(|occluder| occluder.x() <= strip_min && occluder.max().x() >= strip_max)
            .map(|occluder| {
                (
                    occluder.y().max(region.y()),
                    occluder.max().y().min(region.max().y()),
                )
            })
            .collect::<Vec<_>>();
        intervals.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
        });

        let mut covered_to = region.y();
        for (minimum, maximum) in intervals {
            if minimum > covered_to {
                return true;
            }
            covered_to = covered_to.max(maximum);
            if covered_to >= region.max().y() {
                break;
            }
        }
        if covered_to < region.max().y() {
            return true;
        }
    }
    false
}

fn rect_is_exact_intersection(
    actual: LogicalRect,
    first: LogicalRect,
    second: LogicalRect,
) -> bool {
    let min_x = first.x().max(second.x());
    let min_y = first.y().max(second.y());
    let max_x = first.max().x().min(second.max().x());
    let max_y = first.max().y().min(second.max().y());
    max_x > min_x
        && max_y > min_y
        && actual.x() == min_x
        && actual.y() == min_y
        && actual.max().x() == max_x
        && actual.max().y() == max_y
}

fn target_reference_is_current(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    record: &DropTargetRecord,
) -> bool {
    match record.destination() {
        DropDestination::Topology(
            DockTarget::Center(target) | DockTarget::TabGap { target, .. },
        ) => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.tabs())
                && matches!(workspace.node(target.tabs()), Some(Node::Tabs { .. }))
        }
        DropDestination::Topology(
            DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target),
        ) => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.node())
        }
        DropDestination::SurfaceBackground(_) => true,
    }
}

fn target_structure_is_valid(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    surface: SurfaceId,
    record: &DropTargetRecord,
) -> bool {
    let Some(root) = target_root(record.destination()) else {
        return false;
    };
    if !workspace_index.root_belongs_to_surface(surface, root) {
        return false;
    }
    match (record.id(), record.destination()) {
        (
            DropTargetId::TabGap { .. },
            DropDestination::Topology(DockTarget::TabGap { target, index }),
        ) => matches!(
            workspace.node(target.tabs()),
            Some(Node::Tabs { items, .. }) if *index <= items.len()
        ),
        (
            DropTargetId::OuterEdge { root, node, .. },
            DropDestination::Topology(DockTarget::OuterEdge(_)),
        ) => workspace_index.root_node(root) == Some(node),
        (
            DropTargetId::InnerEdge { .. },
            DropDestination::Topology(DockTarget::InnerEdge(target)),
        ) => {
            matches!(workspace.node(target.node()), Some(Node::Tabs { .. }))
        }
        _ => true,
    }
}

fn validate_drop_visual(
    ready: &PresentationPlan,
    target: &DropTargetRecord,
) -> Result<(), SceneBuildError> {
    let visual = target.visual().rect();
    if visual.width() <= 0.0 || visual.height() <= 0.0 {
        return Err(SceneBuildError::EmptyDropVisual {
            surface: ready.surface,
            target: target.id(),
        });
    }
    let bounds_min = ready.bounds.min();
    let bounds_max = ready.bounds.max();
    let visual_min = visual.min();
    let visual_max = visual.max();
    if visual_min.x() < bounds_min.x()
        || visual_min.y() < bounds_min.y()
        || visual_max.x() > bounds_max.x()
        || visual_max.y() > bounds_max.y()
    {
        return Err(SceneBuildError::DropVisualOutsideSurface {
            surface: ready.surface,
            target: target.id(),
        });
    }
    Ok(())
}

fn target_root(target: &DropDestination) -> Option<RootId> {
    match target {
        DropDestination::Topology(
            DockTarget::Center(target) | DockTarget::TabGap { target, .. },
        ) => Some(target.root()),
        DropDestination::Topology(
            DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target),
        ) => Some(target.root()),
        DropDestination::SurfaceBackground(_) => None,
    }
}

struct IndexedRoot {
    owner: RootPresentationOwner,
    layer: SceneLayerKey,
    root_node: NodeId,
    nodes: HashSet<NodeId>,
    fingerprint: Option<NodeFingerprint>,
}

#[derive(Debug, Clone, Copy)]
struct IndexedContained {
    surface: SurfaceId,
    root: RootId,
    rect: LogicalRect,
    layer: SceneLayerKey,
}

enum SceneWorkspaceIndex<'a> {
    Complete(CompleteSceneWorkspaceIndex),
    SharedSurface(SharedSurfaceSceneWorkspaceIndex<'a>),
}

struct CompleteSceneWorkspaceIndex {
    roots: HashMap<RootId, IndexedRoot>,
    contained: HashMap<FloatingPresentationId, IndexedContained>,
    contained_by_surface: HashMap<SurfaceId, Vec<FloatingPresentationId>>,
}

struct SharedSurfaceSceneWorkspaceIndex<'a> {
    workspace_index: WorkspaceIndexView<'a>,
    surface: SurfaceId,
    contained: HashMap<FloatingPresentationId, IndexedContained>,
    contained_roster: Vec<FloatingPresentationId>,
}

impl<'a> SceneWorkspaceIndex<'a> {
    fn new(workspace: &Workspace) -> Result<Self, SceneBuildError> {
        let mut root_presentations = HashMap::with_capacity(workspace.roots().count());
        let mut contained = HashMap::with_capacity(workspace.contained_floatings().count());
        let mut contained_by_surface = HashMap::with_capacity(workspace.surfaces().count());
        for (surface, presentation) in workspace.surfaces() {
            if let Some(main_root) = presentation.main_root {
                root_presentations.insert(
                    main_root,
                    (
                        RootPresentationOwner::Main { surface },
                        SceneLayerKey::surface_base(),
                    ),
                );
            }
            for (index, floating) in presentation.contained.iter().copied().enumerate() {
                let layer = SceneLayerKey::contained(index)
                    .ok_or(SceneBuildError::ContainedLayerCapacityExceeded { surface, floating })?;
                if let Some(record) = workspace.contained_floating(floating) {
                    let owner = RootPresentationOwner::Contained { surface, floating };
                    root_presentations.insert(record.root, (owner, layer));
                    contained.insert(
                        floating,
                        IndexedContained {
                            surface,
                            root: record.root,
                            rect: record.rect,
                            layer,
                        },
                    );
                }
            }
            contained_by_surface.insert(surface, presentation.contained.clone());
        }

        let mut roots = HashMap::with_capacity(root_presentations.len());
        for (root, record) in workspace.roots() {
            let Some((owner, layer)) = root_presentations.get(&root).copied() else {
                continue;
            };
            let mut nodes = HashSet::new();
            let mut stack = vec![record.node];
            while let Some(node) = stack.pop() {
                if !nodes.insert(node) {
                    continue;
                }
                if let Some(Node::Split { children, .. }) = workspace.node(node) {
                    stack.extend(children.iter().copied());
                }
            }
            let fingerprint = workspace
                .capture_node_source(root, record.node)
                .ok()
                .map(|source| source.fingerprint().clone());
            roots.insert(
                root,
                IndexedRoot {
                    owner,
                    layer,
                    root_node: record.node,
                    nodes,
                    fingerprint,
                },
            );
        }
        Ok(Self::Complete(CompleteSceneWorkspaceIndex {
            roots,
            contained,
            contained_by_surface,
        }))
    }

    fn from_manifest_surface(
        workspace: &Workspace,
        workspace_index: WorkspaceIndexView<'a>,
        surface: SurfaceId,
    ) -> Result<Self, SceneBuildError> {
        let presentation = workspace
            .surface(surface)
            .ok_or(SceneBuildError::UnexpectedSceneSurface { surface })?;
        let mut contained = HashMap::with_capacity(presentation.contained.len());
        let mut contained_roster = Vec::with_capacity(presentation.contained.len());
        for (index, floating) in presentation.contained.iter().copied().enumerate() {
            let layer = SceneLayerKey::contained(index)
                .ok_or(SceneBuildError::ContainedLayerCapacityExceeded { surface, floating })?;
            contained_roster.push(floating);
            if let Some(record) = workspace.contained_floating(floating) {
                contained.insert(
                    floating,
                    IndexedContained {
                        surface,
                        root: record.root,
                        rect: record.rect,
                        layer,
                    },
                );
            }
        }
        Ok(Self::SharedSurface(SharedSurfaceSceneWorkspaceIndex {
            workspace_index,
            surface,
            contained,
            contained_roster,
        }))
    }

    fn bound_surface(&self) -> Option<SurfaceId> {
        match self {
            Self::Complete(_) => None,
            Self::SharedSurface(index) => Some(index.surface),
        }
    }

    fn root_belongs_to_surface(&self, surface: SurfaceId, root: RootId) -> bool {
        self.root_owner(root).is_some_and(|owner| match owner {
            RootPresentationOwner::Main {
                surface: owner_surface,
            }
            | RootPresentationOwner::Contained {
                surface: owner_surface,
                ..
            } => owner_surface == surface,
        })
    }

    fn contains_node(&self, root: RootId, node: NodeId) -> bool {
        match self {
            Self::Complete(index) => index
                .roots
                .get(&root)
                .is_some_and(|record| record.nodes.contains(&node)),
            Self::SharedSurface(index) => index.workspace_index.root_contains_node(root, node),
        }
    }

    fn semantic_node_exists(&self, surface: SurfaceId, root: RootId, node: NodeId) -> bool {
        self.root_belongs_to_surface(surface, root) && self.contains_node(root, node)
    }

    fn fingerprint_is_current(&self, root: RootId, expected: &NodeFingerprint) -> bool {
        match self {
            Self::Complete(index) => index
                .roots
                .get(&root)
                .and_then(|record| record.fingerprint.as_ref())
                .is_some_and(|current| current == expected),
            Self::SharedSurface(index) => {
                index.workspace_index.fingerprint_is_current(root, expected)
            }
        }
    }

    fn root_node(&self, root: RootId) -> Option<NodeId> {
        match self {
            Self::Complete(index) => index.roots.get(&root).map(|record| record.root_node),
            Self::SharedSurface(index) => index.workspace_index.root_node(root),
        }
    }

    fn root_layer(&self, root: RootId) -> Option<SceneLayerKey> {
        match self {
            Self::Complete(index) => index.roots.get(&root).map(|record| record.layer),
            Self::SharedSurface(index) => match index.workspace_index.root_owner(root)? {
                RootPresentationOwner::Main {
                    surface: owner_surface,
                } if owner_surface == index.surface => Some(SceneLayerKey::surface_base()),
                RootPresentationOwner::Contained {
                    surface: owner_surface,
                    floating,
                } if owner_surface == index.surface => {
                    index.contained.get(&floating).map(|record| record.layer)
                }
                _ => None,
            },
        }
    }

    fn root_owner(&self, root: RootId) -> Option<RootPresentationOwner> {
        match self {
            Self::Complete(index) => index.roots.get(&root).map(|record| record.owner),
            Self::SharedSurface(index) => index.workspace_index.root_owner(root),
        }
    }

    fn contained_belongs_to_surface(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    ) -> bool {
        self.contained_records()
            .get(&floating)
            .is_some_and(|record| {
                record.surface == surface
                    && self.root_owner(record.root)
                        == Some(RootPresentationOwner::Contained { surface, floating })
            })
    }

    fn contained_layer(&self, floating: FloatingPresentationId) -> Option<SceneLayerKey> {
        self.contained_records()
            .get(&floating)
            .map(|record| record.layer)
    }

    fn contained_rect(&self, floating: FloatingPresentationId) -> Option<LogicalRect> {
        self.contained_records()
            .get(&floating)
            .map(|record| record.rect)
    }

    fn contained_layers(&self, surface: SurfaceId) -> impl Iterator<Item = SceneLayerKey> + '_ {
        self.contained_records()
            .values()
            .filter(move |record| record.surface == surface)
            .map(|record| record.layer)
    }

    fn contained_on_surface(
        &self,
        surface: SurfaceId,
    ) -> impl Iterator<Item = FloatingPresentationId> + '_ {
        let roster = match self {
            Self::Complete(index) => index
                .contained_by_surface
                .get(&surface)
                .map_or(&[] as &[FloatingPresentationId], Vec::as_slice),
            Self::SharedSurface(index) if index.surface == surface => &index.contained_roster,
            Self::SharedSurface(_) => &[] as &[FloatingPresentationId],
        };
        roster.iter().copied()
    }

    fn contained_records(&self) -> &HashMap<FloatingPresentationId, IndexedContained> {
        match self {
            Self::Complete(index) => &index.contained,
            Self::SharedSurface(index) => &index.contained,
        }
    }
}
