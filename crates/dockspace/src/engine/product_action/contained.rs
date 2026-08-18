use super::*;

use crate::command::{ContainedPosition, ContainedRosterSource};
use crate::geometry::LogicalRect;
use crate::model::{DockAnchor, DockEdge, DockFraction, DockPlacement};

struct ProductContainedSource {
    root: RootId,
    surface: SurfaceId,
    floating: FloatingPresentationId,
    rect: LogicalRect,
    source: NodeSource,
    roster: ContainedRosterSource,
}

impl DockEngine {
    pub(in crate::engine) fn derive_product_root_float_rect(
        &self,
        root: RootId,
        surface: SurfaceId,
    ) -> Result<LogicalRect, DockspaceActionRejection> {
        self.require_product_surface(surface)?;
        let target = self
            .interaction_projection(surface)
            .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?;
        let bounds = target.plan().bounds();
        let source = match self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?
        {
            crate::RootPresentationOwner::Contained { floating, .. } => self
                .workspace
                .contained_floating(floating)
                .map(|contained| contained.rect)
                .ok_or(DockspaceActionRejection::Conflict)?,
            crate::RootPresentationOwner::Main {
                surface: source_surface,
            } => self
                .interaction_projection(source_surface)
                .and_then(|projection| projection.plan().layout_facts())
                .and_then(|facts| facts.root(root))
                .map(|facts| facts.bounds())
                .ok_or(DockspaceActionRejection::PresentationUnavailable {
                    surface: source_surface,
                })?,
        };
        self.derive_product_root_float_rect_from_bounds(surface, bounds, source)
    }

    pub(in crate::engine) fn derive_product_root_float_rect_from_plan(
        &self,
        root: RootId,
        surface: SurfaceId,
        plan: &crate::scene::PresentationPlan,
    ) -> Result<LogicalRect, DockspaceActionRejection> {
        self.require_product_surface(surface)?;
        let source = match self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?
        {
            crate::RootPresentationOwner::Contained {
                surface: owner_surface,
                floating,
            } if owner_surface == surface => plan
                .contained_record(floating)
                .filter(|record| record.root() == root)
                .map(crate::scene::ContainedRecord::outer_bounds)
                .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?,
            crate::RootPresentationOwner::Main {
                surface: owner_surface,
            } if owner_surface == surface => plan
                .layout_facts()
                .and_then(|facts| facts.root(root))
                .map(|facts| facts.bounds())
                .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?,
            crate::RootPresentationOwner::Main {
                surface: owner_surface,
            }
            | crate::RootPresentationOwner::Contained {
                surface: owner_surface,
                ..
            } => {
                return Err(DockspaceActionRejection::PresentationUnavailable {
                    surface: owner_surface,
                });
            }
        };
        self.derive_product_root_float_rect_from_bounds(surface, plan.bounds(), source)
    }

    fn derive_product_root_float_rect_from_bounds(
        &self,
        surface: SurfaceId,
        bounds: LogicalRect,
        source: LogicalRect,
    ) -> Result<LogicalRect, DockspaceActionRejection> {
        let minimum = self.presentation_config().minimum_floating_size();
        let width = source
            .width()
            .min(bounds.width() * 0.72)
            .max(minimum.width())
            .min(bounds.width());
        let height = source
            .height()
            .min(bounds.height() * 0.72)
            .max(minimum.height())
            .min(bounds.height());
        let requested = LogicalRect::new(
            bounds.x() + (bounds.width() - width) * 0.5,
            bounds.y() + (bounds.height() - height) * 0.5,
            width,
            height,
        )
        .map_err(|_| DockspaceActionRejection::Conflict)?;
        clamp_contained_rect(surface, bounds, requested, minimum)
            .map_err(|_| DockspaceActionRejection::Conflict)
    }

    pub(in crate::engine) fn compile_product_dock_back(
        &self,
        root: RootId,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        let crate::RootPresentationOwner::Contained { surface, .. } = self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?
        else {
            return Err(DockspaceActionRejection::DockBackUnavailable { root });
        };
        let placement = self.derive_product_dock_back_placement(root, surface)?;
        self.compile_product_root_move(root, placement)
    }

    fn derive_product_dock_back_placement(
        &self,
        root: RootId,
        surface: SurfaceId,
    ) -> Result<DockPlacement, DockspaceActionRejection> {
        let presentation = self
            .workspace
            .surface(surface)
            .ok_or(DockspaceActionRejection::DockBackUnavailable { root })?;
        match presentation.main_root {
            None => Ok(DockPlacement::Main(surface)),
            Some(target_root) => {
                let source = self
                    .workspace
                    .root(root)
                    .and_then(|record| self.workspace.node(record.node))
                    .ok_or(DockspaceActionRejection::DockBackUnavailable { root })?;
                if matches!(source, Node::Split { .. }) {
                    let fraction =
                        DockFraction::new(self.presentation_config().dock_fraction() as f32)
                            .map_err(|_| DockspaceActionRejection::DockBackUnavailable { root })?;
                    return Ok(DockPlacement::OuterEdge {
                        root: target_root,
                        edge: DockEdge::Right,
                        fraction,
                    });
                }
                let target = self
                    .workspace
                    .root(target_root)
                    .ok_or(DockspaceActionRejection::DockBackUnavailable { root })?;
                match target.central {
                    Some(_) => Ok(DockPlacement::Center(DockAnchor::Central(target_root))),
                    None => {
                        let item = self
                            .workspace
                            .collect_items_in_subtree(target.node)
                            .into_iter()
                            .next()
                            .ok_or(DockspaceActionRejection::DockBackUnavailable { root })?;
                        Ok(DockPlacement::Center(DockAnchor::Item(item)))
                    }
                }
            }
        }
    }

    pub(in crate::engine) fn compile_product_root_float(
        &self,
        root: RootId,
        surface: SurfaceId,
        rect: LogicalRect,
    ) -> Result<ProductActionPlan, DockspaceActionRejection> {
        self.require_product_surface(surface)?;
        let (source, payload, items) = self.capture_product_root_payload(root)?;
        let complete = self
            .capture_complete_root_payload(&payload)?
            .filter(|complete| complete == &source)
            .ok_or(DockspaceActionRejection::Conflict)?;
        let floating = match self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::Conflict)?
        {
            crate::RootPresentationOwner::Contained { floating, .. } => floating,
            crate::RootPresentationOwner::Main { .. } => self.next_product_floating()?,
        };
        Ok(ProductActionPlan::Command {
            command: WorkspaceCommand::RehomeRoot {
                source: complete,
                target: RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    position: ContainedPosition::Front,
                },
            },
            context: ProductCommandContext::FloatRootRehome {
                root,
                surface,
                floating,
                items,
            },
        })
    }

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
