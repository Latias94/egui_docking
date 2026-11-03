//! Semantic admission of receivers from the output still visible to the host.

use super::*;
use crate::command::DockTarget;
use crate::graph::SplitWeight;
use crate::ids::NodeId;

impl DockEngine {
    pub(crate) fn presented_layering_is_current(&self, plan: &PresentationPlan) -> bool {
        self.workspace
            .surface(plan.surface())
            .is_some_and(|surface| {
                plan.contained_order()
                    .is_none_or(|order| order == surface.contained)
            })
    }

    fn current_presented_split_weights(
        &self,
        root: RootId,
        node: NodeId,
    ) -> Option<&[SplitWeight]> {
        self.interaction
            .active_resize_view()
            .and_then(|resize| {
                resize
                    .updates()
                    .iter()
                    .find(|update| update.split().root() == root && update.split().node() == node)
            })
            .map(SplitResize::weights)
            .or_else(|| match self.workspace.node(node) {
                Some(Node::Split { weights, .. }) => Some(weights.as_slice()),
                Some(Node::Tabs { .. }) | None => None,
            })
    }

    fn presented_split_weights_are_current(
        &self,
        root: RootId,
        node: NodeId,
        weights: &[SplitWeight],
    ) -> bool {
        self.current_presented_split_weights(root, node) == Some(weights)
    }

    pub(crate) fn presented_transient_geometry_is_current(&self, plan: &PresentationPlan) -> bool {
        plan.contained_projection().is_none_or(|projection| {
            self.current_presented_contained_rect(projection.surface, projection.floating)
                == Some(projection.rect)
        }) && plan.resize_projection().iter().all(|update| {
            self.presented_split_weights_are_current(
                update.split().root(),
                update.split().node(),
                update.weights(),
            )
        })
    }

