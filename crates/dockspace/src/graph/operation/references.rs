use super::*;

#[derive(Clone, Copy)]
pub(super) enum ReferenceAuthority<'a> {
    Live,
    Indexed {
        version: WorkspaceVersion,
        index: &'a WorkspaceIndex,
    },
}

impl ReferenceAuthority<'_> {
    fn verify_reference(
        self,
        workspace: &Workspace,
        root: RootId,
        node: NodeId,
        expected: &crate::command::RootReference,
        role: ReferenceRole,
    ) -> Result<(), CommandError> {
        match self {
            Self::Live => workspace.verify_reference(root, node, expected, role),
            Self::Indexed { version, index } => {
                index.verify_reference(workspace, version, root, node, expected, role)
            }
        }
    }

    fn capture_tab_target(
        self,
        workspace: &Workspace,
        root: RootId,
        tabs: NodeId,
    ) -> Result<TabTarget, CommandError> {
        match self {
            Self::Live => workspace.capture_tab_target(root, tabs),
            Self::Indexed { version, index } => {
                index.capture_tab_target(workspace, version, root, tabs)
            }
        }
    }

    fn capture_inner_edge_target(
        self,
        workspace: &Workspace,
        target: &EdgeTarget,
    ) -> Result<EdgeTarget, CommandError> {
        match self {
            Self::Live => workspace.capture_inner_edge_target(
                target.root(),
                target.node(),
                target.edge(),
                target.fraction(),
            ),
            Self::Indexed { version, index } => index.capture_inner_edge_target(
                workspace,
                version,
                target.root(),
                target.node(),
                target.edge(),
                target.fraction(),
            ),
        }
    }

    fn capture_outer_edge_target(
        self,
        workspace: &Workspace,
        target: &EdgeTarget,
    ) -> Result<EdgeTarget, CommandError> {
        match self {
            Self::Live => {
                workspace.capture_outer_edge_target(target.root(), target.edge(), target.fraction())
            }
            Self::Indexed { version, index } => index.capture_outer_edge_target(
                workspace,
                version,
                target.root(),
                target.edge(),
                target.fraction(),
            ),
        }
    }
}

pub(super) fn validate_item_source(
    workspace: &Workspace,
    source: &ItemSource,
) -> Result<(), CommandError> {
    validate_item_source_with(workspace, ReferenceAuthority::Live, source)
}

fn validate_item_source_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    source: &ItemSource,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        source.root(),
        source.tabs(),
        source.reference(),
        ReferenceRole::Source,
    )?;
    match workspace.storage.nodes.get(source.tabs()) {
        Some(Node::Tabs { items, .. }) if items.contains(&source.item()) => Ok(()),
        Some(Node::Tabs { .. }) => Err(CommandError::ItemNotInTabs {
            tabs: source.tabs(),
            item: source.item(),
        }),
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: source.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Source,
            node: source.tabs(),
        }),
    }
}

pub(super) fn validate_node_source(
    workspace: &Workspace,
    source: &NodeSource,
) -> Result<(), CommandError> {
    validate_node_source_with(workspace, ReferenceAuthority::Live, source)
}

fn validate_node_source_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    source: &NodeSource,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        source.root(),
        source.node(),
        source.reference(),
        ReferenceRole::Source,
    )
}

pub(super) fn validate_root_source(
    workspace: &Workspace,
    source: &NodeSource,
) -> Result<(RootId, NodeId), CommandError> {
    validate_node_source(workspace, source)?;
    let root = source.root();
    let root_node = workspace
        .storage
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?
        .node;
    if root_node != source.node() {
        return Err(CommandError::NodeIsNotRoot {
            root,
            node: source.node(),
        });
    }
    Ok((root, root_node))
}

pub(super) fn validate_move_payload(
    workspace: &Workspace,
    payload: &MovePayload,
) -> Result<(), CommandError> {
    validate_move_payload_with(workspace, ReferenceAuthority::Live, payload)
}

