//! Retained renderer measurements needed for source-aware drop projection.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::interaction::{ContainedTransformSessionId, DragSessionId};
use crate::layout::LayoutProjection;
use crate::presentation_config::DockPresentationConfig;
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, PaneMinimumKey, SurfaceMeasurements, TabIntrinsic,
    TabIntrinsicKey, TabStripKey, TabStripMetrics,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainedLayoutOwner {
    Drag(DragSessionId),
    Transform(ContainedTransformSessionId),
}

/// A transient root layout input, never a mutation of durable placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContainedLayoutProjection {
    pub(crate) owner: ContainedLayoutOwner,
    pub(crate) surface: SurfaceId,
    pub(crate) root: RootId,
    pub(crate) floating: FloatingPresentationId,
    pub(crate) source_rect: LogicalRect,
    pub(crate) rect: LogicalRect,
    /// Target geometry lineage preserved only by recompilation of identical measurements.
    pub(crate) target_basis: Option<super::super::SurfaceSceneStamp>,
}

impl ContainedLayoutProjection {
    pub(crate) fn drop_source_suppression(self) -> crate::drop_target::DropSourceSuppression {
        match self.owner {
            ContainedLayoutOwner::Drag(_) => {
                crate::drop_target::DropSourceSuppression::complete_root(
                    self.root,
                    Some(self.floating),
                )
            }
            ContainedLayoutOwner::Transform(_) => Default::default(),
        }
    }
}

/// Exact layout inputs retained with one compiled presentation plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PresentationLayoutFacts {
    config: DockPresentationConfig,
    roots: Arc<BTreeMap<RootId, RootLayoutFacts>>,
    measurements: Arc<SurfaceMeasurements>,
}

impl PresentationLayoutFacts {
    pub(crate) fn new(
        config: DockPresentationConfig,
        measurements: AuthoritativeSurfaceMeasurements<'_>,
    ) -> Self {
        Self {
            config,
            roots: Arc::new(BTreeMap::new()),
            measurements: Arc::new(measurements.snapshot()),
        }
    }

    pub(crate) const fn config(&self) -> &DockPresentationConfig {
        &self.config
    }

    pub(crate) fn root(&self, root: RootId) -> Option<&RootLayoutFacts> {
        self.roots.get(&root)
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = &RootLayoutFacts> {
        self.roots.values()
    }

    pub(crate) fn insert_root(&mut self, facts: RootLayoutFacts) -> bool {
        Arc::make_mut(&mut self.roots)
            .insert(facts.root(), facts)
            .is_none()
    }

    pub(crate) fn tab_intrinsic(&self, key: TabIntrinsicKey) -> Option<TabIntrinsic> {
        self.measurements.measured_tab_intrinsic(key)
    }

    pub(crate) fn tab_strip(&self, key: TabStripKey) -> Option<TabStripMetrics> {
        self.measurements.measured_tab_strip(key)
    }

    pub(crate) fn measurements(&self) -> &Arc<SurfaceMeasurements> {
        &self.measurements
    }

    pub(crate) fn rebind_measurements(&self, measurements: SurfaceMeasurements) -> Self {
        Self {
            config: self.config.clone(),
            roots: Arc::clone(&self.roots),
            measurements: Arc::new(measurements),
        }
    }

    pub(crate) fn same_inputs(&self, other: &Self) -> bool {
        self.config == other.config
            && (Arc::ptr_eq(&self.roots, &other.roots) || self.roots == other.roots)
            && (Arc::ptr_eq(&self.measurements, &other.measurements)
                || self.measurements == other.measurements)
    }
}

/// Surface-local bounds and raw pane minima measured for one presented root.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RootLayoutFacts {
    root: RootId,
    bounds: LogicalRect,
    pane_minimums: BTreeMap<PaneMinimumKey, LogicalSize>,
    projection: Arc<LayoutProjection>,
}

impl RootLayoutFacts {
    pub(crate) fn new(
        root: RootId,
        bounds: LogicalRect,
        pane_minimums: BTreeMap<PaneMinimumKey, LogicalSize>,
        projection: Arc<LayoutProjection>,
    ) -> Self {
        Self {
            root,
            bounds,
            pane_minimums,
            projection,
        }
    }

    pub(crate) const fn root(&self) -> RootId {
        self.root
    }

    pub(crate) const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    pub(crate) fn projection(&self) -> &LayoutProjection {
        &self.projection
    }

    pub(crate) fn pane_minimums(&self) -> impl Iterator<Item = (PaneMinimumKey, LogicalSize)> + '_ {
        self.pane_minimums
            .iter()
            .map(|(key, minimum)| (*key, *minimum))
    }

    pub(crate) fn pane_minimum(&self, key: PaneMinimumKey) -> Option<LogicalSize> {
        self.pane_minimums.get(&key).copied()
    }
}
