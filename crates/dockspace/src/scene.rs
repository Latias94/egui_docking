//! Type-state construction of immutable semantic docking scenes.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use thiserror::Error;

use crate::command::{DockTarget, NodeFingerprint};
use crate::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetKind, DropTargetRecord,
    DropTargetUnavailable,
};
use crate::geometry::LogicalRect;
use crate::graph::{Node, Workspace};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::DockPolicy;
use crate::transition::WorkspaceVersion;

pub use crate::drop_target::SceneLayerKey;

/// Monotonic renderer-scene generation within an engine lifetime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SceneGeneration(u64);

impl SceneGeneration {
    /// Creates a scene generation from its runtime representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the runtime representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the generation without wrapping.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Exact workspace and renderer generation from which a scene was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneStamp {
    workspace: WorkspaceVersion,
    generation: SceneGeneration,
}

impl PartialOrd for SceneStamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SceneStamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.workspace
            .epoch()
            .cmp(&other.workspace.epoch())
            .then(self.workspace.revision().cmp(&other.workspace.revision()))
            .then(self.generation.cmp(&other.generation))
    }
}

impl SceneStamp {
    /// Creates an exact scene stamp.
    #[must_use]
    pub(crate) const fn new(workspace: WorkspaceVersion, generation: SceneGeneration) -> Self {
        Self {
            workspace,
            generation,
        }
    }

    /// Returns the durable state version used to build the scene.
    #[must_use]
    pub const fn workspace(self) -> WorkspaceVersion {
        self.workspace
    }

    /// Returns the renderer scene generation.
    #[must_use]
    pub const fn generation(self) -> SceneGeneration {
        self.generation
    }
}

/// Structural identity of a rendered node rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeSceneId {
    /// Owning root.
    pub root: RootId,
    /// Rendered node.
    pub node: NodeId,
}

/// Structural identity of a rendered tab-bar rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabBarSceneId {
    /// Owning root.
    pub root: RootId,
    /// Tabs node owning the bar.
    pub tabs: NodeId,
}

/// Structural identity of one rendered tab rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabSceneId {
    /// Owning root.
    pub root: RootId,
    /// Tabs node owning the item.
    pub tabs: NodeId,
    /// Stable item identity.
    pub item: ItemId,
}

/// Structural identity of one rendered splitter rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SplitterSceneId {
    /// Owning root.
    pub root: RootId,
    /// Split node owning the separator.
    pub split: NodeId,
    /// Gap before child `index + 1`.
    pub index: usize,
}

/// Exact rectangle and layer for one semantic scene identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticRect<Id> {
    id: Id,
    rect: LogicalRect,
    layer: SceneLayerKey,
}

impl<Id> SemanticRect<Id> {
    /// Creates an exact semantic rectangle.
    #[must_use]
    pub const fn new(id: Id, rect: LogicalRect, layer: SceneLayerKey) -> Self {
        Self { id, rect, layer }
    }

    /// Returns the semantic identity.
    #[must_use]
    pub const fn id(&self) -> &Id {
        &self.id
    }

    /// Returns the exact logical rectangle.
    #[must_use]
    pub const fn rect(&self) -> LogicalRect {
        self.rect
    }

    /// Returns the explicit layer.
    #[must_use]
    pub const fn layer(&self) -> SceneLayerKey {
        self.layer
    }
}

/// Complete semantic facts for one surface that was painted this generation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySurfaceScene {
    surface: SurfaceId,
    bounds: LogicalRect,
    nodes: Vec<SemanticRect<NodeSceneId>>,
    tab_bars: Vec<SemanticRect<TabBarSceneId>>,
    tabs: Vec<SemanticRect<TabSceneId>>,
    splitters: Vec<SemanticRect<SplitterSceneId>>,
    drop_occlusions: Vec<DropOcclusionRecord>,
    drop_targets: Vec<DropTargetRecord>,
}

