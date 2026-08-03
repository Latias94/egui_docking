//! Core-owned derivation and compilation of semantic presentation scenes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[cfg(test)]
use std::{cell::Cell, thread_local};

use thiserror::Error;

use crate::command::{DockFraction, DockTarget, Edge};
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideEdgeSet, DropGuideTargetRecord,
};
use crate::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey, SurfaceBackground,
};
use crate::error::CommandError;
use crate::geometry::{Constraints, GeometryError, LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{EngineAuthorityDomainId, FloatingPresentationId, NodeId, RootId, SurfaceId};
use crate::layout::{
    LayoutError, LayoutMetrics, LayoutMetricsError, SplitWeightOverride,
    project_root_with_overrides,
};
use crate::policy::{
    DockPolicySnapshot, DockTabBarPolicyRequest, TabBarInteraction, TabBarVisibility,
};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::scene::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
    PaneRecord, PaneSceneId, PresentationPlan, RootLayoutFacts, SceneBuildError,
    SplitterGapPresentation, SplitterGapRecord, SplitterJunctionRecord, SplitterRecord,
    SplitterSceneId, TabBarRecord, TabBarSceneId, TabGroupDragRecord, TabListMenuBackdropRecord,
    TabListMenuGeometryAvailability, TabListMenuRecord, TabListMenuRowRecord, TabRecord,
    TabSceneId, TabStripControlRecord, TabStripMemberRecord, TabStripMemberVisibility,
};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, ManifestBuildError, ManifestMeasurementError,
    MeasurementAuthorityError, PaneMinimumKey, PolicyRevision, RequirementBuildError,
    RequirementRevision, SceneRequirementDraft, SceneRequirementManifest, SurfaceMeasurementTicket,
    SurfaceMeasurements, SurfaceRequirementRevision, SurfaceRequirements, TabBarRequirement,
    TabIntrinsicKey, TabStripControlMetrics, TabStripControlPlacement, TabStripKey,
};
use crate::splitter_junction_index::derive_splitter_junction_candidates;
use crate::tab_strip::{
    ActiveTabListMenu, PopupPlaneRequirement, TabStripControlId, TabStripStateKey,
    TabStripStateStore,
};
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

mod future_layout;
mod requirements;
mod surface;

pub(crate) use future_layout::{
    FutureLayoutProjectionError, project_future_root, project_future_tab_gap_visual,
};
pub(crate) use requirements::{
    derive_scene_requirement_draft, derive_scene_requirement_draft_with_index,
};
pub(crate) use surface::compile_surface_measurements;
pub use surface::{PresentationCompilationError, SceneCompilationError};

#[cfg(test)]
use requirements::SceneRequirementDerivationError;

#[cfg(test)]
pub(crate) use surface::{reset_surface_compilation_count, surface_compilation_count};

#[cfg(test)]
use surface::{edge_preview, rect_contains, reveal_tab_range, tab_strip_control_layout};

#[cfg(test)]
mod tests;
