//! Queries that map interaction payloads back to their owning surface.

use dockspace::RootPresentationOwner;
use dockspace::command::MovePayload;
use dockspace::graph::Workspace;
use dockspace::ids::{RootId, SurfaceId};

pub(super) fn payload_surface(workspace: &Workspace, payload: &MovePayload) -> Option<SurfaceId> {
    let root = match payload {
        MovePayload::Item(source) => source.root(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
    };
    root_surface(workspace, root)
}

fn root_surface(workspace: &Workspace, root: RootId) -> Option<SurfaceId> {
    match workspace.presentation_for_root(root)? {
        RootPresentationOwner::Main { surface }
        | RootPresentationOwner::Contained { surface, .. } => Some(surface),
    }
}