impl ReadySurfaceScene {
    /// Starts a complete ready-surface fact with explicit surface bounds.
    #[must_use]
    pub fn new(surface: SurfaceId, bounds: LogicalRect) -> Self {
        Self {
            surface,
            bounds,
            nodes: Vec::new(),
            tab_bars: Vec::new(),
            tabs: Vec::new(),
            splitters: Vec::new(),
            drop_occlusions: Vec::new(),
            drop_targets: Vec::new(),
        }
    }

    /// Returns the owning surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the authoritative logical surface bounds.
    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    /// Adds one node rectangle before this fact is submitted to a scene.
    pub fn push_node(&mut self, region: SemanticRect<NodeSceneId>) {
        self.nodes.push(region);
    }

    /// Adds one tab-bar rectangle before this fact is submitted to a scene.
    pub fn push_tab_bar(&mut self, region: SemanticRect<TabBarSceneId>) {
        self.tab_bars.push(region);
    }

    /// Adds one tab rectangle before this fact is submitted to a scene.
    pub fn push_tab(&mut self, region: SemanticRect<TabSceneId>) {
        self.tabs.push(region);
    }

    /// Adds one splitter rectangle before this fact is submitted to a scene.
    pub fn push_splitter(&mut self, region: SemanticRect<SplitterSceneId>) {
        self.splitters.push(region);
    }

    /// Adds one exact contained-floating occlusion before scene submission.
    pub fn push_drop_occlusion(&mut self, occlusion: DropOcclusionRecord) {
        self.drop_occlusions.push(occlusion);
    }

    /// Adds one structural drop target before this fact is submitted to a scene.
    pub fn push_drop_target(&mut self, target: DropTargetRecord) {
        self.drop_targets.push(target);
    }

    /// Returns node rectangles in canonical structural-identity order after sealing.
    #[must_use]
    pub fn nodes(&self) -> &[SemanticRect<NodeSceneId>] {
        &self.nodes
    }

    /// Returns tab-bar rectangles in canonical structural-identity order after sealing.
    #[must_use]
    pub fn tab_bars(&self) -> &[SemanticRect<TabBarSceneId>] {
        &self.tab_bars
    }

    /// Returns tab rectangles in canonical structural-identity order after sealing.
    #[must_use]
    pub fn tabs(&self) -> &[SemanticRect<TabSceneId>] {
        &self.tabs
    }

    /// Returns splitter rectangles in canonical structural-identity order after sealing.
    #[must_use]
    pub fn splitters(&self) -> &[SemanticRect<SplitterSceneId>] {
        &self.splitters
    }

    /// Returns contained-floating occlusions in stable identity order after sealing.
    #[must_use]
    pub fn drop_occlusions(&self) -> &[DropOcclusionRecord] {
        &self.drop_occlusions
    }

    /// Returns drop targets in canonical structural-identity order after sealing.
    #[must_use]
    pub fn drop_targets(&self) -> &[DropTargetRecord] {
        &self.drop_targets
    }
}

/// A roster surface that did not publish ready facts this generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapSurfaceScene {
    surface: SurfaceId,
}

impl BootstrapSurfaceScene {
    /// Returns the unavailable roster surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// State of one frozen roster surface in a sealed scene.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceScene {
    /// Complete semantic facts were supplied.
    Ready(ReadySurfaceScene),
    /// The surface exists in the active roster but has not painted a scene yet.
    Bootstrap(BootstrapSurfaceScene),
}

impl SurfaceScene {
    /// Returns the owning surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        match self {
            Self::Ready(scene) => scene.surface,
            Self::Bootstrap(scene) => scene.surface,
        }
    }
}

/// Mutable type-state used only while collecting one renderer generation.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingScene {
    surfaces: BTreeMap<SurfaceId, Option<ReadySurfaceScene>>,
}

