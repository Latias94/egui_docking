//! Type-state construction of immutable semantic docking scenes.

mod records;

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
use crate::presentation_hit::PresentationHitManifest;
use crate::presentation_observation::{
    HostPresentationEndpoint, PresentationAuthorityRejection, PresentationOutputSerial,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::scene_manifest::{
    MeasurementAuthorityError, SceneRequirementManifest, SurfaceMeasurementTicket,
    SurfaceSceneRevision,
};
use crate::splitter_junction_index::derive_splitter_junction_candidates;
use crate::tab_strip::{PopupPlaneRequirement, TabStripControlId, TabStripStateKey};
#[cfg(test)]
use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId};
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

    pub(crate) fn region_is_operable(&self, region: LogicalRect, layer: SceneLayerKey) -> bool {
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

/// Exact authority of one independently replaceable surface presentation.
///
/// The requirement ticket binds semantic topology, policy-facing presentation
/// configuration, and the logical surface. The revision binds the current
/// Ready/Stale/Bootstrap authority state. Deliberately omitting ordering keeps
/// callers from treating unrelated surface histories as one global timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceSceneStamp {
    requirement: SurfaceMeasurementTicket,
    revision: SurfaceSceneRevision,
}

impl SurfaceSceneStamp {
    pub(crate) const fn new(
        requirement: SurfaceMeasurementTicket,
        revision: SurfaceSceneRevision,
    ) -> Self {
        Self {
            requirement,
            revision,
        }
    }

    /// Returns the exact measurement requirement represented by this authority.
    #[must_use]
    pub const fn requirement(self) -> SurfaceMeasurementTicket {
        self.requirement
    }

    /// Returns this surface's engine-local authority revision.
    #[must_use]
    pub const fn revision(self) -> SurfaceSceneRevision {
        self.revision
    }

    /// Returns the sole surface whose presentation this stamp can authorize.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.requirement.surface()
    }
}

/// Why a paintable fallback currently lacks hit-test authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleSurfaceSceneReason {
    /// Core-derived semantic requirements changed.
    RequirementsChanged,
    /// Binding, lifecycle, coordinate generation, bounds, or scale changed.
    CoordinateAuthorityChanged,
    /// The surface has a stable native association which is not ready to authorize geometry.
    CoordinateAuthorityUnavailable,
    /// The exact adapter contribution explicitly lacked one required fact.
    MeasurementsUnavailable(MeasurementAuthorityError),
    /// Exact host geometry could not project the active popup without violating its plane.
    PopupGeometryUnavailable(PopupGeometryUnavailableReason),
    /// Core-owned transient presentation input changed after the last projection.
    TransientPresentationChanged,
}

/// Why a rostered surface has neither hit-test authority nor a paint fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapSurfaceSceneReason {
    /// The surface has not delivered its first exact contribution.
    AwaitingContribution,
    /// Core-derived semantic requirements changed before any ready plan existed.
    RequirementsChanged,
    /// A workspace epoch replacement invalidated and discarded every prior plan.
    WorkspaceReplaced,
    /// Binding, lifecycle, coordinate generation, bounds, or scale changed.
    CoordinateAuthorityChanged,
    /// The surface has a stable native association which is not ready to authorize geometry.
    CoordinateAuthorityUnavailable,
    /// The exact adapter contribution explicitly lacked one required fact.
    MeasurementsUnavailable(MeasurementAuthorityError),
    /// The exact adapter contribution reported non-positive surface area.
    EmptyBounds,
    /// Exact host geometry could not project the active popup without violating its plane.
    PopupGeometryUnavailable(PopupGeometryUnavailableReason),
    /// Core-owned transient presentation input changed before a paint fallback existed.
    TransientPresentationChanged,
}

/// Why exact host geometry could not project the active popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupGeometryUnavailableReason {
    /// The measured popup plane had no positive area.
    EmptyPlane,
    /// The exact owner anchor was outside the measured popup plane.
    AnchorOutsidePlane,
    /// Neither side of the owner anchor had positive popup space.
    NoSpace,
    /// Complete measured geometry could not produce the required owner popup record.
    ProjectionUnavailable,
}

/// Current surface projection state with separate paint and interaction authority.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySurfaceScene {
    stamp: SurfaceSceneStamp,
    candidate: Box<SurfacePlanScene>,
    paint_fallback: Option<ReadySurfacePaintFallback>,
    confirmed_paint_fallback: Option<SurfacePresentationOutputTicket>,
    interaction_authority: Option<PresentedSurfaceAuthority>,
}

#[derive(Debug, Clone, PartialEq)]
enum ReadySurfacePaintFallback {
    Candidate,
    Retained(Box<SurfacePlanScene>),
}

/// One exact compiled plan slot with its coordinate capture.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePlanScene {
    stamp: SurfaceSceneStamp,
    output_ticket: SurfacePresentationOutputTicket,
    plan: Arc<PresentationPlan>,
    hit_manifest: Arc<PresentationHitManifest>,
    coordinate_capture: SurfaceCoordinateCapture,
}

impl ReadySurfaceScene {
    /// Returns the current state and candidate stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the exact candidate an adapter may paint.
    #[must_use]
    pub const fn candidate(&self) -> &SurfacePlanScene {
        &self.candidate
    }

    /// Returns the output capability of the current candidate.
    #[must_use]
    pub const fn output_ticket(&self) -> SurfacePresentationOutputTicket {
        self.candidate.output_ticket()
    }

    /// Returns the candidate plan an adapter may paint.
    #[must_use]
    pub fn plan(&self) -> &PresentationPlan {
        self.candidate.plan()
    }

    pub(crate) const fn coordinate_capture(&self) -> SurfaceCoordinateCapture {
        self.candidate.coordinate_capture()
    }

    /// Returns the last plan proven painted, which may differ from the candidate.
    #[must_use]
    pub fn paint_fallback(&self) -> Option<&SurfacePlanScene> {
        match self.paint_fallback.as_ref()? {
            ReadySurfacePaintFallback::Candidate => Some(&self.candidate),
            ReadySurfacePaintFallback::Retained(fallback) => Some(fallback),
        }
    }

    fn interaction_authority(&self) -> Option<PresentedSurfaceAuthority> {
        self.interaction_authority
    }

    fn interaction_plan(&self) -> Option<&SurfacePlanScene> {
        let authority = self.interaction_authority?;
        if self.candidate.output_ticket() == authority.ticket() {
            return Some(&self.candidate);
        }
        self.paint_fallback()
            .filter(|fallback| fallback.output_ticket() == authority.ticket())
    }

    fn into_retained_authority(
        self,
    ) -> (
        Option<Box<SurfacePlanScene>>,
        Option<PresentedSurfaceAuthority>,
        Option<SurfacePresentationOutputTicket>,
    ) {
        let interaction_authority = self.interaction_authority;
        let confirmed_paint_fallback = self.confirmed_paint_fallback;
        let paint_fallback = match self.paint_fallback {
            Some(ReadySurfacePaintFallback::Candidate) => Some(self.candidate),
            Some(ReadySurfacePaintFallback::Retained(fallback)) => Some(fallback),
            // A ticketed candidate may have reached an adapter before a final
            // presentation observation arrives. Keep it as the single delayed
            // fallback when a replacement commits in the meantime.
            None => Some(self.candidate),
        };
        debug_assert!(interaction_authority.is_none_or(|authority| {
            paint_fallback
                .as_deref()
                .is_some_and(|fallback| fallback.output_ticket() == authority.ticket())
        }));
        let confirmed_paint_fallback = confirmed_paint_fallback.filter(|ticket| {
            paint_fallback
                .as_deref()
                .is_some_and(|fallback| fallback.output_ticket() == *ticket)
        });
        (
            paint_fallback,
            interaction_authority,
            confirmed_paint_fallback,
        )
    }

    fn has_confirmed_paint_fallback(&self) -> bool {
        self.confirmed_paint_fallback.is_some_and(|ticket| {
            self.paint_fallback()
                .is_some_and(|fallback| fallback.output_ticket() == ticket)
        })
    }

    /// Transfers only an observed paint fallback across an authority
    /// invalidation. A pending output may remain available for a delayed
    /// observation while a replacement candidate is compiled under the same
    /// requirements, but it cannot survive a requirement or coordinate change
    /// without reintroducing stale hit/paint authority.
    fn into_confirmed_paint_fallback(self) -> Option<Box<SurfacePlanScene>> {
        let confirmed = self.confirmed_paint_fallback?;
        if self.candidate.output_ticket() == confirmed {
            return Some(self.candidate);
        }
        match self.paint_fallback {
            Some(ReadySurfacePaintFallback::Retained(fallback))
                if fallback.output_ticket() == confirmed =>
            {
                Some(fallback)
            }
            Some(ReadySurfacePaintFallback::Candidate)
            | Some(ReadySurfacePaintFallback::Retained(_))
            | None => None,
        }
    }

    fn retained_output(
        &self,
        ticket: SurfacePresentationOutputTicket,
    ) -> Option<&SurfacePlanScene> {
        if self.candidate.output_ticket() == ticket {
            return Some(&self.candidate);
        }
        self.paint_fallback()
            .filter(|fallback| fallback.output_ticket() == ticket)
    }

    fn validate_presented_authority(
        &self,
        authority: PresentedSurfaceAuthority,
        popup: PopupPlaneRequirement,
    ) -> Result<(), PresentationAuthorityRejection> {
        let ticket = authority.ticket();
        let Some(output) = self.retained_output(ticket) else {
            return Err(PresentationAuthorityRejection::TicketNotRetained {
                candidate: self.candidate.output_ticket(),
                paint_fallback: self.paint_fallback().map(SurfacePlanScene::output_ticket),
            });
        };
        if output.plan().popup() != popup
            || !output
                .coordinate_capture()
                .matches_presented_authority(authority)
        {
            return Err(PresentationAuthorityRejection::OutputProofMismatch { ticket });
        }
        Ok(())
    }

    fn observe_presented_for_paint(
        &mut self,
        authority: PresentedSurfaceAuthority,
        popup: PopupPlaneRequirement,
    ) -> Result<(), PresentationAuthorityRejection> {
        self.validate_presented_authority(authority, popup)?;
        let ticket = authority.ticket();
        let retained_as_candidate = self.candidate.output_ticket() == ticket;
        if retained_as_candidate {
            self.paint_fallback = Some(ReadySurfacePaintFallback::Candidate);
        }
        self.confirmed_paint_fallback = Some(ticket);
        Ok(())
    }
}

impl SurfaceCoordinateCapture {
    fn matches_presented_authority(self, authority: PresentedSurfaceAuthority) -> bool {
        let (endpoint, generation) = match self {
            Self::Headless {
                authority_generation,
            } => (HostPresentationEndpoint::Headless, authority_generation),
            Self::NativeUnavailable {
                binding,
                authority_generation,
                ..
            } => (
                HostPresentationEndpoint::Native(binding),
                authority_generation,
            ),
            Self::NativeReady {
                coordinates,
                authority_generation,
            } => (
                HostPresentationEndpoint::Native(coordinates.binding()),
                authority_generation,
            ),
        };
        authority.endpoint() == endpoint && authority.coordinate_generation() == generation
    }
}

impl SurfacePlanScene {
    /// Returns the exact plan authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the opaque output capability minted with this exact scene.
    #[must_use]
    pub const fn output_ticket(&self) -> SurfacePresentationOutputTicket {
        self.output_ticket
    }

    /// Returns the complete core-compiled presentation plan.
    #[must_use]
    pub fn plan(&self) -> &PresentationPlan {
        &self.plan
    }

    /// Returns the canonical hit inventory bound to this exact output.
    #[must_use]
    pub fn hit_manifest(&self) -> &PresentationHitManifest {
        &self.hit_manifest
    }

    pub(crate) const fn coordinate_capture(&self) -> SurfaceCoordinateCapture {
        self.coordinate_capture
    }
}

/// A surface whose last ready plan may still paint but cannot authorize input.
#[derive(Debug, Clone, PartialEq)]
pub struct StaleSurfaceScene {
    stamp: SurfaceSceneStamp,
    paint_fallback: Box<SurfacePlanScene>,
    reason: StaleSurfaceSceneReason,
}

impl StaleSurfaceScene {
    /// Returns the current non-interactive authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the previous painted presentation retained only as a fallback.
    #[must_use]
    pub const fn paint_fallback(&self) -> &SurfacePlanScene {
        &self.paint_fallback
    }

    /// Returns why the current requirement has no hit-test authority.
    #[must_use]
    pub const fn reason(&self) -> StaleSurfaceSceneReason {
        self.reason
    }
}

/// A roster surface with no paintable ready presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapSurfaceScene {
    stamp: SurfaceSceneStamp,
    reason: BootstrapSurfaceSceneReason,
}

impl BootstrapSurfaceScene {
    /// Returns the unavailable roster surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.stamp.surface()
    }

    /// Returns the current non-interactive authority stamp.
    #[must_use]
    pub const fn stamp(self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns why no paint fallback or hit authority exists.
    #[must_use]
    pub const fn reason(self) -> BootstrapSurfaceSceneReason {
        self.reason
    }
}

/// Independent presentation authority state for one rostered surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceScene {
    /// A current candidate may paint; independent retained authority controls input.
    Ready(Box<ReadySurfaceScene>),
    /// A previous plan may paint, but current facts cannot authorize input.
    Stale(Box<StaleSurfaceScene>),
    /// No current or previous plan is available.
    Bootstrap(BootstrapSurfaceScene),
}

