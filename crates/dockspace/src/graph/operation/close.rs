//! Frozen close preparation against one checked workspace candidate.

use super::*;

/// Core-private frozen payload for an approved content close.
///
/// Unlike [`WorkspaceCommand`], this type cannot be constructed by callers and
/// always carries the complete item multiset and policy capabilities captured
/// when the close plan was opened.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreparedContentClose {
    Item {
        source: ItemSource,
        expected_items: BTreeMap<ItemId, usize>,
        requirement: CloseItemRequirement,
    },
    Root {
        source: NodeSource,
        items: Vec<ItemId>,
        expected_items: BTreeMap<ItemId, usize>,
        requirements: Vec<CloseItemRequirement>,
    },
}

/// Core-private frozen payload for closing one complete surface's content.
///
/// Root entries are ordered as the optional main root followed by contained
/// roots in normative back-to-front roster order. Each entry carries the exact
/// root source and its deterministic subtree item order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedSurfaceContentClose {
    roster: SurfaceRosterSource,
    roots: Vec<(NodeSource, Vec<ItemId>)>,
    items: Vec<ItemId>,
    requirements: Vec<CloseItemRequirement>,
}

impl PreparedSurfaceContentClose {
    #[must_use]
    pub(crate) fn new(
        roster: SurfaceRosterSource,
        roots: Vec<(NodeSource, Vec<ItemId>)>,
        requirements: Vec<CloseItemRequirement>,
    ) -> Self {
        let items = roots
            .iter()
            .flat_map(|(_, items)| items.iter().copied())
            .collect();
        Self {
            roster,
            roots,
            items,
            requirements,
        }
    }

    pub(crate) const fn roster(&self) -> &SurfaceRosterSource {
        &self.roster
    }

    pub(crate) const fn surface(&self) -> SurfaceId {
        self.roster.surface()
    }

    pub(crate) fn roots(&self) -> &[(NodeSource, Vec<ItemId>)] {
        &self.roots
    }

    pub(crate) fn items(&self) -> &[ItemId] {
        &self.items
    }

    pub(crate) fn requirements(&self) -> &[CloseItemRequirement] {
        &self.requirements
    }
}

#[derive(Debug)]
pub(crate) struct PreparedContentCloseCommit {
    pub(crate) candidate: Workspace,
    pub(crate) outcome: CloseCommitOutcome,
}

/// Prepares one approved complete-surface content close against an isolated workspace candidate.
///
/// No topology is removed until every frozen roster, root, item, and policy fact
/// has been revalidated against the same candidate.
pub(crate) fn prepare_surface_content_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &PreparedSurfaceContentClose,
) -> Result<PreparedContentCloseCommit, TransactionError> {
    let (candidate, mut outcomes) = prepare_surface_content_closes(workspace, policy, &[prepared])?;
    Ok(PreparedContentCloseCommit {
        candidate,
        outcome: outcomes.remove(0),
    })
}

/// Applies one complete native-window content roster to a single isolated draft.
/// A rejected member discards the entire draft and every sibling mutation.
pub(crate) fn prepare_surface_content_closes(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &[&PreparedSurfaceContentClose],
) -> Result<(Workspace, Vec<CloseCommitOutcome>), TransactionError> {
    if prepared.iter().all(|prepared| prepared.items().is_empty()) {
        return Err(close_invariant("validate frozen close-surface item order"));
    }
    let mut surfaces = BTreeSet::new();
    for prepared in prepared {
        if !surfaces.insert(prepared.surface()) {
            return Err(close_invariant("validate duplicate close-window surface"));
        }
        validate_surface_content_close(workspace, policy, prepared)?;
    }
    let mut candidate = clone_workspace_draft(workspace);
    let mut expected_after = workspace.item_multiset();
    let mut outcomes = Vec::with_capacity(prepared.len());
    for prepared in prepared {
        for item in prepared.items() {
            subtract_closed_item(&mut expected_after, *item)?;
        }
        for (source, _) in prepared.roots() {
            retire_root(&mut candidate, source.root()).map_err(close_command_error)?;
        }
        outcomes.push(CloseCommitOutcome::SurfaceClosed {
            surface: prepared.surface(),
            items: prepared.items().to_vec(),
        });
    }
    prune_vacant_surfaces(&mut candidate);
    canonicalize_draft(&mut candidate).map_err(TransactionError::from)?;
    let actual_after = candidate.item_multiset();
    if actual_after != expected_after {
        return Err(TransactionError::ItemReconciliation {
            expected: expected_after,
            actual: actual_after,
        });
    }
    Ok((candidate, outcomes))
}