impl BuildingScene {
    /// Freezes the complete active surface roster for a new generation.
    ///
    /// # Errors
    ///
    /// Returns [`SceneBuildError::DuplicateRosterSurface`] when the input names
    /// one logical surface more than once.
    pub fn new(
        active_surfaces: impl IntoIterator<Item = SurfaceId>,
    ) -> Result<Self, SceneBuildError> {
        let mut surfaces = BTreeMap::new();
        for surface in active_surfaces {
            if surfaces.insert(surface, None).is_some() {
                return Err(SceneBuildError::DuplicateRosterSurface { surface });
            }
        }
        Ok(Self { surfaces })
    }

    /// Inserts complete ready facts for one roster surface in any surface order.
    ///
    /// The insertion is atomic: an error leaves all previously accepted facts
    /// unchanged and does not partially publish the new surface.
    ///
    /// # Errors
    ///
    /// Returns [`SceneBuildError`] for an unknown or already-ready surface, or
    /// any duplicate semantic identity in the complete building scene.
    pub fn insert_ready(&mut self, ready: ReadySurfaceScene) -> Result<(), SceneBuildError> {
        let surface = ready.surface;
        match self.surfaces.get(&surface) {
            None => return Err(SceneBuildError::SurfaceOutsideRoster { surface }),
            Some(Some(_)) => return Err(SceneBuildError::DuplicateReadySurface { surface }),
            Some(None) => {}
        }

        self.check_semantic_uniqueness(&ready)?;
        self.surfaces.insert(surface, Some(ready));
        Ok(())
    }

    /// Consumes the build state and validates a complete immutable scene.
    ///
    /// Missing ready facts become explicit bootstrap surfaces. Consuming `self`
    /// makes late mutation of the sealed generation impossible.
    ///
    /// # Errors
    ///
    /// Returns [`SceneBuildError::TargetSemanticMismatch`] if any structural
    /// target identity disagrees with its exact checked [`crate::command::DockTarget`].
    pub(crate) fn seal(
        self,
        stamp: SceneStamp,
        workspace: &Workspace,
        policy: &DockPolicy,
    ) -> Result<SealedScene, SceneBuildError> {
        let workspace_index = SceneWorkspaceIndex::new(workspace);
        let mut sealed = BTreeMap::new();
        for (surface, ready) in self.surfaces {
            let state = match ready {
                Some(mut ready) => {
                    validate_ready_semantics(&ready, workspace, &workspace_index)?;
                    for occlusion in &ready.drop_occlusions {
                        let floating = occlusion.floating();
                        if !workspace_index.contained_belongs_to_surface(surface, floating) {
                            return Err(SceneBuildError::InvalidDropOcclusion {
                                surface,
                                floating,
                            });
                        }
                    }
                    for target in &ready.drop_targets {
                        validate_drop_visual(&ready, target)?;
                        if target.id().surface() != surface
                            || !target.semantics_match()
                            || (target_reference_is_current(workspace, &workspace_index, target)
                                && !target_structure_is_valid(
                                    workspace,
                                    &workspace_index,
                                    surface,
                                    target,
                                ))
                        {
                            return Err(SceneBuildError::TargetSemanticMismatch {
                                surface,
                                target: target.id(),
                            });
                        }
                    }
                    canonicalize_ready(&mut ready, workspace, &workspace_index, policy);
                    SurfaceScene::Ready(ready)
                }
                None => SurfaceScene::Bootstrap(BootstrapSurfaceScene { surface }),
            };
            sealed.insert(surface, state);
        }
        Ok(SealedScene {
            stamp,
            surfaces: sealed,
        })
    }

