//! Versioned, renderer-neutral persistence for validated workspaces.
//!
//! Snapshot node identities are local integers, not serialized [`crate::ids::NodeId`] values.
//! Restoring a snapshot always allocates fresh generational runtime identities and publishes only
//! a fully validated candidate.
//!
//! The version-first envelope targets self-describing Serde formats whose deserializers support
//! `deserialize_ignored_any`, such as JSON and RON. Applications using a non-self-describing
//! binary codec must frame and reject unsupported versions before invoking its payload decoder.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::canonical::CanonicalizationError;
use crate::engine::{DockEngine, EngineError};
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{
    Axis, ContainedFloating, InvalidSplitWeight, Node, RootRecord, SplitWeight,
    SurfacePresentation, Workspace, WorkspaceBuildError, WorkspaceBuilder,
};
use crate::ids::{FloatingPresentationId, InputSequence, ItemId, NodeId, RootId, SurfaceId};
use crate::validation::WorkspaceValidationErrors;

/// The snapshot schema version emitted and accepted by this crate release.
pub const WORKSPACE_SNAPSHOT_VERSION: u32 = 1;

/// A complete renderer-neutral workspace snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshot {
    /// Snapshot schema version.
    pub version: u32,
    /// Flat node arena with snapshot-local integer identities.
    pub nodes: Vec<SnapshotNodeRecord>,
    /// Stable docking roots.
    pub roots: Vec<SnapshotRootRecord>,
    /// Stable presentation surfaces.
    pub surfaces: Vec<SnapshotSurfaceRecord>,
    /// Contained-floating presentations.
    pub contained_floatings: Vec<SnapshotContainedFloatingRecord>,
}

#[derive(Serialize)]
struct WorkspaceSnapshotPayloadRef<'snapshot> {
    nodes: &'snapshot [SnapshotNodeRecord],
    roots: &'snapshot [SnapshotRootRecord],
    surfaces: &'snapshot [SnapshotSurfaceRecord],
    contained_floatings: &'snapshot [SnapshotContainedFloatingRecord],
}

impl Serialize for WorkspaceSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut document = serializer.serialize_tuple(2)?;
        document.serialize_element(&self.version)?;
        document.serialize_element(&WorkspaceSnapshotPayloadRef {
            nodes: &self.nodes,
            roots: &self.roots,
            surfaces: &self.surfaces,
            contained_floatings: &self.contained_floatings,
        })?;
        document.end()
    }
}

/// Version-first decoding envelope for a self-describing serialized workspace snapshot.
///
/// Deserialize external data into this type, then call [`Self::into_snapshot`].
/// Unsupported versions are accepted by the envelope regardless of their body
/// shape and become a typed [`SnapshotRestoreError::UnsupportedVersion`]. The
/// supported version still rejects missing, duplicate, and unknown fields.
/// Non-self-describing binary codecs require an application-owned outer version
/// and length frame instead of this envelope's unsupported-payload path.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshotEnvelope {
    version: u32,
    snapshot: Option<WorkspaceSnapshot>,
}

impl WorkspaceSnapshotEnvelope {
    /// Returns the version declared by the serialized document.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Returns the strictly decoded supported snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotRestoreError::UnsupportedVersion`] for every version
    /// other than [`WORKSPACE_SNAPSHOT_VERSION`].
    pub fn into_snapshot(self) -> Result<WorkspaceSnapshot, SnapshotRestoreError> {
        self.snapshot
            .ok_or(SnapshotRestoreError::UnsupportedVersion {
                found: self.version,
                supported: WORKSPACE_SNAPSHOT_VERSION,
            })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSnapshotV1 {
    nodes: Vec<SnapshotNodeRecord>,
    roots: Vec<SnapshotRootRecord>,
    surfaces: Vec<SnapshotSurfaceRecord>,
    contained_floatings: Vec<SnapshotContainedFloatingRecord>,
}

impl WorkspaceSnapshotV1 {
    fn into_snapshot(self) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: WORKSPACE_SNAPSHOT_VERSION,
            nodes: self.nodes,
            roots: self.roots,
            surfaces: self.surfaces,
            contained_floatings: self.contained_floatings,
        }
    }
}

impl<'de> Deserialize<'de> for WorkspaceSnapshotEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, WorkspaceSnapshotEnvelopeVisitor)
    }
}

struct WorkspaceSnapshotEnvelopeVisitor;

impl<'de> Visitor<'de> for WorkspaceSnapshotEnvelopeVisitor {
    type Value = WorkspaceSnapshotEnvelope;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a [version, payload] workspace snapshot document")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let version = sequence
            .next_element::<u32>()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let snapshot = if version == WORKSPACE_SNAPSHOT_VERSION {
            let payload = sequence
                .next_element::<WorkspaceSnapshotV1>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            Some(payload.into_snapshot())
        } else {
            sequence
                .next_element::<IgnoredAny>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            None
        };
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::invalid_length(3, &self));
        }
        Ok(WorkspaceSnapshotEnvelope { version, snapshot })
    }
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
        selected: Option<u64>,
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
    pub central: Option<u64>,
}

