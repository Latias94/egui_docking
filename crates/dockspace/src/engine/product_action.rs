use super::*;

use crate::command::{
    DockFraction as CommandDockFraction, DockTarget, Edge as CommandEdge, RootContent,
};
use crate::ids::NodeId;
use crate::model::{
    DockAnchor, DockEdge, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection,
    ProductAction,
};
use crate::workspace::WorkspaceIndex;

enum ProductActionPlan {
    Noop(DockspaceActionOutcome),
    Command {
        command: WorkspaceCommand,
        context: ProductCommandContext,
    },
}

#[derive(Debug, Clone, Copy)]
enum ProductCommandContext {
    Select {
        item: ItemId,
    },
    Open {
        item: ItemId,
    },
    DockMove {
        item: ItemId,
        source_root: RootId,
        target_root: RootId,
    },
    DockInstallMain {
        item: ItemId,
        source_root: RootId,
        target_root: RootId,
        surface: SurfaceId,
    },
    DockRehomeMain {
        item: ItemId,
        root: RootId,
        surface: SurfaceId,
    },
    DockPromoteContained {
        item: ItemId,
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
}

impl DockEngine {
    pub(super) fn reduce_product_action(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        action: ProductAction,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let accepted_base = self.version;
        if expected != accepted_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            });
        }

        let plan = match self.compile_product_action(action) {
            Ok(plan) => plan,
            Err(reason) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason,
                    version: self.version,
                });
            }
        };
        let (command, context) = match plan {
            ProductActionPlan::Noop(outcome) => {
                return Ok(InputOutcome::ProductActionProcessed {
                    outcome,
                    version: self.version,
                });
            }
            ProductActionPlan::Command { command, context } => (command, context),
        };

        match self.reduce_workspace_command(
            input,
            expected,
            accepted_base,
            &command,
            policy,
            events,
            interaction_events,
        )? {
            InputOutcome::CommandProcessed {
                outcome,
                changed,
                version,
            } => Ok(InputOutcome::ProductActionProcessed {
                outcome: context.map_outcome(outcome, changed)?,
                version,
            }),
            InputOutcome::CommandRejected { error, version } => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: product_command_rejection(action, &error)?,
                    version,
                })
            }
            InputOutcome::StaleRejected {
                expected,
                accepted_base,
            } => Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base,
            }),
            _ => Err(EngineError::ReductionCauseInvariant {
                detail: "product action command produced an unrelated input outcome",
            }),
        }
    }

    fn compile_product_action(
        &self,
        action: ProductAction,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        match action {
            ProductAction::SelectItem { item } => {
                let source = self.capture_product_item(item)?;
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::Select { source },
                    context: ProductCommandContext::Select { item },
                })
            }
            ProductAction::OpenItem { item, placement } => {
                if self
                    .workspace
                    .capture_item_source_by_id(item)
                    .map_err(|_| DockspaceActionRejection::Conflict)?
                    .is_some()
                {
                    return Ok(ProductActionPlan::Noop(DockspaceActionOutcome::Existing {
                        item,
                    }));
                }
                let command =
                    self.compile_product_placement(RootContent::OpenItem(item), placement)?;
                Ok(ProductActionPlan::Command {
                    command,
                    context: ProductCommandContext::Open { item },
                })
            }
            ProductAction::DockItem { item, placement } => {
                let source = self.capture_product_item(item)?;
                self.compile_product_move(item, source, placement)
            }
        }
    }

    fn capture_product_item(
        &self,
        item: ItemId,
    ) -> Result<crate::command::ItemSource, DockspaceActionRejection> {
        self.workspace
            .capture_item_source_by_id(item)
            .map_err(|_| DockspaceActionRejection::Conflict)?
            .ok_or(DockspaceActionRejection::ItemUnavailable { item })
    }

    fn compile_product_placement(
        &self,
        content: RootContent,
        placement: DockPlacement,
    ) -> Result<WorkspaceCommand, DockspaceActionRejection> {
        match placement {
            DockPlacement::Main(surface) => {
                self.require_rootless_surface(surface)?;
                Ok(WorkspaceCommand::InstallMainRoot {
                    surface,
                    root: self.next_product_root()?,
                    content,
                })
            }
            _ => Ok(WorkspaceCommand::Open {
                item: match content {
                    RootContent::OpenItem(item) => item,
                    RootContent::Move(_) => {
                        return Err(DockspaceActionRejection::Conflict);
                    }
                },
                target: self.capture_product_target(placement)?.0,
            }),
        }
    }

    fn compile_product_move(
        &self,
        item: ItemId,
        source: crate::command::ItemSource,
        placement: DockPlacement,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let source_root = source.root();
        let payload = MovePayload::Item(source);
        let DockPlacement::Main(surface) = placement else {
            let (target, target_root) = self.capture_product_target(placement)?;
            return Ok(ProductActionPlan::Command {
                command: WorkspaceCommand::Move { payload, target },
                context: ProductCommandContext::DockMove {
                    item,
                    source_root,
                    target_root,
                },
            });
        };

        let index = WorkspaceIndex::build(&self.workspace, self.version)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        let complete_root = index
            .capture_complete_root_source(&self.workspace, self.version, &payload)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        let Some(complete_root) = complete_root else {
            self.require_rootless_surface(surface)?;
            let root = self.next_product_root()?;
            return Ok(ProductActionPlan::Command {
                command: WorkspaceCommand::InstallMainRoot {
                    surface,
                    root,
                    content: RootContent::Move(payload),
                },
                context: ProductCommandContext::DockInstallMain {
                    item,
                    source_root,
                    target_root: root,
                    surface,
                },
            });
        };

        let owner = self
            .workspace
            .presentation_for_root(source_root)
            .ok_or(DockspaceActionRejection::Conflict)?;
        match owner {
            crate::RootPresentationOwner::Contained {
                surface: current_surface,
                floating,
            } if current_surface == surface => {
                self.require_rootless_surface(surface)?;
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::PromoteContained {
                        source: complete_root,
                        surface,
                        floating,
                    },
                    context: ProductCommandContext::DockPromoteContained {
                        item,
                        root: source_root,
                        surface,
                        floating,
                    },
                })
            }
            crate::RootPresentationOwner::Main {
                surface: current_surface,
            } if current_surface == surface => Ok(ProductActionPlan::Command {
                command: WorkspaceCommand::RehomeRoot {
                    source: complete_root,
                    target: RootPresentationTarget::Main { surface },
                },
                context: ProductCommandContext::DockRehomeMain {
                    item,
                    root: source_root,
                    surface,
                },
            }),
            crate::RootPresentationOwner::Main { .. }
            | crate::RootPresentationOwner::Contained { .. } => {
                self.require_rootless_surface(surface)?;
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::RehomeRoot {
                        source: complete_root,
                        target: RootPresentationTarget::Main { surface },
                    },
                    context: ProductCommandContext::DockRehomeMain {
                        item,
                        root: source_root,
                        surface,
                    },
                })
            }
        }
    }

    fn capture_product_target(
        &self,
        placement: DockPlacement,
    ) -> Result<(DockTarget, RootId), DockspaceActionRejection> {
        match placement {
            DockPlacement::Center(anchor) => {
                let (root, tabs) = self.capture_product_anchor(anchor)?;
                let target = self
                    .workspace
                    .capture_tab_target(root, tabs)
                    .map_err(|_| DockspaceActionRejection::AnchorUnavailable { anchor })?;
                Ok((DockTarget::Center(target), root))
            }
            DockPlacement::Before(item) | DockPlacement::After(item) => {
                let anchor = DockAnchor::Item(item);
                let source = self
                    .workspace
                    .capture_item_source_by_id(item)
                    .map_err(|_| DockspaceActionRejection::Conflict)?
                    .ok_or(DockspaceActionRejection::AnchorUnavailable { anchor })?;
                let root = source.root();
                let tabs = source.tabs();
                let items = match self.workspace.node(tabs) {
                    Some(Node::Tabs { items, .. }) => items,
                    Some(Node::Split { .. }) | None => {
                        return Err(DockspaceActionRejection::Conflict);
                    }
                };
                let current = items
                    .iter()
                    .position(|candidate| *candidate == item)
                    .ok_or(DockspaceActionRejection::Conflict)?;
                let index = if matches!(placement, DockPlacement::After(_)) {
                    current + 1
                } else {
                    current
                };
                let target = self
                    .workspace
                    .capture_tab_target(root, tabs)
                    .map_err(|_| DockspaceActionRejection::AnchorUnavailable { anchor })?;
                Ok((DockTarget::TabGap { target, index }, root))
            }
            DockPlacement::InnerEdge {
                anchor,
                edge,
                fraction,
            } => {
                let (root, tabs) = self.capture_product_anchor(anchor)?;
                let target = self
                    .workspace
                    .capture_inner_edge_target(
                        root,
                        tabs,
                        command_edge(edge),
                        command_fraction(fraction),
                    )
                    .map_err(|_| DockspaceActionRejection::AnchorUnavailable { anchor })?;
                Ok((DockTarget::InnerEdge(target), root))
            }
            DockPlacement::OuterEdge {
                root,
                edge,
                fraction,
            } => {
                let target = self
                    .workspace
                    .capture_outer_edge_target(root, command_edge(edge), command_fraction(fraction))
                    .map_err(|_| DockspaceActionRejection::RootUnavailable { root })?;
                Ok((DockTarget::OuterEdge(target), root))
            }
            DockPlacement::Main(surface) => {
                Err(DockspaceActionRejection::MainSurfaceUnavailable { surface })
            }
        }
    }

    fn capture_product_anchor(
        &self,
        anchor: DockAnchor,
    ) -> Result<(RootId, NodeId), DockspaceActionRejection> {
        match anchor {
            DockAnchor::Item(item) => self
                .workspace
                .capture_item_source_by_id(item)
                .map_err(|_| DockspaceActionRejection::Conflict)?
                .map(|source| (source.root(), source.tabs()))
                .ok_or(DockspaceActionRejection::AnchorUnavailable { anchor }),
            DockAnchor::Central(root) => self
                .workspace
                .root(root)
                .and_then(|record| record.central.map(|tabs| (root, tabs)))
                .ok_or(DockspaceActionRejection::AnchorUnavailable { anchor }),
        }
    }

    fn require_rootless_surface(&self, surface: SurfaceId) -> Result<(), DockspaceActionRejection> {
        self.workspace
            .surface(surface)
            .filter(|presentation| presentation.main_root.is_none())
            .map(|_| ())
            .ok_or(DockspaceActionRejection::MainSurfaceUnavailable { surface })
    }

    fn next_product_root(&self) -> Result<RootId, DockspaceActionRejection> {
        self.prepare_presentation_root_identity()
            .ok_or(DockspaceActionRejection::IdentityExhausted)
    }
}

