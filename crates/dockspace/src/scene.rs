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
use crate::tab_strip::{PopupPlaneRequirement, TabStripControlId, TabStripStateKey};
#[cfg(test)]
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId};
use crate::transition::WorkspaceVersion;
use crate::viewport::{CoordinateGeneration, ViewportBinding};
use crate::viewport_registry::ViewportLifecycle;

pub use self::records::{
    ContainedMinimumMeasurement, ContainedRecord, ContainedResizeDirection, ContainedResizeRecord,
    PaneRecord, PaneSceneId, SplitterGapPresentation, SplitterGapRecord, SplitterJunctionDirection,
    SplitterJunctionId, SplitterJunctionRecord, SplitterRecord, SplitterResizeHitError,
    SplitterResizeTarget, SplitterSceneId, TabBarRecord, TabBarSceneId, TabGroupDragRecord,
    TabListMenuBackdropRecord, TabListMenuGeometryAvailability, TabListMenuRecord,
    TabListMenuRowRecord, TabRecord, TabSceneId, TabStripControlRecord, TabStripMemberRecord,
    TabStripMemberVisibility,
};
pub(crate) use self::records::{PresentationLayoutFacts, RootLayoutFacts};
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

/// Complete semantic facts compiled as one candidate surface projection.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationPlan {
    surface: SurfaceId,
    provenance: PresentationPlanProvenance,
    popup: PopupPlaneRequirement,
    bounds: LogicalRect,
    popup_plane_bounds: Option<LogicalRect>,
    layout_facts: Option<PresentationLayoutFacts>,
    pane_records: Vec<PaneRecord>,
    tab_records: Vec<TabRecord>,
    tab_bar_records: Vec<TabBarRecord>,
    tab_strip_control_records: Vec<TabStripControlRecord>,
    tab_list_menu_records: Vec<TabListMenuRecord>,
    tab_list_menu_backdrop_records: Vec<TabListMenuBackdropRecord>,
    splitter_gap_records: Vec<SplitterGapRecord>,
    splitter_records: Vec<SplitterRecord>,
    splitter_junction_records: Vec<SplitterJunctionRecord>,
    contained_records: Vec<ContainedRecord>,
    contained_minimums: Vec<ContainedMinimumMeasurement>,
    surface_background: Option<DropTargetRecord>,
    drop_occlusions: Vec<DropOcclusionRecord>,
    drop_guide_clusters: Vec<DropGuideClusterRecord>,
    drop_targets: Vec<DropTargetRecord>,
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
            pane_records: Vec::new(),
            tab_records: Vec::new(),
            tab_bar_records: Vec::new(),
            tab_strip_control_records: Vec::new(),
            tab_list_menu_records: Vec::new(),
            tab_list_menu_backdrop_records: Vec::new(),
            splitter_gap_records: Vec::new(),
            splitter_records: Vec::new(),
            splitter_junction_records: Vec::new(),
            contained_records: Vec::new(),
            contained_minimums: Vec::new(),
            surface_background: None,
            drop_occlusions: Vec::new(),
            drop_guide_clusters: Vec::new(),
            drop_targets: Vec::new(),
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

    pub(crate) fn push_root_layout_facts(&mut self, facts: RootLayoutFacts) {
        let inserted = self
            .layout_facts
            .as_mut()
            .is_some_and(|layout| layout.insert_root(facts));
        debug_assert!(inserted);
    }

    pub(crate) const fn layout_facts(&self) -> Option<&PresentationLayoutFacts> {
        self.layout_facts.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn clone_layout_facts_from(&mut self, source: &Self) {
        self.layout_facts.clone_from(&source.layout_facts);
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
        self.tab_list_menu_records.push(record);
    }

    pub(crate) fn push_tab_list_menu_backdrop_record(&mut self, record: TabListMenuBackdropRecord) {
        self.tab_list_menu_backdrop_records.push(record);
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
        self.surface_background = Some(background);
        Ok(())
    }

    /// Adds one exact, unclipped contained-floating occlusion before scene submission.
    pub(crate) fn push_drop_occlusion(&mut self, occlusion: DropOcclusionRecord) {
        self.drop_occlusions.push(occlusion);
    }

    /// Adds one complete docking-guide cluster before scene submission.
    pub(crate) fn push_drop_guide_cluster(&mut self, cluster: DropGuideClusterRecord) {
        self.drop_guide_clusters.push(cluster);
    }

    /// Adds one structural drop target before this fact is submitted to a scene.
    pub(crate) fn push_drop_target(&mut self, target: DropTargetRecord) {
        self.drop_targets.push(target);
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

    /// Returns core-compiled resize regions for structural splitter junctions.
    #[must_use]
    pub fn splitter_junction_records(&self) -> &[SplitterJunctionRecord] {
        &self.splitter_junction_records
    }

    /// Resolves the unique splitter target at a logical point.
    ///
    /// The frontmost contained occlusion first selects the sole presentation
    /// layer eligible for input. On that layer an exact junction intersection
    /// takes precedence over an ordinary handle. Ambiguous geometry is rejected
    /// rather than resolved by distance, insertion order, or stable identity.
    ///
    /// # Errors
    ///
    /// Returns [`SplitterResizeHitError`] when multiple targets of the winning
    /// class claim the same point on the authoritative layer.
    pub fn splitter_resize_target_at(
        &self,
        point: crate::geometry::LogicalPoint,
    ) -> Result<Option<SplitterResizeTarget>, SplitterResizeHitError> {
        if !self.bounds.contains(point) {
            return Ok(None);
        }
        let layer = self
            .drop_occlusions
            .iter()
            .filter(|occlusion| occlusion.region().contains(point))
            .map(|occlusion| occlusion.layer())
            .max()
            .unwrap_or_else(SceneLayerKey::surface_base);

        let mut junctions = self
            .splitter_junction_records
            .iter()
            .filter(|junction| junction.layer() == layer && junction.hit().contains(point));
        let junction = junctions.next().map(SplitterJunctionRecord::id);
        let extra_junctions = junctions.count();
        if extra_junctions > 0 {
            return Err(SplitterResizeHitError::AmbiguousJunctions {
                surface: self.surface,
                layer,
                count: extra_junctions + 1,
            });
        }
        if let Some(junction) = junction {
            return Ok(Some(SplitterResizeTarget::Junction(junction)));
        }

        let mut handles = self.splitter_records.iter().filter(|splitter| {
            splitter.operable() && splitter.layer() == layer && splitter.hit().contains(point)
        });
        let handle = handles.next().map(|splitter| *splitter.id());
        let extra_handles = handles.count();
        if extra_handles > 0 {
            return Err(SplitterResizeHitError::AmbiguousHandles {
                surface: self.surface,
                layer,
                count: extra_handles + 1,
            });
        }
        Ok(handle.map(SplitterResizeTarget::Handle))
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
    pub fn drop_targets(&self) -> &[DropTargetRecord] {
        &self.drop_targets
    }
}

/// Deterministic rejection while constructing, sealing, or reconciling a scene.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneBuildError {
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
