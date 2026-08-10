//! Opaque product paint records for docking-guide geometry and resolver state.

use std::fmt;

use crate::drop_guide::{DropGuideScope, DropGuideSlot};
use crate::drop_resolver::{
    DropAffordance, DropAffordanceCluster, DropAffordanceTarget,
    DropGuideEligibility as CoreDropGuideEligibility, DropGuideTargetKey,
};
use crate::drop_target::{DropTargetAvailability, DropTargetKind};
use crate::geometry::LogicalRect;
use crate::ids::{RootId, SurfaceId};

use super::{DockspacePaintLayer, DockspaceVisualId, VisualIdentity};

/// Public guide-cluster class without the internal target-node identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceGuideScope {
    /// Guide cluster local to one tabs leaf.
    Inner,
    /// Guide cluster spanning one root edge.
    Outer,
}

/// Product-facing direction represented by one docking-guide button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceDropDirection {
    /// Merge into the target tab stack.
    Center,
    /// Split at the target's left edge.
    Left,
    /// Split at the target's right edge.
    Right,
    /// Split at the target's top edge.
    Top,
    /// Split at the target's bottom edge.
    Bottom,
}

impl DockspaceDropDirection {
    const fn from_core(slot: DropGuideSlot) -> Self {
        match slot {
            DropGuideSlot::Center => Self::Center,
            DropGuideSlot::Edge(crate::command::Edge::Left) => Self::Left,
            DropGuideSlot::Edge(crate::command::Edge::Right) => Self::Right,
            DropGuideSlot::Edge(crate::command::Edge::Top) => Self::Top,
            DropGuideSlot::Edge(crate::command::Edge::Bottom) => Self::Bottom,
        }
    }
}

/// Payload-specific result for one visible docking-guide button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceDropEligibility {
    /// The frozen payload can commit to this exact target.
    Eligible,
    /// The button remains visible, but policy or topology rejects the payload.
    Rejected,
}

/// Read-only guide target geometry and availability.
#[derive(Clone, Copy)]
pub struct DropGuideTargetPaintRecord<'plan> {
    pub(super) slot: DropGuideSlot,
    pub(super) record: &'plan crate::drop_guide::DropGuideTargetRecord,
}

impl fmt::Debug for DropGuideTargetPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropGuideTargetPaintRecord")
            .field("visual_id", &self.visual_id())
            .field("direction", &self.direction())
            .field("kind", &self.kind())
            .field("draw_bounds", &self.draw_bounds())
            .field("hit_bounds", &self.hit_bounds())
            .field("preview_bounds", &self.preview_bounds())
            .field("availability", &self.availability())
            .finish()
    }
}

impl DropGuideTargetPaintRecord<'_> {
    /// Returns an opaque visual identity suitable for renderer widget keys.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropTarget(self.record.id()))
    }

    /// Returns the semantic center or edge represented by this button.
    #[must_use]
    pub const fn direction(self) -> DockspaceDropDirection {
        DockspaceDropDirection::from_core(self.slot)
    }

    /// Returns the target's stable semantic class.
    #[must_use]
    pub const fn kind(self) -> DropTargetKind {
        self.record.id().kind()
    }

    /// Returns the exact visible button rectangle.
    #[must_use]
    pub const fn draw_bounds(self) -> LogicalRect {
        self.record.draw()
    }

    /// Returns the exact pointer hit rectangle.
    #[must_use]
    pub const fn hit_bounds(self) -> LogicalRect {
        self.record.target().region().rect()
    }

    /// Returns the exact preview rectangle for this static target.
    #[must_use]
    pub const fn preview_bounds(self) -> LogicalRect {
        self.record.target().visual().rect()
    }

    /// Returns scene-level availability before a payload is evaluated.
    #[must_use]
    pub const fn availability(self) -> DropTargetAvailability {
        self.record.target().availability()
    }
}

/// One payload-specific guide button selected by the core resolver.
#[derive(Clone, Copy)]
pub struct DropAffordanceTargetPaintRecord<'plan> {
    target: &'plan DropAffordanceTarget,
    active: Option<DropGuideTargetKey>,
}

impl fmt::Debug for DropAffordanceTargetPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropAffordanceTargetPaintRecord")
            .field("visual_id", &self.visual_id())
            .field("direction", &self.direction())
            .field("draw_bounds", &self.draw_bounds())
            .field("active", &self.is_active())
            .field("eligibility", &self.eligibility())
            .finish()
    }
}

impl DropAffordanceTargetPaintRecord<'_> {
    /// Returns an opaque visual identity suitable for renderer widget keys.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropTarget(self.target.target_id()))
    }

    /// Returns the semantic center or edge represented by this button.
    #[must_use]
    pub const fn direction(self) -> DockspaceDropDirection {
        DockspaceDropDirection::from_core(self.target.slot())
    }

    /// Returns the exact visible button rectangle.
    #[must_use]
    pub const fn draw_bounds(self) -> LogicalRect {
        self.target.draw()
    }

    /// Returns whether this exact button won the authoritative hit query.
    #[must_use]
    pub fn is_active(self) -> bool {
        self.active == Some(self.target.key())
    }

    /// Returns payload-specific eligibility without exposing command rejection internals.
    #[must_use]
    pub const fn eligibility(self) -> DockspaceDropEligibility {
        match self.target.eligibility() {
            CoreDropGuideEligibility::Eligible => DockspaceDropEligibility::Eligible,
            CoreDropGuideEligibility::Rejected(_) => DockspaceDropEligibility::Rejected,
        }
    }
}