impl ProductCommandContext {
    fn map_outcome(
        self,
        outcome: CommandOutcome,
        changed: bool,
    ) -> Result<DockspaceActionOutcome, EngineError> {
        match (self, outcome) {
            (Self::Select { item }, CommandOutcome::Selected { item: selected, .. })
                if selected == item =>
            {
                Ok(DockspaceActionOutcome::Selected { item, changed })
            }
            (Self::Open { item }, CommandOutcome::Opened { item: opened, root })
                if opened == item =>
            {
                Ok(DockspaceActionOutcome::Opened { item, root })
            }
            (Self::Open { item }, CommandOutcome::MainRootInstalled { root, items, .. })
                if items.as_slice() == [item] =>
            {
                Ok(DockspaceActionOutcome::Opened { item, root })
            }
            (
                Self::DockMove {
                    item,
                    source_root,
                    target_root,
                },
                CommandOutcome::Moved {
                    items,
                    source_root: moved_source,
                    target_root: moved_target,
                    ..
                },
            ) if items.as_slice() == [item]
                && moved_source == source_root
                && moved_target == target_root =>
            {
                Ok(DockspaceActionOutcome::Docked {
                    item,
                    source_root,
                    target_root,
                    changed,
                })
            }
            (
                Self::DockInstallMain {
                    item,
                    source_root,
                    target_root,
                    surface: target_surface,
                },
                CommandOutcome::MainRootInstalled {
                    surface,
                    root,
                    items,
                },
            ) if items.as_slice() == [item] && root == target_root && target_surface == surface => {
                Ok(DockspaceActionOutcome::Docked {
                    item,
                    source_root,
                    target_root,
                    changed,
                })
            }
            (
                Self::DockRehomeMain {
                    item,
                    root: expected_root,
                    surface: target_surface,
                },
                CommandOutcome::RootRehomed {
                    root,
                    surface,
                    floating: None,
                    changed: outcome_changed,
                },
            ) if root == expected_root
                && target_surface == surface
                && changed == outcome_changed =>
            {
                Ok(DockspaceActionOutcome::Docked {
                    item,
                    source_root: root,
                    target_root: root,
                    changed,
                })
            }
            (
                Self::DockPromoteContained {
                    item,
                    root: expected_root,
                    surface: target_surface,
                    floating: expected_floating,
                },
                CommandOutcome::ContainedPromoted {
                    surface,
                    root,
                    floating,
                },
            ) if root == expected_root
                && target_surface == surface
                && floating == expected_floating
                && changed =>
            {
                Ok(DockspaceActionOutcome::Docked {
                    item,
                    source_root: root,
                    target_root: root,
                    changed,
                })
            }
            _ => Err(EngineError::ReductionCauseInvariant {
                detail: "product action command outcome did not match its compiled action",
            }),
        }
    }
}

