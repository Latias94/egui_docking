//! Immutable presentation records grouped by semantic owner.

mod contained;
mod layout;
mod presentation;
mod splitter;
mod tab;

pub(crate) use layout::{PresentationLayoutFacts, RootLayoutFacts};

pub use contained::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
};
pub use presentation::{PresentationMenuAnchorHost, PresentationMenuAnchorRecord};
pub use splitter::{
    SplitterGapPresentation, SplitterGapRecord, SplitterJunctionDirection, SplitterJunctionId,
    SplitterJunctionRecord, SplitterRecord, SplitterResizeHitError, SplitterResizeTarget,
    SplitterSceneId,
};
pub use tab::{
    PaneRecord, PaneSceneId, TabBarRecord, TabBarSceneId, TabGroupDragRecord,
    TabGroupDragRegionKind, TabGroupDragRegionRecord, TabListMenuBackdropRecord,
    TabListMenuGeometryAvailability, TabListMenuRecord, TabListMenuRowRecord, TabRecord,
    TabSceneId, TabStripControlRecord, TabStripMemberRecord, TabStripMemberVisibility,
};