    fn check_semantic_uniqueness(
        &self,
        incoming: &ReadySurfaceScene,
    ) -> Result<(), SceneBuildError> {
        let mut node_ids = HashSet::new();
        let mut tab_bar_ids = HashSet::new();
        let mut tab_ids = HashSet::new();
        let mut splitter_ids = HashSet::new();
        let mut occlusion_ids = HashSet::new();
        let mut target_ids = HashSet::new();

        for ready in self.surfaces.values().flatten().chain([incoming]) {
            for region in &ready.nodes {
                if !node_ids.insert(region.id) {
                    return Err(SceneBuildError::DuplicateNode { id: region.id });
                }
            }
            for region in &ready.tab_bars {
                if !tab_bar_ids.insert(region.id) {
                    return Err(SceneBuildError::DuplicateTabBar { id: region.id });
                }
            }
            for region in &ready.tabs {
                if !tab_ids.insert(region.id) {
                    return Err(SceneBuildError::DuplicateTab { id: region.id });
                }
            }
            for region in &ready.splitters {
                if !splitter_ids.insert(region.id) {
                    return Err(SceneBuildError::DuplicateSplitter { id: region.id });
                }
            }
            for occlusion in &ready.drop_occlusions {
                if !occlusion_ids.insert(occlusion.floating()) {
                    return Err(SceneBuildError::DuplicateDropOcclusion {
                        floating: occlusion.floating(),
                    });
                }
            }
            for target in &ready.drop_targets {
                if !target_ids.insert(target.id()) {
                    return Err(SceneBuildError::DuplicateDropTarget { id: target.id() });
                }
            }
        }
        Ok(())
    }
}

fn validate_ready_semantics(
    ready: &ReadySurfaceScene,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    for region in &ready.nodes {
        let id = *region.id();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.node) {
            return Err(SceneBuildError::InvalidNodeSemantic {
                surface: ready.surface,
                id,
            });
        }
    }
    for region in &ready.tab_bars {
        let id = *region.id();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.tabs)
            || !matches!(workspace.node(id.tabs), Some(Node::Tabs { .. }))
        {
            return Err(SceneBuildError::InvalidTabBarSemantic {
                surface: ready.surface,
                id,
            });
        }
    }
    for region in &ready.tabs {
        let id = *region.id();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.tabs)
            || !matches!(
                workspace.node(id.tabs),
                Some(Node::Tabs { items, .. }) if items.contains(&id.item)
            )
        {
            return Err(SceneBuildError::InvalidTabSemantic {
                surface: ready.surface,
                id,
            });
        }
    }
    for region in &ready.splitters {
        let id = *region.id();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.split)
            || !matches!(
                workspace.node(id.split),
                Some(Node::Split { children, .. })
                    if id.index < children.len().saturating_sub(1)
            )
        {
            return Err(SceneBuildError::InvalidSplitterSemantic {
                surface: ready.surface,
                id,
            });
        }
    }
    Ok(())
}

fn canonicalize_ready(
    ready: &mut ReadySurfaceScene,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicy,
) {
    ready.nodes.sort_unstable_by_key(|region| *region.id());
    ready.tab_bars.sort_unstable_by_key(|region| *region.id());
    ready.tabs.sort_unstable_by_key(|region| *region.id());
    ready.splitters.sort_unstable_by_key(|region| *region.id());
    ready
        .drop_occlusions
        .sort_unstable_by_key(|occlusion| occlusion.floating());
    ready
        .drop_targets
        .sort_unstable_by_key(DropTargetRecord::id);

    for target in &mut ready.drop_targets {
        if !target.availability().is_available() {
            continue;
        }
        if !target_reference_is_current(workspace, workspace_index, target)
            || !target_structure_is_valid(workspace, workspace_index, ready.surface, target)
        {
            target.set_availability(DropTargetAvailability::Unavailable(
                DropTargetUnavailable::Stale,
            ));
            continue;
        }
        let allowed = match target.id().kind() {
            DropTargetKind::TabGap | DropTargetKind::Center => policy.allows_tab_merge(),
            DropTargetKind::InnerEdge | DropTargetKind::OuterEdge => policy.allows_edge_split(),
        };
        if !allowed {
            target.set_availability(DropTargetAvailability::Unavailable(
                DropTargetUnavailable::PolicyDisabled,
            ));
        }
    }
}

