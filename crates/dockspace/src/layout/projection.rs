//! Root-local dense ownership for complete immutable projection results.

use std::fmt;
use std::iter::FusedIterator;
use std::ops::Index;
use std::slice;
use std::sync::Arc;

use super::{Axis, Constraints, LayoutNodeKind, LogicalRect, NodeId, RootLayoutTopology};

/// Immutable node values with deterministic identity-ordered iteration.
///
/// Storage is proportional to this root's reachable nodes, independently of the
/// workspace arena's slot count. Lookup compares the complete generational ID.
#[derive(Clone)]
pub struct NodeMap<T> {
    topology: Arc<RootLayoutTopology>,
    pub(super) values: Vec<T>,
}

impl<T> NodeMap<T> {
    pub(super) fn new(topology: Arc<RootLayoutTopology>, values: Vec<T>) -> Self {
        debug_assert_eq!(topology.nodes.len(), values.len());
        Self { topology, values }
    }

    /// Returns the value for this exact node identity.
    pub fn get(&self, node: &NodeId) -> Option<&T> {
        self.topology.row(node).map(|row| &self.values[row])
    }

    /// Returns whether this exact node identity has a value.
    pub fn contains_key(&self, node: &NodeId) -> bool {
        self.topology.row(node).is_some()
    }

    /// Returns the number of reachable nodes.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns whether the map contains no nodes.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Visits entries in ascending node identity order.
    pub fn iter(&self) -> NodeIter<'_, T> {
        NodeIter {
            map: self,
            rows: self.topology.ordered_rows.iter(),
        }
    }

    /// Visits stored entries in left-to-right depth-first first-visit order.
    ///
    /// Every complete generational identity appears once. This borrows only
    /// the owned projection and allocates no traversal state. Consumers joining
    /// a live tree must compare each yielded identity with the expected node;
    /// iteration does not authorize that tree's commands or current contents.
    pub fn iter_preorder(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&NodeId, &T)> + ExactSizeIterator + FusedIterator {
        self.topology
            .nodes
            .iter()
            .zip(&self.values)
            .map(|(node, value)| (&node.id, value))
    }

    /// Visits node identities in ascending order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &NodeId> + ExactSizeIterator {
        self.iter().map(|(node, _)| node)
    }

    /// Visits values in ascending node identity order.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator {
        self.iter().map(|(_, value)| value)
    }
}

impl<T: fmt::Debug> fmt::Debug for NodeMap<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self).finish()
    }
}

impl<T: PartialEq> PartialEq for NodeMap<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: Eq> Eq for NodeMap<T> {}

impl<T> Index<&NodeId> for NodeMap<T> {
    type Output = T;

    fn index(&self, node: &NodeId) -> &Self::Output {
        self.get(node)
            .expect("node is absent from layout projection")
    }
}

impl<'a, T> IntoIterator for &'a NodeMap<T> {
    type Item = (&'a NodeId, &'a T);
    type IntoIter = NodeIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Identity-ordered borrowed entries from a [`NodeMap`].
pub struct NodeIter<'a, T> {
    map: &'a NodeMap<T>,
    rows: slice::Iter<'a, usize>,
}

impl<'a, T> Iterator for NodeIter<'a, T> {
    type Item = (&'a NodeId, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        self.rows
            .next()
            .map(|&row| (&self.map.topology.nodes[row].id, &self.map.values[row]))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl<T> DoubleEndedIterator for NodeIter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.rows
            .next_back()
            .map(|&row| (&self.map.topology.nodes[row].id, &self.map.values[row]))
    }
}

impl<T> ExactSizeIterator for NodeIter<'_, T> {}
impl<T> FusedIterator for NodeIter<'_, T> {}

/// Borrowed geometry and constraint outcomes for one split node.
///
/// Child and splitter slices refer to the projection's flat owned arrays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitProjection<'a> {
    /// Direction in which child rectangles were allocated.
    pub axis: Axis,
    /// Logical rectangles reserved between adjacent children.
    pub splitter_rects: &'a [LogicalRect],
    /// Minimum projected extent for each direct child along this split's axis.
    pub child_minimum_extents: &'a [f64],
    /// Maximum projected extent for each direct child along this split's axis.
    pub child_maximum_extents: &'a [f64],
    /// Actual projected extent for each direct child along this split's axis.
    pub child_extents: &'a [f64],
    /// Direct child which contains the root's declared central leaf, when any.
    pub central_index: Option<usize>,
    /// Extent by which child minima exceed the split bounds.
    pub overflow: f64,
    /// Extent left unused after every child reached its maximum.
    pub unallocated: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct SplitOutcome {
    central_index: Option<usize>,
    overflow: f64,
    unallocated: f64,
}