/// Persisted presentation surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSurfaceRecord {
    /// Stable surface identity.
    pub id: u64,
    /// Stable root occupying the main dock area.
    pub main_root: u64,
    /// Stable contained-floating identities in roster order.
    pub contained: Vec<u64>,
}

/// Persisted contained-floating presentation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotContainedFloatingRecord {
    /// Stable floating presentation identity.
    pub id: u64,
    /// Stable presented root identity.
    pub root: u64,
    /// Stable owning surface identity.
    pub surface: u64,
    /// Logical bounds relative to the owning surface.
    pub rect: SnapshotLogicalRect,
    /// Explicit stacking order within the surface.
    pub z_order: u64,
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
    /// The snapshot version is not implemented by this crate release.
    #[error("unsupported workspace snapshot version {found}; supported version is {supported}")]
    UnsupportedVersion {
        /// Version read from the snapshot.
        found: u32,
        /// Version implemented by this crate release.
        supported: u32,
    },
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

/// Failure to validate and queue a snapshot replacement through the authoritative engine.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SnapshotReplacementError {
    /// The snapshot could not produce a complete validated candidate.
    #[error(transparent)]
    Candidate(#[from] SnapshotRestoreError),
    /// The engine could not assign an input sequence to the replacement.
    #[error(transparent)]
    Enqueue(#[from] EngineError),
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
                main_root: surface.main_root.get(),
                contained: surface.contained.iter().map(|id| id.get()).collect(),
            })
            .collect();
        let contained_floatings = workspace
            .contained_floatings()
            .map(|(_, floating)| SnapshotContainedFloatingRecord {
                id: floating.id.get(),
                root: floating.root.get(),
                surface: floating.surface.get(),
                rect: SnapshotLogicalRect::from(floating.rect),
                z_order: floating.z_order,
            })
            .collect();

        Ok(Self {
            version: WORKSPACE_SNAPSHOT_VERSION,
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
    /// Prefer [`DockEngine::enqueue_snapshot_replacement`] when publishing into
    /// an existing engine so the workspace epoch advances and every old runtime
    /// [`NodeId`] becomes stale.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotRestoreError`] for unsupported versions, duplicate identities, unknown
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

impl DockEngine {
    /// Validates a snapshot and queues it as an epoch-advancing replacement.
    ///
    /// The engine remains completely unchanged when candidate construction or
    /// input sequencing fails. A successful call only queues the replacement;
    /// [`DockEngine::reduce_pending`] publishes it at the next engine boundary.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotReplacementError`] when the snapshot is invalid, an
    /// item is absent from the registry, or the engine input sequence is exhausted.
    pub fn enqueue_snapshot_replacement(
        &mut self,
        snapshot: &WorkspaceSnapshot,
        contains_item: impl Fn(ItemId) -> bool,
    ) -> Result<InputSequence, SnapshotReplacementError> {
        let candidate = snapshot.build_candidate(contains_item)?;
        self.enqueue_workspace_replacement(candidate)
            .map_err(SnapshotReplacementError::Enqueue)
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
    surfaces: HashSet<u64>,
    contained_floatings: HashSet<u64>,
}

impl SnapshotIndex {
    fn new(snapshot: &WorkspaceSnapshot) -> Result<Self, SnapshotRestoreError> {
        if snapshot.version != WORKSPACE_SNAPSHOT_VERSION {
            return Err(SnapshotRestoreError::UnsupportedVersion {
                found: snapshot.version,
                supported: WORKSPACE_SNAPSHOT_VERSION,
            });
        }

        Ok(Self {
            nodes: unique_ids(
                snapshot.nodes.iter().map(|record| record.id),
                SnapshotEntityKind::Node,
            )?,
            roots: unique_ids(
                snapshot.roots.iter().map(|record| record.id),
                SnapshotEntityKind::Root,
            )?,
            surfaces: unique_ids(
                snapshot.surfaces.iter().map(|record| record.id),
                SnapshotEntityKind::Surface,
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
        Node::Tabs { items, selected } => SnapshotNode::Tabs {
            items: items.iter().map(|item| item.get()).collect(),
            selected: selected.map(ItemId::get),
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
        require_reference(
            &index.roots,
            owner,
            SnapshotEntityKind::Root,
            surface.main_root,
        )?;
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
        require_reference(
            &index.surfaces,
            owner,
            SnapshotEntityKind::Surface,
            floating.surface,
        )?;
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
        } = &record.node
        {
            items.extend(tab_items.iter().copied());
            items.extend(selected.iter().copied());
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
        SnapshotNode::Tabs { items, selected } => Ok(Node::tabs_with_selection(
            items.iter().copied().map(ItemId::new),
            selected.map(ItemId::new),
        )),
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
                main_root: RootId::new(surface.main_root),
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
        builder.set_contained_floating(ContainedFloating::new(
            FloatingPresentationId::new(floating.id),
            RootId::new(floating.root),
            SurfaceId::new(floating.surface),
            restore_rect(floating)?,
            floating.z_order,
        ));
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
