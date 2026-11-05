//! Core-owned renderer decoration for one active drag source.

use std::collections::BTreeSet;
use std::fmt;

use crate::command::{
    FingerprintNode, FingerprintPresentation, MovePayload, NodeFingerprint, NodeSource,
};
use crate::geometry::LogicalRect;
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::interaction::{ActiveDragView, DragPhase, DragSessionId};
use crate::scene::{
    ContainedLayoutOwner, PaneSceneId, PresentationPlan, SurfaceSceneSet, TabSceneId,
};

use super::{DockspaceVisualId, VisualIdentity};

/// Renderer-facing semantic class of the content currently being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceDragSourceKind {
    /// One tab item.
    Item,
    /// One complete tabs leaf, including its pane body.
    TabGroup,
    /// One proper structural subtree.
    Subtree,
    /// One complete root.
    Root,
}

/// Stable, renderer-neutral decoration facts for one core-owned active drag.
///
/// This value is intentionally independent from docking preview acknowledgement.
/// It exposes no graph node, scene stamp, preview token, or gesture session.
#[derive(Clone, Copy)]
pub struct DockspaceDragDecoration<'frame> {
    source_surface: SurfaceId,
    kind: DockspaceDragSourceKind,
    source_visual: DragSourceVisual,
    source_bounds: LogicalRect,
    projected_root_bounds: Option<LogicalRect>,
    source_is_projected: bool,
    item: Option<ItemId>,
    ghost_item: Option<ItemId>,
    source: DragDecorationSource<'frame>,
}

impl fmt::Debug for DockspaceDragDecoration<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceDragDecoration")
            .field("source_surface", &self.source_surface)
            .field("kind", &self.kind)
            .field("source_visual", &self.source_visual())
            .field("source_bounds", &self.source_bounds)
            .field("source_is_projected", &self.source_is_projected)
            .field("item", &self.item)
            .field("ghost_item", &self.ghost_item)
            .finish_non_exhaustive()
    }
}

// A drag source needs only these four identities. Storing the general visual
// enum here makes every surface-program step pay for unrelated junction IDs.
#[derive(Clone, Copy)]
enum DragSourceVisual {
    Tab(TabSceneId),
    Pane(PaneSceneId),
    Contained(FloatingPresentationId),
    Subtree { root: RootId, node: NodeId },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragDecorationSource<'frame> {
    Item(TabSceneId),
    TabGroup(PaneSceneId),
    Subtree {
        root: crate::ids::RootId,
        node: NodeId,
        fingerprint: &'frame NodeFingerprint,
        complete_root: bool,
    },
}

impl DockspaceDragDecoration<'_> {
    /// Returns the surface which still presents the drag source.
    #[must_use]
    pub const fn source_surface(self) -> SurfaceId {
        self.source_surface
    }

    /// Returns the semantic class of the source payload.
    #[must_use]
    pub const fn kind(self) -> DockspaceDragSourceKind {
        self.kind
    }

    /// Returns an opaque stable identity for the decorated source visual.
    #[must_use]
    pub const fn source_visual(self) -> DockspaceVisualId {
        DockspaceVisualId(match self.source_visual {
            DragSourceVisual::Tab(tab) => VisualIdentity::Tab(tab),
            DragSourceVisual::Pane(pane) => VisualIdentity::Pane(pane),
            DragSourceVisual::Contained(floating) => VisualIdentity::Contained(floating),
            DragSourceVisual::Subtree { root, node } => VisualIdentity::DragSource { root, node },
        })
    }

    /// Returns the exact source-surface bounds represented by the decoration.
    #[must_use]
    pub const fn source_bounds(self) -> LogicalRect {
        self.source_bounds
    }

    /// Returns the full-window projection for a contained root drag.
    #[must_use]
    pub const fn projected_root_bounds(self) -> Option<LogicalRect> {
        self.projected_root_bounds
    }

    /// Returns whether the complete source tree is already painted at its projection.
    ///
    /// Adapters retain cursor feedback but must not paint a duplicate source ghost.
    #[must_use]
    pub const fn source_is_projected(self) -> bool {
        self.source_is_projected
    }

    /// Returns the dragged item when the source is one tab.
    #[must_use]
    pub const fn item(self) -> Option<ItemId> {
        self.item
    }

    /// Returns an item whose title may label a non-interactive pointer ghost.
    #[must_use]
    pub const fn ghost_item(self) -> Option<ItemId> {
        self.ghost_item
    }

    /// Returns whether the active source decoration omits one opaque visual.
    ///
    /// Adapters retain the dock visual's response, receiver, and adapter-owned
    /// accessibility registration; only its paint is omitted. Pane adapters may
    /// render application content through a transparent painter to preserve widget
    /// identity while leaving a real visual gap. Structural membership remains
    /// core-owned and no graph identity crosses this boundary.
    #[must_use]
    pub(super) fn omits_visual(self, visual: DockspaceVisualId) -> bool {
        if self.source_is_projected {
            return false;
        }
        match self.source {
            DragDecorationSource::Item(tab) => {
                matches!(visual.0, VisualIdentity::Tab(candidate) if candidate == tab)
            }
            DragDecorationSource::TabGroup(pane) => visual_belongs_to_tabs_leaf(visual, pane),
            DragDecorationSource::Subtree {
                root,
                node,
                fingerprint,
                complete_root,
            } => {
                visual == self.source_visual()
                    || visual_belongs_to_subtree(visual, root, node, fingerprint, complete_root)
            }
        }
    }
}