    fn current_presented_contained_rect(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    ) -> Option<LogicalRect> {
        self.contained_layout_projection(surface)
            .filter(|projection| projection.floating == floating)
            .map(|projection| projection.rect)
            .or_else(|| {
                self.workspace
                    .contained_floating(floating)
                    .map(|record| record.rect)
            })
    }
    pub(crate) fn presented_root_structure_is_current(
        &self,
        plan: &PresentationPlan,
        root: RootId,
    ) -> bool {
        if !self.presented_root_owner_is_current(plan, root) {
            return false;
        }
        let frozen = plan.drop_targets().iter().find_map(|record| {
            let crate::drop_target::DropDestination::Topology(target) = record.destination() else {
                return None;
            };
            let fingerprint = match target {
                DockTarget::Center(target) | DockTarget::TabGap { target, .. } => {
                    target.fingerprint()
                }
                DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target) => {
                    target.fingerprint()
                }
            };
            (fingerprint.root() == root).then_some(fingerprint)
        });
        frozen.is_some_and(|frozen| {
            self.workspace
                .root_fingerprint(root)
                .is_ok_and(|current| frozen.presentation_topology_eq(&current))
                && frozen.0.nodes.iter().all(|node| {
                    let crate::command::FingerprintNode::Split { weight_bits, .. } = &node.node
                    else {
                        return true;
                    };
                    if let Some(update) = plan.resize_projection().iter().find(|update| {
                        update.split().root() == root && update.split().node() == node.id
                    }) {
                        return self.presented_split_weights_are_current(
                            root,
                            node.id,
                            update.weights(),
                        );
                    }
                    let expected = self.current_presented_split_weights(root, node.id);
                    expected.is_some_and(|weights| {
                        weights
                            .iter()
                            .map(|weight| weight.get().to_bits())
                            .eq(weight_bits.iter().copied())
                    })
                })
        })
    }

    fn presented_root_owner_is_current(&self, plan: &PresentationPlan, root: RootId) -> bool {
        match self.workspace.presentation_for_root(root) {
            Some(crate::RootPresentationOwner::Main { surface }) => {
                surface == plan.surface()
                    && !plan
                        .contained_records()
                        .iter()
                        .any(|record| record.root() == root)
            }
            Some(crate::RootPresentationOwner::Contained { surface, floating }) => {
                surface == plan.surface()
                    && plan
                        .contained_record(floating)
                        .is_some_and(|record| record.root() == root)
                    && plan.unclipped_contained_bounds(floating)
                        == self.current_presented_contained_rect(surface, floating)
            }
            None => false,
        }
    }

    pub(crate) fn presented_receiver_is_current(
        &self,
        plan: &PresentationPlan,
        region: PresentationHitRegionId,
    ) -> bool {
        if !self.presented_layering_is_current(plan) {
            return false;
        }
        let item_is_current = |tab: crate::scene::TabSceneId| {
            self.workspace
                .locate_item_by_id(tab.item)
                .is_some_and(|(root, tabs, _)| root == tab.root && tabs == tab.tabs)
                && self.presented_root_owner_is_current(plan, tab.root)
        };
        let bar_is_current = |bar: crate::scene::TabBarSceneId| {
            let Some(record) = plan
                .tab_bar_records()
                .iter()
                .find(|record| *record.id() == bar)
            else {
                return false;
            };
            matches!(self.workspace.node(bar.tabs), Some(Node::Tabs { items, .. })
                if items.iter().copied().eq(record.members().iter().map(|member| member.tab().item)))
                && self.presented_root_owner_is_current(plan, bar.root)
        };
        match region.kind() {
            PresentationHitRegionKind::TabBody(tab)
            | PresentationHitRegionKind::TabClose(tab)
            | PresentationHitRegionKind::TabListMenuRow { tab, .. } => item_is_current(tab),
            PresentationHitRegionKind::TabGroupDrag { bar, .. }
            | PresentationHitRegionKind::TabStripScroll(bar) => bar_is_current(bar),
            PresentationHitRegionKind::PaneBody(pane) => {
                self.presented_root_owner_is_current(plan, pane.root)
                    && plan
                        .pane_records()
                        .iter()
                        .find(|record| record.id() == pane)
                        .is_some_and(|record| {
                            matches!(self.workspace.node(pane.tabs),
                            Some(Node::Tabs { selected, .. }) if *selected == record.selected())
                        })
            }
            PresentationHitRegionKind::SplitterHandle(splitter) => {
                self.presented_root_owner_is_current(plan, splitter.root)
                    && self.presented_root_structure_is_current(plan, splitter.root)
            }
            PresentationHitRegionKind::SplitterJunction(junction) => {
                junction.splitters().iter().all(|splitter| {
                    self.presented_root_owner_is_current(plan, splitter.root)
                        && self.presented_root_structure_is_current(plan, splitter.root)
                })
            }
            PresentationHitRegionKind::ContainedTitle(floating)
            | PresentationHitRegionKind::ContainedClose(floating)
            | PresentationHitRegionKind::ContainedResize { floating, .. }
            | PresentationHitRegionKind::ContainedFrameBlocker(floating) => {
                plan.contained_record(floating).is_some_and(|record| {
                    self.presented_root_owner_is_current(plan, record.root())
                        && self.presented_root_structure_is_current(plan, record.root())
                })
            }
            PresentationHitRegionKind::DropGuideActivation(cluster) => {
                self.presented_root_structure_is_current(plan, cluster.root)
            }
            PresentationHitRegionKind::DropTarget(target) => match target {
                crate::drop_target::DropTargetId::SurfaceBackground { surface } => self
                    .workspace
                    .surface(surface)
                    .is_some_and(|presentation| presentation.main_root.is_none()),
                crate::drop_target::DropTargetId::TabGap { root, .. }
                | crate::drop_target::DropTargetId::Center { root, .. }
                | crate::drop_target::DropTargetId::InnerEdge { root, .. }
                | crate::drop_target::DropTargetId::OuterEdge { root, .. } => {
                    self.presented_root_structure_is_current(plan, root)
                }
            },
            PresentationHitRegionKind::TabStripControl(control) => bar_is_current(control.bar()),
            PresentationHitRegionKind::TabListMenuScroll(_)
            | PresentationHitRegionKind::TabListMenuBlocker(_)
            | PresentationHitRegionKind::TabListMenuBackdrop(_) => true,
        }
    }
}