fn command_edge(edge: DockEdge) -> CommandEdge {
    match edge {
        DockEdge::Left => CommandEdge::Left,
        DockEdge::Right => CommandEdge::Right,
        DockEdge::Top => CommandEdge::Top,
        DockEdge::Bottom => CommandEdge::Bottom,
    }
}

fn command_fraction(fraction: crate::model::DockFraction) -> CommandDockFraction {
    CommandDockFraction::new(fraction.get())
        .expect("product dock fractions are validated at construction")
}

fn product_command_rejection(
    action: ProductAction,
    error: &CommandError,
) -> Result<DockspaceActionRejection, EngineError> {
    match error {
        CommandError::Policy(_) => Ok(DockspaceActionRejection::PolicyDenied),
        CommandError::RetiredRootId { .. }
        | CommandError::RootIdCollision { .. }
        | CommandError::RetiredSurfaceId { .. }
        | CommandError::SurfaceIdCollision { .. }
        | CommandError::RetiredFloatingId { .. }
        | CommandError::FloatingIdCollision { .. } => Err(EngineError::ReductionCauseInvariant {
            detail: "product identity authority prepared an unavailable presentation identity",
        }),
        CommandError::MissingSurface { surface }
        | CommandError::SurfaceMainOccupied { surface, .. }
            if matches!(
                action,
                ProductAction::OpenItem {
                    placement: DockPlacement::Main(_),
                    ..
                } | ProductAction::DockItem {
                    placement: DockPlacement::Main(_),
                    ..
                }
            ) =>
        {
            Ok(DockspaceActionRejection::MainSurfaceUnavailable { surface: *surface })
        }
        _ => Ok(DockspaceActionRejection::Conflict),
    }
}