impl SurfaceScene {
    /// Returns the owning surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.stamp().surface()
    }

    /// Returns the exact current authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        match self {
            Self::Ready(scene) => scene.stamp(),
            Self::Stale(scene) => scene.stamp(),
            Self::Bootstrap(scene) => scene.stamp(),
        }
    }

    /// Returns the current authoritative presentation, if one exists.
    #[must_use]
    pub const fn ready(&self) -> Option<&ReadySurfaceScene> {
        match self {
            Self::Ready(scene) => Some(scene),
            Self::Stale(_) | Self::Bootstrap(_) => None,
        }
    }

    fn interaction_projection(
        &self,
        popup_gate_revision: PopupInteractionGateRevision,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        let Self::Ready(scene) = self else {
            return None;
        };
        let authority = scene.interaction_authority()?;
        let output = scene.interaction_plan()?;
        debug_assert!(authority.matches_output(output.output_ticket()));
        Some(SurfaceInteractionProjection {
            output,
            authority,
            popup_gate_revision,
        })
    }

    /// Returns a typed paint-only projection, if one exists.
    #[must_use]
    pub fn paint_projection(&self) -> Option<SurfacePaintProjection<'_>> {
        match self {
            Self::Ready(scene) => Some(SurfacePaintProjection {
                plan: scene.plan(),
                hit_manifest: scene.candidate().hit_manifest(),
                plan_stamp: scene.candidate().stamp(),
                output_ticket: scene.candidate().output_ticket(),
                coordinate_capture: scene.candidate().coordinate_capture(),
                current_stamp: scene.stamp(),
            }),
            Self::Stale(scene) => Some(SurfacePaintProjection {
                plan: scene.paint_fallback().plan(),
                hit_manifest: scene.paint_fallback().hit_manifest(),
                plan_stamp: scene.paint_fallback().stamp(),
                output_ticket: scene.paint_fallback().output_ticket(),
                coordinate_capture: scene.paint_fallback().coordinate_capture(),
                current_stamp: scene.stamp(),
            }),
            Self::Bootstrap(_) => None,
        }
    }

    /// Returns exact plan stamps retained by this scene for adapter-side caching.
    #[must_use]
    pub fn retained_plan_stamps(&self) -> RetainedSurfacePlanStamps {
        match self {
            Self::Ready(scene) => RetainedSurfacePlanStamps {
                candidate: Some(scene.candidate().stamp()),
                paint_fallback: scene.paint_fallback().map(SurfacePlanScene::stamp),
            },
            Self::Stale(scene) => RetainedSurfacePlanStamps {
                candidate: None,
                paint_fallback: Some(scene.paint_fallback().stamp()),
            },
            Self::Bootstrap(_) => RetainedSurfacePlanStamps {
                candidate: None,
                paint_fallback: None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupInteractionGatePhase {
    InactiveIndependent,
    Staging,
    ActivePresented,
}

/// Monotonic identity of one workspace-global popup interaction authority epoch.
///
/// Exhaustion permanently keeps the gate fail-closed rather than permitting an
/// ABA identity reuse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PopupInteractionGateRevision(u64);

impl PopupInteractionGateRevision {
    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Interaction authority coordinator for the workspace popup receiver plane.
///
/// With no active popup, each surface proves and loses authority independently.
/// An active popup spans the workspace, so opening it, replacing it, and
/// clearing it all require one exact-roster presentation barrier. Once the
/// inactive close successor crosses that barrier, surfaces return to independent
/// authority without weakening the exact per-output proof checks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PopupInteractionGate {
    requirement: PopupPlaneRequirement,
    roster: BTreeSet<SurfaceId>,
    proofs: BTreeMap<SurfaceId, PresentedSurfaceAuthority>,
    phase: PopupInteractionGatePhase,
    revision: Option<PopupInteractionGateRevision>,
}

impl PopupInteractionGate {
    fn new(manifest: &SceneRequirementManifest) -> Self {
        let requirement = manifest.popup();
        Self {
            requirement,
            roster: manifest.surfaces().map(|(surface, _)| surface).collect(),
            proofs: BTreeMap::new(),
            phase: match requirement {
                PopupPlaneRequirement::Inactive { .. } => {
                    PopupInteractionGatePhase::InactiveIndependent
                }
                PopupPlaneRequirement::Active { .. } => PopupInteractionGatePhase::Staging,
            },
            revision: Some(PopupInteractionGateRevision::default()),
        }
    }

    fn requires_roster_barrier(&self) -> bool {
        self.phase != PopupInteractionGatePhase::InactiveIndependent
    }

    fn is_active_presented(&self) -> bool {
        self.phase == PopupInteractionGatePhase::ActivePresented && self.revision.is_some()
    }

    fn interaction_revision(&self) -> Option<PopupInteractionGateRevision> {
        if self.phase == PopupInteractionGatePhase::Staging {
            return None;
        }
        self.revision
    }

    fn reconcile_identity(&mut self, manifest: &SceneRequirementManifest) -> bool {
        let previous_requirement = self.requirement;
        let roster = manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let requirement = manifest.popup();
        let changed = previous_requirement != requirement || self.roster != roster;
        let requires_fresh_barrier = changed
            && (matches!(previous_requirement, PopupPlaneRequirement::Active { .. })
                || matches!(requirement, PopupPlaneRequirement::Active { .. })
                || self.phase == PopupInteractionGatePhase::Staging);
        if changed {
            self.requirement = requirement;
            self.roster = roster;
        }
        requires_fresh_barrier
    }

    fn reset_barrier(&mut self) {
        self.revision = self
            .revision
            .and_then(PopupInteractionGateRevision::checked_next);
        self.proofs.clear();
        self.phase = PopupInteractionGatePhase::Staging;
    }
}

/// Exact output-bound projection eligible for pointer receiver validation.
///
/// Keeping these values in one view prevents adapters from pairing a current
/// candidate plan with authority retained for an older painted fallback.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceInteractionProjection<'a> {
    output: &'a SurfacePlanScene,
    authority: PresentedSurfaceAuthority,
    popup_gate_revision: PopupInteractionGateRevision,
}

impl<'a> SurfaceInteractionProjection<'a> {
    pub(crate) const fn from_frozen_parts(
        output: &'a SurfacePlanScene,
        authority: PresentedSurfaceAuthority,
        popup_gate_revision: PopupInteractionGateRevision,
    ) -> Self {
        Self {
            output,
            authority,
            popup_gate_revision,
        }
    }

    pub(crate) const fn output(self) -> &'a SurfacePlanScene {
        self.output
    }

    /// Returns the exact scene stamp compiled into this presented output.
    #[must_use]
    pub const fn plan_stamp(self) -> SurfaceSceneStamp {
        self.output.stamp()
    }

    /// Returns the exact semantic presentation output.
    #[must_use]
    pub const fn output_ticket(self) -> SurfacePresentationOutputTicket {
        self.output.output_ticket()
    }

    /// Returns the plan compiled for this exact output.
    #[must_use]
    pub fn plan(self) -> &'a PresentationPlan {
        self.output.plan()
    }

    /// Returns the hit manifest compiled for this exact output.
    #[must_use]
    pub fn hit_manifest(self) -> &'a PresentationHitManifest {
        self.output.hit_manifest()
    }

    /// Returns final-presentation authority for this exact output.
    #[must_use]
    pub const fn authority(self) -> PresentedSurfaceAuthority {
        self.authority
    }

    pub(crate) const fn popup_gate_revision(self) -> PopupInteractionGateRevision {
        self.popup_gate_revision
    }
}

/// Exact candidate and fallback identities retained for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedSurfacePlanStamps {
    candidate: Option<SurfaceSceneStamp>,
    paint_fallback: Option<SurfaceSceneStamp>,
}

impl RetainedSurfacePlanStamps {
    /// Returns the current paint candidate identity, when one exists.
    #[must_use]
    pub const fn candidate(self) -> Option<SurfaceSceneStamp> {
        self.candidate
    }

    /// Returns the retained painted fallback identity, when one exists.
    #[must_use]
    pub const fn paint_fallback(self) -> Option<SurfaceSceneStamp> {
        self.paint_fallback
    }

    /// Iterates unique retained identities in candidate-then-fallback order.
    pub fn unique(self) -> impl Iterator<Item = SurfaceSceneStamp> {
        let fallback = if self.paint_fallback == self.candidate {
            None
        } else {
            self.paint_fallback
        };
        [self.candidate, fallback].into_iter().flatten()
    }
}

/// Paint projection which keeps fallback identity separate from current authority.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePaintProjection<'a> {
    plan: &'a PresentationPlan,
    hit_manifest: &'a PresentationHitManifest,
    plan_stamp: SurfaceSceneStamp,
    output_ticket: SurfacePresentationOutputTicket,
    coordinate_capture: SurfaceCoordinateCapture,
    current_stamp: SurfaceSceneStamp,
}

impl<'a> SurfacePaintProjection<'a> {
    /// Returns the plan eligible only for painting.
    #[must_use]
    pub const fn plan(self) -> &'a PresentationPlan {
        self.plan
    }

    /// Returns the hit inventory compiled from the same exact paint output.
    #[must_use]
    pub const fn hit_manifest(self) -> &'a PresentationHitManifest {
        self.hit_manifest
    }

    /// Returns the exact authority under which this paint candidate was compiled.
    #[must_use]
    pub const fn plan_stamp(self) -> SurfaceSceneStamp {
        self.plan_stamp
    }

    /// Returns the opaque ticket for the exact output that may be painted.
    #[must_use]
    pub const fn output_ticket(self) -> SurfacePresentationOutputTicket {
        self.output_ticket
    }

    pub(crate) const fn coordinate_capture(self) -> SurfaceCoordinateCapture {
        self.coordinate_capture
    }

    /// Returns the current surface state authority, which may be non-interactive.
    #[must_use]
    pub const fn current_stamp(self) -> SurfaceSceneStamp {
        self.current_stamp
    }
}

/// Complete independently revisioned presentation roster.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSceneSet {
    surfaces: BTreeMap<SurfaceId, SurfaceScene>,
    revision_tombstones: BTreeMap<SurfaceId, SurfaceSceneRevision>,
    popup_interaction_gate: PopupInteractionGate,
}