pub(super) fn resolve<'frame>(
    scenes: &'frame SurfaceSceneSet,
    drag: ActiveDragView<'frame>,
) -> Option<DockspaceDragDecoration<'frame>> {
    if drag.phase() != DragPhase::Dragging {
        return None;
    }
    let payload = drag.payload();
    let source_surface = source_surface(payload);
    let plan = scenes.surface(source_surface)?.ready()?.candidate().plan();
    match payload {
        MovePayload::Item(source) => {
            let bar = plan.tab_bar_records().iter().find(|record| {
                let id = record.id();
                id.root == source.root() && id.tabs == source.tabs()
            })?;
            // Strip members retain exact source geometry even after scrolling clips its paint.
            let member = bar
                .members()
                .iter()
                .find(|member| member.tab().item == source.item())?;
            let tab = member.tab();
            let projection = source_projection(plan, source_surface, source.root(), drag.session());
            Some(DockspaceDragDecoration {
                source_surface,
                kind: DockspaceDragSourceKind::Item,
                source_visual: DragSourceVisual::Tab(tab),
                source_bounds: member.full_bounds(),
                projected_root_bounds: projection,
                source_is_projected: projection.is_some(),
                item: Some(source.item()),
                ghost_item: Some(source.item()),
                source: DragDecorationSource::Item(tab),
            })
        }
        MovePayload::Tabs(source) => {
            let pane = plan.pane_records().iter().find(|record| {
                let id = record.id();
                id.root == source.root() && id.tabs == source.node()
            })?;
            let projection = (source.node() == source.fingerprint().0.root_node)
                .then(|| source_projection(plan, source_surface, source.root(), drag.session()))
                .flatten();
            Some(DockspaceDragDecoration {
                source_surface,
                kind: DockspaceDragSourceKind::TabGroup,
                source_visual: DragSourceVisual::Pane(pane.id()),
                source_bounds: pane.bounds(),
                projected_root_bounds: projection,
                source_is_projected: projection.is_some(),
                item: None,
                ghost_item: pane.selected(),
                source: DragDecorationSource::TabGroup(pane.id()),
            })
        }
        MovePayload::Subtree(source) => resolve_subtree(
            plan,
            source_surface,
            source,
            drag.session(),
            drag.contained_projection_rect(),
        ),
    }
}

fn source_surface(payload: &MovePayload) -> SurfaceId {
    let fingerprint = match payload {
        MovePayload::Item(source) => source.fingerprint(),
        MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.fingerprint(),
    };
    match fingerprint.0.presentation {
        FingerprintPresentation::Main { surface }
        | FingerprintPresentation::Contained { surface, .. } => surface,
    }
}

