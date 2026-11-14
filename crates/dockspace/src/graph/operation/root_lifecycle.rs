//! Root ownership changes and complete destruction of empty presentations.

use super::*;

/// Examines the completed content transfer, never its temporarily empty donor.
/// An already detached root has no remaining source tree to clean up.
pub(super) fn cleanup_empty_source_root(
    workspace: &mut Workspace,
    root: RootId,
) -> Result<Option<SurfaceId>, CommandError> {
    let Some(record) = workspace.root(root).copied() else {
        return Ok(None);
    };
    if workspace.retains_empty_central_root(root) || workspace.subtree_has_items(record.node) {
        return Ok(None);
    }
    retire_root(workspace, root).map(|owner| Some(owner.surface()))
}

/// Detaches ownership while preserving the subtree for a transfer.
pub(super) fn detach_root(
    workspace: &mut Workspace,
    root: RootId,
) -> Result<(RootRecord, RootPresentationOwner), CommandError> {
    let presentation = workspace
        .presentation_for_root(root)
        .ok_or(CommandError::Invariant {
            stage: "locate root presentation owner",
        })?;
    detach_root_presentation(workspace, root, presentation)?;
    let record = Arc::make_mut(&mut workspace.storage.roots)
        .remove(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    workspace.unroot_subtree(record.node);
    workspace.complete_root_change(root)?;
    Ok((record, presentation))
}

/// Destroys a root, its presentation and every remaining node together.
pub(super) fn retire_root(
    workspace: &mut Workspace,
    root: RootId,
) -> Result<RootPresentationOwner, CommandError> {
    let (record, owner) = detach_root(workspace, root)?;
    remove_subtree_nodes(workspace, record.node)?;
    Ok(owner)
}

fn remove_subtree_nodes(workspace: &mut Workspace, root: NodeId) -> Result<(), CommandError> {
    let mut stack = vec![root];
    while let Some(node_id) = stack.pop() {
        let node = workspace
            .remove_runtime_node(node_id)
            .ok_or(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: node_id,
            })?;
        if let Node::Split { children, .. } = node.as_ref() {
            stack.extend(children.iter().rev().copied());
        }
    }
    Ok(())
}

pub(super) fn detach_root_presentation(
    workspace: &mut Workspace,
    root: RootId,
    presentation: RootPresentationOwner,
) -> Result<(), CommandError> {
    match presentation {
        RootPresentationOwner::Main { surface } => {
            if workspace
                .storage
                .surfaces
                .get(&surface)
                .map(|entry| entry.main_root)
                != Some(Some(root))
            {
                return Err(CommandError::Invariant {
                    stage: "detach matching main-root presentation",
                });
            }
            workspace
                .storage
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?
                .main_root = None;
        }
        RootPresentationOwner::Contained { surface, floating } => {
            let record = workspace
                .storage
                .contained_floatings
                .remove(&floating)
                .ok_or(CommandError::MissingFloating { floating })?;
            if record.root != root {
                return Err(CommandError::Invariant {
                    stage: "detach matching contained-root presentation",
                });
            }
            let presentation = workspace
                .storage
                .surfaces
                .get_mut(&surface)
                .ok_or(CommandError::MissingSurface { surface })?;
            let index = presentation
                .contained
                .iter()
                .position(|entry| *entry == floating)
                .ok_or(CommandError::Invariant {
                    stage: "remove contained presentation backlink",
                })?;
            presentation.contained.remove(index);
        }
    }
    Ok(())
}
