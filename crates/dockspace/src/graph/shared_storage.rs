//! Shared canonical payloads behind the graph owner's existing value access.

use std::sync::Arc;

use slotmap::SlotMap;

use super::Node;
use crate::ids::NodeId;

/// The allocator is independently cloned; only immutable node payloads share.
#[derive(Debug, Default)]
pub(crate) struct NodeStore {
    slots: SlotMap<NodeId, Arc<Node>>,
}

impl Clone for NodeStore {
    fn clone(&self) -> Self {
        #[cfg(test)]
        crate::drop_resolver::structural_work::record_node_store_clone(self.slots.len());
        Self {
            slots: self.slots.clone(),
        }
    }
}

impl NodeStore {
    pub(crate) fn shared(&self, node: NodeId) -> Option<&Arc<Node>> {
        self.slots.get(node)
    }

    pub(crate) fn get(&self, node: NodeId) -> Option<&Node> {
        self.slots.get(node).map(Arc::as_ref)
    }

    pub(crate) fn get_mut(&mut self, node: NodeId) -> Option<&mut Node> {
        self.slots.get_mut(node).map(Arc::make_mut)
    }

    pub(crate) fn insert(&mut self, node: Node) -> NodeId {
        self.slots.insert(Arc::new(node))
    }

    /// Replacing a whole record must not copy the payload being discarded.
    pub(crate) fn replace(&mut self, id: NodeId, node: Node) -> Option<Arc<Node>> {
        let slot = self.slots.get_mut(id)?;
        Some(std::mem::replace(slot, Arc::new(node)))
    }

    /// Retiring a shared record transfers its handle without copying its Vecs.
    pub(crate) fn remove(&mut self, node: NodeId) -> Option<Arc<Node>> {
        self.slots.remove(node)
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(NodeId, &Node) -> bool) {
        self.slots.retain(|id, node| keep(id, node.as_ref()));
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.slots.iter().map(|(id, node)| (id, node.as_ref()))
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &Node> {
        self.slots.values().map(Arc::as_ref)
    }

    pub(crate) fn keys(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.slots.keys()
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.slots.capacity()
    }

    pub(crate) fn contains_key(&self, node: NodeId) -> bool {
        self.slots.contains_key(node)
    }
}