fn source_projection(
    plan: &PresentationPlan,
    surface: SurfaceId,
    root: RootId,
    session: DragSessionId,
) -> Option<LogicalRect> {
    let projection = plan.contained_projection()?;
    (projection.owner == ContainedLayoutOwner::Drag(session)
        && projection.surface == surface
        && projection.root == root
        && plan
            .contained_records()
            .iter()
            .any(|record| record.root() == root && record.floating() == projection.floating))
    .then_some(projection.rect)
}

fn resolve_subtree<'frame>(
    plan: &PresentationPlan,
    source_surface: SurfaceId,
    source: &'frame NodeSource,
    session: DragSessionId,
    pointer_projection: Option<LogicalRect>,
) -> Option<DockspaceDragDecoration<'frame>> {
    let complete_root = source.node() == source.fingerprint().0.root_node;
    let source_identity = DragDecorationSource::Subtree {
        root: source.root(),
        node: source.node(),
        fingerprint: source.fingerprint(),
        complete_root,
    };
    let ghost_item = subtree_ghost_item(plan, source);
    if complete_root
        && let Some(contained) = plan
            .contained_records()
            .iter()
            .find(|record| record.root() == source.root())
    {
        let projection = source_projection(plan, source_surface, source.root(), session);
        return Some(DockspaceDragDecoration {
            source_surface,
            kind: DockspaceDragSourceKind::Root,
            source_visual: DragSourceVisual::Contained(contained.floating()),
            source_bounds: contained.outer_bounds(),
            projected_root_bounds: projection.or(pointer_projection),
            source_is_projected: projection.is_some(),
            item: None,
            ghost_item,
            source: source_identity,
        });
    }

    Some(DockspaceDragDecoration {
        source_surface,
        kind: if complete_root {
            DockspaceDragSourceKind::Root
        } else {
            DockspaceDragSourceKind::Subtree
        },
        source_visual: DragSourceVisual::Subtree {
            root: source.root(),
            node: source.node(),
        },
        source_bounds: subtree_bounds(plan, source)?,
        projected_root_bounds: None,
        source_is_projected: false,
        item: None,
        ghost_item,
        source: source_identity,
    })
}

fn visual_belongs_to_tabs_leaf(visual: DockspaceVisualId, pane: PaneSceneId) -> bool {
    match visual.0 {
        VisualIdentity::Pane(candidate) => candidate == pane,
        VisualIdentity::Tab(candidate) => {
            candidate.root == pane.root && candidate.tabs == pane.tabs
        }
        VisualIdentity::TabBar(candidate) => {
            candidate.root == pane.root && candidate.tabs == pane.tabs
        }
        VisualIdentity::TabGroupDragRegion { bar, .. } => {
            bar.root == pane.root && bar.tabs == pane.tabs
        }
        VisualIdentity::TabStripControl(control) => {
            let bar = control.bar();
            bar.root == pane.root && bar.tabs == pane.tabs
        }
        VisualIdentity::TabListMenu(session)
        | VisualIdentity::TabListMenuRow { session, .. }
        | VisualIdentity::TabListMenuBackdrop { session, .. } => {
            let bar = session.key().bar();
            bar.root == pane.root && bar.tabs == pane.tabs
        }
        VisualIdentity::PresentationMenuAnchor(_) => false,
        VisualIdentity::DragSource { .. }
        | VisualIdentity::Splitter(_)
        | VisualIdentity::SplitterJunction(_)
        | VisualIdentity::Contained(_)
        | VisualIdentity::DropGuide(_)
        | VisualIdentity::DropTarget(_)
        | VisualIdentity::Receiver(_) => false,
    }
}