fn validate_surface_content_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &PreparedSurfaceContentClose,
) -> Result<(), TransactionError> {
    if !workspace.matches_surface_roster_source(prepared.roster()) {
        return Err(TransactionError::Precondition {
            index: 0,
            source: TransactionPreconditionError::StaleSurfaceRoster {
                surface: prepared.surface(),
            },
        });
    }
    evaluate_policy(
        policy,
        &DockPolicyRequest::CloseSurface {
            surface: prepared.surface(),
        },
    )
    .map_err(close_command_error)?;

    let roster_sources = prepared
        .roster()
        .main_source()
        .into_iter()
        .chain(
            prepared
                .roster()
                .contained()
                .iter()
                .map(crate::command::ContainedRootSource::source),
        )
        .collect::<Vec<_>>();
    if roster_sources.len() != prepared.roots().len()
        || roster_sources
            .iter()
            .zip(prepared.roots())
            .any(|(roster, (frozen, _))| *roster != frozen)
    {
        return Err(close_invariant("validate frozen close-surface root order"));
    }

    let roster_items = prepared
        .roots()
        .iter()
        .flat_map(|(_, items)| items.iter().copied())
        .collect::<Vec<_>>();
    if roster_items.as_slice() != prepared.items() {
        return Err(close_invariant("validate frozen close-surface item order"));
    }

    let requirement_items = prepared
        .requirements()
        .iter()
        .map(|requirement| requirement.item())
        .collect::<Vec<_>>();
    if requirement_items.as_slice() != prepared.items() {
        return Err(close_invariant(
            "validate frozen close-surface item requirements",
        ));
    }

    let mut frozen_roots = BTreeSet::new();
    let mut frozen_items = BTreeSet::new();
    for (source, items) in prepared.roots() {
        if !frozen_roots.insert(source.root()) {
            return Err(close_invariant(
                "validate duplicate frozen close-surface root",
            ));
        }
        for item in items {
            if !frozen_items.insert(*item) {
                return Err(close_invariant(
                    "validate duplicate frozen close-surface item",
                ));
            }
        }
    }
    for requirement in prepared.requirements() {
        validate_close_requirement(policy, *requirement)?;
    }
    for (source, frozen_items) in prepared.roots() {
        let (_, root_node) =
            validate_root_source(workspace, source).map_err(close_command_error)?;
        if workspace.collect_items_in_subtree(root_node) != *frozen_items {
            return Err(close_invariant(
                "validate frozen close-surface root item order",
            ));
        }
    }

    // The baseline is deliberately captured at apply time. A close plan owns
    // only its frozen source roster; unrelated surfaces may change while a
    // native close confirmation is pending.
    let before_items = workspace.item_multiset();
    for item in prepared.items() {
        if before_items.get(item) != Some(&1) {
            return Err(close_invariant(
                "validate unique frozen close-surface item ownership",
            ));
        }
    }
    Ok(())
}

