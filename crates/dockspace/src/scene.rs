//! Type-state construction of immutable semantic docking scenes.

mod lifecycle;
mod records;
mod validation;

pub(crate) use self::lifecycle::PopupInteractionGateRevision;
pub use self::lifecycle::{
    BootstrapSurfaceScene, BootstrapSurfaceSceneReason, PopupGeometryUnavailableReason,
    ReadySurfaceScene, RetainedSurfacePlanStamps, StaleSurfaceScene, StaleSurfaceSceneReason,
    SurfaceInteractionProjection, SurfacePaintProjection, SurfacePlanScene, SurfaceScene,
    SurfaceSceneSet, SurfaceSceneStamp,
};
pub(crate) use self::validation::PresentationPlanValidator;
use self::validation::region_has_authoritative_area;
#[cfg(test)]
use self::validation::splitter_extent_satisfies_constraints;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use thiserror::Error;

use crate::RootPresentationOwner;
use crate::command::{DockTarget, NodeFingerprint};
use crate::coordinates::CoordinateSnapshot;
use crate::drop_guide::{
    DropGuideClusterId, DropGuideClusterRecord, DropGuideScope, DropGuideSlot,
    DropGuideTargetRecord,
};
use crate::drop_target::{
    DropDestination, DropOcclusionRecord, DropTargetAvailability, DropTargetId, DropTargetKind,
    DropTargetRecord, DropTargetUnavailable,
};
use crate::geometry::{GeometryError, LogicalRect};
use crate::graph::{Node, Workspace};
use crate::ids::{
    EngineAuthorityDomainId, FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId,
};
use crate::policy::{
    DockPolicySnapshot, DockTabBarPolicyRequest, TabBarInteraction, TabBarVisibility,
};
use crate::presentation_config::DockPresentationConfig;
use crate::presentation_hit::PresentationHitManifest;
use crate::presentation_observation::{
    HostPresentationEndpoint, PresentationAuthorityRejection, PresentationOutputSerial,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::scene_manifest::{
    AuthoritativeSurfaceMeasurements, MeasurementAuthorityError, SceneRequirementManifest,
    SurfaceMeasurementTicket, SurfaceSceneRevision,
};
use crate::semantic_manifest::PresentationSemanticManifest;
use crate::splitter_junction_index::derive_splitter_junction_candidates;
#[cfg(test)]
use crate::tab_strip::PopupRoutingRevision;
use crate::tab_strip::{
    PopupPlaneRequirement, TabListMenuSessionId, TabStripControlId, TabStripStateKey,
};
use crate::target_field::{MagneticEdgeTargets, MagneticTargetCandidate, MagneticTargetField};
use crate::transition::WorkspaceVersion;
use crate::viewport::{CoordinateGeneration, ViewportBinding};
use crate::viewport_registry::ViewportLifecycle;

pub(crate) use self::records::{
    ContainedLayoutOwner, ContainedLayoutProjection, PresentationLayoutFacts, RootLayoutFacts,
};
pub use self::records::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
    PaneRecord, PaneSceneId, PresentationMenuAnchorHost, PresentationMenuAnchorRecord,
    SplitterGapPresentation, SplitterGapRecord, SplitterJunctionDirection, SplitterJunctionId,
    SplitterJunctionRecord, SplitterRecord, SplitterResizeTarget, SplitterSceneId, TabBarRecord,
    TabBarSceneId, TabGroupDragRecord, TabGroupDragRegionKind, TabGroupDragRegionRecord,
    TabListMenuBackdropRecord, TabListMenuGeometryAvailability, TabListMenuRecord,
    TabListMenuRowRecord, TabRecord, TabSceneId, TabStripControlRecord, TabStripMemberRecord,
    TabStripMemberVisibility,
};
pub use crate::drop_target::SceneLayerKey;

#[cfg(test)]
pub(crate) mod interaction_authority_work {
    use std::cell::Cell;

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(crate) struct Metrics {
        pub(crate) scans: usize,
        pub(crate) surface_visits: usize,
    }

    thread_local! {
        static METRICS: Cell<Metrics> = Cell::new(Metrics::default());
    }

    pub(crate) fn reset() {
        METRICS.with(|metrics| metrics.set(Metrics::default()));
    }

    pub(crate) fn snapshot() -> Metrics {
        METRICS.with(Cell::get)
    }

    pub(crate) fn record_scan(surface_count: usize) {
        METRICS.with(|metrics| {
            let mut next = metrics.get();
            next.scans += 1;
            next.surface_visits += surface_count;
            metrics.set(next);
        });
    }
}

/// Capture-time native association and coordinate authority for one roster surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SurfaceCoordinateCapture {
    /// No viewport record existed when scene collection began.
    Headless {
        /// Local association watermark retained even while no record exists.
        authority_generation: CoordinateGeneration,
    },
    /// A viewport association existed but was not ready to authorize scene geometry.
    NativeUnavailable {
        /// Exact native association observed at capture time.
        binding: ViewportBinding,
        /// Non-ready lifecycle observed at capture time.
        lifecycle: ViewportLifecycle,
        /// Exact local association/coordinate authority watermark.
        authority_generation: CoordinateGeneration,
    },
    /// An exact ready association and coordinate snapshot authorized collection.
    NativeReady {
        /// Binding-, generation-, bounds-, and scale-bound coordinate snapshot.
        coordinates: CoordinateSnapshot,
        /// Exact local association/coordinate authority watermark.
        authority_generation: CoordinateGeneration,
    },
}

impl SurfaceCoordinateCapture {
    pub(crate) const fn authority_generation(self) -> CoordinateGeneration {
        match self {
            Self::Headless {
                authority_generation,
            }
            | Self::NativeUnavailable {
                authority_generation,
                ..
            }
            | Self::NativeReady {
                authority_generation,
                ..
            } => authority_generation,
        }
    }