fn visual_belongs_to_subtree(
    visual: DockspaceVisualId,
    root: crate::ids::RootId,
    source: NodeId,
    fingerprint: &NodeFingerprint,
    complete_root: bool,
) -> bool {
    match visual.0 {
        VisualIdentity::Pane(candidate) => {
            candidate.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, candidate.tabs))
        }
        VisualIdentity::Tab(candidate) => {
            candidate.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, candidate.tabs))
        }
        VisualIdentity::TabBar(candidate) => {
            candidate.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, candidate.tabs))
        }
        VisualIdentity::TabGroupDragRegion { bar, .. } => {
            bar.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, bar.tabs))
        }
        VisualIdentity::TabStripControl(control) => {
            let bar = control.bar();
            bar.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, bar.tabs))
        }
        VisualIdentity::TabListMenu(session)
        | VisualIdentity::TabListMenuRow { session, .. }
        | VisualIdentity::TabListMenuBackdrop { session, .. } => {
            let bar = session.key().bar();
            bar.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, bar.tabs))
        }
        VisualIdentity::PresentationMenuAnchor(candidate) => candidate == root && complete_root,
        VisualIdentity::Splitter(splitter) => {
            splitter.root == root
                && (complete_root || subtree_contains_node(fingerprint, source, splitter.split))
        }
        VisualIdentity::SplitterJunction(junction) => {
            junction.root() == root
                && (complete_root
                    || junction
                        .splitters()
                        .into_iter()
                        .all(|splitter| subtree_contains_node(fingerprint, source, splitter.split)))
        }
        VisualIdentity::DragSource {
            root: candidate,
            node,
        } => {
            candidate == root && (complete_root || subtree_contains_node(fingerprint, source, node))
        }
        VisualIdentity::Contained(_)
        | VisualIdentity::DropGuide(_)
        | VisualIdentity::DropTarget(_)
        | VisualIdentity::Receiver(_) => false,
    }
}

fn subtree_contains_node(fingerprint: &NodeFingerprint, source: NodeId, target: NodeId) -> bool {
    if source == target {
        return true;
    }
    let Some(record) = fingerprint
        .0
        .nodes
        .iter()
        .find(|record| record.id == source)
    else {
        return false;
    };
    match &record.node {
        FingerprintNode::Tabs { .. } => false,
        FingerprintNode::Split { children, .. } => children
            .iter()
            .copied()
            .any(|child| subtree_contains_node(fingerprint, child, target)),
    }
}

fn subtree_ghost_item(plan: &PresentationPlan, source: &NodeSource) -> Option<ItemId> {
    let tabs = subtree_tabs(source.fingerprint(), source.node())?;
    plan.pane_records()
        .iter()
        .find(|record| {
            let id = record.id();
            id.root == source.root() && tabs.contains(&id.tabs)
        })
        .and_then(|record| record.selected())
}

fn subtree_bounds(plan: &PresentationPlan, source: &NodeSource) -> Option<LogicalRect> {
    let tabs = subtree_tabs(source.fingerprint(), source.node())?;
    let mut panes = tabs.into_iter().map(|tabs| {
        plan.pane_records()
            .iter()
            .find(|record| {
                let id = record.id();
                id.root == source.root() && id.tabs == tabs
            })
            .map(|record| record.bounds())
    });
    let mut bounds = panes.next()??;
    for pane in panes {
        bounds = union_rect(bounds, pane?)?;
    }
    Some(bounds)
}

fn subtree_tabs(fingerprint: &NodeFingerprint, source: NodeId) -> Option<BTreeSet<NodeId>> {
    let mut pending = vec![source];
    let mut visited = BTreeSet::new();
    let mut tabs = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node) {
            continue;
        }
        let record = fingerprint
            .0
            .nodes
            .iter()
            .find(|record| record.id == node)?;
        match &record.node {
            FingerprintNode::Tabs { .. } => {
                tabs.insert(node);
            }
            FingerprintNode::Split { children, .. } => {
                pending.extend(children.iter().copied());
            }
        }
    }
    (!tabs.is_empty()).then_some(tabs)
}

fn union_rect(left: LogicalRect, right: LogicalRect) -> Option<LogicalRect> {
    let min_x = left.x().min(right.x());
    let min_y = left.y().min(right.y());
    let max_x = left.max().x().max(right.max().x());
    let max_y = left.max().y().max(right.max().y());
    LogicalRect::new(min_x, min_y, max_x - min_x, max_y - min_y).ok()
}