impl SurfaceSceneSet {
    pub(crate) fn new(manifest: &SceneRequirementManifest) -> Result<Self, SceneBuildError> {
        let mut set = Self {
            surfaces: BTreeMap::new(),
            revision_tombstones: BTreeMap::new(),
            popup_interaction_gate: PopupInteractionGate::new(manifest),
        };
        for (surface, requirements) in manifest.surfaces() {
            let revision = set.next_revision(surface)?;
            set.surfaces.insert(
                surface,
                SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp: SurfaceSceneStamp::new(requirements.ticket(), revision),
                    reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                }),
            );
        }
        Ok(set)
    }

    /// Returns one surface authority, or `None` outside the current roster.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceScene> {
        self.surfaces.get(&surface)
    }

    /// Returns one ready surface eligible for hit testing and proof creation.
    #[must_use]
    pub fn ready_surface(&self, surface: SurfaceId) -> Option<&SurfacePlanScene> {
        self.interaction_projection(surface)
            .map(|projection| projection.output)
    }

    /// Returns one indivisible plan, hit manifest, output, and presentation
    /// authority view after its required popup-plane proof boundary is crossed.
    #[must_use]
    pub(crate) fn interaction_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        let revision = self.popup_interaction_gate.interaction_revision()?;
        self.surface(surface)
            .and_then(|scene| scene.interaction_projection(revision))
    }

    /// Returns the exact observed interaction authority after the applicable gate.
    #[must_use]
    pub(crate) fn interaction_authority(
        &self,
        surface: SurfaceId,
    ) -> Option<PresentedSurfaceAuthority> {
        self.interaction_projection(surface)
            .map(SurfaceInteractionProjection::authority)
    }

    pub(crate) fn interaction_authorities(&self) -> BTreeMap<SurfaceId, PresentedSurfaceAuthority> {
        #[cfg(test)]
        interaction_authority_work::record_scan(self.surfaces.len());
        self.surfaces
            .keys()
            .filter_map(|surface| {
                self.interaction_authority(*surface)
                    .map(|authority| (*surface, authority))
            })
            .collect()
    }

    /// Iterates concrete emissions retained inside ready scenes, including proofs temporarily
    /// hidden behind the workspace-global popup presentation barrier.
    ///
    /// The latter must remain available to adapter receiver stores: completing the sibling
    /// roster can reactivate those proofs without emitting the same concrete output again.
    pub(crate) fn retained_interaction_emissions(
        &self,
    ) -> impl Iterator<Item = crate::presentation_observation::HostFrameKey> + '_ {
        self.surfaces.values().filter_map(|scene| match scene {
            SurfaceScene::Ready(ready) => ready
                .interaction_authority()
                .map(PresentedSurfaceAuthority::emission),
            SurfaceScene::Stale(_) | SurfaceScene::Bootstrap(_) => None,
        })
    }

    pub(crate) fn ready_candidate(&self, surface: SurfaceId) -> Option<&SurfacePlanScene> {
        match self.surface(surface)? {
            SurfaceScene::Ready(ready) => Some(ready.candidate()),
            SurfaceScene::Bootstrap(_) | SurfaceScene::Stale(_) => None,
        }
    }

    /// Revokes only interaction authority minted by one of the terminated
    /// presentation streams, retaining every paint candidate and fallback.
    ///
    /// A superseded host may still have streams for a surface now authorized by
    /// another host. Exact stream matching prevents retirement of the former
    /// from disturbing the latter.
    pub(crate) fn revoke_interaction_authority_for_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
    ) -> BTreeSet<SurfaceId> {
        if self.popup_interaction_gate.requires_roster_barrier() {
            let intersects_gate = self
                .popup_interaction_gate
                .proofs
                .values()
                .any(|authority| streams.contains(&authority.stream()));
            if !intersects_gate {
                return BTreeSet::new();
            }
            return self.reset_popup_interaction_barrier();
        }
        let affected = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                streams.contains(&authority.stream()).then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        self.revoke_independent_surface_authorities(&affected)
    }

    /// Returns exact retained candidate and fallback stamps for one rostered surface.
    #[must_use]
    pub fn retained_plan_stamps(&self, surface: SurfaceId) -> Option<RetainedSurfacePlanStamps> {
        self.surface(surface)
            .map(SurfaceScene::retained_plan_stamps)
    }

    pub(crate) fn retained_output_capture(
        &self,
        ticket: SurfacePresentationOutputTicket,
    ) -> Result<SurfaceCoordinateCapture, PresentationAuthorityRejection> {
        let surface = ticket.surface();
        let Some(scene) = self.surfaces.get(&surface) else {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        };
        let SurfaceScene::Ready(ready) = scene else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        let candidate = ready.candidate();
        if candidate.output_ticket() == ticket {
            return Ok(candidate.coordinate_capture());
        }
        if let Some(fallback) = ready.paint_fallback()
            && fallback.output_ticket() == ticket
        {
            return Ok(fallback.coordinate_capture());
        }
        Err(PresentationAuthorityRejection::TicketNotRetained {
            candidate: candidate.output_ticket(),
            paint_fallback: ready.paint_fallback().map(SurfacePlanScene::output_ticket),
        })
    }

    pub(crate) fn accept_observed_authority(
        &mut self,
        authority: PresentedSurfaceAuthority,
    ) -> Result<BTreeSet<SurfaceId>, PresentationAuthorityRejection> {
        let surface = authority.surface();
        if !self.popup_interaction_gate.roster.contains(&surface) {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        }
        let Some(SurfaceScene::Ready(ready)) = self.surfaces.get(&surface) else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        ready.validate_presented_authority(authority, self.popup_interaction_gate.requirement)?;

        let previous = self.popup_interaction_gate.proofs.get(&surface).copied();
        if let Some(previous) = previous {
            if authority.emission() == previous.emission() {
                debug_assert_eq!(
                    authority, previous,
                    "one core-minted emission key must identify one exact authority"
                );
                return Ok(BTreeSet::new());
            }
            if authority.emission() < previous.emission() {
                return Err(PresentationAuthorityRejection::EmissionRegressed {
                    current: previous.emission(),
                    submitted: authority.emission(),
                });
            }
        }

        let interaction_semantics_changed =
            previous.is_some_and(|previous| !previous.same_interaction_semantics(authority));
        let mut changed =
            if self.popup_interaction_gate.is_active_presented() && interaction_semantics_changed {
                self.reset_popup_interaction_barrier()
            } else {
                BTreeSet::new()
            };

        let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(&surface) else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        ready.observe_presented_for_paint(authority, self.popup_interaction_gate.requirement)?;
        let phase = self.popup_interaction_gate.phase;
        let authority_changed = ready
            .interaction_authority
            .is_none_or(|current| !current.same_interaction_semantics(authority));
        if phase != PopupInteractionGatePhase::Staging {
            ready.interaction_authority = Some(authority);
            if authority_changed {
                changed.insert(surface);
            }
        }
        self.popup_interaction_gate
            .proofs
            .insert(surface, authority);

        if phase != PopupInteractionGatePhase::Staging {
            return Ok(changed);
        }
        changed.extend(self.try_present_popup_interaction_gate());
        Ok(changed)
    }

    #[cfg(test)]
    pub(crate) fn invalidate_interaction_authority_for_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
    ) -> BTreeSet<SurfaceId> {
        self.revoke_interaction_authority_for_streams(streams)
    }

    pub(crate) fn invalidate_interaction_authority_for_current_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
        current_surfaces: &BTreeSet<SurfaceId>,
    ) -> BTreeSet<SurfaceId> {
        let affected_surfaces = current_surfaces
            .intersection(&self.popup_interaction_gate.roster)
            .copied()
            .collect::<BTreeSet<_>>();
        let proof_surfaces = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                streams.contains(&authority.stream()).then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        if affected_surfaces.is_empty() && proof_surfaces.is_empty() {
            return BTreeSet::new();
        }
        if self.popup_interaction_gate.requires_roster_barrier() {
            let mut changed = self.reset_popup_interaction_barrier();
            changed.extend(affected_surfaces);
            return changed;
        }
        let mut revoked = affected_surfaces.clone();
        revoked.extend(proof_surfaces);
        let mut changed = self.revoke_independent_surface_authorities(&revoked);
        changed.extend(affected_surfaces);
        changed
    }

    /// Iterates the complete current roster in stable surface order.
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (&SurfaceId, &SurfaceScene)> {
        self.surfaces.iter()
    }

    pub(crate) fn reconcile_manifest(
        &mut self,
        manifest: &SceneRequirementManifest,
    ) -> Result<(), SceneBuildError> {
        let retained = manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<HashSet<_>>();
        self.surfaces
            .retain(|surface, _| retained.contains(surface));

        for (surface, requirements) in manifest.surfaces() {
            let ticket = requirements.ticket();
            let Some(current) = self.surfaces.remove(&surface) else {
                let revision = self.next_revision(surface)?;
                self.surfaces.insert(
                    surface,
                    SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                    }),
                );
                continue;
            };
            if current.stamp().requirement() == ticket {
                self.surfaces.insert(surface, current);
                continue;
            }

            let epoch_replaced =
                current.stamp().requirement().workspace_epoch() != ticket.workspace_epoch();
            let revision = self.next_revision(surface)?;
            let replacement = if epoch_replaced {
                SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp: SurfaceSceneStamp::new(ticket, revision),
                    reason: BootstrapSurfaceSceneReason::WorkspaceReplaced,
                })
            } else {
                match current {
                    SurfaceScene::Ready(previous) => {
                        match previous.into_confirmed_paint_fallback() {
                            Some(paint_fallback) => {
                                SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                                    stamp: SurfaceSceneStamp::new(ticket, revision),
                                    paint_fallback,
                                    reason: StaleSurfaceSceneReason::RequirementsChanged,
                                }))
                            }
                            None => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                                stamp: SurfaceSceneStamp::new(ticket, revision),
                                reason: BootstrapSurfaceSceneReason::RequirementsChanged,
                            }),
                        }
                    }
                    SurfaceScene::Stale(previous) => {
                        SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                            stamp: SurfaceSceneStamp::new(ticket, revision),
                            paint_fallback: previous.paint_fallback,
                            reason: StaleSurfaceSceneReason::RequirementsChanged,
                        }))
                    }
                    SurfaceScene::Bootstrap(_) => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::RequirementsChanged,
                    }),
                }
            };
            self.surfaces.insert(surface, replacement);
        }
        let requires_fresh_barrier = self.popup_interaction_gate.reconcile_identity(manifest);
        if requires_fresh_barrier
            || (self.popup_interaction_gate.requires_roster_barrier()
                && !self.popup_gate_proofs_are_current())
        {
            self.reset_popup_interaction_barrier();
        } else if !self.popup_interaction_gate.requires_roster_barrier() {
            self.revoke_invalid_independent_authorities();
        }
        Ok(())
    }

    pub(crate) fn install_ready(
        &mut self,
        plan: PresentationPlan,
        coordinate_capture: SurfaceCoordinateCapture,
        authority_domain: EngineAuthorityDomainId,
        output_serial: PresentationOutputSerial,
    ) -> Result<(SurfaceSceneStamp, SurfacePresentationOutputTicket), SceneBuildError> {
        let surface = plan.surface();
        if plan.popup() != self.popup_interaction_gate.requirement {
            return Err(SceneBuildError::PopupPlaneRecordSetMismatch { surface });
        }
        let ticket = plan
            .measurement_ticket()
            .ok_or(SceneBuildError::SyntheticPresentationRejected { surface })?;
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let expected_ticket = current.stamp().requirement();
        if expected_ticket != ticket {
            self.surfaces.insert(surface, current);
            return Err(SceneBuildError::PresentationTicketMismatch {
                surface,
                expected: Some(expected_ticket),
                actual: ticket,
            });
        }
        let revision = self.next_revision(surface)?;
        let stamp = SurfaceSceneStamp::new(ticket, revision);
        let output_ticket =
            SurfacePresentationOutputTicket::mint(authority_domain, output_serial, stamp);
        let hit_manifest = PresentationHitManifest::compile(output_ticket, &plan);
        let (retained_fallback, retained_interaction, confirmed_paint_fallback) = match current {
            SurfaceScene::Ready(ready) => ready.into_retained_authority(),
            SurfaceScene::Stale(stale) => {
                let confirmed = stale.paint_fallback.output_ticket();
                (Some(stale.paint_fallback), None, Some(confirmed))
            }
            SurfaceScene::Bootstrap(_) => (None, None, None),
        };
        let paint_fallback = retained_fallback.map(ReadySurfacePaintFallback::Retained);
        let interaction_authority = retained_interaction;
        self.surfaces.insert(
            surface,
            SurfaceScene::Ready(Box::new(ReadySurfaceScene {
                stamp,
                candidate: Box::new(SurfacePlanScene {
                    stamp,
                    output_ticket,
                    plan: Arc::new(plan),
                    hit_manifest: Arc::new(hit_manifest),
                    coordinate_capture,
                }),
                paint_fallback,
                confirmed_paint_fallback,
                interaction_authority,
            })),
        );
        if !self.popup_gate_proofs_are_current() {
            if self.popup_interaction_gate.requires_roster_barrier() {
                self.reset_popup_interaction_barrier();
            } else {
                self.revoke_invalid_independent_authorities();
            }
        }
        Ok((stamp, output_ticket))
    }

    pub(crate) fn demote_to_stale(
        &mut self,
        surface: SurfaceId,
        reason: StaleSurfaceSceneReason,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        match self.surfaces.get(&surface) {
            Some(SurfaceScene::Ready(ready)) if !ready.has_confirmed_paint_fallback() => {
                return Err(SceneBuildError::ReadySurfaceHasNoPaintFallback { surface });
            }
            Some(SurfaceScene::Bootstrap(_)) => {
                return Err(SceneBuildError::BootstrapCannotBecomeStale { surface });
            }
            Some(SurfaceScene::Ready(_) | SurfaceScene::Stale(_)) => {}
            None => return Err(SceneBuildError::SurfaceOutsideRoster { surface }),
        }
        self.invalidate_popup_authority_for_surface(surface);
        let current = self
            .surfaces
            .remove(&surface)
            .expect("surface fallback preflight established roster membership");
        let current_stamp = current.stamp();
        let paint_fallback = match current {
            SurfaceScene::Ready(ready) => ready
                .into_confirmed_paint_fallback()
                .expect("ready fallback preflight established a retained plan"),
            SurfaceScene::Stale(stale) => stale.paint_fallback,
            SurfaceScene::Bootstrap(_) => {
                unreachable!("bootstrap preflight returned before scene removal")
            }
        };
        let revision = self.next_revision(surface)?;
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        self.surfaces.insert(
            surface,
            SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback,
                reason,
            })),
        );
        Ok(stamp)
    }

    /// Advances one surface's authority after core-owned presentation input changes.
    ///
    /// A painted fallback remains paintable but loses hit authority. Surfaces
    /// without such a fallback remain bootstrap. In both cases the new revision
    /// makes every contribution token captured before this change stale.
    pub(crate) fn invalidate_presentation_input(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let current_stamp = current.stamp();
        let revision = self.next_revision(surface)?;
        self.invalidate_popup_authority_for_surface(surface);
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        let replacement = match current {
            SurfaceScene::Ready(ready) => match ready.into_confirmed_paint_fallback() {
                Some(paint_fallback) => SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                    stamp,
                    paint_fallback,
                    reason: StaleSurfaceSceneReason::TransientPresentationChanged,
                })),
                None => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp,
                    reason: BootstrapSurfaceSceneReason::TransientPresentationChanged,
                }),
            },
            SurfaceScene::Stale(stale) => SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback: stale.paint_fallback,
                reason: StaleSurfaceSceneReason::TransientPresentationChanged,
            })),
            SurfaceScene::Bootstrap(_) => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                stamp,
                reason: BootstrapSurfaceSceneReason::TransientPresentationChanged,
            }),
        };
        self.surfaces.insert(surface, replacement);
        Ok(stamp)
    }

    pub(crate) fn replace_with_bootstrap(
        &mut self,
        surface: SurfaceId,
        reason: BootstrapSurfaceSceneReason,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let revision = self.next_revision(surface)?;
        self.invalidate_popup_authority_for_surface(surface);
        let stamp = SurfaceSceneStamp::new(current.stamp().requirement(), revision);
        self.surfaces.insert(
            surface,
            SurfaceScene::Bootstrap(BootstrapSurfaceScene { stamp, reason }),
        );
        Ok(stamp)
    }

    fn popup_gate_proofs_are_current(&self) -> bool {
        self.popup_interaction_gate
            .proofs
            .iter()
            .all(|(surface, authority)| {
                *surface == authority.surface()
                    && self
                        .surfaces
                        .get(surface)
                        .and_then(SurfaceScene::ready)
                        .is_some_and(|ready| {
                            ready
                                .validate_presented_authority(
                                    *authority,
                                    self.popup_interaction_gate.requirement,
                                )
                                .is_ok()
                        })
            })
    }

    fn try_present_popup_interaction_gate(&mut self) -> BTreeSet<SurfaceId> {
        if !self.popup_interaction_gate.requires_roster_barrier() {
            return BTreeSet::new();
        }
        if self.popup_interaction_gate.roster.is_empty()
            || self.popup_interaction_gate.proofs.len() != self.popup_interaction_gate.roster.len()
            || !self
                .popup_interaction_gate
                .roster
                .iter()
                .all(|surface| self.popup_interaction_gate.proofs.contains_key(surface))
            || !self.popup_gate_proofs_are_current()
        {
            return BTreeSet::new();
        }

        let (gate, surfaces) = (&mut self.popup_interaction_gate, &mut self.surfaces);
        let Some(_) = gate.revision else {
            return BTreeSet::new();
        };
        let mut changed = BTreeSet::new();
        for surface in &gate.roster {
            let authority = *gate
                .proofs
                .get(surface)
                .expect("popup gate preflight established an exact proof roster");
            let Some(SurfaceScene::Ready(ready)) = surfaces.get_mut(surface) else {
                unreachable!("popup gate preflight established an exact ready roster");
            };
            if ready
                .interaction_authority
                .is_none_or(|current| !current.same_interaction_semantics(authority))
            {
                changed.insert(*surface);
            }
            ready.interaction_authority = Some(authority);
        }
        gate.phase = match gate.requirement {
            PopupPlaneRequirement::Inactive { .. } => {
                PopupInteractionGatePhase::InactiveIndependent
            }
            PopupPlaneRequirement::Active { .. } => PopupInteractionGatePhase::ActivePresented,
        };
        changed
    }

    fn reset_popup_interaction_barrier(&mut self) -> BTreeSet<SurfaceId> {
        self.popup_interaction_gate.reset_barrier();
        let mut changed = BTreeSet::new();
        for (surface, scene) in &mut self.surfaces {
            let SurfaceScene::Ready(ready) = scene else {
                continue;
            };
            if ready.interaction_authority.take().is_some() {
                changed.insert(*surface);
            }
        }
        changed
    }

    fn invalidate_popup_authority_for_surface(&mut self, surface: SurfaceId) {
        if self.popup_interaction_gate.requires_roster_barrier() {
            self.reset_popup_interaction_barrier();
            return;
        }
        self.popup_interaction_gate.proofs.remove(&surface);
        if let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(&surface) {
            ready.interaction_authority = None;
        }
    }

    fn revoke_independent_surface_authorities(
        &mut self,
        surfaces: &BTreeSet<SurfaceId>,
    ) -> BTreeSet<SurfaceId> {
        debug_assert!(!self.popup_interaction_gate.requires_roster_barrier());
        let mut changed = BTreeSet::new();
        for surface in surfaces {
            self.popup_interaction_gate.proofs.remove(surface);
            let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(surface) else {
                continue;
            };
            if ready.interaction_authority.take().is_some() {
                changed.insert(*surface);
            }
        }
        changed
    }

    fn revoke_invalid_independent_authorities(&mut self) -> BTreeSet<SurfaceId> {
        debug_assert!(!self.popup_interaction_gate.requires_roster_barrier());
        let invalid = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                let current = *surface == authority.surface()
                    && self
                        .surfaces
                        .get(surface)
                        .and_then(SurfaceScene::ready)
                        .is_some_and(|ready| {
                            ready
                                .validate_presented_authority(
                                    *authority,
                                    self.popup_interaction_gate.requirement,
                                )
                                .is_ok()
                        });
                (!current).then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        self.revoke_independent_surface_authorities(&invalid)
    }

    fn next_revision(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceSceneRevision, SceneBuildError> {
        let previous = self
            .revision_tombstones
            .get(&surface)
            .copied()
            .unwrap_or_default();
        let revision = previous
            .checked_next()
            .ok_or(SceneBuildError::SurfaceSceneRevisionExhausted { surface })?;
        self.revision_tombstones.insert(surface, revision);
        Ok(revision)
    }
}

