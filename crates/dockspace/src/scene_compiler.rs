//! Core-owned derivation and compilation of semantic presentation scenes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[cfg(test)]
use std::{cell::Cell, thread_local};

use thiserror::Error;

use crate::command::{DockFraction, Edge};
use crate::drop_guide::DropGuideClusterId;
use crate::drop_target::{
    DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetRecord,
    DropTargetUnavailable, DropVisual, SceneLayerKey, SurfaceBackground,
};
use crate::error::CommandError;
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{Node, Workspace};
use crate::hit_region::HitRegion;
use crate::ids::{EngineAuthorityDomainId, FloatingPresentationId, NodeId, RootId, SurfaceId};
use crate::layout::SplitWeightOverride;
use crate::policy::{DockPolicySnapshot, TabBarInteraction, TabBarVisibility};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::scene::{
    ContainedLayoutProjection, ContainedMinimumMeasurement, PresentationPlan,
    PresentationPlanValidator, RootLayoutFacts, SceneBuildError, SplitterGapPresentation,
    SplitterGapRecord, SplitterJunctionRecord, SplitterRecord, SplitterSceneId,
    SurfaceProgramRootOperation, SurfaceProgramRootPresentation, TabBarSceneId,
    TabListMenuBackdropRecord, TabSceneId,
};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, ManifestBuildError, PaneMinimumKey, RequirementBuildError,
    RequirementRevision, SceneRequirementDraft, SceneRequirementManifest, SurfaceMeasurementTicket,
    SurfaceMeasurements, SurfaceRequirementRevision, SurfaceRequirements, TabBarRequirement,
    TabIntrinsicKey, TabStripKey,
};
use crate::splitter_junction_index::derive_splitter_junction_candidates;
use crate::tab_strip::{PopupPlaneRequirement, TabStripStateStore};
use crate::target_field::MagneticEdgeTargets;
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

mod future_layout;
mod requirements;
mod surface;
mod tab_strip_layout;

pub(crate) use tab_strip_layout::reveal_tab_bar_bounds;

pub(crate) use future_layout::{
    FutureLayoutProjectionError, FutureTab, FutureTabStrip, candidate_tab_bar_policy,
    project_future_root, project_future_tab_strip,
};
pub(crate) use requirements::{
    SceneRequirementDerivationError, derive_scene_requirement_draft,
    derive_scene_requirement_draft_with_index,
};
pub(crate) use surface::{
    CompiledSurfacePlan, compile_surface_plan, contained_minimum_size, pane_outer_minimum,
    selection_geometry_unchanged,
};
pub use surface::{PresentationCompilationError, SceneCompilationError};

#[cfg(test)]
pub(crate) use surface::{
    compile_surface_measurements, compile_surface_measurements_without_guides,
    reset_surface_compilation_count, surface_compilation_count,
};

#[cfg(test)]
use surface::{edge_preview, rect_contains, reveal_tab_range, tab_strip_control_layout};

#[cfg(test)]
mod tests;