fn target_reference_is_current(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    record: &DropTargetRecord,
) -> bool {
    match record.target() {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.tabs())
                && matches!(workspace.node(target.tabs()), Some(Node::Tabs { .. }))
        }
        DockTarget::Edge(target) => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.node())
        }
    }
}

fn target_structure_is_valid(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    surface: SurfaceId,
    record: &DropTargetRecord,
) -> bool {
    if !workspace_index.root_belongs_to_surface(surface, target_root(record.target())) {
        return false;
    }
    match (record.id(), record.target()) {
        (DropTargetId::TabGap { .. }, DockTarget::TabGap { target, index }) => matches!(
            workspace.node(target.tabs()),
            Some(Node::Tabs { items, .. }) if *index <= items.len()
        ),
        (DropTargetId::OuterEdge { root, node, .. }, DockTarget::Edge(_)) => {
            workspace_index.root_node(root) == Some(node)
        }
        (DropTargetId::InnerEdge { .. }, DockTarget::Edge(target)) => {
            matches!(workspace.node(target.node()), Some(Node::Tabs { .. }))
        }
        _ => true,
    }
}

fn validate_drop_visual(
    ready: &ReadySurfaceScene,
    target: &DropTargetRecord,
) -> Result<(), SceneBuildError> {
    let visual = target.visual().rect();
    if visual.width() <= 0.0 || visual.height() <= 0.0 {
        return Err(SceneBuildError::EmptyDropVisual {
            surface: ready.surface,
            target: target.id(),
        });
    }
    let bounds_min = ready.bounds.min();
    let bounds_max = ready.bounds.max();
    let visual_min = visual.min();
    let visual_max = visual.max();
    if visual_min.x() < bounds_min.x()
        || visual_min.y() < bounds_min.y()
        || visual_max.x() > bounds_max.x()
        || visual_max.y() > bounds_max.y()
    {
        return Err(SceneBuildError::DropVisualOutsideSurface {
            surface: ready.surface,
            target: target.id(),
        });
    }
    Ok(())
}

fn target_root(target: &DockTarget) -> RootId {
    match target {
        DockTarget::Center(target) | DockTarget::TabGap { target, .. } => target.root(),
        DockTarget::Edge(target) => target.root(),
    }
}

struct IndexedRoot {
    surface: SurfaceId,
    root_node: NodeId,
    nodes: HashSet<NodeId>,
    fingerprint: Option<NodeFingerprint>,
}

struct SceneWorkspaceIndex {
    roots: HashMap<RootId, IndexedRoot>,
    contained_surfaces: HashMap<FloatingPresentationId, SurfaceId>,
}

impl SceneWorkspaceIndex {
    fn new(workspace: &Workspace) -> Self {
        let mut root_surfaces = HashMap::with_capacity(workspace.roots().count());
        let mut contained_surfaces =
            HashMap::with_capacity(workspace.contained_floatings().count());
        for (surface, presentation) in workspace.surfaces() {
            root_surfaces.insert(presentation.main_root, surface);
            for floating in &presentation.contained {
                if let Some(record) = workspace.contained_floating(*floating) {
                    root_surfaces.insert(record.root, surface);
                    contained_surfaces.insert(*floating, surface);
                }
            }
        }

        let mut roots = HashMap::with_capacity(root_surfaces.len());
        for (root, record) in workspace.roots() {
            let Some(surface) = root_surfaces.get(&root).copied() else {
                continue;
            };
            let mut nodes = HashSet::new();
            let mut stack = vec![record.node];
            while let Some(node) = stack.pop() {
                if !nodes.insert(node) {
                    continue;
                }
                if let Some(Node::Split { children, .. }) = workspace.node(node) {
                    stack.extend(children.iter().copied());
                }
            }
            let fingerprint = workspace
                .capture_node_source(root, record.node)
                .ok()
                .map(|source| source.fingerprint().clone());
            roots.insert(
                root,
                IndexedRoot {
                    surface,
                    root_node: record.node,
                    nodes,
                    fingerprint,
                },
            );
        }
        Self {
            roots,
            contained_surfaces,
        }
    }

