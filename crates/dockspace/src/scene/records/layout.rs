//! Retained renderer measurements needed for source-aware drop projection.

use std::collections::BTreeMap;

use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::RootId;
use crate::presentation_config::DockPresentationConfig;
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, PaneMinimumKey, TabIntrinsic, TabIntrinsicKey, TabStripKey,
    TabStripMetrics,
};

/// Exact layout inputs retained with one compiled presentation plan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PresentationLayoutFacts {
    config: DockPresentationConfig,
    roots: BTreeMap<RootId, RootLayoutFacts>,
    tab_intrinsics: BTreeMap<TabIntrinsicKey, TabIntrinsic>,
    tab_strips: BTreeMap<TabStripKey, TabStripMetrics>,
}

impl PresentationLayoutFacts {
    pub(crate) fn new(
        config: DockPresentationConfig,
        measurements: AuthoritativeSurfaceMeasurements<'_>,
    ) -> Self {
        Self {
            config,
            roots: BTreeMap::new(),
            tab_intrinsics: measurements.tab_intrinsics().collect(),
            tab_strips: measurements
                .tab_strips()
                .map(|(key, metrics)| (key, metrics.without_legacy_scroll_offset()))
                .collect(),
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
        self.roots.insert(facts.root(), facts).is_none()
    }

    pub(crate) fn tab_intrinsic(&self, key: TabIntrinsicKey) -> Option<TabIntrinsic> {
        self.tab_intrinsics.get(&key).copied()
    }

    pub(crate) fn tab_strip(&self, key: TabStripKey) -> Option<TabStripMetrics> {
        self.tab_strips.get(&key).copied()
    }
}

/// Surface-local bounds and raw pane minima measured for one presented root.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RootLayoutFacts {
    root: RootId,
    bounds: LogicalRect,
    pane_minimums: BTreeMap<PaneMinimumKey, LogicalSize>,
}

impl RootLayoutFacts {
    pub(crate) fn new(
        root: RootId,
        bounds: LogicalRect,
        pane_minimums: BTreeMap<PaneMinimumKey, LogicalSize>,
    ) -> Self {
        Self {
            root,
            bounds,
            pane_minimums,
        }
    }

    pub(crate) const fn root(&self) -> RootId {
        self.root
    }

    pub(crate) const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    pub(crate) fn pane_minimums(&self) -> impl Iterator<Item = (PaneMinimumKey, LogicalSize)> + '_ {
        self.pane_minimums
            .iter()
            .map(|(key, minimum)| (*key, *minimum))
    }
}