#[cfg(test)]
mod surface_paint_ledger_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch, WorkspaceRevision};
    use crate::policy::TabBarPolicy;
    use crate::presentation_config::PresentationConfigRevision;
    use crate::presentation_observation::{
        PresentationAuthorityRejection, PresentationOutputSerial, PresentedSurfaceAuthority,
    };
    use crate::scene_manifest::{
        PolicyRevision, RequirementRevision, SceneRequirementDraft, SurfaceRequirementRevision,
        SurfaceRequirements, TabBarRequirement, TabStripKey,
    };
    use crate::transition::WorkspaceVersion;
    use crate::viewport::CoordinateGeneration;

    fn manifest(
        surface: SurfaceId,
        surface_revision: u64,
        manifest_revision: u64,
    ) -> SceneRequirementManifest {
        manifest_for_roster(
            [(surface, surface_revision)],
            manifest_revision,
            PopupPlaneRequirement::default(),
            None,
        )
    }

    fn manifest_for_roster(
        surfaces: impl IntoIterator<Item = (SurfaceId, u64)>,
        manifest_revision: u64,
        popup: PopupPlaneRequirement,
        popup_owner: Option<TabStripStateKey>,
    ) -> SceneRequirementManifest {
        let authority = EngineAuthorityDomainId::new_for_test(73);
        let epoch = WorkspaceEpoch::new(5);
        let config = PresentationConfigRevision::default();
        let policy = PolicyRevision::default();
        let surfaces = surfaces
            .into_iter()
            .map(|(surface, surface_revision)| {
                let ticket = SurfaceMeasurementTicket::new(
                    authority,
                    epoch,
                    config,
                    policy,
                    SurfaceRequirementRevision::new(surface_revision),
                    surface,
                );
                let (tab_strips, tab_bars) = popup_owner
                    .filter(|owner| owner.surface() == surface)
                    .map_or_else(
                        || (BTreeSet::new(), BTreeMap::new()),
                        |owner| {
                            let bar = owner.bar();
                            (
                                BTreeSet::from([TabStripKey::new(surface, bar)]),
                                BTreeMap::from([(
                                    bar,
                                    TabBarRequirement::new(
                                        bar,
                                        TabBarPolicy::default(),
                                        BTreeMap::new(),
                                    ),
                                )]),
                            )
                        },
                    );
                let requirements = SurfaceRequirements::new(
                    ticket,
                    BTreeSet::new(),
                    BTreeSet::new(),
                    tab_strips,
                    tab_bars,
                )
                .expect("test requirements are coherent");
                (surface, requirements)
            })
            .collect();
        SceneRequirementDraft::new_for_test(
            authority,
            WorkspaceVersion::new(epoch, WorkspaceRevision::default()),
            config,
            policy,
            RequirementRevision::new(manifest_revision),
            surfaces,
        )
        .and_then(|draft| draft.finalize(popup))
        .expect("test manifest is coherent")
    }

    fn candidate_with_popup(
        ticket: SurfaceMeasurementTicket,
        popup: PopupPlaneRequirement,
    ) -> PresentationPlan {
        let bounds = LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("positive bounds");
        PresentationPlan::from_measurements(
            ticket,
            popup,
            bounds,
            matches!(popup, PopupPlaneRequirement::Active { .. }).then_some(bounds),
        )
    }

    fn headless_capture() -> SurfaceCoordinateCapture {
        SurfaceCoordinateCapture::Headless {
            authority_generation: CoordinateGeneration::default(),
        }
    }

    fn install(
        scenes: &mut SurfaceSceneSet,
        ticket: SurfaceMeasurementTicket,
        serial: u64,
    ) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
        install_with_popup(scenes, ticket, PopupPlaneRequirement::default(), serial)
    }

    fn install_with_popup(
        scenes: &mut SurfaceSceneSet,
        ticket: SurfaceMeasurementTicket,
        popup: PopupPlaneRequirement,
        serial: u64,
    ) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
        scenes
            .install_ready(
                candidate_with_popup(ticket, popup),
                headless_capture(),
                ticket.authority_domain(),
                PresentationOutputSerial::new_for_test(serial),
            )
            .expect("candidate installs")
    }

    fn acknowledge(
        scenes: &mut SurfaceSceneSet,
        ticket: SurfacePresentationOutputTicket,
        serial: u64,
    ) -> Result<bool, PresentationAuthorityRejection> {
        scenes
            .accept_observed_authority(
                PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
                    ticket,
                    CoordinateGeneration::default(),
                    serial,
                ),
            )
            .map(|changed| !changed.is_empty())
    }

    fn observed(
        output: SurfacePresentationOutputTicket,
        emission: u64,
    ) -> PresentedSurfaceAuthority {
        PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
            output,
            CoordinateGeneration::default(),
            emission,
        )
    }

    fn raw_interaction_authority(
        scenes: &SurfaceSceneSet,
        surface: SurfaceId,
    ) -> Option<PresentedSurfaceAuthority> {
        scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .and_then(ReadySurfaceScene::interaction_authority)
    }

    #[test]
    fn unpainted_projection_cannot_hit_or_survive_requirement_invalidation() {
        let surface = SurfaceId::new(11);
        let initial = manifest(surface, 0, 0);
        let ticket = initial.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
        let _ = install(&mut scenes, ticket, 1);

        assert!(scenes.ready_surface(surface).is_none());

        scenes
            .reconcile_manifest(&manifest(surface, 1, 1))
            .expect("requirements reconcile");
        assert!(matches!(
            scenes.surface(surface),
            Some(SurfaceScene::Bootstrap(_))
        ));
        assert!(
            scenes
                .surface(surface)
                .and_then(SurfaceScene::paint_projection)
                .is_none()
        );
    }

    #[test]
    fn stale_fallback_survives_an_unpainted_ready_candidate_without_regaining_hit_authority() {
        let surface = SurfaceId::new(12);
        let initial = manifest(surface, 0, 0);
        let initial_ticket = initial.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
        let (painted, painted_output) = install(&mut scenes, initial_ticket, 1);
        assert_eq!(acknowledge(&mut scenes, painted_output, 1), Ok(true));

        let changed = manifest(surface, 1, 1);
        scenes
            .reconcile_manifest(&changed)
            .expect("requirements reconcile");
        assert!(scenes.ready_surface(surface).is_none());

        let changed_ticket = changed.surface(surface).expect("surface exists").ticket();
        let (pending, _) = install(&mut scenes, changed_ticket, 2);
        assert_ne!(painted, pending);
        assert!(scenes.ready_surface(surface).is_none());
        let projection = scenes
            .surface(surface)
            .and_then(SurfaceScene::paint_projection)
            .expect("next candidate projects");
        assert_eq!(projection.plan_stamp(), pending);
        assert!(scenes.interaction_projection(surface).is_none());

        scenes
            .demote_to_stale(surface, StaleSurfaceSceneReason::CoordinateAuthorityChanged)
            .expect("painted fallback permits stale transition");
        assert!(scenes.ready_surface(surface).is_none());
        let stale = scenes
            .surface(surface)
            .and_then(SurfaceScene::paint_projection)
            .expect("painted fallback remains visible");
        assert_eq!(stale.plan_stamp(), painted);
        assert!(scenes.interaction_projection(surface).is_none());
    }

    #[test]
    fn exact_observation_grants_fallback_and_interaction_authority() {
        let surface = SurfaceId::new(13);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

        let (painted, output) = install(&mut scenes, ticket, 1);
        assert!(scenes.ready_surface(surface).is_none());
        assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
        let scene = scenes.surface(surface).expect("surface remains rostered");
        let ready = scene.ready().expect("painted candidate is ready");

        assert_eq!(ready.candidate().stamp(), painted);
        assert_eq!(
            ready.paint_fallback().map(SurfacePlanScene::stamp),
            Some(painted)
        );
        assert_eq!(
            ready
                .interaction_authority()
                .map(|authority| authority.ticket()),
            Some(output)
        );
        assert_eq!(
            scenes.ready_surface(surface).map(SurfacePlanScene::stamp),
            Some(painted)
        );
    }

    #[test]
    fn inactive_surface_authority_does_not_wait_for_a_bootstrap_sibling() {
        let first = SurfaceId::new(131);
        let second = SurfaceId::new(132);
        let manifest = manifest_for_roster(
            [(first, 0), (second, 0)],
            0,
            PopupPlaneRequirement::default(),
            None,
        );
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let (_, first_output) = install(
            &mut scenes,
            manifest.surface(first).expect("first exists").ticket(),
            1,
        );
        assert_eq!(
            scenes.accept_observed_authority(observed(first_output, 1)),
            Ok(BTreeSet::from([first]))
        );
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_none());
        assert_eq!(
            raw_interaction_authority(&scenes, first).map(PresentedSurfaceAuthority::ticket),
            Some(first_output)
        );
        assert!(raw_interaction_authority(&scenes, second).is_none());
        assert!(matches!(
            scenes.surface(second),
            Some(SurfaceScene::Bootstrap(_))
        ));

        let (_, second_output) = install(
            &mut scenes,
            manifest.surface(second).expect("second exists").ticket(),
            2,
        );

        assert_eq!(
            scenes.accept_observed_authority(observed(second_output, 2)),
            Ok(BTreeSet::from([second]))
        );
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());
        assert_eq!(
            raw_interaction_authority(&scenes, first).map(PresentedSurfaceAuthority::ticket),
            Some(first_output)
        );
        assert_eq!(
            raw_interaction_authority(&scenes, second).map(PresentedSurfaceAuthority::ticket),
            Some(second_output)
        );
    }

    #[test]
    fn popup_gate_presentation_is_independent_of_acknowledgement_order() {
        fn present(order: [SurfaceId; 2]) -> BTreeMap<SurfaceId, SurfacePresentationOutputTicket> {
            let first = SurfaceId::new(133);
            let second = SurfaceId::new(134);
            let manifest = manifest_for_roster(
                [(first, 0), (second, 0)],
                0,
                PopupPlaneRequirement::default(),
                None,
            );
            let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
            let outputs = [first, second]
                .into_iter()
                .enumerate()
                .map(|(index, surface)| {
                    let (_, output) = install(
                        &mut scenes,
                        manifest.surface(surface).expect("surface exists").ticket(),
                        index as u64 + 1,
                    );
                    (surface, output)
                })
                .collect::<BTreeMap<_, _>>();

            for (index, surface) in order.into_iter().enumerate() {
                scenes
                    .accept_observed_authority(observed(outputs[&surface], index as u64 + 1))
                    .expect("proof is accepted");
            }
            assert!(scenes.ready_surface(first).is_some());
            assert!(scenes.ready_surface(second).is_some());
            outputs
        }

        let first = SurfaceId::new(133);
        let second = SurfaceId::new(134);
        assert_eq!(present([first, second]), present([second, first]));
    }

    #[test]
    fn inactive_revocation_removes_every_surface_proven_by_the_exact_stream() {
        let first = SurfaceId::new(135);
        let second = SurfaceId::new(136);
        let manifest = manifest_for_roster(
            [(first, 0), (second, 0)],
            0,
            PopupPlaneRequirement::default(),
            None,
        );
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let (_, first_output) = install(
            &mut scenes,
            manifest.surface(first).expect("first exists").ticket(),
            1,
        );
        let (_, second_output) = install(
            &mut scenes,
            manifest.surface(second).expect("second exists").ticket(),
            2,
        );
        scenes
            .accept_observed_authority(observed(first_output, 1))
            .expect("first proof is accepted");
        scenes
            .accept_observed_authority(observed(second_output, 2))
            .expect("second proof is accepted");
        let invalidated_stream = raw_interaction_authority(&scenes, first)
            .expect("first surface is interactive")
            .stream();

        assert_eq!(
            scenes.invalidate_interaction_authority_for_streams(&BTreeSet::from([
                invalidated_stream,
            ])),
            BTreeSet::from([first, second])
        );
        assert!(raw_interaction_authority(&scenes, first).is_none());
        assert!(raw_interaction_authority(&scenes, second).is_none());
        assert!(scenes.ready_surface(first).is_none());
        assert!(scenes.ready_surface(second).is_none());
        assert!(
            scenes
                .surface(first)
                .and_then(SurfaceScene::paint_projection)
                .is_some()
        );
        assert!(
            scenes
                .surface(second)
                .and_then(SurfaceScene::paint_projection)
                .is_some()
        );
    }

    #[test]
    fn inactive_coordinate_invalidation_preserves_sibling_interaction_authority() {
        let first = SurfaceId::new(137);
        let second = SurfaceId::new(138);
        let manifest = manifest_for_roster(
            [(first, 0), (second, 0)],
            0,
            PopupPlaneRequirement::default(),
            None,
        );
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        for (serial, surface) in [first, second].into_iter().enumerate() {
            let (_, output) = install(
                &mut scenes,
                manifest.surface(surface).expect("surface exists").ticket(),
                serial as u64 + 1,
            );
            scenes
                .accept_observed_authority(observed(output, serial as u64 + 1))
                .expect("proof is accepted");
        }
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());

        scenes
            .invalidate_presentation_input(first)
            .expect("surface invalidates");

        assert!(scenes.ready_surface(first).is_none());
        assert!(scenes.ready_surface(second).is_some());
        assert!(raw_interaction_authority(&scenes, second).is_some());
        assert!(
            scenes
                .surface(second)
                .and_then(SurfaceScene::paint_projection)
                .is_some()
        );
    }

    #[test]
    fn popup_close_requires_the_new_inactive_revision_across_the_exact_roster() {
        let first = SurfaceId::new(139);
        let second = SurfaceId::new(140);
        let bar = TabBarSceneId {
            root: RootId::new(141),
            tabs: NodeId::default(),
        };
        let owner = TabStripStateKey::new(first, bar);
        let active = PopupPlaneRequirement::Active {
            revision: PopupRoutingRevision::new_for_test(1),
            session: TabListMenuSessionId::new_for_test(owner, 1),
            owner,
        };
        let active_manifest =
            manifest_for_roster([(first, 0), (second, 0)], 0, active, Some(owner));
        let mut scenes = SurfaceSceneSet::new(&active_manifest).expect("scene set initializes");
        let mut active_outputs = BTreeMap::new();
        for (serial, surface) in [first, second].into_iter().enumerate() {
            let (_, output) = install_with_popup(
                &mut scenes,
                active_manifest
                    .surface(surface)
                    .expect("surface exists")
                    .ticket(),
                active,
                serial as u64 + 1,
            );
            active_outputs.insert(surface, output);
        }
        scenes
            .accept_observed_authority(observed(active_outputs[&first], 1))
            .expect("first active proof is accepted");
        assert!(scenes.ready_surface(first).is_none());
        assert!(scenes.ready_surface(second).is_none());
        scenes
            .accept_observed_authority(observed(active_outputs[&second], 2))
            .expect("second active proof is accepted");
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());

        let inactive = PopupPlaneRequirement::Inactive {
            revision: PopupRoutingRevision::new_for_test(2),
        };
        let inactive_manifest =
            manifest_for_roster([(first, 1), (second, 1)], 1, inactive, Some(owner));
        scenes
            .reconcile_manifest(&inactive_manifest)
            .expect("close revision reconciles");
        assert!(scenes.ready_surface(first).is_none());
        assert!(scenes.ready_surface(second).is_none());
        assert!(
            scenes
                .accept_observed_authority(observed(active_outputs[&first], 3))
                .is_err(),
            "an old active-plane proof cannot authorize the close successor"
        );

        let mut inactive_outputs = BTreeMap::new();
        for (serial, surface) in [first, second].into_iter().enumerate() {
            let (_, output) = install_with_popup(
                &mut scenes,
                inactive_manifest
                    .surface(surface)
                    .expect("surface exists")
                    .ticket(),
                inactive,
                serial as u64 + 3,
            );
            inactive_outputs.insert(surface, output);
        }
        scenes
            .accept_observed_authority(observed(inactive_outputs[&first], 4))
            .expect("first close proof is accepted");
        assert!(scenes.ready_surface(first).is_none());
        assert!(scenes.ready_surface(second).is_none());
        scenes
            .accept_observed_authority(observed(inactive_outputs[&second], 5))
            .expect("second close proof is accepted");
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());
    }

    #[test]
    fn inactive_roster_changes_preserve_retained_surface_authority() {
        let first = SurfaceId::new(143);
        let second = SurfaceId::new(144);
        let third = SurfaceId::new(145);
        let initial = manifest_for_roster(
            [(first, 0), (second, 0)],
            0,
            PopupPlaneRequirement::default(),
            None,
        );
        let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
        for (serial, surface) in [first, second].into_iter().enumerate() {
            let (_, output) = install(
                &mut scenes,
                initial.surface(surface).expect("surface exists").ticket(),
                serial as u64 + 1,
            );
            scenes
                .accept_observed_authority(observed(output, serial as u64 + 1))
                .expect("initial proof is accepted");
        }
        assert!(scenes.ready_surface(first).is_some());

        let added = manifest_for_roster(
            [(first, 0), (second, 0), (third, 0)],
            1,
            PopupPlaneRequirement::default(),
            None,
        );
        scenes
            .reconcile_manifest(&added)
            .expect("added roster reconciles");
        let (_, third_output) = install(
            &mut scenes,
            added.surface(third).expect("third exists").ticket(),
            3,
        );
        scenes
            .accept_observed_authority(observed(third_output, 3))
            .expect("third proof is accepted");
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());
        assert!(scenes.ready_surface(third).is_some());

        let removed = manifest_for_roster(
            [(first, 0), (second, 0)],
            2,
            PopupPlaneRequirement::default(),
            None,
        );
        scenes
            .reconcile_manifest(&removed)
            .expect("removed roster reconciles");
        assert!(scenes.surface(third).is_none());
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());
    }

    #[test]
    fn repeated_identical_authority_is_idempotent() {
        let surface = SurfaceId::new(14);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let (candidate, output) = install(&mut scenes, ticket, 1);

        assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
        assert_eq!(acknowledge(&mut scenes, output, 1), Ok(false));
        assert_eq!(
            scenes.ready_surface(surface).map(SurfacePlanScene::stamp),
            Some(candidate)
        );
    }

    #[test]
    fn refreshed_emission_advances_provenance_without_changing_interaction_semantics() {
        let surface = SurfaceId::new(141);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let (_, output) = install(&mut scenes, ticket, 1);

        assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
        let first = scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .and_then(ReadySurfaceScene::interaction_authority)
            .expect("first observation grants interaction authority");

        assert_eq!(acknowledge(&mut scenes, output, 2), Ok(false));
        let refreshed = scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .and_then(ReadySurfaceScene::interaction_authority)
            .expect("refreshed observation retains interaction authority");
        assert_ne!(first, refreshed);
        assert!(first.emission() < refreshed.emission());
        assert!(first.same_interaction_semantics(refreshed));
    }

    #[test]
    fn late_older_emission_cannot_regress_concrete_presentation_provenance() {
        let surface = SurfaceId::new(142);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let (_, output) = install(&mut scenes, ticket, 1);

        assert_eq!(acknowledge(&mut scenes, output, 2), Ok(true));
        let current = scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .and_then(ReadySurfaceScene::interaction_authority)
            .expect("newer observation grants interaction authority");
        let submitted = PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
            output,
            CoordinateGeneration::default(),
            1,
        );

        assert_eq!(
            scenes.accept_observed_authority(submitted),
            Err(PresentationAuthorityRejection::EmissionRegressed {
                current: current.emission(),
                submitted: submitted.emission(),
            })
        );
        assert_eq!(
            scenes
                .surface(surface)
                .and_then(SurfaceScene::ready)
                .and_then(ReadySurfaceScene::interaction_authority),
            Some(current)
        );
    }

    #[test]
    fn delayed_fallback_ticket_can_grant_authority_after_a_replacement_candidate() {
        let surface = SurfaceId::new(15);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

        let (_painted_stamp, delayed_output) = install(&mut scenes, ticket, 1);
        let (replacement_stamp, replacement_output) = install(&mut scenes, ticket, 2);

        assert_eq!(acknowledge(&mut scenes, delayed_output, 1), Ok(true));
        let ready = scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .expect("replacement scene remains ready");
        assert_eq!(ready.candidate().stamp(), replacement_stamp);
        assert_eq!(ready.candidate().output_ticket(), replacement_output);
        assert_eq!(
            ready.paint_fallback().map(SurfacePlanScene::output_ticket),
            Some(delayed_output)
        );
        assert_eq!(
            ready
                .interaction_authority()
                .map(|authority| authority.ticket()),
            Some(delayed_output)
        );
    }

    #[test]
    fn displaced_unpainted_candidate_cannot_be_observed_after_one_more_replacement() {
        let surface = SurfaceId::new(16);
        let manifest = manifest(surface, 0, 0);
        let ticket = manifest.surface(surface).expect("surface exists").ticket();
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

        let (_, delayed_output) = install(&mut scenes, ticket, 1);
        let (_, displaced_output) = install(&mut scenes, ticket, 2);
        let (_, current_output) = install(&mut scenes, ticket, 3);

        assert_eq!(
            acknowledge(&mut scenes, displaced_output, 1),
            Err(PresentationAuthorityRejection::TicketNotRetained {
                candidate: current_output,
                paint_fallback: Some(delayed_output),
            })
        );
    }
}

