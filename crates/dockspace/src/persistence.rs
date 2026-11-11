//! Renderer-neutral persistence payloads for validated workspaces.
//!
//! Snapshot node identities are local integers, not serialized [`crate::ids::NodeId`] values.
//! Restoring a snapshot always allocates fresh generational runtime identities and publishes only
//! a fully validated candidate.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::canonical::CanonicalizationError;
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{
    Axis, ContainedFloating, InvalidSplitWeight, Node, RootRecord, SplitWeight,
    SurfacePresentation, Workspace, WorkspaceBuildError, WorkspaceBuilder,
};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::validation::WorkspaceValidationErrors;

/// A complete renderer-neutral workspace snapshot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    /// Flat node arena with snapshot-local integer identities.
    pub nodes: Vec<SnapshotNodeRecord>,
    /// Stable docking roots.
    pub roots: Vec<SnapshotRootRecord>,
    /// Stable presentation surfaces.
    pub surfaces: Vec<SnapshotSurfaceRecord>,
    /// Contained-floating presentations.
    pub contained_floatings: Vec<SnapshotContainedFloatingRecord>,
}

/// One node and its snapshot-local integer identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotNodeRecord {
    /// Snapshot-local node identity.
    pub id: u64,
    /// Persisted node payload.
    pub node: SnapshotNode,
}

/// Persisted node payload using only durable or snapshot-local identities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SnapshotNode {
    /// Ordered dockable items sharing one content rectangle.
    Tabs {
        /// Stable item identities in tab order.
        items: Vec<u64>,
        /// Selected stable item identity.
        #[serde(deserialize_with = "deserialize_required_option")]
        selected: Option<u64>,
        /// Complete stable item permutation in most-recent-first order.
        mru: Vec<u64>,
    },
    /// Ordered N-ary split children.
    Split {
        /// Split layout direction.
        axis: SnapshotAxis,
        /// Snapshot-local child node identities.
        children: Vec<u64>,
        /// Exact normalized shares from the runtime model.
        weights: Vec<f32>,
    },
}

/// Persisted split direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAxis {
    /// Children are ordered from left to right.
    Horizontal,
    /// Children are ordered from top to bottom.
    Vertical,
}

/// Persisted docking root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRootRecord {
    /// Stable root identity.
    pub id: u64,
    /// Snapshot-local topology root node.
    pub node: u64,
    /// Snapshot-local central tabs leaf.
    #[serde(deserialize_with = "deserialize_required_option")]
    pub central: Option<u64>,
}

/// Persisted presentation surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSurfaceRecord {
    /// Stable surface identity.
    pub id: u64,
    /// Stable root occupying the main dock area, when present.
    #[serde(deserialize_with = "deserialize_required_option")]
    pub main_root: Option<u64>,
    /// Stable contained-floating identities in roster order.
    pub contained: Vec<u64>,
}

pub(crate) fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Persisted contained-floating presentation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotContainedFloatingRecord {
    /// Stable floating presentation identity.
    pub id: u64,
    /// Stable presented root identity.
    pub root: u64,
    /// Logical bounds relative to the owning surface.
    pub rect: SnapshotLogicalRect,
}

/// Persisted logical rectangle represented by scalar components.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotLogicalRect {
    /// Minimum horizontal coordinate.
    pub x: f64,
    /// Minimum vertical coordinate.
    pub y: f64,
    /// Non-negative width.
    pub width: f64,
    /// Non-negative height.
    pub height: f64,
}

/// Stable kind of entity declared by a snapshot record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotEntityKind {
    /// Snapshot-local graph node.
    Node,
    /// Stable docking root.
    Root,
    /// Stable presentation surface.
    Surface,
    /// Stable contained-floating presentation.
    ContainedFloating,
}

/// Record which owns a persisted reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotReferenceOwner {
    /// A node record.
    Node(u64),
    /// A root record.
    Root(u64),
    /// A surface record.
    Surface(u64),
    /// A contained-floating record.
    ContainedFloating(u64),
}

/// Failure to capture a validated runtime workspace.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SnapshotCaptureError {
    /// The runtime workspace did not satisfy its durable invariants.
    #[error("cannot snapshot an invalid workspace: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// This platform exposed more nodes than the `u64` snapshot identity space can represent.
    #[error("workspace has too many nodes for the snapshot identity space")]
    NodeIdentitySpaceExhausted,
    /// A validated runtime record unexpectedly referenced an unindexed node.
    #[error("runtime record references unindexed node {node:?}")]
    UnindexedRuntimeNode {
        /// Runtime identity absent from the capture index.
        node: NodeId,
    },
}