pub(super) fn validate_move_payload_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    payload: &MovePayload,
) -> Result<(), CommandError> {
    match payload {
        MovePayload::Item(source) => validate_item_source_with(workspace, references, source),
        MovePayload::Tabs(source) => {
            validate_node_source_with(workspace, references, source)?;
            if !matches!(
                workspace.storage.nodes.get(source.node()),
                Some(Node::Tabs { .. })
            ) {
                return Err(CommandError::NodeIsNotTabs {
                    node: source.node(),
                });
            }
            ensure_nonempty_payload(workspace, source.node())
        }
        MovePayload::Subtree(source) => {
            validate_node_source_with(workspace, references, source)?;
            ensure_nonempty_payload(workspace, source.node())
        }
    }
}

fn ensure_nonempty_payload(workspace: &Workspace, node: NodeId) -> Result<(), CommandError> {
    if workspace.subtree_has_items(node) {
        Ok(())
    } else {
        Err(CommandError::EmptyPayload { node })
    }
}

pub(super) fn validate_target(
    workspace: &Workspace,
    target: &DockTarget,
) -> Result<(), CommandError> {
    validate_target_with(workspace, ReferenceAuthority::Live, target)
}

pub(super) fn validate_target_with(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &DockTarget,
) -> Result<(), CommandError> {
    match target {
        DockTarget::Center(target) => validate_tab_target(workspace, references, target, None),
        DockTarget::TabGap { target, index } => {
            validate_tab_target(workspace, references, target, Some(*index))
        }
        DockTarget::InnerEdge(target) => {
            validate_edge_target(workspace, references, target, EdgeTargetScope::Inner)
        }
        DockTarget::OuterEdge(target) => {
            validate_edge_target(workspace, references, target, EdgeTargetScope::Outer)
        }
    }
}

fn validate_edge_target(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &EdgeTarget,
    requested: EdgeTargetScope,
) -> Result<(), CommandError> {
    if target.scope() != requested {
        return Err(CommandError::EdgeTargetScopeMismatch {
            node: target.node(),
            captured: target.scope(),
            requested,
        });
    }
    references.verify_reference(
        workspace,
        target.root(),
        target.node(),
        target.reference(),
        ReferenceRole::Target,
    )?;
    let actual = match requested {
        EdgeTargetScope::Inner => references.capture_inner_edge_target(workspace, target)?,
        EdgeTargetScope::Outer => references.capture_outer_edge_target(workspace, target)?,
    };
    if actual == *target {
        Ok(())
    } else {
        Err(CommandError::Invariant {
            stage: "rederive frozen edge target facts",
        })
    }
}

fn validate_tab_target(
    workspace: &Workspace,
    references: ReferenceAuthority<'_>,
    target: &TabTarget,
    insertion_index: Option<usize>,
) -> Result<(), CommandError> {
    references.verify_reference(
        workspace,
        target.root(),
        target.tabs(),
        target.reference(),
        ReferenceRole::Target,
    )?;
    let actual = references.capture_tab_target(workspace, target.root(), target.tabs())?;
    if actual != *target {
        return Err(CommandError::Invariant {
            stage: "rederive frozen tabs target facts",
        });
    }
    match workspace.storage.nodes.get(target.tabs()) {
        Some(Node::Tabs { items, .. }) => {
            if let Some(index) = insertion_index
                && index > items.len()
            {
                return Err(CommandError::TabIndexOutOfBounds {
                    tabs: target.tabs(),
                    index,
                    len: items.len(),
                });
            }
            Ok(())
        }
        Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs {
            node: target.tabs(),
        }),
        None => Err(CommandError::MissingNode {
            role: ReferenceRole::Target,
            node: target.tabs(),
        }),
    }
}

pub(super) fn validate_root_content(
    workspace: &Workspace,
    content: &RootContent,
) -> Result<(), CommandError> {
    match content {
        RootContent::OpenItem(_) => Ok(()),
        RootContent::Move(payload) => {
            validate_move_payload(workspace, payload)?;
            if move_removes_complete_root(workspace, payload)? {
                return Err(CommandError::WholeRootRequiresRehome {
                    root: payload_root(payload),
                });
            }
            Ok(())
        }
    }
}

fn move_removes_complete_root(
    workspace: &Workspace,
    payload: &MovePayload,
) -> Result<bool, CommandError> {
    let root = payload_root(payload);
    let record = workspace
        .storage
        .roots
        .get(&root)
        .ok_or(CommandError::MissingRoot { root })?;
    Ok(match payload {
        MovePayload::Item(_) => workspace
            .item_move_removes_root(root, workspace.collect_items_in_subtree(record.node).len()),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.node() == record.node,
    })
}
