//! Root-scoped presentation command menu anchor records.

use crate::drop_target::SceneLayerKey;
use crate::geometry::LogicalRect;
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, RootId};

use super::TabBarSceneId;

/// Structural host of one root-scoped presentation command menu anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresentationMenuAnchorHost {
    /// The sole selected tab bar of a main or native-main root.
    TabBar(TabBarSceneId),
    /// The title chrome of one contained-floating presentation.
    ContainedTitle(FloatingPresentationId),
}

/// Core-compiled exact draw and hit geometry for one root presentation menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationMenuAnchorRecord {
    root: RootId,
    host: PresentationMenuAnchorHost,
    bounds: Option<LogicalRect>,
    hit: Option<HitRegion>,
    layer: SceneLayerKey,
}

impl PresentationMenuAnchorRecord {
    pub(crate) const fn ready(
        root: RootId,
        host: PresentationMenuAnchorHost,
        bounds: LogicalRect,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            root,
            host,
            bounds: Some(bounds),
            hit: Some(HitRegion::new(bounds)),
            layer,
        }
    }

    pub(crate) const fn compacted(
        root: RootId,
        host: PresentationMenuAnchorHost,
        layer: SceneLayerKey,
    ) -> Self {
        Self {
            root,
            host,
            bounds: None,
            hit: None,
            layer,
        }
    }

    /// Returns the root whose presentation commands this anchor opens.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the exact structural chrome host selected by core.
    #[must_use]
    pub const fn host(self) -> PresentationMenuAnchorHost {
        self.host
    }

    /// Returns the exact draw bounds reserved by core, or `None` when compacted.
    #[must_use]
    pub const fn ready_bounds(self) -> Option<LogicalRect> {
        self.bounds
    }

    /// Returns the exact half-open interaction region, or `None` when compacted.
    #[must_use]
    pub const fn ready_hit(self) -> Option<HitRegion> {
        self.hit
    }

    /// Returns whether this root currently publishes usable menu chrome.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        self.bounds.is_some()
    }

    /// Returns the roster-derived semantic layer.
    #[must_use]
    pub const fn layer(self) -> SceneLayerKey {
        self.layer
    }
}