/// Reusable validator for one complete surface presentation plan.
///
/// The validator owns the workspace-derived index so callers validating a batch
/// of independent surface contributions pay the indexing cost only once.
pub(crate) struct PresentationPlanValidator<'a> {
    workspace: &'a Workspace,
    workspace_index: SceneWorkspaceIndex,
    policy: &'a DockPolicySnapshot,
}

impl<'a> PresentationPlanValidator<'a> {
    pub(crate) fn new(
        workspace: &'a Workspace,
        policy: &'a DockPolicySnapshot,
    ) -> Result<Self, SceneBuildError> {
        Ok(Self {
            workspace,
            workspace_index: SceneWorkspaceIndex::new(workspace)?,
            policy,
        })
    }

    /// Validates and canonicalizes one plan without publishing partial state.
    pub(crate) fn validate_and_canonicalize(
        &self,
        mut ready: PresentationPlan,
    ) -> Result<PresentationPlan, SceneBuildError> {
        let surface = ready.surface;
        validate_semantic_uniqueness(&ready)?;
        validate_popup_plane(&ready)?;
        if !rect_has_area(ready.bounds) {
            return Err(SceneBuildError::EmptyReadySurfaceBounds { surface });
        }
        validate_surface_background(&ready, self.workspace, &self.workspace_index)?;
        if ready.measurement_ticket().is_some() {
            validate_ready_semantics(&ready, self.workspace, &self.workspace_index, self.policy)?;
        }
        let published_occlusions: HashSet<_> = ready
            .drop_occlusions
            .iter()
            .map(|occlusion| occlusion.floating())
            .collect();
        for floating in self.workspace_index.contained_on_surface(surface) {
            if !published_occlusions.contains(&floating) {
                return Err(SceneBuildError::MissingDropOcclusion { surface, floating });
            }
        }
        for occlusion in &ready.drop_occlusions {
            let floating = occlusion.floating();
            if !self
                .workspace_index
                .contained_belongs_to_surface(surface, floating)
            {
                return Err(SceneBuildError::InvalidDropOcclusion { surface, floating });
            }
            let expected = self
                .workspace_index
                .contained_layer(floating)
                .ok_or(SceneBuildError::InvalidDropOcclusion { surface, floating })?;
            if occlusion.layer() != expected {
                return Err(SceneBuildError::DropOcclusionLayerMismatch {
                    surface,
                    floating,
                    expected,
                    actual: occlusion.layer(),
                });
            }
            let region = occlusion.region().rect();
            if !rect_has_area(region) {
                return Err(SceneBuildError::EmptyDropOcclusionRegion { surface, floating });
            }
            let expected = self
                .workspace_index
                .contained_rect(floating)
                .ok_or(SceneBuildError::InvalidDropOcclusion { surface, floating })?;
            if region != expected {
                return Err(SceneBuildError::DropOcclusionGeometryMismatch {
                    surface,
                    floating,
                    expected,
                    actual: region,
                });
            }
        }
        for target in &ready.drop_targets {
            validate_drop_target(&ready, target, self.workspace, &self.workspace_index)?;
        }
        for cluster in &ready.drop_guide_clusters {
            validate_drop_guide_cluster(&ready, cluster, self.workspace, &self.workspace_index)?;
        }
        validate_contained_minimums(&ready, &self.workspace_index)?;
        canonicalize_ready(
            &mut ready,
            self.workspace,
            &self.workspace_index,
            self.policy,
        );
        Ok(ready)
    }
}

fn validate_popup_plane(ready: &PresentationPlan) -> Result<(), SceneBuildError> {
    let backdrops = &ready.tab_list_menu_backdrop_records;
    match ready.popup {
        PopupPlaneRequirement::Inactive { .. } => {
            if ready.popup_plane_bounds.is_some()
                || !backdrops.is_empty()
                || !ready.tab_list_menu_records.is_empty()
            {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
        }
        PopupPlaneRequirement::Active {
            revision,
            session,
            owner,
        } => {
            let Some(popup_plane_bounds) = ready.popup_plane_bounds else {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            };
            if backdrops.len() != 1
                || backdrops[0].session() != session
                || backdrops[0].revision() != revision
                || !rect_has_area(popup_plane_bounds)
                || backdrops[0].bounds() != popup_plane_bounds
            {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
            let owner_surface = owner.surface() == ready.surface;
            if owner_surface {
                if ready.tab_list_menu_records.len() != 1
                    || ready.tab_list_menu_records[0].session() != session
                    || ready.tab_list_menu_records[0].bar() != owner.bar()
                {
                    return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                        surface: ready.surface,
                    });
                }
            } else if !ready.tab_list_menu_records.is_empty() {
                return Err(SceneBuildError::PopupPlaneRecordSetMismatch {
                    surface: ready.surface,
                });
            }
        }
    }
    Ok(())
}

fn validate_semantic_uniqueness(ready: &PresentationPlan) -> Result<(), SceneBuildError> {
    fn stable_duplicate<T: Copy + Ord>(values: impl IntoIterator<Item = T>) -> Option<T> {
        let mut seen = BTreeSet::new();
        let mut duplicates = BTreeSet::new();
        for value in values {
            if !seen.insert(value) {
                duplicates.insert(value);
            }
        }
        duplicates.into_iter().next()
    }

    if let Some(id) = stable_duplicate(ready.pane_records.iter().map(PaneRecord::id)) {
        return Err(SceneBuildError::DuplicatePane { id });
    }
    if let Some(id) = stable_duplicate(ready.tab_bar_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateTabBar { id });
    }
    if let Some(id) = stable_duplicate(ready.tab_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateTab { id });
    }
    if let Some(id) = stable_duplicate(ready.splitter_gap_records.iter().map(SplitterGapRecord::id))
    {
        return Err(SceneBuildError::DuplicateSplitterGap { id });
    }
    if let Some(id) = stable_duplicate(ready.splitter_records.iter().map(|record| *record.id())) {
        return Err(SceneBuildError::DuplicateSplitter { id });
    }
    if let Some(id) = stable_duplicate(
        ready
            .splitter_junction_records
            .iter()
            .map(SplitterJunctionRecord::id),
    ) {
        return Err(SceneBuildError::DuplicateSplitterJunction { id });
    }
    if let Some(floating) = stable_duplicate(
        ready
            .contained_records
            .iter()
            .map(ContainedRecord::floating),
    ) {
        return Err(SceneBuildError::DuplicateContainedRecord { floating });
    }
    if let Some(floating) = stable_duplicate(
        ready
            .contained_minimums
            .iter()
            .map(|measurement| measurement.floating()),
    ) {
        return Err(SceneBuildError::DuplicateContainedMinimumMeasurement { floating });
    }
    if let Some(floating) =
        stable_duplicate(ready.drop_occlusions.iter().map(|record| record.floating()))
    {
        return Err(SceneBuildError::DuplicateDropOcclusion { floating });
    }
    if let Some(id) = stable_duplicate(
        ready
            .drop_guide_clusters
            .iter()
            .map(DropGuideClusterRecord::id),
    ) {
        return Err(SceneBuildError::DuplicateDropGuideCluster { id });
    }
    let targets = ready
        .surface_background
        .iter()
        .map(DropTargetRecord::id)
        .chain(ready.drop_targets.iter().map(DropTargetRecord::id))
        .chain(
            ready
                .drop_guide_clusters
                .iter()
                .flat_map(|cluster| cluster.targets().map(|(_, guide_target)| guide_target.id())),
        );
    if let Some(id) = stable_duplicate(targets) {
        return Err(SceneBuildError::DuplicateDropTarget { id });
    }
    Ok(())
}

fn validate_contained_minimums(
    ready: &PresentationPlan,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let surface = ready.surface;
    let published: HashSet<_> = ready
        .contained_minimums
        .iter()
        .map(|measurement| measurement.floating())
        .collect();
    if let Some(floating) = ready
        .contained_minimums
        .iter()
        .map(|measurement| measurement.floating())
        .filter(|floating| !workspace_index.contained_belongs_to_surface(surface, *floating))
        .min()
    {
        return Err(SceneBuildError::UnexpectedContainedMinimumMeasurement { surface, floating });
    }
    for floating in workspace_index.contained_on_surface(surface) {
        if !published.contains(&floating) {
            return Err(SceneBuildError::MissingContainedMinimumMeasurement { surface, floating });
        }
    }
    Ok(())
}

fn validate_surface_background(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let Some(presentation) = workspace.surface(ready.surface) else {
        if let Some(background) = &ready.surface_background {
            return Err(SceneBuildError::UnexpectedSurfaceBackground {
                surface: ready.surface,
                target: background.id(),
            });
        }
        return Ok(());
    };

    match (presentation.main_root, &ready.surface_background) {
        (None, None) => {
            return Err(SceneBuildError::MissingSurfaceBackground {
                surface: ready.surface,
            });
        }
        (Some(_), Some(background)) => {
            return Err(SceneBuildError::UnexpectedSurfaceBackground {
                surface: ready.surface,
                target: background.id(),
            });
        }
        (Some(_), None) => return Ok(()),
        (None, Some(_)) => {}
    }

    let background =
        ready
            .surface_background
            .as_ref()
            .ok_or(SceneBuildError::MissingSurfaceBackground {
                surface: ready.surface,
            })?;
    if background.id().surface() != ready.surface
        || !background.semantics_match()
        || background.availability() != DropTargetAvailability::Available
        || !matches!(
            background.destination(),
            DropDestination::SurfaceBackground(destination)
                if destination.surface() == ready.surface
        )
    {
        return Err(SceneBuildError::InvalidSurfaceBackgroundRecord {
            surface: ready.surface,
            target: background.id(),
        });
    }
    if background.layer() != SceneLayerKey::surface_base()
        || workspace_index
            .contained_layers(ready.surface)
            .any(|layer| background.layer() >= layer)
    {
        return Err(SceneBuildError::SurfaceBackgroundLayerMismatch {
            surface: ready.surface,
            actual: background.layer(),
        });
    }
    if !rect_has_area(background.region().rect())
        || !rect_contains(ready.bounds, background.region().rect())
    {
        return Err(SceneBuildError::InvalidSurfaceBackgroundHitRegion {
            surface: ready.surface,
        });
    }
    validate_drop_visual(ready, background)
}

fn validate_ready_semantics(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) -> Result<(), SceneBuildError> {
    validate_contained_records(ready, workspace, workspace_index)?;
    let expected_panes = expected_pane_records(ready, workspace, workspace_index)?;
    validate_pane_and_tab_records(ready, workspace, workspace_index, policy, &expected_panes)?;
    validate_splitter_records(ready, workspace, workspace_index, policy)?;
    Ok(())
}