/// Failure to restore a workspace snapshot.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SnapshotRestoreError {
    /// Two records declare the same identity.
    #[error("duplicate {kind:?} snapshot identity {id}")]
    DuplicateIdentity {
        /// Kind of duplicated record.
        kind: SnapshotEntityKind,
        /// Repeated numeric identity.
        id: u64,
    },
    /// A record points at an identity absent from the corresponding record set.
    #[error("{owner:?} references unknown {kind:?} identity {id}")]
    UnknownReference {
        /// Record containing the reference.
        owner: SnapshotReferenceOwner,
        /// Kind of referenced entity.
        kind: SnapshotEntityKind,
        /// Missing numeric identity.
        id: u64,
    },
    /// An application registry could not resolve a persisted item identity.
    #[error("application registry does not contain item {item}")]
    UnknownItem {
        /// Missing stable application item identity.
        item: ItemId,
    },
    /// A contained-floating record has invalid logical geometry.
    #[error("floating presentation {floating} has invalid geometry: {source}")]
    InvalidFloatingGeometry {
        /// Stable floating presentation identity.
        floating: u64,
        /// Geometry validation failure.
        source: GeometryError,
    },
    /// A split contains a non-finite or non-positive scalar weight.
    #[error("node {node} has invalid split weight at index {index}: {source}")]
    InvalidSplitWeight {
        /// Snapshot-local split node identity.
        node: u64,
        /// Invalid weight position.
        index: usize,
        /// Scalar validation failure.
        source: InvalidSplitWeight,
    },
    /// An internal draft operation failed while assembling the candidate.
    #[error("could not assemble workspace candidate: {0}")]
    Assembly(#[source] WorkspaceBuildError),
    /// The fully assembled candidate violates strict workspace invariants.
    #[error("restored workspace candidate is invalid: {0}")]
    InvalidWorkspace(#[source] WorkspaceValidationErrors),
    /// The strictly validated candidate could not be finalized.
    #[error("could not finalize restored workspace candidate: {0}")]
    Build(#[source] CanonicalizationError),
}

impl WorkspaceSnapshot {
    /// Captures a validated runtime workspace as a deterministic flat snapshot.
    ///
    /// Runtime [`NodeId`] values are remapped to contiguous snapshot-local integers. Stable item,
    /// root, surface, and floating identities retain their numeric representation.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotCaptureError`] when the runtime workspace is invalid, its node count does
    /// not fit the snapshot identity space, or a runtime node reference cannot be indexed.
    pub fn capture(workspace: &Workspace) -> Result<Self, SnapshotCaptureError> {
        workspace
            .validate()
            .map_err(SnapshotCaptureError::InvalidWorkspace)?;

        let node_ids = capture_node_ids(workspace)?;
        let nodes = workspace
            .nodes()
            .map(|(id, node)| capture_node(id, node, &node_ids))
            .collect::<Result<Vec<_>, _>>()?;
        let roots = workspace
            .roots()
            .map(|(id, root)| {
                Ok(SnapshotRootRecord {
                    id: id.get(),
                    node: captured_id(&node_ids, root.node)?,
                    central: root
                        .central
                        .map(|central| captured_id(&node_ids, central))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, SnapshotCaptureError>>()?;
        let surfaces = workspace
            .surfaces()
            .map(|(id, surface)| SnapshotSurfaceRecord {
                id: id.get(),
                main_root: surface.main_root.map(RootId::get),
                contained: surface.contained.iter().map(|id| id.get()).collect(),
            })
            .collect();
        let contained_floatings = workspace
            .contained_floatings()
            .map(|(id, floating)| SnapshotContainedFloatingRecord {
                id: id.get(),
                root: floating.root.get(),
                rect: SnapshotLogicalRect::from(floating.rect),
            })
            .collect();
        Ok(Self {
            nodes,
            roots,
            surfaces,
            contained_floatings,
        })
    }

    /// Builds a fresh workspace candidate after checking every persisted item.
    ///
    /// `contains_item` is a read-only registry query called exactly once for every
    /// distinct item identity, after all graph and presentation validation succeeds.
    /// Every identity is queried even when an earlier one is missing. Returning
    /// `false` rejects the snapshot as stale application state.
    /// Submit the resulting candidate through an explicit
    /// [`crate::engine::EngineInput::ReplaceWorkspace`] reducer tick when
    /// publishing into an existing engine. The replacement advances the workspace
    /// epoch and makes every old runtime [`NodeId`] stale.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotRestoreError`] for duplicate identities, unknown
    /// references or items, invalid scalar data, or any strict workspace invariant violation.
    pub fn build_candidate(
        &self,
        contains_item: impl Fn(ItemId) -> bool,
    ) -> Result<Workspace, SnapshotRestoreError> {
        let index = SnapshotIndex::new(self)?;
        preflight_references_and_scalars(self, &index)?;

        let mut builder = Workspace::builder();
        let runtime_nodes = allocate_runtime_nodes(self, &mut builder)?;
        populate_runtime_nodes(self, &runtime_nodes, &mut builder)?;
        populate_roots(self, &runtime_nodes, &mut builder)?;
        populate_surfaces(self, &mut builder);
        populate_contained_floatings(self, &mut builder)?;

        builder
            .validate()
            .map_err(SnapshotRestoreError::InvalidWorkspace)?;
        let candidate = builder.build().map_err(SnapshotRestoreError::Build)?;
        validate_registered_items(self, contains_item)?;
        Ok(candidate)
    }
}

impl From<Axis> for SnapshotAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => Self::Horizontal,
            Axis::Vertical => Self::Vertical,
        }
    }
}

impl From<SnapshotAxis> for Axis {
    fn from(axis: SnapshotAxis) -> Self {
        match axis {
            SnapshotAxis::Horizontal => Self::Horizontal,
            SnapshotAxis::Vertical => Self::Vertical,
        }
    }
}

impl From<LogicalRect> for SnapshotLogicalRect {
    fn from(rect: LogicalRect) -> Self {
        Self {
            x: rect.x(),
            y: rect.y(),
            width: rect.width(),
            height: rect.height(),
        }
    }
}

#[derive(Debug)]
struct SnapshotIndex {
    nodes: HashSet<u64>,
    roots: HashSet<u64>,
    contained_floatings: HashSet<u64>,
}

impl SnapshotIndex {
    fn new(snapshot: &WorkspaceSnapshot) -> Result<Self, SnapshotRestoreError> {
        unique_ids(
            snapshot.surfaces.iter().map(|record| record.id),
            SnapshotEntityKind::Surface,
        )?;

        Ok(Self {
            nodes: unique_ids(
                snapshot.nodes.iter().map(|record| record.id),
                SnapshotEntityKind::Node,
            )?,
            roots: unique_ids(
                snapshot.roots.iter().map(|record| record.id),
                SnapshotEntityKind::Root,
            )?,
            contained_floatings: unique_ids(
                snapshot.contained_floatings.iter().map(|record| record.id),
                SnapshotEntityKind::ContainedFloating,
            )?,
        })
    }
}

fn unique_ids(
    ids: impl IntoIterator<Item = u64>,
    kind: SnapshotEntityKind,
) -> Result<HashSet<u64>, SnapshotRestoreError> {
    let mut unique = HashSet::new();
    for id in ids {
        if !unique.insert(id) {
            return Err(SnapshotRestoreError::DuplicateIdentity { kind, id });
        }
    }
    Ok(unique)
}

fn capture_node_ids(workspace: &Workspace) -> Result<HashMap<NodeId, u64>, SnapshotCaptureError> {
    let mut node_ids = HashMap::with_capacity(workspace.nodes().count());
    for (index, (node, _)) in workspace.nodes().enumerate() {
        let snapshot_id =
            u64::try_from(index).map_err(|_| SnapshotCaptureError::NodeIdentitySpaceExhausted)?;
        node_ids.insert(node, snapshot_id);
    }
    Ok(node_ids)
}

fn captured_id(node_ids: &HashMap<NodeId, u64>, node: NodeId) -> Result<u64, SnapshotCaptureError> {
    node_ids
        .get(&node)
        .copied()
        .ok_or(SnapshotCaptureError::UnindexedRuntimeNode { node })
}

fn capture_node(
    id: NodeId,
    node: &Node,
    node_ids: &HashMap<NodeId, u64>,
) -> Result<SnapshotNodeRecord, SnapshotCaptureError> {
    let node = match node {
        Node::Tabs {
            items,
            selected,
            mru,
        } => SnapshotNode::Tabs {
            items: items.iter().map(|item| item.get()).collect(),
            selected: selected.map(ItemId::get),
            mru: mru.iter().map(|item| item.get()).collect(),
        },
        Node::Split {
            axis,
            children,
            weights,
        } => SnapshotNode::Split {
            axis: (*axis).into(),
            children: children
                .iter()
                .map(|child| captured_id(node_ids, *child))
                .collect::<Result<Vec<_>, _>>()?,
            weights: weights.iter().map(|weight| weight.get()).collect(),
        },
    };
    Ok(SnapshotNodeRecord {
        id: captured_id(node_ids, id)?,
        node,
    })
}

fn preflight_references_and_scalars(
    snapshot: &WorkspaceSnapshot,
    index: &SnapshotIndex,
) -> Result<(), SnapshotRestoreError> {
    for record in &snapshot.nodes {
        if let SnapshotNode::Split {
            children, weights, ..
        } = &record.node
        {
            for child in children {
                require_reference(
                    &index.nodes,
                    SnapshotReferenceOwner::Node(record.id),
                    SnapshotEntityKind::Node,
                    *child,
                )?;
            }
            for (weight_index, weight) in weights.iter().copied().enumerate() {
                SplitWeight::new(weight).map_err(|source| {
                    SnapshotRestoreError::InvalidSplitWeight {
                        node: record.id,
                        index: weight_index,
                        source,
                    }
                })?;
            }
        }
    }

    for root in &snapshot.roots {
        let owner = SnapshotReferenceOwner::Root(root.id);
        require_reference(&index.nodes, owner, SnapshotEntityKind::Node, root.node)?;
        if let Some(central) = root.central {
            require_reference(&index.nodes, owner, SnapshotEntityKind::Node, central)?;
        }
    }

    for surface in &snapshot.surfaces {
        let owner = SnapshotReferenceOwner::Surface(surface.id);
        if let Some(main_root) = surface.main_root {
            require_reference(&index.roots, owner, SnapshotEntityKind::Root, main_root)?;
        }
        for floating in &surface.contained {
            require_reference(
                &index.contained_floatings,
                owner,
                SnapshotEntityKind::ContainedFloating,
                *floating,
            )?;
        }
    }

    for floating in &snapshot.contained_floatings {
        let owner = SnapshotReferenceOwner::ContainedFloating(floating.id);
        require_reference(&index.roots, owner, SnapshotEntityKind::Root, floating.root)?;
        restore_rect(floating)?;
    }
    Ok(())
}

fn require_reference(
    known: &HashSet<u64>,
    owner: SnapshotReferenceOwner,
    kind: SnapshotEntityKind,
    id: u64,
) -> Result<(), SnapshotRestoreError> {
    if known.contains(&id) {
        Ok(())
    } else {
        Err(SnapshotRestoreError::UnknownReference { owner, kind, id })
    }
}

fn validate_registered_items(
    snapshot: &WorkspaceSnapshot,
    contains_item: impl Fn(ItemId) -> bool,
) -> Result<(), SnapshotRestoreError> {
    let mut items = BTreeSet::new();
    for record in &snapshot.nodes {
        if let SnapshotNode::Tabs {
            items: tab_items,
            selected,
            mru,
        } = &record.node
        {
            items.extend(tab_items.iter().copied());
            items.extend(selected.iter().copied());
            items.extend(mru.iter().copied());
        }
    }

    let mut first_missing = None;
    for item in items.into_iter().map(ItemId::new) {
        if !contains_item(item) && first_missing.is_none() {
            first_missing = Some(item);
        }
    }
    first_missing.map_or(Ok(()), |item| {
        Err(SnapshotRestoreError::UnknownItem { item })
    })
}

fn allocate_runtime_nodes(
    snapshot: &WorkspaceSnapshot,
    builder: &mut WorkspaceBuilder,
) -> Result<HashMap<u64, NodeId>, SnapshotRestoreError> {
    let mut runtime_nodes = HashMap::with_capacity(snapshot.nodes.len());
    for record in &snapshot.nodes {
        let runtime = builder.insert_node(Node::tabs([]));
        if runtime_nodes.insert(record.id, runtime).is_some() {
            return Err(SnapshotRestoreError::DuplicateIdentity {
                kind: SnapshotEntityKind::Node,
                id: record.id,
            });
        }
    }
    Ok(runtime_nodes)
}

fn populate_runtime_nodes(
    snapshot: &WorkspaceSnapshot,
    runtime_nodes: &HashMap<u64, NodeId>,
    builder: &mut WorkspaceBuilder,
) -> Result<(), SnapshotRestoreError> {
    for record in &snapshot.nodes {
        let runtime = runtime_node(
            runtime_nodes,
            SnapshotReferenceOwner::Node(record.id),
            record.id,
        )?;
        let node = restore_node(record, runtime_nodes)?;
        builder
            .replace_node(runtime, node)
            .map_err(SnapshotRestoreError::Assembly)?;
    }
    Ok(())
}

fn restore_node(
    record: &SnapshotNodeRecord,
    runtime_nodes: &HashMap<u64, NodeId>,
) -> Result<Node, SnapshotRestoreError> {
    match &record.node {
        SnapshotNode::Tabs {
            items,
            selected,
            mru,
        } => Ok(Node::Tabs {
            items: items.iter().copied().map(ItemId::new).collect(),
            selected: selected.map(ItemId::new),
            mru: mru.iter().copied().map(ItemId::new).collect(),
        }),
        SnapshotNode::Split {
            axis,
            children,
            weights,
        } => {
            let children = children
                .iter()
                .copied()
                .map(|child| {
                    runtime_node(
                        runtime_nodes,
                        SnapshotReferenceOwner::Node(record.id),
                        child,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let weights = weights
                .iter()
                .copied()
                .enumerate()
                .map(|(index, weight)| {
                    SplitWeight::new(weight).map_err(|source| {
                        SnapshotRestoreError::InvalidSplitWeight {
                            node: record.id,
                            index,
                            source,
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Node::Split {
                axis: (*axis).into(),
                children,
                weights,
            })
        }
    }
}

fn populate_roots(
    snapshot: &WorkspaceSnapshot,
    runtime_nodes: &HashMap<u64, NodeId>,
    builder: &mut WorkspaceBuilder,
) -> Result<(), SnapshotRestoreError> {
    for root in &snapshot.roots {
        let owner = SnapshotReferenceOwner::Root(root.id);
        let node = runtime_node(runtime_nodes, owner, root.node)?;
        let central = root
            .central
            .map(|central| runtime_node(runtime_nodes, owner, central))
            .transpose()?;
        builder.set_root(RootId::new(root.id), RootRecord { node, central });
    }
    Ok(())
}

fn populate_surfaces(snapshot: &WorkspaceSnapshot, builder: &mut WorkspaceBuilder) {
    for surface in &snapshot.surfaces {
        builder.set_surface(
            SurfaceId::new(surface.id),
            SurfacePresentation {
                main_root: surface.main_root.map(RootId::new),
                contained: surface
                    .contained
                    .iter()
                    .copied()
                    .map(FloatingPresentationId::new)
                    .collect(),
            },
        );
    }
}

fn populate_contained_floatings(
    snapshot: &WorkspaceSnapshot,
    builder: &mut WorkspaceBuilder,
) -> Result<(), SnapshotRestoreError> {
    for floating in &snapshot.contained_floatings {
        builder.set_contained_floating(
            FloatingPresentationId::new(floating.id),
            ContainedFloating::new(RootId::new(floating.root), restore_rect(floating)?),
        );
    }
    Ok(())
}

fn restore_rect(
    floating: &SnapshotContainedFloatingRecord,
) -> Result<LogicalRect, SnapshotRestoreError> {
    LogicalRect::new(
        floating.rect.x,
        floating.rect.y,
        floating.rect.width,
        floating.rect.height,
    )
    .map_err(|source| SnapshotRestoreError::InvalidFloatingGeometry {
        floating: floating.id,
        source,
    })
}

fn runtime_node(
    runtime_nodes: &HashMap<u64, NodeId>,
    owner: SnapshotReferenceOwner,
    id: u64,
) -> Result<NodeId, SnapshotRestoreError> {
    runtime_nodes
        .get(&id)
        .copied()
        .ok_or(SnapshotRestoreError::UnknownReference {
            owner,
            kind: SnapshotEntityKind::Node,
            id,
        })
}