    pub(crate) fn same_projection_authority(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::Headless {
                    authority_generation: left,
                },
                Self::Headless {
                    authority_generation: right,
                },
            ) => left == right,
            (
                Self::NativeUnavailable {
                    binding: left_binding,
                    lifecycle: left_lifecycle,
                    authority_generation: left_generation,
                },
                Self::NativeUnavailable {
                    binding: right_binding,
                    lifecycle: right_lifecycle,
                    authority_generation: right_generation,
                },
            ) => {
                left_binding == right_binding
                    && left_lifecycle == right_lifecycle
                    && left_generation == right_generation
            }
            (
                Self::NativeReady {
                    coordinates: left,
                    authority_generation: left_generation,
                },
                Self::NativeReady {
                    coordinates: right,
                    authority_generation: right_generation,
                },
            ) => left_generation == right_generation && left.same_projection_authority(right),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceProgramRootPresentation {
    Main,
    Contained(FloatingPresentationId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceProgramRootOperation {
    TabBar(TabBarSceneId),
    Tab(TabSceneId),
    Pane(PaneSceneId),
    Splitter(SplitterSceneId),
    SplitterJunction(SplitterJunctionId),
    TabStripControl(TabStripControlId),
    ContainedControls(FloatingPresentationId),
    PresentationMenuAnchor(RootId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SurfaceProgramBlock {
    RootBegin {
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
        title_item: Option<ItemId>,
    },
    RootOperation(SurfaceProgramRootOperation),
    RootEnd {
        root: RootId,
    },
    PopupBackdrop(TabListMenuSessionId),
    PopupMenu(TabListMenuSessionId),
    DragPreview,
    DropTarget(DropTargetId),
    DropGuide(DropGuideClusterId),
    ContainedTransformPreview,
    DragDecoration,
    InteractionFeedback,
    SurfaceEnd,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedSurfaceProgramRoot {
    root: RootId,
    contained: Option<FloatingPresentationId>,
    title_item: Option<ItemId>,
}

impl ResolvedSurfaceProgramRoot {
    pub(crate) const fn root(self) -> RootId {
        self.root
    }

    pub(crate) const fn contained(self) -> Option<FloatingPresentationId> {
        self.contained
    }

    pub(crate) const fn title_item(self) -> Option<ItemId> {
        self.title_item
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolvedSurfaceProgramOperation<'a> {
    Pane(&'a PaneRecord),
    TabBar(&'a TabBarRecord),
    Tab(&'a TabRecord),
    Splitter(&'a SplitterRecord),
    SplitterJunction(&'a SplitterJunctionRecord),
    TabStripControl(&'a TabStripControlRecord),
    ContainedControls(&'a ContainedRecord),
    PresentationMenuAnchor(&'a PresentationMenuAnchorRecord),
}

#[allow(
    dead_code,
    reason = "the common visitor carries root provenance for the renderer sink"
)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum ResolvedSurfaceProgramStep<'a> {
    RootBegin(ResolvedSurfaceProgramRoot),
    Operation {
        root: ResolvedSurfaceProgramRoot,
        operation: ResolvedSurfaceProgramOperation<'a>,
    },
    RootEnd(ResolvedSurfaceProgramRoot),
    PopupBackdrop(&'a TabListMenuBackdropRecord),
    PopupMenu(&'a TabListMenuRecord),
    DragPreview,
    DropTarget(&'a DropTargetRecord),
    DropGuide(&'a DropGuideClusterRecord),
    ContainedTransformPreview,
    DragDecoration,
    InteractionFeedback,
    SurfaceEnd,
}

#[derive(Debug)]
pub(crate) enum SurfaceProgramVisitError<E> {
    InvalidProgram,
    Sink(E),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum SurfaceProgramBuildError {
    #[error("surface program already has an open root")]
    RootAlreadyOpen,
    #[error("surface program has no open root")]
    RootNotOpen,
    #[error("surface program expected open root {expected}, found {actual}")]
    RootMismatch { expected: RootId, actual: RootId },
    #[error("surface program already contains root {root}")]
    DuplicateRoot { root: RootId },
    #[error("surface program does not contain root {root}")]
    MissingRoot { root: RootId },
    #[error("surface program was already finished")]
    AlreadyFinished,
    #[error("surface program repeats operation {operation:?} for root {root}")]
    DuplicateOperation {
        root: RootId,
        operation: SurfaceProgramRootOperation,
    },
    #[error("surface program repeats guide cluster {id:?}")]
    DuplicateGuide { id: DropGuideClusterId },
    #[error("surface program repeats popup backdrop {session:?}")]
    DuplicatePopupBackdrop { session: TabListMenuSessionId },
    #[error("surface program repeats popup menu {session:?}")]
    DuplicatePopupMenu { session: TabListMenuSessionId },
    #[error("surface program popup session {session:?} has no backdrop")]
    PopupMenuWithoutBackdrop { session: TabListMenuSessionId },
    #[error("surface program repeats drop target {id:?}")]
    DuplicateDropTarget { id: DropTargetId },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfaceProgram {
    blocks: Vec<SurfaceProgramBlock>,
    root_drafts: Vec<SurfaceProgramRootDraft>,
    root_indices: std::collections::BTreeMap<RootId, usize>,
    active_root: Option<usize>,
    deferred_popup_backdrops: Vec<TabListMenuSessionId>,
    deferred_popup_menus: Vec<TabListMenuSessionId>,
    deferred_drop_targets: Vec<DropTargetId>,
    deferred_surface_background: Option<DropTargetId>,
    deferred_guides: Vec<DropGuideClusterId>,
    deferred_guide_targets: Vec<DropTargetId>,
    finished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SurfaceProgramRootDraft {
    root: RootId,
    presentation: SurfaceProgramRootPresentation,
    title_item: Option<ItemId>,
    operations: Vec<SurfaceProgramRootOperation>,
}

impl SurfaceProgram {
    fn begin_root(
        &mut self,
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
    ) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        if self.active_root.is_some() {
            return Err(SurfaceProgramBuildError::RootAlreadyOpen);
        }
        if self.root_indices.contains_key(&root) {
            return Err(SurfaceProgramBuildError::DuplicateRoot { root });
        }
        let index = self.root_drafts.len();
        self.root_drafts.push(SurfaceProgramRootDraft {
            root,
            presentation,
            title_item: None,
            operations: Vec::new(),
        });
        self.root_indices.insert(root, index);
        self.active_root = Some(index);
        Ok(())
    }

    fn push_root_operation(
        &mut self,
        operation: SurfaceProgramRootOperation,
        title_item: Option<ItemId>,
    ) -> Result<(), SurfaceProgramBuildError> {
        let index = self
            .active_root
            .ok_or(SurfaceProgramBuildError::RootNotOpen)?;
        if self.root_drafts[index].title_item.is_none() {
            self.root_drafts[index].title_item = title_item;
        }
        self.root_drafts[index].operations.push(operation);
        Ok(())
    }

    fn end_root(&mut self, expected: RootId) -> Result<(), SurfaceProgramBuildError> {
        let index = self
            .active_root
            .ok_or(SurfaceProgramBuildError::RootNotOpen)?;
        let actual = self.root_drafts[index].root;
        if actual != expected {
            return Err(SurfaceProgramBuildError::RootMismatch { expected, actual });
        }
        self.active_root = None;
        Ok(())
    }

    fn push_derived_root_operation(
        &mut self,
        root: RootId,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        let index = self
            .root_indices
            .get(&root)
            .copied()
            .ok_or(SurfaceProgramBuildError::MissingRoot { root })?;
        self.root_drafts[index].operations.push(operation);
        Ok(())
    }

    fn defer_popup_backdrop(&mut self, session: TabListMenuSessionId) {
        self.deferred_popup_backdrops.push(session);
    }

    fn defer_popup_menu(&mut self, session: TabListMenuSessionId) {
        self.deferred_popup_menus.push(session);
    }

    fn defer_drop_target(&mut self, id: DropTargetId) {
        self.deferred_drop_targets.push(id);
    }

    fn defer_surface_background(&mut self, id: DropTargetId) {
        self.deferred_surface_background = Some(id);
    }

    fn defer_guide(&mut self, cluster: &DropGuideClusterRecord) {
        self.deferred_guides.push(cluster.id());
        self.deferred_guide_targets
            .extend(cluster.targets().map(|(_, target)| target.id()));
    }

    fn finish(&mut self) -> Result<(), SurfaceProgramBuildError> {
        if self.finished {
            return Err(SurfaceProgramBuildError::AlreadyFinished);
        }
        if self.active_root.is_some() {
            return Err(SurfaceProgramBuildError::RootAlreadyOpen);
        }
        let mut seen_guides = std::collections::HashSet::new();
        for id in &self.deferred_guides {
            if !seen_guides.insert(*id) {
                return Err(SurfaceProgramBuildError::DuplicateGuide { id: *id });
            }
        }
        let mut seen_popup_backdrops = std::collections::HashSet::new();
        for session in &self.deferred_popup_backdrops {
            if !seen_popup_backdrops.insert(*session) {
                return Err(SurfaceProgramBuildError::DuplicatePopupBackdrop { session: *session });
            }
        }
        let mut seen_popup_menus = std::collections::HashSet::new();
        for session in &self.deferred_popup_menus {
            if !seen_popup_menus.insert(*session) {
                return Err(SurfaceProgramBuildError::DuplicatePopupMenu { session: *session });
            }
        }
        for session in &self.deferred_popup_menus {
            if !seen_popup_backdrops.contains(session) {
                return Err(SurfaceProgramBuildError::PopupMenuWithoutBackdrop {
                    session: *session,
                });
            }
        }
        // A non-owner surface may legitimately retain only the workspace
        // backdrop while another surface owns the popup menu.
        let mut seen_drop_targets = std::collections::HashSet::new();
        for id in &self.deferred_drop_targets {
            if !seen_drop_targets.insert(*id) {
                return Err(SurfaceProgramBuildError::DuplicateDropTarget { id: *id });
            }
        }
        for root in &self.root_drafts {
            let mut seen_operations = std::collections::HashSet::new();
            for operation in &root.operations {
                if !seen_operations.insert(*operation) {
                    return Err(SurfaceProgramBuildError::DuplicateOperation {
                        root: root.root,
                        operation: *operation,
                    });
                }
            }
        }
        let root_drafts = std::mem::take(&mut self.root_drafts);
        self.root_indices.clear();
        for mut root in root_drafts {
            root.operations
                .sort_by_key(|operation| operation.paint_rank());
            self.blocks.push(SurfaceProgramBlock::RootBegin {
                root: root.root,
                presentation: root.presentation,
                title_item: root.title_item,
            });
            self.blocks.extend(
                root.operations
                    .iter()
                    .copied()
                    .map(SurfaceProgramBlock::RootOperation),
            );
            self.blocks
                .push(SurfaceProgramBlock::RootEnd { root: root.root });
        }
        self.blocks.push(SurfaceProgramBlock::DragPreview);
        if let Some(id) = self.deferred_surface_background.take() {
            self.blocks.push(SurfaceProgramBlock::DropTarget(id));
        }
        let guide_targets = std::mem::take(&mut self.deferred_guide_targets)
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        self.blocks.extend(
            std::mem::take(&mut self.deferred_drop_targets)
                .into_iter()
                .filter(|id| id.kind() == DropTargetKind::TabGap && !guide_targets.contains(id))
                .map(SurfaceProgramBlock::DropTarget),
        );
        for id in std::mem::take(&mut self.deferred_guides) {
            self.blocks.push(SurfaceProgramBlock::DropGuide(id));
        }
        self.blocks
            .push(SurfaceProgramBlock::ContainedTransformPreview);
        self.blocks.push(SurfaceProgramBlock::DragDecoration);
        self.blocks.extend(
            std::mem::take(&mut self.deferred_popup_backdrops)
                .into_iter()
                .map(SurfaceProgramBlock::PopupBackdrop),
        );
        self.blocks.extend(
            std::mem::take(&mut self.deferred_popup_menus)
                .into_iter()
                .map(SurfaceProgramBlock::PopupMenu),
        );
        self.blocks.push(SurfaceProgramBlock::InteractionFeedback);
        self.blocks.push(SurfaceProgramBlock::SurfaceEnd);
        self.finished = true;
        Ok(())
    }

    pub(crate) fn blocks(&self) -> &[SurfaceProgramBlock] {
        &self.blocks
    }
}

impl SurfaceProgramRootOperation {
    const fn paint_rank(self) -> usize {
        match self {
            Self::Pane(_) => 0,
            Self::TabBar(_) => 1,
            Self::Tab(_) => 2,
            Self::Splitter(_) | Self::SplitterJunction(_) => 3,
            Self::TabStripControl(_) => 4,
            Self::ContainedControls(_) => 5,
            Self::PresentationMenuAnchor(_) => 6,
        }
    }
}

/// Complete semantic facts compiled as one candidate surface projection.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationPlan {
    surface: SurfaceId,
    provenance: PresentationPlanProvenance,
    popup: PopupPlaneRequirement,
    bounds: LogicalRect,
    popup_plane_bounds: Option<LogicalRect>,
    layout_facts: Option<PresentationLayoutFacts>,
    contained_projection: Option<ContainedLayoutProjection>,
    pane_records: Vec<PaneRecord>,
    tab_records: Vec<TabRecord>,
    tab_bar_records: Vec<TabBarRecord>,
    tab_strip_control_records: Vec<TabStripControlRecord>,
    tab_list_menu_records: Vec<TabListMenuRecord>,
    tab_list_menu_backdrop_records: Vec<TabListMenuBackdropRecord>,
    presentation_menu_anchor_records: Vec<PresentationMenuAnchorRecord>,
    splitter_gap_records: Vec<SplitterGapRecord>,
    splitter_records: Vec<SplitterRecord>,
    splitter_junction_records: Vec<SplitterJunctionRecord>,
    contained_records: Vec<ContainedRecord>,
    contained_minimums: Vec<ContainedMinimumMeasurement>,
    surface_background: Option<DropTargetRecord>,
    drop_occlusions: Vec<DropOcclusionRecord>,
    drop_guide_clusters: Vec<DropGuideClusterRecord>,
    drop_targets: Vec<Arc<DropTargetRecord>>,
    magnetic_target_field: MagneticTargetField,
    surface_program: SurfaceProgram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationPlanProvenance {
    Measured(SurfaceMeasurementTicket),
    #[cfg(test)]
    Synthetic,
}

impl PresentationPlan {
    /// Starts a complete ready-surface fact with explicit surface bounds.
    ///
    /// Sealing rejects bounds without positive logical area; omit ready facts to
    /// keep an unmeasured roster surface in [`SurfaceScene::Bootstrap`].
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(surface: SurfaceId, bounds: LogicalRect) -> Self {
        Self::with_provenance(
            surface,
            PresentationPlanProvenance::Synthetic,
            PopupPlaneRequirement::default(),
            bounds,
            None,
        )
    }

    pub(crate) fn from_measurements(
        ticket: SurfaceMeasurementTicket,
        popup: PopupPlaneRequirement,
        bounds: LogicalRect,
        popup_plane_bounds: Option<LogicalRect>,
    ) -> Self {
        Self::with_provenance(
            ticket.surface(),
            PresentationPlanProvenance::Measured(ticket),
            popup,
            bounds,
            popup_plane_bounds,
        )
    }

    /// Rebinds a test fixture to an exact measurement ticket without rebuilding
    /// any record arena or losing its already-finished surface program.
    #[cfg(test)]
    pub(crate) fn rebind_measurement_ticket_for_test(
        &self,
        ticket: SurfaceMeasurementTicket,
    ) -> Self {
        assert_eq!(
            ticket.surface(),
            self.surface,
            "a test plan may only rebind a ticket for its own surface"
        );
        let mut rebound = self.clone();
        rebound.provenance = PresentationPlanProvenance::Measured(ticket);
        rebound
    }

    fn with_provenance(
        surface: SurfaceId,
        provenance: PresentationPlanProvenance,
        popup: PopupPlaneRequirement,
        bounds: LogicalRect,
        popup_plane_bounds: Option<LogicalRect>,
    ) -> Self {
        Self {
            surface,
            provenance,
            popup,
            bounds,
            popup_plane_bounds,
            layout_facts: None,
            contained_projection: None,
            pane_records: Vec::new(),
            tab_records: Vec::new(),
            tab_bar_records: Vec::new(),
            tab_strip_control_records: Vec::new(),
            tab_list_menu_records: Vec::new(),
            tab_list_menu_backdrop_records: Vec::new(),
            presentation_menu_anchor_records: Vec::new(),
            splitter_gap_records: Vec::new(),
            splitter_records: Vec::new(),
            splitter_junction_records: Vec::new(),
            contained_records: Vec::new(),
            contained_minimums: Vec::new(),
            surface_background: None,
            drop_occlusions: Vec::new(),
            drop_guide_clusters: Vec::new(),
            drop_targets: Vec::new(),
            magnetic_target_field: MagneticTargetField::default(),
            surface_program: SurfaceProgram::default(),
        }
    }

    /// Returns the owning surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the authoritative surface-local dock layout bounds.
    ///
    /// Popup geometry is instead constrained by [`Self::popup_plane_bounds`].
    #[must_use]
    pub const fn bounds(&self) -> LogicalRect {
        self.bounds
    }

    /// Returns the authoritative surface-local popup plane when a workspace popup is active.
    #[must_use]
    pub const fn popup_plane_bounds(&self) -> Option<LogicalRect> {
        self.popup_plane_bounds
    }

    /// Returns the exact workspace-global popup requirement compiled here.
    #[must_use]
    pub const fn popup(&self) -> PopupPlaneRequirement {
        self.popup
    }

    pub(crate) fn install_layout_facts(
        &mut self,
        config: DockPresentationConfig,
        measurements: AuthoritativeSurfaceMeasurements<'_>,
    ) {
        debug_assert!(self.layout_facts.is_none());
        self.layout_facts = Some(PresentationLayoutFacts::new(config, measurements));
    }

    pub(crate) const fn contained_projection(&self) -> Option<ContainedLayoutProjection> {
        self.contained_projection
    }

    pub(crate) fn set_contained_projection(
        &mut self,
        projection: Option<ContainedLayoutProjection>,
    ) {
        self.contained_projection = projection;
    }

    pub(crate) fn push_root_layout_facts(&mut self, facts: RootLayoutFacts) {
        let inserted = self
            .layout_facts
            .as_mut()
            .is_some_and(|layout| layout.insert_root(facts));
        debug_assert!(inserted);
    }

    pub(crate) fn begin_surface_program_root(
        &mut self,
        root: RootId,
        presentation: SurfaceProgramRootPresentation,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.begin_root(root, presentation)
    }

    pub(crate) fn push_surface_program_root_operation(
        &mut self,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        let title_item = match operation {
            SurfaceProgramRootOperation::Pane(id) => self
                .pane_records
                .iter()
                .find(|record| record.id() == id)
                .and_then(PaneRecord::selected),
            _ => None,
        };
        self.surface_program
            .push_root_operation(operation, title_item)
    }

    pub(crate) fn push_surface_program_derived_root_operation(
        &mut self,
        root: RootId,
        operation: SurfaceProgramRootOperation,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program
            .push_derived_root_operation(root, operation)
    }

    pub(crate) fn end_surface_program_root(
        &mut self,
        root: RootId,
    ) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.end_root(root)
    }

    pub(crate) fn finish_surface_program(&mut self) -> Result<(), SurfaceProgramBuildError> {
        self.surface_program.finish()
    }

    pub(crate) const fn surface_program(&self) -> &SurfaceProgram {
        &self.surface_program
    }

    pub(crate) fn visit_surface_program<'plan, E>(
        &'plan self,
        mut visitor: impl FnMut(ResolvedSurfaceProgramStep<'plan>) -> Result<(), E>,
    ) -> Result<(), SurfaceProgramVisitError<E>> {
        let mut active_root = None;
        let mut contained_records = self.contained_records.iter();
        let mut roots_closed = false;
        let mut popup_menu_seen = false;
        let mut drag_preview_seen = false;
        let mut drop_guide_seen = false;
        let mut contained_preview_seen = false;
        let mut drag_decoration_seen = false;
        let mut interaction_feedback_seen = false;
        let mut surface_end_seen = false;

        for block in self.surface_program.blocks() {
            match block {
                SurfaceProgramBlock::RootBegin {
                    root,
                    presentation,
                    title_item,
                } => {
                    if active_root.is_some() || roots_closed {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    let contained = match presentation {
                        SurfaceProgramRootPresentation::Main => None,
                        SurfaceProgramRootPresentation::Contained(floating) => {
                            let Some(record) = contained_records.next().filter(|record| {
                                record.floating() == *floating && record.root() == *root
                            }) else {
                                return Err(SurfaceProgramVisitError::InvalidProgram);
                            };
                            Some(record.floating())
                        }
                    };
                    let root = ResolvedSurfaceProgramRoot {
                        root: *root,
                        contained,
                        title_item: *title_item,
                    };
                    active_root = Some(root);
                    visitor(ResolvedSurfaceProgramStep::RootBegin(root))
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::RootOperation(operation) => {
                    let Some(root) = active_root else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    let Some(operation) = self.resolve_surface_program_operation(*operation, root)
                    else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    visitor(ResolvedSurfaceProgramStep::Operation { root, operation })
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::RootEnd { root } => {
                    let Some(active) = active_root.take() else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    if active.root() != *root {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    visitor(ResolvedSurfaceProgramStep::RootEnd(active))
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::PopupBackdrop(session) => {
                    if active_root.is_some()
                        || !drag_decoration_seen
                        || popup_menu_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    roots_closed = true;
                    let Some(backdrop) = self
                        .tab_list_menu_backdrop_records
                        .iter()
                        .find(|record| record.session() == *session)
                    else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    visitor(ResolvedSurfaceProgramStep::PopupBackdrop(backdrop))
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::PopupMenu(session) => {
                    if active_root.is_some()
                        || !drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    roots_closed = true;
                    popup_menu_seen = true;
                    let Some(menu) = self
                        .tab_list_menu_records
                        .iter()
                        .find(|record| record.session() == *session)
                    else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    visitor(ResolvedSurfaceProgramStep::PopupMenu(menu))
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::DragPreview => {
                    if active_root.is_some()
                        || drag_preview_seen
                        || contained_preview_seen
                        || drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    roots_closed = true;
                    drag_preview_seen = true;
                    visitor(ResolvedSurfaceProgramStep::DragPreview)
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::DropTarget(id) => {
                    if active_root.is_some()
                        || !drag_preview_seen
                        || drop_guide_seen
                        || contained_preview_seen
                        || drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    let record = self
                        .surface_background
                        .as_ref()
                        .filter(|record| record.id() == *id)
                        .or_else(|| {
                            self.drop_targets
                                .iter()
                                .find(|record| record.id() == *id)
                                .map(AsRef::as_ref)
                        })
                        .ok_or(SurfaceProgramVisitError::InvalidProgram)?;
                    visitor(ResolvedSurfaceProgramStep::DropTarget(record))
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::DropGuide(id) => {
                    if active_root.is_some()
                        || !drag_preview_seen
                        || contained_preview_seen
                        || drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    drop_guide_seen = true;
                    let Ok(index) = self
                        .drop_guide_clusters
                        .binary_search_by_key(id, DropGuideClusterRecord::id)
                    else {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    };
                    visitor(ResolvedSurfaceProgramStep::DropGuide(
                        &self.drop_guide_clusters[index],
                    ))
                    .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::ContainedTransformPreview => {
                    if active_root.is_some()
                        || !drag_preview_seen
                        || contained_preview_seen
                        || drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    contained_preview_seen = true;
                    visitor(ResolvedSurfaceProgramStep::ContainedTransformPreview)
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::DragDecoration => {
                    if active_root.is_some()
                        || !contained_preview_seen
                        || drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    drag_decoration_seen = true;
                    visitor(ResolvedSurfaceProgramStep::DragDecoration)
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::InteractionFeedback => {
                    if active_root.is_some()
                        || !drag_decoration_seen
                        || interaction_feedback_seen
                        || surface_end_seen
                    {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    interaction_feedback_seen = true;
                    visitor(ResolvedSurfaceProgramStep::InteractionFeedback)
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
                SurfaceProgramBlock::SurfaceEnd => {
                    if active_root.is_some() || !interaction_feedback_seen || surface_end_seen {
                        return Err(SurfaceProgramVisitError::InvalidProgram);
                    }
                    surface_end_seen = true;
                    visitor(ResolvedSurfaceProgramStep::SurfaceEnd)
                        .map_err(SurfaceProgramVisitError::Sink)?;
                }
            }
        }

        if active_root.is_some()
            || !drag_preview_seen
            || !contained_preview_seen
            || !drag_decoration_seen
            || !interaction_feedback_seen
            || !surface_end_seen
            || contained_records.next().is_some()
        {
            return Err(SurfaceProgramVisitError::InvalidProgram);
        }
        Ok(())
    }

    fn resolve_surface_program_operation<'plan>(
        &'plan self,
        operation: SurfaceProgramRootOperation,
        root: ResolvedSurfaceProgramRoot,
    ) -> Option<ResolvedSurfaceProgramOperation<'plan>> {
        match operation {
            SurfaceProgramRootOperation::Pane(id) if id.root == root.root() => self
                .pane_records
                .binary_search_by_key(&id, PaneRecord::id)
                .ok()
                .map(|index| ResolvedSurfaceProgramOperation::Pane(&self.pane_records[index])),
            SurfaceProgramRootOperation::TabBar(id) if id.root == root.root() => self
                .tab_bar_records
                .binary_search_by(|record| record.id().cmp(&id))
                .ok()
                .map(|index| ResolvedSurfaceProgramOperation::TabBar(&self.tab_bar_records[index])),
            SurfaceProgramRootOperation::Tab(id) if id.root == root.root() => self
                .tab_records
                .binary_search_by(|record| record.id().cmp(&id))
                .ok()
                .map(|index| ResolvedSurfaceProgramOperation::Tab(&self.tab_records[index])),
            SurfaceProgramRootOperation::Splitter(id) if id.root == root.root() => self
                .splitter_records
                .binary_search_by(|record| record.id().cmp(&id))
                .ok()
                .map(|index| {
                    ResolvedSurfaceProgramOperation::Splitter(&self.splitter_records[index])
                }),
            SurfaceProgramRootOperation::SplitterJunction(id) if id.root() == root.root() => self
                .splitter_junction_records
                .binary_search_by_key(&id, SplitterJunctionRecord::id)
                .ok()
                .map(|index| {
                    ResolvedSurfaceProgramOperation::SplitterJunction(
                        &self.splitter_junction_records[index],
                    )
                }),
            SurfaceProgramRootOperation::TabStripControl(id) if id.bar().root == root.root() => {
                self.tab_strip_control_records
                    .binary_search_by_key(&id, |record| record.id())
                    .ok()
                    .map(|index| {
                        ResolvedSurfaceProgramOperation::TabStripControl(
                            &self.tab_strip_control_records[index],
                        )
                    })
            }
            SurfaceProgramRootOperation::ContainedControls(floating)
                if root.contained().is_some_and(|actual| actual == floating) =>
            {
                self.contained_records
                    .iter()
                    .find(|record| record.floating() == floating)
                    .map(ResolvedSurfaceProgramOperation::ContainedControls)
            }
            SurfaceProgramRootOperation::PresentationMenuAnchor(expected)
                if expected == root.root() =>
            {
                self.presentation_menu_anchor_records
                    .binary_search_by_key(&expected, |record| record.root())
                    .ok()
                    .map(|index| {
                        ResolvedSurfaceProgramOperation::PresentationMenuAnchor(
                            &self.presentation_menu_anchor_records[index],
                        )
                    })
            }
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_operation_for_test(
        &mut self,
        operation: SurfaceProgramRootOperation,
    ) -> bool {
        let before = self.surface_program.blocks.len();
        self.surface_program.blocks.retain(|block| {
            !matches!(block, SurfaceProgramBlock::RootOperation(current) if *current == operation)
        });
        self.surface_program.blocks.len() != before
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_popup_menu_for_test(
        &mut self,
        session: TabListMenuSessionId,
    ) -> bool {
        let before = self.surface_program.blocks.len();
        self.surface_program.blocks.retain(|block| {
            !matches!(block, SurfaceProgramBlock::PopupMenu(current) if *current == session)
        });
        self.surface_program.blocks.len() != before
    }

    #[cfg(test)]
    pub(crate) fn remove_surface_program_drop_target_for_test(
        &mut self,
        target: DropTargetId,
    ) -> bool {
        let before = self.surface_program.blocks.len();
        self.surface_program.blocks.retain(|block| {
            !matches!(block, SurfaceProgramBlock::DropTarget(current) if *current == target)
        });
        self.surface_program.blocks.len() != before
    }

    #[cfg(test)]
    pub(crate) fn clear_magnetic_target_field_for_test(&mut self) {
        self.magnetic_target_field = MagneticTargetField::default();
    }

    #[cfg(test)]
    pub(crate) fn replace_drop_target_for_test(
        &mut self,
        replacement: Arc<DropTargetRecord>,
    ) -> bool {
        let id = replacement.id();
        let Some(target) = self
            .drop_targets
            .iter_mut()
            .find(|target| target.id() == id)
        else {
            return false;
        };
        *target = Arc::clone(&replacement);
        for cluster in &mut self.drop_guide_clusters {
            cluster.for_each_target_mut(|guide| {
                if guide.id() == id {
                    guide.rebind_target(Arc::clone(&replacement));
                }
            });
        }
        true
    }

    pub(crate) const fn layout_facts(&self) -> Option<&PresentationLayoutFacts> {
        self.layout_facts.as_ref()
    }

    /// Returns the exact manifest ticket which authorized this compiled plan.
    ///
    /// Production plans always return `Some`. `None` exists only for crate-local
    /// malformed-scene tests and cannot be constructed through the public API.
    #[must_use]
    pub const fn measurement_ticket(&self) -> Option<SurfaceMeasurementTicket> {
        match self.provenance {
            PresentationPlanProvenance::Measured(ticket) => Some(ticket),
            #[cfg(test)]
            PresentationPlanProvenance::Synthetic => None,
        }
    }

    pub(crate) fn push_tab_record(&mut self, record: TabRecord) {
        self.tab_records.push(record);
    }

    pub(crate) fn push_pane_record(&mut self, record: PaneRecord) {
        self.pane_records.push(record);
    }

    pub(crate) fn push_tab_bar_record(&mut self, record: TabBarRecord) {
        self.tab_bar_records.push(record);
    }

    pub(crate) fn push_tab_strip_control_record(&mut self, record: TabStripControlRecord) {
        self.tab_strip_control_records.push(record);
    }

    pub(crate) fn push_tab_list_menu_record(&mut self, record: TabListMenuRecord) {
        self.surface_program.defer_popup_menu(record.session());
        self.tab_list_menu_records.push(record);
    }

    pub(crate) fn push_tab_list_menu_backdrop_record(&mut self, record: TabListMenuBackdropRecord) {
        self.surface_program.defer_popup_backdrop(record.session());
        self.tab_list_menu_backdrop_records.push(record);
    }

    pub(crate) fn push_presentation_menu_anchor_record(
        &mut self,
        record: PresentationMenuAnchorRecord,
    ) {
        self.presentation_menu_anchor_records.push(record);
    }

    pub(crate) fn push_splitter_gap_record(&mut self, record: SplitterGapRecord) {
        self.splitter_gap_records.push(record);
    }

    pub(crate) fn push_splitter_record(&mut self, record: SplitterRecord) {
        self.splitter_records.push(record);
    }

    pub(crate) fn derive_splitter_operability(&mut self) {
        let occlusions = &self.drop_occlusions;
        for record in &mut self.splitter_records {
            record.retain_operability(region_has_authoritative_area(
                record.hit().rect(),
                record.layer(),
                occlusions,
            ));
        }
    }

    /// Returns whether any authoritative area of `region` remains exposed on `layer`.
    ///
    /// UI adapters use this derived capability to disable framework actions for
    /// controls fully covered by a structurally higher contained presentation.
    #[must_use]
    pub fn region_is_operable(&self, region: LogicalRect, layer: SceneLayerKey) -> bool {
        region_has_authoritative_area(region, layer, &self.drop_occlusions)
    }

    pub(crate) fn point_is_on_authoritative_layer(
        &self,
        point: crate::geometry::LogicalPoint,
        layer: SceneLayerKey,
    ) -> bool {
        self.bounds.contains(point)
            && self
                .drop_occlusions
                .iter()
                .filter(|occlusion| occlusion.region().contains(point))
                .map(|occlusion| occlusion.layer())
                .max()
                .unwrap_or_else(SceneLayerKey::surface_base)
                == layer
    }

    pub(crate) fn push_splitter_junction_record(&mut self, record: SplitterJunctionRecord) {
        self.splitter_junction_records.push(record);
    }

    pub(crate) fn push_contained_record(&mut self, record: ContainedRecord) {
        self.contained_records.push(record);
    }

    /// Adds one contained minimum-size measurement before scene submission.
    pub(crate) fn push_contained_minimum(&mut self, measurement: ContainedMinimumMeasurement) {
        self.contained_minimums.push(measurement);
    }

    /// Installs the sole explicit rootless-surface background authority.
    ///
    /// # Errors
    ///
    /// Returns [`SceneBuildError`] when the record is not a surface background
    /// or this ready surface already contains one.
    pub(crate) fn set_surface_background(
        &mut self,
        background: DropTargetRecord,
    ) -> Result<(), SceneBuildError> {
        if !matches!(
            background.destination(),
            DropDestination::SurfaceBackground(_)
        ) {
            return Err(SceneBuildError::InvalidSurfaceBackgroundRecord {
                surface: self.surface,
                target: background.id(),
            });
        }
        if self.surface_background.is_some() {
            return Err(SceneBuildError::DuplicateSurfaceBackground {
                surface: self.surface,
            });
        }
        self.surface_program
            .defer_surface_background(background.id());
        self.surface_background = Some(background);
        Ok(())
    }

    /// Adds one exact, unclipped contained-floating occlusion before scene submission.
    pub(crate) fn push_drop_occlusion(&mut self, occlusion: DropOcclusionRecord) {
        self.drop_occlusions.push(occlusion);
    }

    /// Adds one complete docking-guide cluster before scene submission.
    pub(crate) fn push_drop_guide_cluster(&mut self, cluster: DropGuideClusterRecord) {
        self.surface_program.defer_guide(&cluster);
        self.drop_guide_clusters.push(cluster);
    }

    /// Adds one structural drop target before this fact is submitted to a scene.
    pub(crate) fn push_drop_target(&mut self, target: Arc<DropTargetRecord>) {
        self.surface_program.defer_drop_target(target.id());
        self.drop_targets.push(target);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_magnetic_pane_field(
        &mut self,
        root: RootId,
        tabs: NodeId,
        layer: SceneLayerKey,
        depth: usize,
        content: LogicalRect,
        center_target: DropTargetId,
        edges: MagneticEdgeTargets,
        config: &DockPresentationConfig,
    ) -> Result<(), GeometryError> {
        self.magnetic_target_field.push_pane(
            root,
            tabs,
            layer,
            depth,
            content,
            center_target,
            edges,
            config,
        )
    }

    pub(crate) fn push_magnetic_root_field(
        &mut self,
        root: RootId,
        layer: SceneLayerKey,
        bounds: LogicalRect,
        edges: MagneticEdgeTargets,
        config: &DockPresentationConfig,
    ) {
        self.magnetic_target_field
            .push_root(root, layer, bounds, edges, config);
    }

    /// Returns core-compiled tabs-leaf and pane-content records.
    #[must_use]
    pub fn pane_records(&self) -> &[PaneRecord] {
        &self.pane_records
    }

    /// Returns core-compiled records for every visible operable tab.
    #[must_use]
    pub fn tab_records(&self) -> &[TabRecord] {
        &self.tab_records
    }

    /// Returns core-compiled records for every tab bar.
    #[must_use]
    pub fn tab_bar_records(&self) -> &[TabBarRecord] {
        &self.tab_bar_records
    }

    /// Resolves the sole authoritative tab strip which owns wheel input at `point`.
    ///
    /// Popup coverage and contained-floating occlusion are core scene facts. An
    /// adapter must not repeat those layer decisions from local widget order.
    #[must_use]
    pub fn tab_scroll_owner_at(
        &self,
        point: crate::geometry::LogicalPoint,
    ) -> Option<TabBarSceneId> {
        if self
            .tab_list_menu_backdrop_records
            .iter()
            .any(|backdrop| backdrop.bounds().contains(point))
        {
            return None;
        }

        let mut owner = None;
        for bar in &self.tab_bar_records {
            if !bar.viewport().contains(point)
                || !self.point_is_on_authoritative_layer(point, bar.layer())
            {
                continue;
            }
            match owner {
                None => owner = Some((*bar.id(), bar.layer())),
                Some((_, layer)) if layer < bar.layer() => {
                    owner = Some((*bar.id(), bar.layer()));
                }
                Some((_, layer)) if layer == bar.layer() => return None,
                Some(_) => {}
            }
        }
        owner.map(|(bar, _)| bar)
    }

    /// Returns core-compiled tab-strip controls in canonical identity order.
    #[must_use]
    pub fn tab_strip_control_records(&self) -> &[TabStripControlRecord] {
        &self.tab_strip_control_records
    }

    /// Returns the core-compiled open workspace tab-list menu, when present.
    #[must_use]
    pub fn tab_list_menu_records(&self) -> &[TabListMenuRecord] {
        &self.tab_list_menu_records
    }

    /// Returns full-surface popup receiver coverage for this plan.
    #[must_use]
    pub fn tab_list_menu_backdrop_records(&self) -> &[TabListMenuBackdropRecord] {
        &self.tab_list_menu_backdrop_records
    }

    /// Returns one core-selected presentation command menu anchor per visible root.
    #[must_use]
    pub fn presentation_menu_anchor_records(&self) -> &[PresentationMenuAnchorRecord] {
        &self.presentation_menu_anchor_records
    }

    /// Returns one exact availability record for every structural splitter gap.
    #[must_use]
    pub fn splitter_gap_records(&self) -> &[SplitterGapRecord] {
        &self.splitter_gap_records
    }

    /// Returns core-compiled draw and hit records for every splitter.
    #[must_use]
    pub fn splitter_records(&self) -> &[SplitterRecord] {
        &self.splitter_records
    }

    /// Returns one exact splitter record by structural identity.
    #[must_use]
    pub fn splitter_record(&self, id: SplitterSceneId) -> Option<&SplitterRecord> {
        self.splitter_records
            .iter()
            .find(|record| *record.id() == id)
    }

    /// Returns core-compiled structural splitter junctions.
    ///
    /// A record remains present when only one axis is operable so semantic
    /// adapters can expose that axis without inventing junction topology.
    /// [`SplitterJunctionRecord::operable`] separately reports whether the
    /// complete junction accepts one atomic pointer resize.
    #[must_use]
    pub fn splitter_junction_records(&self) -> &[SplitterJunctionRecord] {
        &self.splitter_junction_records
    }

    /// Returns core-compiled contained-floating chrome in structural roster order.
    #[must_use]
    pub fn contained_records(&self) -> &[ContainedRecord] {
        &self.contained_records
    }

    /// Returns one exact core-compiled contained chrome record by stable identity.
    #[must_use]
    pub fn contained_record(&self, floating: FloatingPresentationId) -> Option<&ContainedRecord> {
        self.contained_records
            .iter()
            .find(|record| record.floating() == floating)
    }

    pub(crate) fn unclipped_contained_bounds(
        &self,
        floating: FloatingPresentationId,
    ) -> Option<LogicalRect> {
        self.drop_occlusions
            .iter()
            .find(|record| record.floating() == floating)
            .map(|record| record.region().rect())
    }

    /// Returns exact contained measurements in stable identity order after sealing.
    #[must_use]
    pub fn contained_minimums(&self) -> &[ContainedMinimumMeasurement] {
        &self.contained_minimums
    }

    /// Returns the sole explicit background authority of a rootless ready surface.
    #[must_use]
    pub const fn surface_background(&self) -> Option<&DropTargetRecord> {
        self.surface_background.as_ref()
    }

    /// Returns contained-floating occlusions in stable identity order after sealing.
    #[must_use]
    pub fn drop_occlusions(&self) -> &[DropOcclusionRecord] {
        &self.drop_occlusions
    }

    /// Returns docking-guide clusters in canonical structural order after sealing.
    #[must_use]
    pub fn drop_guide_clusters(&self) -> &[DropGuideClusterRecord] {
        &self.drop_guide_clusters
    }

    /// Returns drop targets in canonical structural-identity order after sealing.
    #[must_use]
    pub fn drop_targets(&self) -> &[Arc<DropTargetRecord>] {
        &self.drop_targets
    }

    /// Returns one exact drop target regardless of its paint ownership.
    ///
    /// Surface backgrounds, tab gaps, and canonical center or edge targets share
    /// one validated identity namespace. Guide records reference these targets;
    /// they never own availability, destination, or command semantics.
    #[must_use]
    pub(crate) fn drop_target(&self, id: DropTargetId) -> Option<&DropTargetRecord> {
        self.surface_background
            .as_ref()
            .filter(|record| record.id() == id)
            .or_else(|| {
                self.drop_targets
                    .binary_search_by_key(&id, |record| record.id())
                    .ok()
                    .map(|index| self.drop_targets[index].as_ref())
            })
    }

    pub(crate) fn magnetic_target_at(
        &self,
        point: crate::geometry::LogicalPoint,
        layer: SceneLayerKey,
        permits: impl FnMut(DropTargetId) -> bool,
    ) -> Option<MagneticTargetCandidate> {
        self.magnetic_target_field
            .candidate_at(point, layer, permits)
    }
}

/// Deterministic rejection while constructing, sealing, or reconciling a scene.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneBuildError {
    /// The ordered surface program no longer resolves against its canonical records.
    #[error("surface {surface} contains an invalid ordered surface program")]
    InvalidSurfaceProgram {
        /// Surface whose program and canonical records disagree.
        surface: SurfaceId,
    },
    /// The core-owned magnetic target field disagreed with the sealed scene topology.
    #[error("surface {surface} contains an invalid magnetic target field")]
    InvalidMagneticTargetField {
        /// Surface whose acquisition field and canonical scene records disagree.
        surface: SurfaceId,
    },
    /// A surface plan omitted or contradicted its global popup-plane requirement.
    #[error("surface {surface} popup-plane record set does not match its exact requirement")]
    PopupPlaneRecordSetMismatch {
        /// Surface whose backdrop or owner menu inventory was malformed.
        surface: SurfaceId,
    },
    /// One surface's independent presentation authority revision cannot advance.
    #[error("surface {surface} scene revision is exhausted")]
    SurfaceSceneRevisionExhausted {
        /// Surface whose private revision tombstone reached its maximum.
        surface: SurfaceId,
    },
    /// A test-only synthetic plan reached the production contribution boundary.
    #[error("surface {surface} presentation has no core-issued measurement ticket")]
    SyntheticPresentationRejected {
        /// Surface named by the synthetic plan.
        surface: SurfaceId,
    },
    /// The engine already published one scene in the current reduction boundary.
    #[error("a scene was already published in this reduction boundary")]
    AlreadyPublishedInBoundary,
    /// The active roster repeated a surface identity.
    #[error("active scene roster contains duplicate surface {surface}")]
    DuplicateRosterSurface {
        /// Repeated surface.
        surface: SurfaceId,
    },
    /// The frozen scene roster named a surface absent from the source workspace.
    #[error("scene roster contains unexpected workspace surface {surface}")]
    UnexpectedSceneSurface {
        /// Lowest unexpected surface identity in stable roster order.
        surface: SurfaceId,
    },
    /// The frozen scene roster omitted a surface present in the source workspace.
    #[error("scene roster omitted workspace surface {surface}")]
    MissingSceneSurface {
        /// Lowest omitted surface identity in stable workspace order.
        surface: SurfaceId,
    },
    /// A manifest-backed validator was paired with a different workspace revision.
    #[error("workspace index version {actual:?} does not match expected version {expected:?}")]
    WorkspaceIndexVersionMismatch {
        /// Workspace revision required by the current host frame.
        expected: WorkspaceVersion,
        /// Workspace revision captured by the manifest index.
        actual: WorkspaceVersion,
    },
    /// A surface-bound validator received a plan for a different surface.
    #[error("presentation validator for surface {expected} received surface {actual}")]
    PresentationSurfaceMismatch {
        /// Surface whose manifest-backed index was frozen.
        expected: SurfaceId,
        /// Surface carried by the submitted plan.
        actual: SurfaceId,
    },
    /// Ready facts named a surface outside the frozen roster.
    #[error("ready facts name surface {surface} outside the active scene roster")]
    SurfaceOutsideRoster {
        /// Unknown surface.
        surface: SurfaceId,
    },
    /// A Bootstrap entry has no Ready plan that could become a stale paint fallback.
    #[error("bootstrap surface {surface} cannot become stale")]
    BootstrapCannotBecomeStale {
        /// Surface without a paintable fallback.
        surface: SurfaceId,
    },
    /// A Ready entry had only an unpainted next projection and no stale fallback.
    #[error("ready surface {surface} has no painted fallback")]
    ReadySurfaceHasNoPaintFallback {
        /// Surface without a painted fallback slot.
        surface: SurfaceId,
    },
    /// Ready facts were submitted twice for one roster surface.
    #[error("ready facts for surface {surface} were submitted more than once")]
    DuplicateReadySurface {
        /// Repeated surface.
        surface: SurfaceId,
    },
    /// A compiled plan was authorized by a different manifest ticket.
    #[error("presentation for surface {surface} carries ticket {actual:?}, expected {expected:?}")]
    PresentationTicketMismatch {
        /// Surface whose compiled authority is stale or foreign.
        surface: SurfaceId,
        /// Current exact ticket, or `None` when the source workspace already disagreed.
        expected: Option<SurfaceMeasurementTicket>,
        /// Ticket carried by the submitted plan.
        actual: SurfaceMeasurementTicket,
    },
    /// Ready facts attempted to authorize a surface without positive logical area.
    #[error("ready surface {surface} has empty bounds")]
    EmptyReadySurfaceBounds {
        /// Surface whose ready bounds have no area.
        surface: SurfaceId,
    },
    /// A registered ready surface had no coordinate proof captured with its scene facts.
    #[error("ready surface {surface} has no capture-time coordinate proof")]
    SceneCoordinateProofUnavailable {
        /// Registered ready surface missing either captured or current coordinate facts.
        surface: SurfaceId,
    },
    /// A registered ready surface's capture-time coordinate proof is no longer current.
    #[error("ready surface {surface} has a stale capture-time coordinate proof")]
    StaleSceneCoordinateProof {
        /// Registered ready surface whose binding or coordinate generation changed.
        surface: SurfaceId,
    },
    /// A pane semantic identity was repeated.
    #[error("scene contains duplicate pane record {id:?}")]
    DuplicatePane {
        /// Repeated identity.
        id: PaneSceneId,
    },
    /// A tab-bar semantic identity was repeated.
    #[error("scene contains duplicate tab-bar semantic identity {id:?}")]
    DuplicateTabBar {
        /// Repeated identity.
        id: TabBarSceneId,
    },
    /// A tab semantic identity was repeated.
    #[error("scene contains duplicate tab semantic identity {id:?}")]
    DuplicateTab {
        /// Repeated identity.
        id: TabSceneId,
    },
    /// A structural splitter-gap identity was repeated.
    #[error("scene contains duplicate splitter-gap record {id:?}")]
    DuplicateSplitterGap {
        /// Repeated identity.
        id: SplitterSceneId,
    },
    /// A splitter semantic identity was repeated.
    #[error("scene contains duplicate splitter semantic identity {id:?}")]
    DuplicateSplitter {
        /// Repeated identity.
        id: SplitterSceneId,
    },
    /// A directional splitter-junction identity was repeated.
    #[error("scene contains duplicate splitter-junction record {id:?}")]
    DuplicateSplitterJunction {
        /// Repeated identity.
        id: SplitterJunctionId,
    },
    /// A contained presentation record was repeated.
    #[error("scene contains duplicate contained record for {floating}")]
    DuplicateContainedRecord {
        /// Repeated contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// A root published more than one presentation command menu anchor.
    #[error("scene contains duplicate presentation menu anchors for root {root}")]
    DuplicatePresentationMenuAnchor {
        /// Root with repeated anchor records.
        root: RootId,
    },
    /// A contained minimum measurement identity was repeated.
    #[error("scene contains duplicate minimum measurement for contained floating {floating}")]
    DuplicateContainedMinimumMeasurement {
        /// Repeated contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// A contained-floating occlusion identity was repeated.
    #[error("scene contains duplicate drop occlusion for contained floating {floating}")]
    DuplicateDropOcclusion {
        /// Repeated contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// A structural drop-target identity was repeated.
    #[error("scene contains duplicate drop target {id:?}")]
    DuplicateDropTarget {
        /// Repeated identity.
        id: DropTargetId,
    },
    /// A ready surface attempted to install more than one background authority.
    #[error("ready surface {surface} contains more than one surface background")]
    DuplicateSurfaceBackground {
        /// Surface with duplicate background publication.
        surface: SurfaceId,
    },
    /// A rootless non-empty surface omitted its explicit background authority.
    #[error("rootless ready surface {surface} omitted its surface background")]
    MissingSurfaceBackground {
        /// Rootless surface missing the record.
        surface: SurfaceId,
    },
    /// A rooted or unknown surface published a rootless background authority.
    #[error("surface {surface} cannot publish background target {target:?}")]
    UnexpectedSurfaceBackground {
        /// Surface which cannot own a background target.
        surface: SurfaceId,
        /// Unexpected target identity.
        target: DropTargetId,
    },
    /// The background slot received a non-background or differently-owned record.
    #[error("surface {surface} received invalid background record {target:?}")]
    InvalidSurfaceBackgroundRecord {
        /// Surface receiving the invalid record.
        surface: SurfaceId,
        /// Invalid record identity.
        target: DropTargetId,
    },
    /// A background record was also submitted through ordinary topology targets.
    #[error("surface {surface} submitted a background through topology targets")]
    SurfaceBackgroundInTopologyTargets {
        /// Surface containing the misplaced record.
        surface: SurfaceId,
    },
    /// The background was not below every roster-derived contained layer.
    #[error("surface {surface} background uses invalid layer {actual:?}")]
    SurfaceBackgroundLayerMismatch {
        /// Rootless surface.
        surface: SurfaceId,
        /// Invalid background layer.
        actual: SceneLayerKey,
    },
    /// The background hit region was empty or escaped its surface bounds.
    #[error("surface {surface} background has an invalid hit region")]
    InvalidSurfaceBackgroundHitRegion {
        /// Rootless surface.
        surface: SurfaceId,
    },
    /// A contained roster could not be represented by the scene layer key.
    #[error("contained presentation {floating} on surface {surface} exceeds scene layer capacity")]
    ContainedLayerCapacityExceeded {
        /// Owning surface.
        surface: SurfaceId,
        /// Contained identity which could not receive a layer.
        floating: FloatingPresentationId,
    },
    /// A structural docking-guide cluster identity was repeated.
    #[error("scene contains duplicate docking-guide cluster {id:?}")]
    DuplicateDropGuideCluster {
        /// Repeated identity.
        id: DropGuideClusterId,
    },
    /// A guide cluster named an invalid inner tabs or outer split-root scope.
    #[error("docking-guide cluster {cluster:?} is invalid on surface {surface}")]
    InvalidDropGuideCluster {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid cluster identity.
        cluster: DropGuideClusterId,
    },
    /// A guide slot contained a target with the wrong structural identity.
    #[error(
        "docking-guide slot {slot:?} in cluster {cluster:?} contains mismatched target {target:?}"
    )]
    DropGuideTargetSlotMismatch {
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Slot whose target identity was invalid.
        slot: DropGuideSlot,
        /// Invalid target identity.
        target: DropTargetId,
    },
    /// A guide target was assigned to a different layer than its cluster.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} uses layer {target_layer:?} instead of {cluster_layer:?}"
    )]
    DropGuideTargetLayerMismatch {
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with the mismatched layer.
        target: DropTargetId,
        /// Authoritative cluster layer.
        cluster_layer: SceneLayerKey,
        /// Mismatched target layer.
        target_layer: SceneLayerKey,
    },
    /// A guide cluster layer disagreed with its root's roster-derived layer.
    #[error(
        "docking-guide cluster {cluster:?} on surface {surface} uses layer {actual:?} instead of {expected:?}"
    )]
    DropGuideClusterLayerMismatch {
        /// Owning surface.
        surface: SurfaceId,
        /// Cluster with the mismatched layer.
        cluster: DropGuideClusterId,
        /// Roster-derived layer.
        expected: SceneLayerKey,
        /// Adapter-supplied layer.
        actual: SceneLayerKey,
    },
    /// A guide cluster activation region has no hittable logical area.
    #[error(
        "docking-guide cluster {cluster:?} on surface {surface} has an empty activation region"
    )]
    EmptyDropGuideActivation {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Cluster with empty activation geometry.
        cluster: DropGuideClusterId,
    },
    /// A guide cluster activation region extends outside its owning surface.
    #[error("docking-guide cluster {cluster:?} activation lies outside surface {surface} bounds")]
    DropGuideActivationOutsideSurface {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Cluster with out-of-bounds activation geometry.
        cluster: DropGuideClusterId,
    },
    /// A guide target hit region has no hittable logical area.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} on surface {surface} has an empty hit region"
    )]
    EmptyDropGuideHitRegion {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with empty hit geometry.
        target: DropTargetId,
    },
    /// A guide target hit region extends outside its owning surface.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} lies outside surface {surface} bounds"
    )]
    DropGuideHitRegionOutsideSurface {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with out-of-bounds hit geometry.
        target: DropTargetId,
    },
    /// A guide target hit region extends outside its cluster activation region.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} on surface {surface} hits outside cluster activation"
    )]
    DropGuideHitRegionOutsideActivation {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with hit geometry outside activation.
        target: DropTargetId,
    },
    /// A guide target draw rectangle has no paintable logical area.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} on surface {surface} has an empty draw rectangle"
    )]
    EmptyDropGuideDraw {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with empty draw geometry.
        target: DropTargetId,
    },
    /// A guide target draw rectangle extends outside its exact hit region.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} on surface {surface} draws outside its hit region"
    )]
    DropGuideDrawOutsideHitRegion {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with invalid draw geometry.
        target: DropTargetId,
    },
    /// A guide target body preview extends outside its cluster activation region.
    #[error(
        "docking-guide target {target:?} in cluster {cluster:?} on surface {surface} previews outside cluster activation"
    )]
    DropGuidePreviewOutsideActivation {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Target with preview geometry outside activation.
        target: DropTargetId,
    },
    /// Two targets in one cluster have positively overlapping hit regions.
    #[error(
        "docking-guide slots {first:?} and {second:?} in cluster {cluster:?} on surface {surface} overlap"
    )]
    OverlappingDropGuideHitRegions {
        /// Owning logical surface.
        surface: SurfaceId,
        /// Owning guide cluster.
        cluster: DropGuideClusterId,
        /// Earlier canonical slot in the overlapping pair.
        first: DropGuideSlot,
        /// Later canonical slot in the overlapping pair.
        second: DropGuideSlot,
    },
    /// A target ID disagreed with its surface or exact topology target.
    #[error("drop target {target:?} on surface {surface} has mismatched structural semantics")]
    TargetSemanticMismatch {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Mismatched target identity.
        target: DropTargetId,
    },
    /// A target preview has no paintable logical area.
    #[error("drop target {target:?} on surface {surface} has an empty preview visual")]
    EmptyDropVisual {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Target with an empty visual.
        target: DropTargetId,
    },
    /// A target preview cannot be painted completely inside its owning surface.
    #[error("drop target {target:?} preview lies outside surface {surface} bounds")]
    DropVisualOutsideSurface {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Target with an out-of-bounds visual.
        target: DropTargetId,
    },
    /// A semantic rectangle layer disagreed with its root's presentation roster.
    #[error(
        "root {root} semantic on surface {surface} uses layer {actual:?} instead of {expected:?}"
    )]
    SemanticLayerMismatch {
        /// Owning surface.
        surface: SurfaceId,
        /// Root owning the semantic rectangle.
        root: RootId,
        /// Roster-derived layer.
        expected: SceneLayerKey,
        /// Adapter-supplied layer.
        actual: SceneLayerKey,
    },
    /// A topology target layer disagreed with its root's presentation roster.
    #[error(
        "drop target {target:?} on surface {surface} uses layer {actual:?} instead of {expected:?}"
    )]
    DropTargetLayerMismatch {
        /// Owning surface.
        surface: SurfaceId,
        /// Target with the mismatched layer.
        target: DropTargetId,
        /// Roster-derived layer.
        expected: SceneLayerKey,
        /// Adapter-supplied layer.
        actual: SceneLayerKey,
    },
    /// A contained occlusion layer disagreed with its roster position.
    #[error(
        "contained occlusion {floating} on surface {surface} uses layer {actual:?} instead of {expected:?}"
    )]
    DropOcclusionLayerMismatch {
        /// Owning surface.
        surface: SurfaceId,
        /// Contained presentation.
        floating: FloatingPresentationId,
        /// Roster-derived layer.
        expected: SceneLayerKey,
        /// Adapter-supplied layer.
        actual: SceneLayerKey,
    },
    /// The exact tabs-leaf pane roster did not match the compiled roots.
    #[error("pane record set is incomplete or unexpected on surface {surface}")]
    PaneRecordSetMismatch {
        /// Surface whose pane roster was invalid.
        surface: SurfaceId,
    },
    /// A pane record was structurally or geometrically invalid.
    #[error("pane record {id:?} is invalid on surface {surface}")]
    InvalidPaneRecord {
        /// Surface receiving the invalid record.
        surface: SurfaceId,
        /// Invalid pane identity.
        id: PaneSceneId,
    },
    /// The exact tab-bar roster did not match the visible pane roster.
    #[error("tab-bar record set is incomplete or unexpected on surface {surface}")]
    TabBarRecordSetMismatch {
        /// Surface whose tab-bar roster was invalid.
        surface: SurfaceId,
    },
    /// A tab bar record was structurally or geometrically invalid.
    #[error("tab-bar record {id:?} is invalid on surface {surface}")]
    InvalidTabBarRecord {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: TabBarSceneId,
    },
    /// Visible and hidden tab records did not partition their tab bars exactly.
    #[error("tab record set is incomplete or unexpected on surface {surface}")]
    TabRecordSetMismatch {
        /// Surface whose tab roster was invalid.
        surface: SurfaceId,
    },
    /// A visible tab record was structurally or geometrically invalid.
    #[error("tab record {id:?} is invalid on surface {surface}")]
    InvalidTabRecord {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: TabSceneId,
    },
    /// The splitter-gap inventory did not exactly match every compiled split gap.
    #[error("splitter-gap record set is incomplete or unexpected on surface {surface}")]
    SplitterGapRecordSetMismatch {
        /// Surface whose splitter-gap inventory was invalid.
        surface: SurfaceId,
    },
    /// Rendered splitter-gap records and draw/hit records did not match exactly.
    #[error("splitter record set is incomplete or unexpected on surface {surface}")]
    SplitterRecordSetMismatch {
        /// Surface whose splitter roster was invalid.
        surface: SurfaceId,
    },
    /// A splitter record did not name a valid, operable gap.
    #[error("splitter record {id:?} is invalid on surface {surface}")]
    InvalidSplitterRecord {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Invalid semantic identity.
        id: SplitterSceneId,
    },
    /// The splitter-junction roster did not exactly match touching frontiers.
    #[error("splitter-junction record set is incomplete or unexpected on surface {surface}")]
    SplitterJunctionRecordSetMismatch {
        /// Surface whose junction roster was invalid.
        surface: SurfaceId,
    },
    /// A splitter-junction record did not match the exact touching frontier.
    #[error("splitter-junction record {id:?} is invalid on surface {surface}")]
    InvalidSplitterJunctionRecord {
        /// Surface receiving the invalid record.
        surface: SurfaceId,
        /// Invalid junction identity.
        id: SplitterJunctionId,
    },
    /// Valid splitter records could not produce finite junction geometry.
    #[error("splitter-junction geometry is invalid on surface {surface}: {source}")]
    InvalidSplitterJunctionGeometry {
        /// Surface whose junction derivation failed.
        surface: SurfaceId,
        /// Invalid exact intersection geometry.
        source: GeometryError,
    },
    /// The visible contained record set did not match the structural surface roster.
    #[error("contained record set is incomplete or unexpected on surface {surface}")]
    ContainedRecordSetMismatch {
        /// Surface whose contained record roster was invalid.
        surface: SurfaceId,
    },
    /// A visible contained record was structurally or geometrically invalid.
    #[error("contained record {floating} is invalid on surface {surface}")]
    InvalidContainedRecord {
        /// Surface receiving the invalid record.
        surface: SurfaceId,
        /// Invalid contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// The root-scoped presentation menu anchor roster was incomplete or unexpected.
    #[error("presentation menu anchor record set is incomplete or unexpected on surface {surface}")]
    PresentationMenuAnchorRecordSetMismatch {
        /// Surface whose anchor roster was invalid.
        surface: SurfaceId,
    },
    /// A presentation command menu anchor was structurally or geometrically invalid.
    #[error("presentation menu anchor for root {root} is invalid on surface {surface}")]
    InvalidPresentationMenuAnchor {
        /// Surface receiving the invalid anchor.
        surface: SurfaceId,
        /// Root named by the invalid anchor.
        root: RootId,
    },
    /// A root selected for compilation was absent from the indexed presentation forest.
    #[error("compiled root {root} is unavailable on surface {surface}")]
    CompiledRootUnavailable {
        /// Surface compiling the root.
        surface: SurfaceId,
        /// Missing or differently-owned root.
        root: RootId,
    },
    /// A drop occlusion named a missing floating or one owned by another surface.
    #[error("drop occlusion for contained floating {floating} is invalid on surface {surface}")]
    InvalidDropOcclusion {
        /// Surface receiving the invalid fact.
        surface: SurfaceId,
        /// Missing or differently-owned contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// A ready surface omitted the blocking region for a rostered contained floating.
    #[error("ready surface {surface} omitted drop occlusion for contained floating {floating}")]
    MissingDropOcclusion {
        /// Surface missing the required occlusion.
        surface: SurfaceId,
        /// Rostered contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// A ready surface omitted the measurement for a rostered contained floating.
    #[error(
        "ready surface {surface} omitted minimum measurement for contained floating {floating}"
    )]
    MissingContainedMinimumMeasurement {
        /// Surface missing the measurement.
        surface: SurfaceId,
        /// Rostered contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// A ready surface measured a contained identity outside its exact workspace roster.
    #[error(
        "ready surface {surface} contains unexpected minimum measurement for contained floating {floating}"
    )]
    UnexpectedContainedMinimumMeasurement {
        /// Surface receiving the stale or differently-owned identity.
        surface: SurfaceId,
        /// Unexpected contained presentation identity.
        floating: FloatingPresentationId,
    },
    /// A contained-floating occlusion has no hittable logical area.
    #[error("drop occlusion for contained floating {floating} is empty on surface {surface}")]
    EmptyDropOcclusionRegion {
        /// Surface receiving the empty fact.
        surface: SurfaceId,
        /// Contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// A contained-floating occlusion disagreed with the core-owned outer rectangle.
    #[error(
        "drop occlusion for contained floating {floating} on surface {surface} uses {actual:?} instead of {expected:?}"
    )]
    DropOcclusionGeometryMismatch {
        /// Owning surface.
        surface: SurfaceId,
        /// Contained-floating identity.
        floating: FloatingPresentationId,
        /// Core-owned outer rectangle.
        expected: LogicalRect,
        /// Adapter-published occlusion rectangle.
        actual: LogicalRect,
    },
}

#[cfg(test)]
mod tests;
