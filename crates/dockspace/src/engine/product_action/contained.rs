use super::*;

use crate::command::{ContainedPosition, ContainedRosterSource};
use crate::geometry::LogicalRect;

struct ProductContainedSource {
    root: RootId,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
    source: NodeSource,
    roster: ContainedRosterSource,
}

impl DockEngine {
    pub(super) fn compile_product_float(
        &self,
        item: ItemId,
        surface: SurfaceId,
        rect: LogicalRect,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        self.require_product_surface(surface)?;
        let item_source = self.capture_product_item(item)?;
        let source_root = item_source.root();
        let payload = MovePayload::Item(item_source);
        let Some(root_source) = self.capture_complete_root_payload(&payload)? else {
            let root = self.next_product_root()?;
            let floating = self.next_product_floating()?;
            return Ok(ProductActionPlan::Command {
                command: WorkspaceCommand::CreateContainedRoot {
                    surface,
                    root,
                    floating,
                    rect,
                    position: ContainedPosition::Front,
                    content: RootContent::Move(payload),
                },
                context: ProductCommandContext::FloatCreate {
                    item,
                    root,
                    surface,
                    floating,
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
                let contained = self.capture_product_contained(item)?;
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::UpdateContainedPresentation {
                        source: contained.source,
                        floating,
                        expected_rect: contained.rect,
                        expected_roster: contained.roster,
                        rect,
                        position: ContainedPosition::Front,
                    },
                    context: ProductCommandContext::FloatUpdate {
                        item,
                        root: source_root,
                        surface,
                        floating,
                    },
                })
            }
            crate::RootPresentationOwner::Contained { floating, .. } => {
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::RehomeRoot {
                        source: root_source,
                        target: RootPresentationTarget::Contained {
                            surface,
                            floating,
                            rect,
                            position: ContainedPosition::Front,
                        },
                    },
                    context: ProductCommandContext::FloatRehome {
                        item,
                        root: source_root,
                        surface,
                        floating,
                    },
                })
            }
            crate::RootPresentationOwner::Main { .. } => {
                let floating = self.next_product_floating()?;
                Ok(ProductActionPlan::Command {
                    command: WorkspaceCommand::RehomeRoot {
                        source: root_source,
                        target: RootPresentationTarget::Contained {
                            surface,
                            floating,
                            rect,
                            position: ContainedPosition::Front,
                        },
                    },
                    context: ProductCommandContext::FloatRehome {
                        item,
                        root: source_root,
                        surface,
                        floating,
                    },
                })
            }
        }
    }

    pub(super) fn compile_product_contained_rect(
        &self,
        item: ItemId,
        rect: LogicalRect,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let contained = self.capture_product_contained(item)?;
        Ok(ProductActionPlan::Command {
            command: WorkspaceCommand::UpdateContainedRect {
                surface: contained.surface,
                root: contained.root,
                floating: contained.floating,
                expected_rect: contained.rect,
                rect,
            },
            context: ProductCommandContext::SetContainedRect {
                root: contained.root,
                surface: contained.surface,
                floating: contained.floating,
            },
        })
    }

    pub(super) fn compile_product_contained_raise(
        &self,
        item: ItemId,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let contained = self.capture_product_contained(item)?;
        Ok(ProductActionPlan::Command {
            command: WorkspaceCommand::RaiseContained {
                source: contained.source,
                floating: contained.floating,
                expected_roster: contained.roster,
            },
            context: ProductCommandContext::RaiseContained {
                root: contained.root,
                surface: contained.surface,
                floating: contained.floating,
            },
        })
    }

    pub(super) fn compile_product_contained_bring_into_view(
        &self,
        item: ItemId,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let contained = self.capture_product_contained(item)?;
        let ready = self
            .scene()
            .surface(contained.surface)
            .and_then(crate::scene::SurfaceScene::ready)
            .filter(|ready| ready.stamp().requirement().workspace_epoch() == self.version().epoch())
            .ok_or(DockspaceActionRejection::PresentationUnavailable {
                surface: contained.surface,
            })?;
        let minimum = ready
            .plan()
            .contained_minimums()
            .iter()
            .find(|minimum| minimum.floating() == contained.floating)
            .map(|minimum| minimum.minimum_size())
            .ok_or(DockspaceActionRejection::Conflict)?;
        let rect = clamp_contained_rect(
            contained.surface,
            ready.plan().bounds(),
            contained.rect,
            minimum,
        )
        .map_err(|_| DockspaceActionRejection::Conflict)?;
        Ok(ProductActionPlan::Command {
            command: WorkspaceCommand::UpdateContainedRect {
                surface: contained.surface,
                root: contained.root,
                floating: contained.floating,
                expected_rect: contained.rect,
                rect,
            },
            context: ProductCommandContext::SetContainedRect {
                root: contained.root,
                surface: contained.surface,
                floating: contained.floating,
            },
        })
    }

    fn capture_product_contained(
        &self,
        item: ItemId,
    ) -> Result<ProductContainedSource, DockspaceActionRejection> {
        let item_source = self.capture_product_item(item)?;
        let root = item_source.root();
        let crate::RootPresentationOwner::Contained { surface, floating } = self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::Conflict)?
        else {
            return Err(DockspaceActionRejection::ItemNotContained { item });
        };
        let root_record = self
            .workspace
            .root(root)
            .ok_or(DockspaceActionRejection::Conflict)?;
        let source = self
            .workspace
            .capture_node_source(root, root_record.node)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        let rect = self
            .workspace
            .contained_floating(floating)
            .filter(|contained| contained.root == root)
            .map(|contained| contained.rect)
            .ok_or(DockspaceActionRejection::Conflict)?;
        let roster = self
            .workspace
            .capture_contained_roster(surface)
            .map_err(|_| DockspaceActionRejection::Conflict)?;
        Ok(ProductContainedSource {
            root,
            surface,
            floating,
            rect,
            source,
            roster,
        })
    }

    fn require_product_surface(&self, surface: SurfaceId) -> Result<(), DockspaceActionRejection> {
        self.workspace
            .surface(surface)
            .map(|_| ())
            .ok_or(DockspaceActionRejection::SurfaceUnavailable { surface })
    }

    fn next_product_floating(&self) -> Result<FloatingPresentationId, DockspaceActionRejection> {
        self.prepare_presentation_floating_identity()
            .ok_or(DockspaceActionRejection::IdentityExhausted)
    }
}