/// Complete split outcomes backed by root-local flat arrays.
#[derive(Clone)]
pub struct SplitMap {
    topology: Arc<RootLayoutTopology>,
    outcomes: Vec<SplitOutcome>,
    pub(super) splitter_rects: Vec<LogicalRect>,
    pub(super) child_minimum_extents: Vec<f64>,
    pub(super) child_maximum_extents: Vec<f64>,
    pub(super) child_extents: Vec<f64>,
}

impl SplitMap {
    pub(super) fn new(topology: Arc<RootLayoutTopology>, bounds: LogicalRect) -> Self {
        Self {
            outcomes: vec![SplitOutcome::default(); topology.split_rows.len()],
            splitter_rects: vec![bounds; topology.splitter_count],
            child_minimum_extents: vec![0.0; topology.children.len()],
            child_maximum_extents: vec![0.0; topology.children.len()],
            child_extents: vec![0.0; topology.children.len()],
            topology,
        }
    }

    /// Returns the split outcomes for this exact node identity.
    pub fn get(&self, node: &NodeId) -> Option<SplitProjection<'_>> {
        self.topology.row(node).and_then(|row| self.at_row(row))
    }

    /// Returns whether this exact node identity is a projected split.
    pub fn contains_key(&self, node: &NodeId) -> bool {
        self.get(node).is_some()
    }

    /// Returns the number of projected split nodes.
    pub fn len(&self) -> usize {
        self.outcomes.len()
    }

    /// Returns whether this root has no splits.
    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// Visits split identities and borrowed outcomes in ascending identity order.
    pub fn iter(&self) -> SplitIter<'_> {
        SplitIter {
            map: self,
            rows: self.topology.split_rows.iter(),
        }
    }

    /// Visits split identities in ascending order.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &NodeId> + ExactSizeIterator {
        self.iter().map(|(node, _)| node)
    }

    /// Visits borrowed split outcomes in ascending node identity order.
    pub fn values(
        &self,
    ) -> impl DoubleEndedIterator<Item = SplitProjection<'_>> + ExactSizeIterator {
        self.iter().map(|(_, value)| value)
    }

    pub(super) fn set_outcome(
        &mut self,
        split: usize,
        central_index: Option<usize>,
        overflow: f64,
        unallocated: f64,
    ) {
        self.outcomes[split] = SplitOutcome {
            central_index,
            overflow,
            unallocated,
        };
    }

    fn at_row(&self, row: usize) -> Option<SplitProjection<'_>> {
        let LayoutNodeKind::Split {
            axis,
            children,
            split,
            splitters,
            ..
        } = &self.topology.nodes[row].kind
        else {
            return None;
        };
        let outcome = self.outcomes[*split];
        Some(SplitProjection {
            axis: *axis,
            splitter_rects: &self.splitter_rects[splitters.clone()],
            child_minimum_extents: &self.child_minimum_extents[children.clone()],
            child_maximum_extents: &self.child_maximum_extents[children.clone()],
            child_extents: &self.child_extents[children.clone()],
            central_index: outcome.central_index,
            overflow: outcome.overflow,
            unallocated: outcome.unallocated,
        })
    }
}

impl fmt::Debug for SplitMap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_map().entries(self).finish()
    }
}

impl PartialEq for SplitMap {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<'a> IntoIterator for &'a SplitMap {
    type Item = (&'a NodeId, SplitProjection<'a>);
    type IntoIter = SplitIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Identity-ordered borrowed entries from a [`SplitMap`].
pub struct SplitIter<'a> {
    map: &'a SplitMap,
    rows: slice::Iter<'a, usize>,
}

impl<'a> Iterator for SplitIter<'a> {
    type Item = (&'a NodeId, SplitProjection<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|&row| {
            (
                &self.map.topology.nodes[row].id,
                self.map
                    .at_row(row)
                    .expect("split directory contains a split"),
            )
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl DoubleEndedIterator for SplitIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.rows.next_back().map(|&row| {
            (
                &self.map.topology.nodes[row].id,
                self.map
                    .at_row(row)
                    .expect("split directory contains a split"),
            )
        })
    }
}

impl ExactSizeIterator for SplitIter<'_> {}
impl FusedIterator for SplitIter<'_> {}

/// Complete deterministic projection of one workspace root.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutProjection {
    /// Logical rectangle assigned to every reachable node.
    pub node_rects: NodeMap<LogicalRect>,
    /// Composed minimum and maximum logical sizes for every reachable node.
    pub node_constraints: NodeMap<Constraints>,
    /// Complete split-specific geometry and allocation outcomes.
    pub splits: SplitMap,
}
