//! Rebase retired presentation identities without serialization or item remapping.

use std::collections::BTreeMap;

use crate::graph::Workspace;
use crate::ids::{PresentationIdentityFrontier, SurfaceId};
use crate::validation::WorkspaceValidationErrors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceRebaseEntity {
    Surface,
    Root,
    Floating,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WorkspaceRebaseError {
    #[error("replacement {kind:?} identity space is exhausted")]
    Exhausted { kind: WorkspaceRebaseEntity },
    #[error("replacement {kind:?} reference {id} has no rebase entry")]
    MissingReference {
        kind: WorkspaceRebaseEntity,
        id: u64,
    },
    #[error("replacement workspace is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
}

pub(crate) struct RebasedWorkspace {
    pub(crate) workspace: Workspace,
    #[cfg(feature = "serde")]
    pub(crate) frontier: PresentationIdentityFrontier,
    pub(crate) surfaces: BTreeMap<SurfaceId, SurfaceId>,
}

pub(crate) fn rebase_workspace_presentation_identities(
    mut workspace: Workspace,
    active: &Workspace,
    active_frontier: PresentationIdentityFrontier,
    imported_frontier: Option<PresentationIdentityFrontier>,
) -> Result<RebasedWorkspace, WorkspaceRebaseError> {
    use WorkspaceRebaseEntity::{Floating, Root, Surface};

    let mut frontier = active_frontier;
    if let Some(imported) = imported_frontier {
        frontier.merge(imported);
    }
    // Reserve incoming fresh names before allocating replacements for retired ones.
    for (surface, _) in workspace.surfaces() {
        frontier.observe_surface(surface);
    }
    for (root, _) in workspace.roots() {
        frontier.observe_root(root);
    }
    for (floating, _) in workspace.contained_floatings() {
        frontier.observe_floating(floating);
    }

    let mut surfaces = BTreeMap::new();
    for (id, _) in workspace.surfaces() {
        let mapped = if active.surface(id).is_some() || id.get() > active_frontier.last_surface() {
            id
        } else {
            frontier
                .reserve_surface()
                .ok_or(WorkspaceRebaseError::Exhausted { kind: Surface })?
        };
        surfaces.insert(id, mapped);
    }
    let mut roots = BTreeMap::new();
    for (id, _) in workspace.roots() {
        let mapped = if active.root(id).is_some() || id.get() > active_frontier.last_root() {
            id
        } else {
            frontier
                .reserve_root()
                .ok_or(WorkspaceRebaseError::Exhausted { kind: Root })?
        };
        roots.insert(id, mapped);
    }
    let mut floatings = BTreeMap::new();
    for (id, _) in workspace.contained_floatings() {
        let mapped = if active.contained_floating(id).is_some()
            || id.get() > active_frontier.last_floating()
        {
            id
        } else {
            frontier
                .reserve_floating()
                .ok_or(WorkspaceRebaseError::Exhausted { kind: Floating })?
        };
        floatings.insert(id, mapped);
    }

    workspace.roots = std::mem::take(&mut workspace.roots)
        .into_iter()
        .map(|(id, root)| {
            let mapped = roots
                .get(&id)
                .copied()
                .ok_or(WorkspaceRebaseError::MissingReference {
                    kind: Root,
                    id: id.get(),
                })?;
            Ok((mapped, root))
        })
        .collect::<Result<_, WorkspaceRebaseError>>()?;
    workspace.surfaces = std::mem::take(&mut workspace.surfaces)
        .into_iter()
        .map(|(id, mut presentation)| {
            let mapped =
                surfaces
                    .get(&id)
                    .copied()
                    .ok_or(WorkspaceRebaseError::MissingReference {
                        kind: Surface,
                        id: id.get(),
                    })?;
            presentation.main_root = presentation
                .main_root
                .map(|root| {
                    roots
                        .get(&root)
                        .copied()
                        .ok_or(WorkspaceRebaseError::MissingReference {
                            kind: Root,
                            id: root.get(),
                        })
                })
                .transpose()?;
            for floating in &mut presentation.contained {
                *floating = floatings.get(floating).copied().ok_or(
                    WorkspaceRebaseError::MissingReference {
                        kind: Floating,
                        id: floating.get(),
                    },
                )?;
            }
            Ok((mapped, presentation))
        })
        .collect::<Result<_, WorkspaceRebaseError>>()?;
    workspace.contained_floatings = std::mem::take(&mut workspace.contained_floatings)
        .into_iter()
        .map(|(id, mut contained)| {
            let mapped =
                floatings
                    .get(&id)
                    .copied()
                    .ok_or(WorkspaceRebaseError::MissingReference {
                        kind: Floating,
                        id: id.get(),
                    })?;
            contained.root = roots.get(&contained.root).copied().ok_or(
                WorkspaceRebaseError::MissingReference {
                    kind: Root,
                    id: contained.root.get(),
                },
            )?;
            Ok((mapped, contained))
        })
        .collect::<Result<_, WorkspaceRebaseError>>()?;
    workspace
        .validate()
        .map_err(WorkspaceRebaseError::InvalidWorkspace)?;
    Ok(RebasedWorkspace {
        workspace,
        #[cfg(feature = "serde")]
        frontier,
        surfaces,
    })
}