fn validate_contained_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let expected = workspace
        .surface(ready.surface)
        .into_iter()
        .flat_map(|presentation| presentation.contained.iter().copied().enumerate())
        .filter_map(|(ordinal, floating)| {
            let indexed = workspace_index.contained.get(&floating)?;
            rects_overlap_with_area(ready.bounds, indexed.rect).then_some((
                floating,
                ordinal,
                indexed.root,
                indexed.rect,
                indexed.layer,
            ))
        })
        .collect::<Vec<_>>();
    if ready.contained_records.len() != expected.len() {
        return Err(SceneBuildError::ContainedRecordSetMismatch {
            surface: ready.surface,
        });
    }
    let minimums = ready
        .contained_minimums
        .iter()
        .map(|measurement| (measurement.floating(), measurement.minimum_size()))
        .collect::<HashMap<_, _>>();
    const DIRECTIONS: [ContainedResizeDirection; 8] = [
        ContainedResizeDirection::NorthWest,
        ContainedResizeDirection::North,
        ContainedResizeDirection::NorthEast,
        ContainedResizeDirection::East,
        ContainedResizeDirection::SouthEast,
        ContainedResizeDirection::South,
        ContainedResizeDirection::SouthWest,
        ContainedResizeDirection::West,
    ];
    for (record, (floating, ordinal, root, durable, layer)) in
        ready.contained_records.iter().zip(expected)
    {
        let valid_identity = record.floating() == floating
            && record.ordinal() == ordinal
            && record.root() == root
            && record.layer() == layer
            && minimums.get(&floating).copied() == Some(record.minimum_size());
        let valid_partition =
            rect_is_exact_intersection(record.outer_bounds(), ready.bounds, durable)
                && rect_has_area(record.outer_bounds())
                && rect_contains(record.outer_bounds(), record.inner_bounds())
                && rect_contains(record.inner_bounds(), record.title_bounds())
                && rect_contains(record.inner_bounds(), record.content_bounds())
                && rect_contains(record.title_bounds(), record.title_drag_hit().rect())
                && !rects_overlap_with_area(record.title_bounds(), record.content_bounds())
                && record.close_bounds().is_none_or(|close| {
                    rect_has_area(close)
                        && rect_contains(record.title_bounds(), close)
                        && !rects_overlap_with_area(close, record.title_drag_hit().rect())
                });
        let valid_resize =
            record
                .resize()
                .iter()
                .zip(DIRECTIONS)
                .all(|(resize, direction)| {
                    resize.direction() == direction
                        && rect_contains(record.outer_bounds(), resize.hit().rect())
                })
                && record.resize().iter().enumerate().all(|(index, first)| {
                    record.resize().iter().skip(index + 1).all(|second| {
                        !rects_overlap_with_area(first.hit().rect(), second.hit().rect())
                    })
                });
        if !valid_identity || !valid_partition || !valid_resize {
            return Err(SceneBuildError::InvalidContainedRecord {
                surface: ready.surface,
                floating: record.floating(),
            });
        }
    }
    Ok(())
}

fn expected_pane_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<BTreeMap<PaneSceneId, (Option<ItemId>, SceneLayerKey)>, SceneBuildError> {
    let mut roots = Vec::new();
    if let Some(root) = workspace
        .surface(ready.surface)
        .and_then(|presentation| presentation.main_root)
    {
        roots.push(root);
    }
    roots.extend(
        ready
            .contained_records
            .iter()
            .filter(|record| rect_has_area(record.content_bounds()))
            .map(ContainedRecord::root),
    );
    let mut expected = BTreeMap::new();
    for root in roots {
        let layer =
            workspace_index
                .root_layer(root)
                .ok_or(SceneBuildError::CompiledRootUnavailable {
                    surface: ready.surface,
                    root,
                })?;
        let mut pending = vec![workspace_index.root_node(root).ok_or(
            SceneBuildError::CompiledRootUnavailable {
                surface: ready.surface,
                root,
            },
        )?];
        while let Some(node) = pending.pop() {
            match workspace.node(node) {
                Some(Node::Tabs { selected, .. }) => {
                    expected.insert(PaneSceneId { root, tabs: node }, (*selected, layer));
                }
                Some(Node::Split { children, .. }) => {
                    pending.extend(children.iter().rev().copied());
                }
                None => {
                    return Err(SceneBuildError::CompiledRootUnavailable {
                        surface: ready.surface,
                        root,
                    });
                }
            }
        }
    }
    Ok(expected)
}