    fn root_belongs_to_surface(&self, surface: SurfaceId, root: RootId) -> bool {
        self.roots
            .get(&root)
            .is_some_and(|record| record.surface == surface)
    }

    fn contains_node(&self, root: RootId, node: NodeId) -> bool {
        self.roots
            .get(&root)
            .is_some_and(|record| record.nodes.contains(&node))
    }

    fn semantic_node_exists(&self, surface: SurfaceId, root: RootId, node: NodeId) -> bool {
        self.root_belongs_to_surface(surface, root) && self.contains_node(root, node)
    }

    fn fingerprint_is_current(&self, root: RootId, expected: &NodeFingerprint) -> bool {
        self.roots
            .get(&root)
            .and_then(|record| record.fingerprint.as_ref())
            .is_some_and(|current| current == expected)
    }

    fn root_node(&self, root: RootId) -> Option<NodeId> {
        self.roots.get(&root).map(|record| record.root_node)
    }

    fn contained_belongs_to_surface(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    ) -> bool {
        self.contained_surfaces.get(&floating).copied() == Some(surface)
    }
}

/// Immutable semantic scene used by hit testing and interaction proofs.
#[derive(Debug, Clone, PartialEq)]
pub struct SealedScene {
    stamp: SceneStamp,
    surfaces: BTreeMap<SurfaceId, SurfaceScene>,
}

impl SealedScene {
    /// Returns the exact workspace and renderer generation stamp.
    #[must_use]
    pub const fn stamp(&self) -> SceneStamp {
        self.stamp
    }

    /// Returns one surface state, or `None` when it was absent from the frozen roster.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceScene> {
        self.surfaces.get(&surface)
    }

    /// Iterates the complete frozen surface roster in stable identity order.
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (&SurfaceId, &SurfaceScene)> {
        self.surfaces.iter()
    }
}

