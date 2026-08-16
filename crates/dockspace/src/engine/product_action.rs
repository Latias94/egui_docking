use super::*;

use crate::command::{
    DockFraction as CommandDockFraction, DockTarget, Edge as CommandEdge, NodeSource, RootContent,
};
use crate::ids::NodeId;
use crate::model::{
    DockAnchor, DockEdge, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection,
    NativeWindowPlacement, PreparedDockAction, PreparedDockActionAuthorityMismatch, ProductAction,
};
use crate::workspace::WorkspaceIndex;

mod contained;

enum ProductActionPlan {
    Noop(DockspaceActionOutcome),
    Command {
        command: WorkspaceCommand,
        context: ProductCommandContext,
    },
}

struct CompleteRootMainPlan {
    command: WorkspaceCommand,
    disposition: CompleteRootMainDisposition,
}

enum CompleteRootMainDisposition {
    Rehome,
    PromoteContained { floating: FloatingPresentationId },
}

#[derive(Debug)]
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
    DockRootMove {
        root: RootId,
        target_root: RootId,
        items: Vec<ItemId>,
    },
    DockRootRehomeMain {
        root: RootId,
        surface: SurfaceId,
        items: Vec<ItemId>,
    },
    DockRootPromoteContained {
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
        items: Vec<ItemId>,
    },
    FloatCreate {
        item: ItemId,
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    FloatRehome {
        item: ItemId,
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    FloatUpdate {
        item: ItemId,
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    SetContainedRect {
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    RaiseContained {
        root: RootId,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
}

impl DockEngine {
    /// Prepares one selection action against the exact published workspace version.
    #[must_use]
    pub const fn prepare_select_item(&self, item: ItemId) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::SelectItem { item })
    }

    /// Prepares one open action against the exact published workspace version.
    #[must_use]
    pub const fn prepare_open_item(
        &self,
        item: ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::OpenItem { item, placement })
    }

    /// Prepares one docking action against the exact published workspace version.
    #[must_use]
    pub const fn prepare_dock_item(
        &self,
        item: ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::DockItem { item, placement })
    }

    /// Prepares one complete-root docking action against the exact published workspace version.
    #[must_use]
    pub const fn prepare_dock_root(
        &self,
        root: RootId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::DockRoot { root, placement })
    }

    /// Prepares one complete-root native tear-off against the exact published version.
    #[must_use]
    pub const fn prepare_tear_off_root(
        &self,
        root: RootId,
        placement: NativeWindowPlacement,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::TearOffRoot { root, placement })
    }

    /// Prepares one contained-floating action against the exact published workspace version.
    #[must_use]
    pub const fn prepare_float_item(
        &self,
        item: ItemId,
        surface: SurfaceId,
        rect: LogicalRect,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::FloatItem {
            item,
            surface,
            rect,
        })
    }

    /// Prepares one contained-bounds update against the exact published workspace version.
    #[must_use]
    pub const fn prepare_set_contained_rect(
        &self,
        item: ItemId,
        rect: LogicalRect,
    ) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::SetContainedRect { item, rect })
    }

    /// Prepares one contained raise against the exact published workspace version.
    #[must_use]
    pub const fn prepare_raise_contained(&self, item: ItemId) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::RaiseContained { item })
    }

    /// Prepares one contained bring-into-view action against the published workspace version.
    #[must_use]
    pub const fn prepare_bring_contained_into_view(&self, item: ItemId) -> PreparedDockAction {
        self.prepare_product_action(ProductAction::BringContainedIntoView { item })
    }

    const fn prepare_product_action(&self, action: ProductAction) -> PreparedDockAction {
        PreparedDockAction::new(self.authority_domain, self.version, action)
    }

    pub(crate) fn accept_prepared_action(
        &self,
        prepared: PreparedDockAction,
    ) -> Result<EngineInput, PreparedDockActionAuthorityMismatch> {
        let (authority_domain, expected, action) = prepared.into_parts();
        if authority_domain != self.authority_domain {
            return Err(PreparedDockActionAuthorityMismatch);
        }
        Ok(match action {
            ProductAction::SelectItem { item } => EngineInput::SelectItem { expected, item },
            ProductAction::OpenItem { item, placement } => EngineInput::OpenItem {
                expected,
                item,
                placement,
            },
            ProductAction::DockItem { item, placement } => EngineInput::DockItem {
                expected,
                item,
                placement,
            },
            ProductAction::DockRoot { root, placement } => EngineInput::DockRoot {
                expected,
                root,
                placement,
            },
            ProductAction::TearOffRoot { root, placement } => EngineInput::TearOffRoot {
                expected,
                root,
                placement,
            },
            ProductAction::FloatItem {
                item,
                surface,
                rect,
            } => EngineInput::FloatItem {
                expected,
                item,
                surface,
                rect,
            },
            ProductAction::SetContainedRect { item, rect } => EngineInput::SetContainedRect {
                expected,
                item,
                rect,
            },
            ProductAction::RaiseContained { item } => {
                EngineInput::RaiseContained { expected, item }
            }
            ProductAction::BringContainedIntoView { item } => {
                EngineInput::BringContainedIntoView { expected, item }
            }
        })
    }

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
            ProductAction::DockRoot { root, placement } => {
                self.compile_product_root_move(root, placement)
            }
            ProductAction::TearOffRoot { .. } => Err(DockspaceActionRejection::Conflict),
            ProductAction::FloatItem {
                item,
                surface,
                rect,
            } => self.compile_product_float(item, surface, rect),
            ProductAction::SetContainedRect { item, rect } => {
                self.compile_product_contained_rect(item, rect)
            }
            ProductAction::RaiseContained { item } => self.compile_product_contained_raise(item),
            ProductAction::BringContainedIntoView { item } => {
                self.compile_product_contained_bring_into_view(item)
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

        let complete_root = self.capture_complete_root_payload(&payload)?;
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

        let plan = self.compile_complete_root_main(source_root, complete_root, surface)?;
        let context = match plan.disposition {
            CompleteRootMainDisposition::Rehome => ProductCommandContext::DockRehomeMain {
                item,
                root: source_root,
                surface,
            },
            CompleteRootMainDisposition::PromoteContained { floating } => {
                ProductCommandContext::DockPromoteContained {
                    item,
                    root: source_root,
                    surface,
                    floating,
                }
            }
        };
        Ok(ProductActionPlan::Command {
            command: plan.command,
            context,
        })
    }

    fn compile_product_root_move(
        &self,
        root: RootId,
        placement: DockPlacement,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let (_, payload, items) = self.capture_product_root_payload(root)?;

        let DockPlacement::Main(surface) = placement else {
            let (target, target_root) = self.capture_product_target(placement)?;
            return Ok(ProductActionPlan::Command {
                command: WorkspaceCommand::Move { payload, target },
                context: ProductCommandContext::DockRootMove {
                    root,
                    target_root,
                    items,
                },
            });
        };

        let complete_root = self
            .capture_complete_root_payload(&payload)?
            .ok_or(DockspaceActionRejection::Conflict)?;
        let plan = self.compile_complete_root_main(root, complete_root, surface)?;
        let context = match plan.disposition {
            CompleteRootMainDisposition::Rehome => ProductCommandContext::DockRootRehomeMain {
                root,
                surface,
                items,
            },
            CompleteRootMainDisposition::PromoteContained { floating } => {
                ProductCommandContext::DockRootPromoteContained {
                    root,
                    surface,
                    floating,
                    items,
                }
            }
        };
        Ok(ProductActionPlan::Command {
            command: plan.command,
            context,
        })
    }

    pub(super) fn capture_product_root_payload(
        &self,
        root: RootId,
    ) -> Result<(NodeSource, MovePayload, Vec<ItemId>), DockspaceActionRejection> {
        let record = self
            .workspace
            .root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?;
        let source = self
            .workspace
            .capture_node_source(root, record.node)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        let items = self.workspace.collect_items_in_subtree(record.node);
        if items.is_empty() {
            return Err(DockspaceActionRejection::Conflict);
        }
        let payload = match self.workspace.node(record.node) {
            Some(Node::Tabs { .. }) => MovePayload::Tabs(source.clone()),
            Some(Node::Split { .. }) => MovePayload::Subtree(source.clone()),
            None => return Err(DockspaceActionRejection::Conflict),
        };
        Ok((source, payload, items))
    }

    pub(super) fn capture_complete_root_payload(
        &self,
        payload: &MovePayload,
    ) -> Result<Option<NodeSource>, DockspaceActionRejection> {
        let index = WorkspaceIndex::build(&self.workspace, self.version)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        index
            .capture_complete_root_source(&self.workspace, self.version, payload)
            .map_err(|_| DockspaceActionRejection::Conflict)
    }

    fn compile_complete_root_main(
        &self,
        root: RootId,
        source: NodeSource,
        surface: SurfaceId,
    ) -> Result<CompleteRootMainPlan, DockspaceActionRejection> {
        let owner = self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::Conflict)?;
        match owner {
            crate::RootPresentationOwner::Contained {
                surface: current_surface,
                floating,
            } if current_surface == surface => {
                self.require_rootless_surface(surface)?;
                Ok(CompleteRootMainPlan {
                    command: WorkspaceCommand::PromoteContained {
                        source,
                        surface,
                        floating,
                    },
                    disposition: CompleteRootMainDisposition::PromoteContained { floating },
                })
            }
            crate::RootPresentationOwner::Main {
                surface: current_surface,
            } if current_surface == surface => Ok(CompleteRootMainPlan {
                command: WorkspaceCommand::RehomeRoot {
                    source,
                    target: RootPresentationTarget::Main { surface },
                },
                disposition: CompleteRootMainDisposition::Rehome,
            }),
            crate::RootPresentationOwner::Main { .. }
            | crate::RootPresentationOwner::Contained { .. } => {
                self.require_rootless_surface(surface)?;
                Ok(CompleteRootMainPlan {
                    command: WorkspaceCommand::RehomeRoot {
                        source,
                        target: RootPresentationTarget::Main { surface },
                    },
                    disposition: CompleteRootMainDisposition::Rehome,
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
            (
                Self::DockRootMove {
                    root,
                    target_root,
                    items: expected_items,
                },
                CommandOutcome::Moved {
                    items,
                    source_root,
                    target_root: moved_target,
                    ..
                },
            ) if items == expected_items && source_root == root && moved_target == target_root => {
                Ok(DockspaceActionOutcome::RootDocked {
                    root,
                    target_root,
                    items: expected_items,
                    changed,
                })
            }
            (
                Self::DockRootRehomeMain {
                    root: expected_root,
                    surface: target_surface,
                    items,
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
                Ok(DockspaceActionOutcome::RootDocked {
                    root,
                    target_root: root,
                    items,
                    changed,
                })
            }
            (
                Self::DockRootPromoteContained {
                    root: expected_root,
                    surface: target_surface,
                    floating: expected_floating,
                    items,
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
                Ok(DockspaceActionOutcome::RootDocked {
                    root,
                    target_root: root,
                    items,
                    changed,
                })
            }
            (
                Self::FloatCreate {
                    item,
                    root: expected_root,
                    surface: expected_surface,
                    floating: expected_floating,
                },
                CommandOutcome::ContainedRootCreated {
                    surface,
                    root,
                    floating,
                    items,
                },
            ) if surface == expected_surface
                && root == expected_root
                && floating == expected_floating
                && items.as_slice() == [item]
                && changed =>
            {
                Ok(DockspaceActionOutcome::Floated {
                    item,
                    root,
                    surface,
                    floating,
                    changed,
                })
            }
            (
                Self::FloatRehome {
                    item,
                    root: expected_root,
                    surface: expected_surface,
                    floating: expected_floating,
                },
                CommandOutcome::RootRehomed {
                    root,
                    surface,
                    floating: Some(floating),
                    changed: outcome_changed,
                },
            ) if root == expected_root
                && surface == expected_surface
                && floating == expected_floating
                && changed == outcome_changed =>
            {
                Ok(DockspaceActionOutcome::Floated {
                    item,
                    root,
                    surface,
                    floating,
                    changed,
                })
            }
            (
                Self::FloatUpdate {
                    item,
                    root,
                    surface,
                    floating: expected_floating,
                },
                CommandOutcome::ContainedPresentationUpdated {
                    floating,
                    rect_changed,
                    order_changed,
                    ..
                },
            ) if floating == expected_floating && changed == (rect_changed || order_changed) => {
                Ok(DockspaceActionOutcome::Floated {
                    item,
                    root,
                    surface,
                    floating,
                    changed,
                })
            }
            (
                Self::SetContainedRect {
                    root,
                    surface,
                    floating: expected_floating,
                },
                CommandOutcome::ContainedRectUpdated {
                    floating,
                    changed: outcome_changed,
                },
            ) if floating == expected_floating && changed == outcome_changed => {
                Ok(DockspaceActionOutcome::ContainedBoundsUpdated {
                    root,
                    surface,
                    floating,
                    changed,
                })
            }
            (
                Self::RaiseContained {
                    root,
                    surface,
                    floating: expected_floating,
                },
                CommandOutcome::ContainedRaised {
                    floating,
                    changed: outcome_changed,
                    ..
                },
            ) if floating == expected_floating && changed == outcome_changed => {
                Ok(DockspaceActionOutcome::ContainedRaised {
                    root,
                    surface,
                    floating,
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

pub(super) fn product_command_rejection(
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
                } | ProductAction::DockRoot {
                    placement: DockPlacement::Main(_),
                    ..
                }
            ) =>
        {
            Ok(DockspaceActionRejection::MainSurfaceUnavailable { surface: *surface })
        }
        CommandError::MissingSurface { surface }
            if matches!(action, ProductAction::FloatItem { .. }) =>
        {
            Ok(DockspaceActionRejection::SurfaceUnavailable { surface: *surface })
        }
        _ => Ok(DockspaceActionRejection::Conflict),
    }
}