fn validate_pane_and_tab_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
    expected_panes: &BTreeMap<PaneSceneId, (Option<ItemId>, SceneLayerKey)>,
) -> Result<(), SceneBuildError> {
    let panes = ready
        .pane_records
        .iter()
        .map(|record| (record.id(), record))
        .collect::<BTreeMap<_, _>>();
    if panes.len() != expected_panes.len() {
        return Err(SceneBuildError::PaneRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, (selected, layer)) in expected_panes {
        let Some(record) = panes.get(id).copied() else {
            return Err(SceneBuildError::PaneRecordSetMismatch {
                surface: ready.surface,
            });
        };
        if record.selected() != *selected
            || record.layer() != *layer
            || !rect_contains(ready.bounds, record.bounds())
            || !rect_contains(record.bounds(), record.content_bounds())
        {
            return Err(SceneBuildError::InvalidPaneRecord {
                surface: ready.surface,
                id: *id,
            });
        }
    }

    let pane_policies = expected_panes
        .iter()
        .map(|(id, (selected, _))| {
            let target = selected
                .map(|_| workspace.capture_tab_target(id.root, id.tabs))
                .transpose()
                .map_err(|_| SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                })?;
            if target
                .as_ref()
                .is_some_and(|target| target.surface() != ready.surface)
            {
                return Err(SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                });
            }
            Ok((
                *id,
                policy.tab_bar_policy(DockTabBarPolicyRequest::new(
                    ready.surface,
                    target.as_ref().map(|target| target.rule()),
                )),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, SceneBuildError>>()?;
    let bars = ready
        .tab_bar_records
        .iter()
        .map(|record| (*record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let mut controls_by_bar = BTreeMap::<TabBarSceneId, BTreeMap<TabStripControlId, _>>::new();
    for control in &ready.tab_strip_control_records {
        let bar = control.id().bar();
        if controls_by_bar
            .entry(bar)
            .or_default()
            .insert(control.id(), control)
            .is_some()
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar,
            });
        }
    }
    let mut menus_by_bar = BTreeMap::new();
    for menu in &ready.tab_list_menu_records {
        if menus_by_bar.insert(menu.bar(), menu).is_some() {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: menu.bar(),
            });
        }
    }
    if ready.tab_list_menu_records.len() > 1 {
        let id = ready.tab_list_menu_records[0].bar();
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    if controls_by_bar.keys().any(|id| !bars.contains_key(id))
        || menus_by_bar.keys().any(|id| !bars.contains_key(id))
    {
        let id = controls_by_bar
            .keys()
            .chain(menus_by_bar.keys())
            .find(|id| !bars.contains_key(id))
            .copied()
            .expect("a foreign tab-strip record was detected");
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    let expected_bars = panes
        .iter()
        .filter(|(id, pane)| {
            rect_has_area(pane.bounds())
                && pane_policies
                    .get(id)
                    .is_some_and(|policy| policy.visibility() == TabBarVisibility::Visible)
        })
        .count();
    if bars.len() != expected_bars {
        return Err(SceneBuildError::TabBarRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, pane) in &panes {
        let bar_id = TabBarSceneId {
            root: id.root,
            tabs: id.tabs,
        };
        let tab_bar_policy =
            pane_policies
                .get(id)
                .copied()
                .ok_or(SceneBuildError::InvalidPaneRecord {
                    surface: ready.surface,
                    id: *id,
                })?;
        if !rect_has_area(pane.bounds()) || tab_bar_policy.visibility() == TabBarVisibility::Hidden
        {
            if bars.contains_key(&bar_id) {
                return Err(SceneBuildError::InvalidTabBarRecord {
                    surface: ready.surface,
                    id: bar_id,
                });
            }
            continue;
        }
        let Some(bar) = bars.get(&bar_id).copied() else {
            return Err(SceneBuildError::TabBarRecordSetMismatch {
                surface: ready.surface,
            });
        };
        if bar.layer() != pane.layer()
            || bar.interaction() != tab_bar_policy.interaction()
            || !rect_has_area(bar.bounds())
            || !rect_contains(pane.bounds(), bar.bounds())
            || !rect_contains(bar.bounds(), bar.viewport())
            || !bar.scroll_offset().is_finite()
            || !bar.maximum_scroll_offset().is_finite()
            || bar.scroll_offset() < 0.0
            || bar.maximum_scroll_offset() < 0.0
            || bar.scroll_offset() > bar.maximum_scroll_offset()
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
    }

    let tabs_by_bar = ready.tab_records.iter().fold(
        BTreeMap::<TabBarSceneId, Vec<&TabRecord>>::new(),
        |mut grouped, record| {
            grouped
                .entry(TabBarSceneId {
                    root: record.id().root,
                    tabs: record.id().tabs,
                })
                .or_default()
                .push(record);
            grouped
        },
    );
    if tabs_by_bar.keys().any(|id| !bars.contains_key(id)) {
        return Err(SceneBuildError::TabRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (bar_id, bar) in bars {
        let Some(Node::Tabs { items, selected }) = workspace.node(bar_id.tabs) else {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        };
        let group_grip_matches = match bar.group_grip_bounds() {
            None => items.is_empty(),
            Some(grip) => {
                !items.is_empty()
                    && rect_has_area(grip)
                    && grip.x() == bar.bounds().x()
                    && grip.y() == bar.bounds().y()
                    && grip.height() == bar.bounds().height()
                    && rect_contains(bar.bounds(), grip)
                    && !rects_overlap_with_area(grip, bar.viewport())
            }
        };
        let group_interaction_matches = match bar.interaction() {
            TabBarInteraction::Disabled => bar.group_drag().is_none(),
            TabBarInteraction::Enabled => match (bar.group_grip_bounds(), bar.group_drag()) {
                (None, None) => items.is_empty(),
                (Some(grip), Some(group)) => {
                    group.grip_bounds() == grip
                        && group.hit().rect() == grip
                        && rect_has_area(group.hit().rect())
                }
                _ => false,
            },
        };
        if !group_grip_matches || !group_interaction_matches {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
        let visible = tabs_by_bar.get(&bar_id).map_or(&[][..], Vec::as_slice);
        let visible_items = visible
            .iter()
            .map(|record| record.id().item)
            .collect::<HashSet<_>>();
        let expected_hidden = items
            .iter()
            .copied()
            .filter(|item| !visible_items.contains(item))
            .collect::<Vec<_>>();
        if bar.hidden_items() != expected_hidden {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }
        let members_match = bar.members().len() == items.len()
            && bar.members().iter().enumerate().all(|(ordinal, member)| {
                let tab = member.tab();
                let visible_record = visible.iter().find(|record| *record.id() == tab);
                let visibility_matches = match (member.visibility(), visible_record) {
                    (TabStripMemberVisibility::Visible, Some(record)) => {
                        member.full_bounds() == record.full_bounds()
                            && record.full_bounds() == record.visible_bounds()
                    }
                    (TabStripMemberVisibility::PartiallyVisible, Some(record)) => {
                        member.full_bounds() == record.full_bounds()
                            && record.full_bounds() != record.visible_bounds()
                    }
                    (TabStripMemberVisibility::PartiallyVisible, None) => true,
                    (TabStripMemberVisibility::Hidden, None) => true,
                    (TabStripMemberVisibility::Visible, None)
                    | (TabStripMemberVisibility::Hidden, Some(_)) => false,
                };
                let full = member.full_bounds();
                let preceding_edge_matches = if ordinal == 0 {
                    full.x() == bar.viewport().x() - bar.scroll_offset()
                } else {
                    bar.members()[ordinal - 1].full_bounds().max().x() == full.x()
                };
                member.ordinal() == ordinal
                    && tab.root == bar_id.root
                    && tab.tabs == bar_id.tabs
                    && items.get(ordinal).copied() == Some(tab.item)
                    && rect_has_area(full)
                    && full.y() == bar.bounds().y()
                    && full.height() == bar.bounds().height()
                    && preceding_edge_matches
                    && visibility_matches
            });
        if !members_match {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id: bar_id,
            });
        }

        validate_tab_strip_controls_and_menu(
            ready,
            bar,
            items,
            *selected,
            controls_by_bar.get(&bar_id),
            menus_by_bar.get(&bar_id).copied(),
        )?;
        for record in visible {
            let id = *record.id();
            let ordinal_matches = items.get(record.ordinal()).copied() == Some(id.item);
            let close_allowed = policy.pane_close_capability(id.item).allows_close();
            let close_visual_matches = match (close_allowed, record.close_visual_bounds()) {
                (false, None) => true,
                (true, Some(close)) => {
                    rect_has_area(close) && rect_contains(record.visible_bounds(), close)
                }
                _ => false,
            };
            let interaction_geometry_matches = match bar.interaction() {
                TabBarInteraction::Enabled => {
                    rect_has_area(record.drag_hit().rect())
                        && rect_contains(record.visible_bounds(), record.drag_hit().rect())
                        && record.close_bounds() == record.close_visual_bounds()
                        && record.close_bounds().is_none_or(|close| {
                            !rects_overlap_with_area(close, record.drag_hit().rect())
                        })
                }
                TabBarInteraction::Disabled => {
                    !rect_has_area(record.drag_hit().rect()) && record.close_bounds().is_none()
                }
            };
            let geometry_matches = rect_has_area(record.visible_bounds())
                && rect_is_exact_intersection(
                    record.visible_bounds(),
                    record.full_bounds(),
                    bar.viewport(),
                )
                && rect_contains(record.visible_bounds(), record.text_bounds())
                && close_visual_matches
                && interaction_geometry_matches;
            if id.root != bar_id.root
                || id.tabs != bar_id.tabs
                || !ordinal_matches
                || record.selected() != (*selected == Some(id.item))
                || record.layer() != bar.layer()
                || !geometry_matches
                || !workspace_index.semantic_node_exists(ready.surface, id.root, id.tabs)
            {
                return Err(SceneBuildError::InvalidTabRecord {
                    surface: ready.surface,
                    id,
                });
            }
        }
    }
    Ok(())
}

fn validate_tab_strip_controls_and_menu(
    ready: &PresentationPlan,
    bar: &TabBarRecord,
    items: &[ItemId],
    selected: Option<ItemId>,
    controls: Option<&BTreeMap<TabStripControlId, &TabStripControlRecord>>,
    menu: Option<&TabListMenuRecord>,
) -> Result<(), SceneBuildError> {
    let id = *bar.id();
    let expected_control_ids = TabStripControlId::for_bar(id);
    if let Some(controls) = controls {
        if controls.is_empty()
            || controls.len() > expected_control_ids.len()
            || controls
                .keys()
                .any(|control| !expected_control_ids.contains(control))
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
        let records = controls.values().copied().collect::<Vec<_>>();
        let valid_geometry = records.iter().all(|control| {
            rect_has_area(control.bounds())
                && control.hit().rect() == control.bounds()
                && rect_contains(bar.bounds(), control.bounds())
                && control.layer() == bar.layer()
        }) && records.iter().enumerate().all(|(index, control)| {
            records[index + 1..]
                .iter()
                .all(|other| !rects_overlap_with_area(control.bounds(), other.bounds()))
        });
        let interaction_enabled = bar.interaction() == TabBarInteraction::Enabled;
        let valid_enablement = records.iter().all(|control| {
            let expected = match control.id() {
                TabStripControlId::ScrollBackward(_) => {
                    interaction_enabled && bar.scroll_offset() > 0.0
                }
                TabStripControlId::ScrollForward(_) => {
                    interaction_enabled && bar.scroll_offset() < bar.maximum_scroll_offset()
                }
                TabStripControlId::TabListMenu(_) => {
                    interaction_enabled && bar.menu_geometry_availability().is_available()
                }
            };
            control.enabled() == expected
        });
        let menu_control_present = controls.contains_key(&TabStripControlId::TabListMenu(id));
        if !valid_geometry || !valid_enablement || bar.maximum_scroll_offset() <= 0.0 {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
        if (bar.menu_geometry_availability().is_available() && !menu_control_present)
            || (menu.is_some() && !menu_control_present)
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
    } else if menu.is_some() || bar.menu_geometry_availability().is_available() {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }

    let Some(menu) = menu else {
        return Ok(());
    };
    let Some(popup_plane_bounds) = ready.popup_plane_bounds else {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    };
    let menu_control = controls
        .and_then(|controls| controls.get(&TabStripControlId::TabListMenu(id)))
        .copied();
    let valid_menu = bar.interaction() == TabBarInteraction::Enabled
        && bar.menu_geometry_availability().is_available()
        && menu.layer() == bar.layer()
        && menu.session().key() == TabStripStateKey::new(ready.surface, id)
        && menu_control.is_some_and(|control| rect_contains(popup_plane_bounds, control.bounds()))
        && rect_has_area(menu.bounds())
        && rect_contains(popup_plane_bounds, menu.bounds())
        && rect_has_area(menu.viewport())
        && rect_contains(menu.bounds(), menu.viewport())
        && menu.scroll_offset().is_finite()
        && menu.maximum_scroll_offset().is_finite()
        && menu.scroll_offset() >= 0.0
        && menu.maximum_scroll_offset() >= 0.0
        && menu.scroll_offset() <= menu.maximum_scroll_offset()
        && menu.rows().len() == items.len();
    if !valid_menu {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }

    let mut focused = 0_usize;
    for (ordinal, row) in menu.rows().iter().copied().enumerate() {
        let tab = row.tab();
        focused += usize::from(row.focused());
        let hit_matches = match row.hit() {
            Some(hit) => {
                rect_has_area(hit.rect())
                    && rect_contains(row.bounds(), hit.rect())
                    && rect_contains(menu.viewport(), hit.rect())
                    && rect_is_exact_intersection(hit.rect(), row.bounds(), menu.viewport())
            }
            None => !rects_overlap_with_area(row.bounds(), menu.viewport()),
        };
        if row.ordinal() != ordinal
            || items.get(ordinal).copied() != Some(tab.item)
            || tab.root != id.root
            || tab.tabs != id.tabs
            || !rect_has_area(row.bounds())
            || row.bounds().x() != menu.viewport().x()
            || row.bounds().width() != menu.viewport().width()
            || row.selected() != (selected == Some(tab.item))
            || !hit_matches
        {
            return Err(SceneBuildError::InvalidTabBarRecord {
                surface: ready.surface,
                id,
            });
        }
    }
    if focused != 1 {
        return Err(SceneBuildError::InvalidTabBarRecord {
            surface: ready.surface,
            id,
        });
    }
    Ok(())
}

fn splitter_extent_satisfies_constraints(extent: Option<f64>, minimum: f64, maximum: f64) -> bool {
    let Some(extent) = extent else {
        return false;
    };
    if !extent.is_finite()
        || extent < 0.0
        || !minimum.is_finite()
        || minimum < 0.0
        || !maximum.is_finite()
        || maximum < minimum
    {
        return false;
    }
    let tolerance =
        f64::EPSILON * extent.abs().max(minimum.abs()).max(maximum.abs()).max(1.0) * 16.0;
    (extent >= minimum || minimum - extent <= tolerance)
        && (extent <= maximum || extent - maximum <= tolerance)
}

fn validate_splitter_records(
    ready: &PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) -> Result<(), SceneBuildError> {
    let splitters = ready
        .splitter_records
        .iter()
        .map(|record| (*record.id(), record))
        .collect::<BTreeMap<_, _>>();
    let splitter_gaps = ready
        .splitter_gap_records
        .iter()
        .map(|record| (record.id(), record.presentation()))
        .collect::<BTreeMap<_, _>>();
    let compiled_roots = ready
        .pane_records
        .iter()
        .map(|pane| pane.id().root)
        .collect::<BTreeSet<_>>();
    let mut expected_splitters = BTreeSet::new();
    for root in compiled_roots {
        let Some(root_record) = workspace.root(root) else {
            return Err(SceneBuildError::CompiledRootUnavailable {
                surface: ready.surface,
                root,
            });
        };
        let mut pending = vec![root_record.node];
        let mut visited = BTreeSet::new();
        while let Some(node) = pending.pop() {
            if !visited.insert(node) {
                continue;
            }
            match workspace.node(node) {
                Some(Node::Tabs { .. }) => {}
                Some(Node::Split { children, .. }) => {
                    expected_splitters.extend((0..children.len().saturating_sub(1)).map(|index| {
                        SplitterSceneId {
                            root,
                            split: node,
                            index,
                        }
                    }));
                    pending.extend(children.iter().rev().copied());
                }
                None => {
                    return Err(SceneBuildError::CompiledRootUnavailable {
                        surface: ready.surface,
                        root,
                    });
                }
            }
        }
    }
    if splitter_gaps.keys().copied().collect::<BTreeSet<_>>() != expected_splitters {
        return Err(SceneBuildError::SplitterGapRecordSetMismatch {
            surface: ready.surface,
        });
    }
    let rendered_splitters = splitter_gaps
        .iter()
        .filter_map(|(id, presentation)| {
            (*presentation == SplitterGapPresentation::Rendered).then_some(*id)
        })
        .collect::<BTreeSet<_>>();
    if splitters.keys().copied().collect::<BTreeSet<_>>() != rendered_splitters {
        return Err(SceneBuildError::SplitterRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for (id, record) in &splitters {
        let Some(Node::Split { axis, children, .. }) = workspace.node(id.split) else {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        };
        let weight_sum = record
            .weights()
            .iter()
            .map(|weight| f64::from(weight.get()))
            .sum::<f64>();
        let before_extent = record.child_extents().get(id.index).copied();
        let after_extent = record.child_extents().get(id.index + 1).copied();
        if !workspace_index.semantic_node_exists(ready.surface, id.root, id.split)
            || id.index >= children.len().saturating_sub(1)
            || record.axis() != *axis
            || record.weights().len() != children.len()
            || !weight_sum.is_finite()
            || (weight_sum - 1.0).abs() > 1.0e-5
            || !rect_has_area(record.draw_bounds())
            || !rect_has_area(record.hit().rect())
            || !rect_contains(ready.bounds, record.hit().rect())
            || !rect_contains(record.hit().rect(), record.draw_bounds())
            || !rect_contains(ready.bounds, record.before_bounds())
            || !rect_contains(ready.bounds, record.after_bounds())
            || !record.before_minimum_extent().is_finite()
            || record.before_minimum_extent() < 0.0
            || !record.after_minimum_extent().is_finite()
            || record.after_minimum_extent() < 0.0
            || !splitter_extent_satisfies_constraints(
                before_extent,
                record.before_minimum_extent(),
                record.before_maximum_extent(),
            )
            || !splitter_extent_satisfies_constraints(
                after_extent,
                record.after_minimum_extent(),
                record.after_maximum_extent(),
            )
            || record.child_extents().len() != children.len()
            || record
                .child_extents()
                .iter()
                .any(|extent| !extent.is_finite() || *extent < 0.0)
            || record
                .central_index()
                .is_some_and(|index| index >= children.len())
        {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        }
        validate_semantic_layer(ready.surface, id.root, record.layer(), workspace_index)?;
        if record.operable()
            != (policy.splitter_resize_is_allowed(record.axis(), ready.surface)
                && region_has_authoritative_area(
                    record.hit().rect(),
                    record.layer(),
                    &ready.drop_occlusions,
                ))
        {
            return Err(SceneBuildError::InvalidSplitterRecord {
                surface: ready.surface,
                id: *id,
            });
        }
    }

    let expected_junctions = derive_splitter_junction_candidates(splitters.values().copied())
        .map_err(|source| SceneBuildError::InvalidSplitterJunctionGeometry {
            surface: ready.surface,
            source,
        })?
        .into_iter()
        .filter(|candidate| {
            region_has_authoritative_area(candidate.hit, candidate.layer, &ready.drop_occlusions)
        })
        .map(|candidate| (candidate.id(), candidate))
        .collect::<BTreeMap<_, _>>();
    if ready.splitter_junction_records.len() != expected_junctions.len() {
        return Err(SceneBuildError::SplitterJunctionRecordSetMismatch {
            surface: ready.surface,
        });
    }
    for junction in &ready.splitter_junction_records {
        let Some(expected) = expected_junctions.get(&junction.id()).copied() else {
            return Err(SceneBuildError::InvalidSplitterJunctionRecord {
                surface: ready.surface,
                id: junction.id(),
            });
        };
        if junction.layer() != expected.layer
            || !rect_has_area(junction.hit().rect())
            || junction.hit().rect() != expected.hit
        {
            return Err(SceneBuildError::InvalidSplitterJunctionRecord {
                surface: ready.surface,
                id: junction.id(),
            });
        }
    }
    Ok(())
}

fn validate_semantic_layer(
    surface: SurfaceId,
    root: RootId,
    actual: SceneLayerKey,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let expected =
        workspace_index
            .root_layer(root)
            .ok_or(SceneBuildError::SemanticLayerMismatch {
                surface,
                root,
                expected: SceneLayerKey::surface_base(),
                actual,
            })?;
    if actual != expected {
        return Err(SceneBuildError::SemanticLayerMismatch {
            surface,
            root,
            expected,
            actual,
        });
    }
    Ok(())
}

fn canonicalize_ready(
    ready: &mut PresentationPlan,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    policy: &DockPolicySnapshot,
) {
    ready.pane_records.sort_unstable_by_key(PaneRecord::id);
    ready
        .tab_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .tab_bar_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .tab_strip_control_records
        .sort_unstable_by_key(|record| record.id());
    ready
        .tab_list_menu_records
        .sort_unstable_by_key(TabListMenuRecord::session);
    ready
        .splitter_gap_records
        .sort_unstable_by_key(SplitterGapRecord::id);
    ready
        .splitter_records
        .sort_unstable_by_key(|record| *record.id());
    ready
        .splitter_junction_records
        .sort_unstable_by_key(SplitterJunctionRecord::id);
    ready
        .contained_records
        .sort_unstable_by_key(|record| record.ordinal());
    ready
        .contained_minimums
        .sort_unstable_by_key(|measurement| measurement.floating());
    ready
        .drop_occlusions
        .sort_unstable_by_key(|occlusion| occlusion.floating());
    ready
        .drop_guide_clusters
        .sort_unstable_by_key(DropGuideClusterRecord::id);
    ready
        .drop_targets
        .sort_unstable_by_key(DropTargetRecord::id);

    let surface = ready.surface;
    for target in &mut ready.drop_targets {
        canonicalize_drop_target(target, workspace, workspace_index, surface, policy);
    }
    for cluster in &mut ready.drop_guide_clusters {
        cluster.for_each_target_mut(|_, guide_target| {
            canonicalize_drop_target(
                guide_target.target_mut(),
                workspace,
                workspace_index,
                surface,
                policy,
            );
        });
    }
}

fn canonicalize_drop_target(
    target: &mut DropTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    surface: SurfaceId,
    policy: &DockPolicySnapshot,
) {
    if !target.availability().is_available() {
        return;
    }
    if !target_reference_is_current(workspace, workspace_index, target)
        || !target_structure_is_valid(workspace, workspace_index, surface, target)
    {
        target.set_availability(DropTargetAvailability::Unavailable(
            DropTargetUnavailable::Stale,
        ));
        return;
    }
    let allowed = match target.id().kind() {
        DropTargetKind::TabGap | DropTargetKind::Center => policy.allows_tab_merge(),
        DropTargetKind::InnerEdge | DropTargetKind::OuterEdge => policy.allows_edge_split(),
        DropTargetKind::SurfaceBackground => true,
    };
    if !allowed {
        target.set_availability(DropTargetAvailability::Unavailable(
            DropTargetUnavailable::PolicyDisabled,
        ));
    }
}

fn validate_drop_guide_cluster(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let cluster_id = cluster.id();
    validate_drop_guide_cluster_identity(ready, cluster_id, workspace, workspace_index)?;
    let expected_layer = workspace_index.root_layer(cluster_id.root).ok_or(
        SceneBuildError::InvalidDropGuideCluster {
            surface: ready.surface,
            cluster: cluster_id,
        },
    )?;
    if cluster.layer() != expected_layer {
        return Err(SceneBuildError::DropGuideClusterLayerMismatch {
            surface: ready.surface,
            cluster: cluster_id,
            expected: expected_layer,
            actual: cluster.layer(),
        });
    }
    let activation = validate_drop_guide_activation(ready, cluster)?;
    for (slot, guide_target) in cluster.targets() {
        validate_drop_guide_target(
            ready,
            cluster,
            activation,
            slot,
            guide_target,
            workspace,
            workspace_index,
        )?;
    }
    validate_drop_guide_hit_separation(ready.surface, cluster)
}

fn validate_drop_guide_cluster_identity(
    ready: &PresentationPlan,
    cluster: DropGuideClusterId,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let scope_is_valid = match cluster.scope {
        DropGuideScope::Inner(node) => {
            workspace_index.semantic_node_exists(ready.surface, cluster.root, node)
                && matches!(workspace.node(node), Some(Node::Tabs { .. }))
        }
        DropGuideScope::Outer => {
            workspace_index.root_belongs_to_surface(ready.surface, cluster.root)
                && workspace_index
                    .root_node(cluster.root)
                    .and_then(|node| workspace.node(node))
                    .is_some()
        }
    };
    if cluster.surface != ready.surface || !scope_is_valid {
        return Err(SceneBuildError::InvalidDropGuideCluster {
            surface: ready.surface,
            cluster,
        });
    }
    Ok(())
}

fn validate_drop_guide_activation(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
) -> Result<LogicalRect, SceneBuildError> {
    let cluster_id = cluster.id();
    let activation = cluster.activation().rect();
    if !rect_has_area(activation) {
        return Err(SceneBuildError::EmptyDropGuideActivation {
            surface: ready.surface,
            cluster: cluster_id,
        });
    }
    if !rect_contains(ready.bounds, activation) {
        return Err(SceneBuildError::DropGuideActivationOutsideSurface {
            surface: ready.surface,
            cluster: cluster_id,
        });
    }
    Ok(activation)
}

fn validate_drop_guide_target(
    ready: &PresentationPlan,
    cluster: &DropGuideClusterRecord,
    activation: LogicalRect,
    slot: DropGuideSlot,
    guide_target: &DropGuideTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    let cluster_id = cluster.id();
    let target = guide_target.target();
    if !drop_guide_slot_matches(cluster_id, slot, target.id(), workspace, workspace_index) {
        return Err(SceneBuildError::DropGuideTargetSlotMismatch {
            cluster: cluster_id,
            slot,
            target: target.id(),
        });
    }
    if target.layer() != cluster.layer() {
        return Err(SceneBuildError::DropGuideTargetLayerMismatch {
            cluster: cluster_id,
            target: target.id(),
            cluster_layer: cluster.layer(),
            target_layer: target.layer(),
        });
    }

    let hit = target.region().rect();
    if !rect_has_area(hit) {
        return Err(SceneBuildError::EmptyDropGuideHitRegion {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(ready.bounds, hit) {
        return Err(SceneBuildError::DropGuideHitRegionOutsideSurface {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(activation, hit) {
        return Err(SceneBuildError::DropGuideHitRegionOutsideActivation {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }

    let draw = guide_target.draw();
    if !rect_has_area(draw) {
        return Err(SceneBuildError::EmptyDropGuideDraw {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    if !rect_contains(hit, draw) {
        return Err(SceneBuildError::DropGuideDrawOutsideHitRegion {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }

    validate_drop_target(ready, target, workspace, workspace_index)?;
    let preview = target.visual().rect();
    if !rect_contains(activation, preview) {
        return Err(SceneBuildError::DropGuidePreviewOutsideActivation {
            surface: ready.surface,
            cluster: cluster_id,
            target: target.id(),
        });
    }
    Ok(())
}

fn validate_drop_guide_hit_separation(
    surface: SurfaceId,
    cluster: &DropGuideClusterRecord,
) -> Result<(), SceneBuildError> {
    for (index, (first_slot, first)) in cluster.targets().enumerate() {
        for (second_slot, second) in cluster.targets().skip(index + 1) {
            if rects_overlap_with_area(
                first.target().region().rect(),
                second.target().region().rect(),
            ) {
                return Err(SceneBuildError::OverlappingDropGuideHitRegions {
                    surface,
                    cluster: cluster.id(),
                    first: first_slot,
                    second: second_slot,
                });
            }
        }
    }
    Ok(())
}

fn drop_guide_slot_matches(
    cluster: DropGuideClusterId,
    slot: DropGuideSlot,
    target: DropTargetId,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> bool {
    match (cluster.scope, slot) {
        (DropGuideScope::Inner(node), DropGuideSlot::Center) => {
            target
                == (DropTargetId::Center {
                    surface: cluster.surface,
                    root: cluster.root,
                    tabs: node,
                })
        }
        (DropGuideScope::Inner(node), DropGuideSlot::Edge(edge)) => {
            let is_root_central = workspace_index.root_node(cluster.root) == Some(node)
                && workspace
                    .root(cluster.root)
                    .is_some_and(|root| root.central == Some(node));
            !is_root_central
                && target
                    == (DropTargetId::InnerEdge {
                        surface: cluster.surface,
                        root: cluster.root,
                        node,
                        edge,
                    })
        }
        (DropGuideScope::Outer, DropGuideSlot::Center) => false,
        (DropGuideScope::Outer, DropGuideSlot::Edge(edge)) => {
            workspace_index.root_node(cluster.root).is_some_and(|node| {
                target
                    == (DropTargetId::OuterEdge {
                        surface: cluster.surface,
                        root: cluster.root,
                        node,
                        edge,
                    })
            })
        }
    }
}

fn validate_drop_target(
    ready: &PresentationPlan,
    target: &DropTargetRecord,
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
) -> Result<(), SceneBuildError> {
    if matches!(target.destination(), DropDestination::SurfaceBackground(_)) {
        return Err(SceneBuildError::SurfaceBackgroundInTopologyTargets {
            surface: ready.surface,
        });
    }
    validate_drop_visual(ready, target)?;
    let root =
        target_root(target.destination()).ok_or(SceneBuildError::TargetSemanticMismatch {
            surface: ready.surface,
            target: target.id(),
        })?;
    let expected =
        workspace_index
            .root_layer(root)
            .ok_or(SceneBuildError::TargetSemanticMismatch {
                surface: ready.surface,
                target: target.id(),
            })?;
    if target.layer() != expected {
        return Err(SceneBuildError::DropTargetLayerMismatch {
            surface: ready.surface,
            target: target.id(),
            expected,
            actual: target.layer(),
        });
    }
    if target.id().surface() != ready.surface
        || !target.semantics_match()
        || (target_reference_is_current(workspace, workspace_index, target)
            && !target_structure_is_valid(workspace, workspace_index, ready.surface, target))
    {
        return Err(SceneBuildError::TargetSemanticMismatch {
            surface: ready.surface,
            target: target.id(),
        });
    }
    Ok(())
}

fn rect_has_area(rect: LogicalRect) -> bool {
    rect.width() > 0.0 && rect.height() > 0.0
}

fn rect_contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    let outer_min = outer.min();
    let outer_max = outer.max();
    let inner_min = inner.min();
    let inner_max = inner.max();
    inner_min.x() >= outer_min.x()
        && inner_min.y() >= outer_min.y()
        && inner_max.x() <= outer_max.x()
        && inner_max.y() <= outer_max.y()
}

fn rects_overlap_with_area(left: LogicalRect, right: LogicalRect) -> bool {
    left.min().x().max(right.min().x()) < left.max().x().min(right.max().x())
        && left.min().y().max(right.min().y()) < left.max().y().min(right.max().y())
}

fn region_has_authoritative_area(
    region: LogicalRect,
    layer: SceneLayerKey,
    occlusions: &[DropOcclusionRecord],
) -> bool {
    let occluders = occlusions
        .iter()
        .filter(|occlusion| occlusion.layer() > layer)
        .map(|occlusion| occlusion.region().rect())
        .filter(|occlusion| rects_overlap_with_area(region, *occlusion))
        .collect::<Vec<_>>();
    if occluders.is_empty() {
        return true;
    }

    let mut x_edges = vec![region.x(), region.max().x()];
    for occluder in &occluders {
        x_edges.push(occluder.x().max(region.x()));
        x_edges.push(occluder.max().x().min(region.max().x()));
    }
    x_edges.sort_by(|left, right| left.total_cmp(right));
    x_edges.dedup_by(|left, right| left.total_cmp(right).is_eq());

    for strip in x_edges.windows(2) {
        let [strip_min, strip_max] = [strip[0], strip[1]];
        if strip_min >= strip_max {
            continue;
        }
        let mut intervals = occluders
            .iter()
            .filter(|occluder| occluder.x() <= strip_min && occluder.max().x() >= strip_max)
            .map(|occluder| {
                (
                    occluder.y().max(region.y()),
                    occluder.max().y().min(region.max().y()),
                )
            })
            .collect::<Vec<_>>();
        intervals.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.total_cmp(&right.1))
        });

        let mut covered_to = region.y();
        for (minimum, maximum) in intervals {
            if minimum > covered_to {
                return true;
            }
            covered_to = covered_to.max(maximum);
            if covered_to >= region.max().y() {
                break;
            }
        }
        if covered_to < region.max().y() {
            return true;
        }
    }
    false
}

fn rect_is_exact_intersection(
    actual: LogicalRect,
    first: LogicalRect,
    second: LogicalRect,
) -> bool {
    let min_x = first.x().max(second.x());
    let min_y = first.y().max(second.y());
    let max_x = first.max().x().min(second.max().x());
    let max_y = first.max().y().min(second.max().y());
    max_x > min_x
        && max_y > min_y
        && actual.x() == min_x
        && actual.y() == min_y
        && actual.max().x() == max_x
        && actual.max().y() == max_y
}

fn target_reference_is_current(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    record: &DropTargetRecord,
) -> bool {
    match record.destination() {
        DropDestination::Topology(
            DockTarget::Center(target) | DockTarget::TabGap { target, .. },
        ) => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.tabs())
                && matches!(workspace.node(target.tabs()), Some(Node::Tabs { .. }))
        }
        DropDestination::Topology(
            DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target),
        ) => {
            workspace_index.fingerprint_is_current(target.root(), target.fingerprint())
                && workspace_index.contains_node(target.root(), target.node())
        }
        DropDestination::SurfaceBackground(_) => true,
    }
}

fn target_structure_is_valid(
    workspace: &Workspace,
    workspace_index: &SceneWorkspaceIndex,
    surface: SurfaceId,
    record: &DropTargetRecord,
) -> bool {
    let Some(root) = target_root(record.destination()) else {
        return false;
    };
    if !workspace_index.root_belongs_to_surface(surface, root) {
        return false;
    }
    match (record.id(), record.destination()) {
        (
            DropTargetId::TabGap { .. },
            DropDestination::Topology(DockTarget::TabGap { target, index }),
        ) => matches!(
            workspace.node(target.tabs()),
            Some(Node::Tabs { items, .. }) if *index <= items.len()
        ),
        (
            DropTargetId::OuterEdge { root, node, .. },
            DropDestination::Topology(DockTarget::OuterEdge(_)),
        ) => workspace_index.root_node(root) == Some(node),
        (
            DropTargetId::InnerEdge { .. },
            DropDestination::Topology(DockTarget::InnerEdge(target)),
        ) => {
            matches!(workspace.node(target.node()), Some(Node::Tabs { .. }))
        }
        _ => true,
    }
}

fn validate_drop_visual(
    ready: &PresentationPlan,
    target: &DropTargetRecord,
) -> Result<(), SceneBuildError> {
    let visual = target.visual().rect();
    if visual.width() <= 0.0 || visual.height() <= 0.0 {
        return Err(SceneBuildError::EmptyDropVisual {
            surface: ready.surface,
            target: target.id(),
        });
    }
    let bounds_min = ready.bounds.min();
    let bounds_max = ready.bounds.max();
    let visual_min = visual.min();
    let visual_max = visual.max();
    if visual_min.x() < bounds_min.x()
        || visual_min.y() < bounds_min.y()
        || visual_max.x() > bounds_max.x()
        || visual_max.y() > bounds_max.y()
    {
        return Err(SceneBuildError::DropVisualOutsideSurface {
            surface: ready.surface,
            target: target.id(),
        });
    }
    Ok(())
}

fn target_root(target: &DropDestination) -> Option<RootId> {
    match target {
        DropDestination::Topology(
            DockTarget::Center(target) | DockTarget::TabGap { target, .. },
        ) => Some(target.root()),
        DropDestination::Topology(
            DockTarget::InnerEdge(target) | DockTarget::OuterEdge(target),
        ) => Some(target.root()),
        DropDestination::SurfaceBackground(_) => None,
    }
}

struct IndexedRoot {
    owner: RootPresentationOwner,
    layer: SceneLayerKey,
    root_node: NodeId,
    nodes: HashSet<NodeId>,
    fingerprint: Option<NodeFingerprint>,
}

#[derive(Debug, Clone, Copy)]
struct IndexedContained {
    surface: SurfaceId,
    root: RootId,
    rect: LogicalRect,
    layer: SceneLayerKey,
}

struct SceneWorkspaceIndex {
    roots: HashMap<RootId, IndexedRoot>,
    contained: HashMap<FloatingPresentationId, IndexedContained>,
    contained_by_surface: HashMap<SurfaceId, Vec<FloatingPresentationId>>,
}

impl SceneWorkspaceIndex {
    fn new(workspace: &Workspace) -> Result<Self, SceneBuildError> {
        let mut root_presentations = HashMap::with_capacity(workspace.roots().count());
        let mut contained = HashMap::with_capacity(workspace.contained_floatings().count());
        let mut contained_by_surface = HashMap::with_capacity(workspace.surfaces().count());
        for (surface, presentation) in workspace.surfaces() {
            if let Some(main_root) = presentation.main_root {
                root_presentations.insert(
                    main_root,
                    (
                        RootPresentationOwner::Main { surface },
                        SceneLayerKey::surface_base(),
                    ),
                );
            }
            for (index, floating) in presentation.contained.iter().copied().enumerate() {
                let layer = SceneLayerKey::contained(index)
                    .ok_or(SceneBuildError::ContainedLayerCapacityExceeded { surface, floating })?;
                if let Some(record) = workspace.contained_floating(floating) {
                    let owner = RootPresentationOwner::Contained { surface, floating };
                    root_presentations.insert(record.root, (owner, layer));
                    contained.insert(
                        floating,
                        IndexedContained {
                            surface,
                            root: record.root,
                            rect: record.rect,
                            layer,
                        },
                    );
                }
            }
            contained_by_surface.insert(surface, presentation.contained.clone());
        }

        let mut roots = HashMap::with_capacity(root_presentations.len());
        for (root, record) in workspace.roots() {
            let Some((owner, layer)) = root_presentations.get(&root).copied() else {
                continue;
            };
            let mut nodes = HashSet::new();
            let mut stack = vec![record.node];
            while let Some(node) = stack.pop() {
                if !nodes.insert(node) {
                    continue;
                }
                if let Some(Node::Split { children, .. }) = workspace.node(node) {
                    stack.extend(children.iter().copied());
                }
            }
            let fingerprint = workspace
                .capture_node_source(root, record.node)
                .ok()
                .map(|source| source.fingerprint().clone());
            roots.insert(
                root,
                IndexedRoot {
                    owner,
                    layer,
                    root_node: record.node,
                    nodes,
                    fingerprint,
                },
            );
        }
        Ok(Self {
            roots,
            contained,
            contained_by_surface,
        })
    }

    fn root_belongs_to_surface(&self, surface: SurfaceId, root: RootId) -> bool {
        self.root_owner(root).is_some_and(|owner| match owner {
            RootPresentationOwner::Main {
                surface: owner_surface,
            }
            | RootPresentationOwner::Contained {
                surface: owner_surface,
                ..
            } => owner_surface == surface,
        })
    }

    fn contains_node(&self, root: RootId, node: NodeId) -> bool {
        self.roots
            .get(&root)
            .is_some_and(|record| record.nodes.contains(&node))
    }

    fn semantic_node_exists(&self, surface: SurfaceId, root: RootId, node: NodeId) -> bool {
        self.root_belongs_to_surface(surface, root) && self.contains_node(root, node)
    }

    fn fingerprint_is_current(&self, root: RootId, expected: &NodeFingerprint) -> bool {
        self.roots
            .get(&root)
            .and_then(|record| record.fingerprint.as_ref())
            .is_some_and(|current| current == expected)
    }

    fn root_node(&self, root: RootId) -> Option<NodeId> {
        self.roots.get(&root).map(|record| record.root_node)
    }

    fn root_layer(&self, root: RootId) -> Option<SceneLayerKey> {
        self.roots.get(&root).map(|record| record.layer)
    }

    fn root_owner(&self, root: RootId) -> Option<RootPresentationOwner> {
        self.roots.get(&root).map(|record| record.owner)
    }

    fn contained_belongs_to_surface(
        &self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    ) -> bool {
        self.contained.get(&floating).is_some_and(|record| {
            record.surface == surface
                && self.root_owner(record.root)
                    == Some(RootPresentationOwner::Contained { surface, floating })
        })
    }

    fn contained_layer(&self, floating: FloatingPresentationId) -> Option<SceneLayerKey> {
        self.contained.get(&floating).map(|record| record.layer)
    }

    fn contained_rect(&self, floating: FloatingPresentationId) -> Option<LogicalRect> {
        self.contained.get(&floating).map(|record| record.rect)
    }

    fn contained_layers(&self, surface: SurfaceId) -> impl Iterator<Item = SceneLayerKey> + '_ {
        self.contained
            .values()
            .filter(move |record| record.surface == surface)
            .map(|record| record.layer)
    }

    fn contained_on_surface(
        &self,
        surface: SurfaceId,
    ) -> impl Iterator<Item = FloatingPresentationId> + '_ {
        self.contained_by_surface
            .get(&surface)
            .into_iter()
            .flatten()
            .copied()
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