/// Deterministic rejection while constructing or sealing a scene.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneBuildError {
    /// The engine already published one scene in the current reduction boundary.
    #[error("a scene was already published in this reduction boundary")]
    AlreadyPublishedInBoundary,
    /// The active roster repeated a surface identity.
    #[error("active scene roster contains duplicate surface {surface}")]
    DuplicateRosterSurface {
        /// Repeated surface.
        surface: SurfaceId,
    },
    /// Ready facts named a surface outside the frozen roster.
    #[error("ready facts name surface {surface} outside the active scene roster")]
    SurfaceOutsideRoster {
        /// Unknown surface.
        surface: SurfaceId,
    },
    /// Ready facts were submitted twice for one roster surface.
    #[error("ready facts for surface {surface} were submitted more than once")]
    DuplicateReadySurface {
        /// Repeated surface.
        surface: SurfaceId,
    },
    /// A node semantic identity was repeated.
    #[error("scene contains duplicate node semantic identity {id:?}")]
    DuplicateNode {
        /// Repeated identity.
        id: NodeSceneId,
    },
    /// A tab-bar semantic identity was repeated.
    #[error("scene contains duplicate tab-bar semantic identity {id:?}")]
    DuplicateTabBar {
        /// Repeated identity.
        id: TabBarSceneId,
    },
    /// A tab semantic identity was repeated.
    #[error("scene contains duplicate tab semantic identity {id:?}")]
    DuplicateTab {
        /// Repeated identity.
        id: TabSceneId,
    },
    /// A splitter semantic identity was repeated.
    #[error("scene contains duplicate splitter semantic identity {id:?}")]
    DuplicateSplitter {
        /// Repeated identity.
        id: SplitterSceneId,
    },
    /// A contained-floating occlusion identity was repeated.
    #[error("scene contains duplicate drop occlusion for contained floating {floating}")]
    DuplicateDropOcclusion {
        /// Repeated contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// A structural drop-target identity was repeated.
    #[error("scene contains duplicate drop target {id:?}")]
    DuplicateDropTarget {
        /// Repeated identity.
        id: DropTargetId,
    },
    /// A target ID disagreed with its surface or exact topology target.
    #[error("drop target {target:?} on surface {surface} has mismatched structural semantics")]
    TargetSemanticMismatch {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Mismatched target identity.
        target: DropTargetId,
    },
    /// A target preview has no paintable logical area.
    #[error("drop target {target:?} on surface {surface} has an empty preview visual")]
    EmptyDropVisual {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Target with an empty visual.
        target: DropTargetId,
    },
    /// A target preview cannot be painted completely inside its owning surface.
    #[error("drop target {target:?} preview lies outside surface {surface} bounds")]
    DropVisualOutsideSurface {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Target with an out-of-bounds visual.
        target: DropTargetId,
    },
    /// A rendered node was absent, unreachable, or owned by another surface.
    #[error("node semantic identity {id:?} is invalid on surface {surface}")]
    InvalidNodeSemantic {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: NodeSceneId,
    },
    /// A tab bar did not name a reachable tabs node on its surface.
    #[error("tab-bar semantic identity {id:?} is invalid on surface {surface}")]
    InvalidTabBarSemantic {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: TabBarSceneId,
    },
    /// A rendered tab did not belong to its reachable tabs node.
    #[error("tab semantic identity {id:?} is invalid on surface {surface}")]
    InvalidTabSemantic {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: TabSceneId,
    },
    /// A splitter did not name a valid gap in a reachable split node.
    #[error("splitter semantic identity {id:?} is invalid on surface {surface}")]
    InvalidSplitterSemantic {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: SplitterSceneId,
    },
    /// A drop occlusion named a missing floating or one owned by another surface.
    #[error("drop occlusion for contained floating {floating} is invalid on surface {surface}")]
    InvalidDropOcclusion {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Missing or differently-owned contained-floating identity.
        floating: FloatingPresentationId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{WorkspaceEpoch, WorkspaceRevision};

    fn stamp() -> SceneStamp {
        SceneStamp::new(
            WorkspaceVersion::new(WorkspaceEpoch::new(2), WorkspaceRevision::new(3)),
            SceneGeneration::new(4),
        )
    }

    fn bounds() -> LogicalRect {
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("valid bounds")
    }

    #[test]
    fn ready_order_does_not_change_sealed_surface_order() {
        let first = SurfaceId::new(1);
        let second = SurfaceId::new(2);
        let workspace = crate::graph::WorkspaceBuilder::new()
            .build()
            .expect("empty workspace is valid");
        let mut building = BuildingScene::new([second, first]).expect("valid roster");
        building
            .insert_ready(ReadySurfaceScene::new(second, bounds()))
            .expect("second ready");
        building
            .insert_ready(ReadySurfaceScene::new(first, bounds()))
            .expect("first ready");

        let scene = building
            .seal(stamp(), &workspace, &DockPolicy::default())
            .expect("valid scene");
        let surfaces: Vec<_> = scene.surfaces().map(|(id, _)| *id).collect();
        assert_eq!(surfaces, [first, second]);
    }

    #[test]
    fn missing_ready_fact_is_explicit_bootstrap() {
        let surface = SurfaceId::new(1);
        let workspace = crate::graph::WorkspaceBuilder::new()
            .build()
            .expect("empty workspace is valid");
        let scene = BuildingScene::new([surface])
            .expect("valid roster")
            .seal(stamp(), &workspace, &DockPolicy::default())
            .expect("bootstrap scene is valid");

        assert!(matches!(
            scene.surface(surface),
            Some(SurfaceScene::Bootstrap(_))
        ));
    }
}
