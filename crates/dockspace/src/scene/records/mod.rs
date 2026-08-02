//! Immutable presentation records grouped by semantic owner.

mod contained;
mod splitter;
mod tab;

pub use contained::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
};
pub use splitter::{
    SplitterGapPresentation, SplitterGapRecord, SplitterJunctionDirection, SplitterJunctionId,
    SplitterJunctionRecord, SplitterRecord, SplitterResizeHitError, SplitterResizeTarget,
    SplitterSceneId,
};
pub use tab::{
    PaneRecord, PaneSceneId, TabBarRecord, TabBarSceneId, TabGroupDragRecord,
    TabListMenuBackdropRecord, TabListMenuGeometryAvailability, TabListMenuRecord,
    TabListMenuRowRecord, TabRecord, TabSceneId, TabStripControlRecord, TabStripMemberRecord,
    TabStripMemberVisibility,
};