/// One complete guide cluster selected for the current drag point.
#[derive(Clone, Copy)]
pub struct DropAffordanceClusterPaintRecord<'plan> {
    cluster: &'plan DropAffordanceCluster,
    active: Option<DropGuideTargetKey>,
}

impl fmt::Debug for DropAffordanceClusterPaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropAffordanceClusterPaintRecord")
            .field("visual_id", &self.visual_id())
            .field("root", &self.root())
            .field("scope", &self.scope())
            .field("activation_bounds", &self.activation_bounds())
            .field("layer", &self.layer())
            .field("target_count", &self.cluster.targets().len())
            .finish()
    }
}

impl<'plan> DropAffordanceClusterPaintRecord<'plan> {
    /// Returns the opaque cluster identity.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropGuide(self.cluster.id()))
    }

    /// Returns the stable root containing this cluster.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.cluster.id().root
    }

    /// Returns whether the cluster is local to a leaf or spans the root edge.
    #[must_use]
    pub const fn scope(self) -> DockspaceGuideScope {
        match self.cluster.id().scope {
            DropGuideScope::Inner(_) => DockspaceGuideScope::Inner,
            DropGuideScope::Outer => DockspaceGuideScope::Outer,
        }
    }

    /// Returns the exact region which made this cluster visible.
    #[must_use]
    pub const fn activation_bounds(self) -> LogicalRect {
        self.cluster.activation().rect()
    }

    /// Returns the core-derived paint layer.
    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.cluster.layer())
    }

    /// Iterates all center/edge buttons in canonical paint order.
    pub fn targets(
        self,
    ) -> impl ExactSizeIterator<Item = DropAffordanceTargetPaintRecord<'plan>> + 'plan {
        let active = self.active;
        self.cluster
            .targets()
            .iter()
            .map(move |target| DropAffordanceTargetPaintRecord { target, active })
    }
}

/// Complete core-owned docking-guide affordance for one authoritative drag point.
#[derive(Clone, Copy)]
pub struct DropAffordancePaintRecord<'plan> {
    pub(super) affordance: &'plan DropAffordance,
}

impl fmt::Debug for DropAffordancePaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropAffordancePaintRecord")
            .field("surface", &self.surface())
            .field("cluster_count", &self.affordance.clusters().len())
            .finish()
    }
}

impl<'plan> DropAffordancePaintRecord<'plan> {
    /// Returns the logical surface containing the selected guide clusters.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.affordance.surface()
    }

    /// Iterates selected clusters from back to front.
    pub fn clusters(
        self,
    ) -> impl ExactSizeIterator<Item = DropAffordanceClusterPaintRecord<'plan>> + 'plan {
        let active = self
            .affordance
            .active_target()
            .map(DropAffordanceTarget::key);
        self.affordance
            .clusters()
            .iter()
            .map(move |cluster| DropAffordanceClusterPaintRecord { cluster, active })
    }
}

/// Read-only complete docking-guide cluster.
#[derive(Clone, Copy)]
pub struct DropGuidePaintRecord<'plan> {
    pub(super) record: &'plan crate::drop_guide::DropGuideClusterRecord,
}

impl fmt::Debug for DropGuidePaintRecord<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropGuidePaintRecord")
            .field("visual_id", &self.visual_id())
            .field("root", &self.root())
            .field("scope", &self.scope())
            .field("activation_bounds", &self.activation_bounds())
            .field("layer", &self.layer())
            .field("target_count", &self.record.targets().count())
            .finish()
    }
}

impl<'plan> DropGuidePaintRecord<'plan> {
    /// Returns an opaque visual identity suitable for renderer widget keys.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropGuide(self.record.id()))
    }

    /// Returns the stable root containing this cluster.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.record.id().root
    }

    /// Returns whether the cluster is local to a leaf or spans the root edge.
    #[must_use]
    pub const fn scope(self) -> DockspaceGuideScope {
        match self.record.id().scope {
            DropGuideScope::Inner(_) => DockspaceGuideScope::Inner,
            DropGuideScope::Outer => DockspaceGuideScope::Outer,
        }
    }

    /// Returns the exact region which activates this cluster.
    #[must_use]
    pub const fn activation_bounds(self) -> LogicalRect {
        self.record.activation().rect()
    }

    /// Returns the core-derived paint layer.
    #[must_use]
    pub const fn layer(self) -> DockspacePaintLayer {
        DockspacePaintLayer::from_core(self.record.layer())
    }

    /// Iterates all center/edge buttons in canonical paint order.
    pub fn targets(
        self,
    ) -> impl DoubleEndedIterator<Item = DropGuideTargetPaintRecord<'plan>> + 'plan {
        self.record
            .targets()
            .map(|(slot, record)| DropGuideTargetPaintRecord { slot, record })
    }
}