/// Prepare one approved close against a fresh candidate workspace.
///
/// This is intentionally separate from the public command transaction. A
/// close changes the item multiset and is therefore only reachable through a
/// ClosePlan-approved payload.
pub(crate) fn prepare_content_close(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    prepared: &PreparedContentClose,
) -> Result<PreparedContentCloseCommit, TransactionError> {
    let expected_items = match prepared {
        PreparedContentClose::Item { expected_items, .. }
        | PreparedContentClose::Root { expected_items, .. } => expected_items,
    };
    let actual_items = workspace.item_multiset();
    if &actual_items != expected_items {
        return Err(TransactionError::ItemReconciliation {
            expected: expected_items.clone(),
            actual: actual_items,
        });
    }

    let mut candidate = clone_workspace_draft(workspace);
    let outcome = match prepared {
        PreparedContentClose::Item {
            source,
            requirement,
            ..
        } => {
            validate_close_requirement(policy, *requirement)?;
            validate_item_source(&candidate, source).map_err(close_command_error)?;
            let root = source.root();
            let item = source.item();
            let successor = match candidate.node(source.tabs()) {
                Some(Node::Tabs {
                    items, selected, ..
                }) if *selected == Some(item) => items
                    .iter()
                    .position(|entry| *entry == item)
                    .and_then(|index| items.get(index + 1).or_else(|| items[..index].last()))
                    .copied(),
                _ => None,
            };
            remove_item(&mut candidate, source.tabs(), item).map_err(close_command_error)?;
            if let Some(successor) = successor {
                select_tabs_item(&mut candidate, source.tabs(), successor)
                    .map_err(close_command_error)?;
            }
            cleanup_empty_source_root(&mut candidate, root).map_err(close_command_error)?;
            candidate
                .complete_root_change(root)
                .map_err(close_command_error)?;
            CloseCommitOutcome::ItemClosed { item, root }
        }
        PreparedContentClose::Root {
            source,
            items,
            requirements,
            ..
        } => {
            for requirement in requirements {
                validate_close_requirement(policy, *requirement)?;
            }
            let (root, root_node) =
                validate_root_source(&candidate, source).map_err(close_command_error)?;
            let current_items = candidate.collect_items_in_subtree(root_node);
            if current_items != *items {
                return Err(TransactionError::Command {
                    index: 0,
                    source: CommandError::Invariant {
                        stage: "validate frozen close-root item order",
                    },
                });
            }
            if items.is_empty() {
                return Err(TransactionError::Command {
                    index: 0,
                    source: CommandError::RootEmpty { root },
                });
            }
            retire_root(&mut candidate, root).map_err(close_command_error)?;
            CloseCommitOutcome::RootClosed {
                root,
                items: items.clone(),
            }
        }
    };

    let mut expected_after = expected_items.clone();
    match &outcome {
        CloseCommitOutcome::ItemClosed { item, .. } => {
            expected_after.remove(item);
        }
        CloseCommitOutcome::RootClosed { items, .. } => {
            for item in items {
                if expected_after.remove(item).is_none() {
                    return Err(TransactionError::Command {
                        index: 0,
                        source: CommandError::Invariant {
                            stage: "reconcile frozen close-root item multiset",
                        },
                    });
                }
            }
        }
        CloseCommitOutcome::SurfaceClosed { .. } => {
            return Err(close_invariant(
                "reject surface outcome from single-content close",
            ));
        }
    }
    prune_vacant_surfaces(&mut candidate);
    canonicalize_draft(&mut candidate).map_err(TransactionError::from)?;
    let actual_after = candidate.item_multiset();
    if actual_after != expected_after {
        return Err(TransactionError::ItemReconciliation {
            expected: expected_after,
            actual: actual_after,
        });
    }
    Ok(PreparedContentCloseCommit { candidate, outcome })
}

fn close_command_error(error: CommandError) -> TransactionError {
    TransactionError::Command {
        index: 0,
        source: error,
    }
}

fn close_invariant(stage: &'static str) -> TransactionError {
    close_command_error(CommandError::Invariant { stage })
}

fn subtract_closed_item(
    items: &mut BTreeMap<ItemId, usize>,
    item: ItemId,
) -> Result<(), TransactionError> {
    let Some(count) = items.get_mut(&item) else {
        return Err(close_invariant(
            "reconcile frozen close-surface item multiset",
        ));
    };
    if *count == 0 {
        return Err(close_invariant(
            "reconcile zero-count close-surface item multiset",
        ));
    }
    if *count == 1 {
        items.remove(&item);
    } else {
        *count -= 1;
    }
    Ok(())
}

fn validate_close_requirement(
    policy: &DockPolicySnapshot,
    requirement: CloseItemRequirement,
) -> Result<(), TransactionError> {
    evaluate_policy(
        policy,
        &DockPolicyRequest::ClosePane {
            item: requirement.item(),
            deferred: false,
        },
    )
    .map_err(close_command_error)?;
    let actual = policy.pane_close_capability(requirement.item());
    if actual != requirement.capability() {
        return Err(close_command_error(CommandError::CloseCapabilityChanged {
            item: requirement.item(),
            expected: requirement.capability(),
            actual,
        }));
    }
    if !actual.allows_close() {
        return Err(TransactionError::Command {
            index: 0,
            source: CommandError::Invariant {
                stage: "close policy disabled after plan capture",
            },
        });
    }
    Ok(())
}
