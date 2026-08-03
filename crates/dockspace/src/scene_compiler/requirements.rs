use super::*;

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
                if items.is_empty() {
                    debug_assert!(selected.is_none());
                    pane_minimums.insert(PaneMinimumKey::new(root, node, None));
                } else {
                    pane_minimums.extend(
                        items
                            .iter()
                            .copied()
                            .map(|item| PaneMinimumKey::new(root, node, Some(item))),
                    );
                }
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
