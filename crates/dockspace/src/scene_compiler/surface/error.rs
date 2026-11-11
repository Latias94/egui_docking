//! Typed surface and presentation compilation failures.

use thiserror::Error;

use crate::drop_guide::DropGuideClusterId;
use crate::error::CommandError;
use crate::geometry::{GeometryError, LogicalRect};
use crate::ids::{FloatingPresentationId, NodeId, RootId, SurfaceId};
use crate::layout::{LayoutError, LayoutMetricsError};
use crate::scene::{SceneBuildError, SurfaceProgramBuildError, TabBarSceneId, TabSceneId};
use crate::scene_manifest::{
    ManifestMeasurementError, MeasurementAuthorityError, PaneMinimumKey, PolicyRevision,
    SurfaceMeasurementTicket, TabIntrinsicKey, TabStripKey,
};
use crate::tab_strip::TabStripStateKey;
use crate::transition::WorkspaceVersion;

/// Failure to compile authoritative measurements into one semantic surface scene.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SceneCompilationError {
    /// The validated contribution did not echo the exact requirement ticket.
    #[error("surface measurement ticket {actual:?} does not match requirement {expected:?}")]
    TicketMismatch {
        /// Core-owned requirement ticket.
        expected: SurfaceMeasurementTicket,
        /// Contribution ticket.
        actual: SurfaceMeasurementTicket,
    },
    /// The compiler received a policy snapshot other than the revision frozen by the ticket.
    #[error(
        "surface measurement ticket policy {ticket:?} does not match compiler snapshot {snapshot:?}"
    )]
    PolicyRevisionMismatch {
        /// Exact revision frozen into the core-owned measurement ticket.
        ticket: PolicyRevision,
        /// Revision of the snapshot supplied to compilation.
        snapshot: PolicyRevision,
    },
    /// Measured bounds did not have positive area.
    #[error("surface {surface} measured empty bounds")]
    EmptySurfaceBounds {
        /// Surface whose contribution remains bootstrap.
        surface: SurfaceId,
    },
    /// Measured popup-plane bounds did not have positive area.
    #[error("surface {surface} measured empty popup-plane bounds")]
    EmptyPopupPlaneBounds {
        /// Surface whose active popup cannot become authoritative.
        surface: SurfaceId,
    },
    /// Positive cluster bounds lacked enough representable coordinates for every semantic slot.
    #[error("guide cluster {cluster:?} cannot be represented inside positive bounds {bounds:?}")]
    UnrepresentableGuideGeometry {
        /// Cluster whose complete semantic shape could not be represented.
        cluster: DropGuideClusterId,
        /// Positive placement bounds that failed exact draw and hit construction.
        bounds: LogicalRect,
    },
    /// The ticket named a surface outside the workspace roster.
    #[error("workspace does not contain measured surface {surface}")]
    MissingSurface {
        /// Missing surface.
        surface: SurfaceId,
    },
    /// A surface roster named a missing contained presentation.
    #[error("surface {surface} references missing contained presentation {floating}")]
    MissingContainedPresentation {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing presentation.
        floating: FloatingPresentationId,
    },
    /// A contained roster could not receive a structural layer identity.
    #[error("surface {surface} contained presentation {floating} exhausted structural layers")]
    ContainedLayerExhausted {
        /// Owning surface.
        surface: SurfaceId,
        /// Presentation without a layer.
        floating: FloatingPresentationId,
    },
    /// A surface-owned root record was absent.
    #[error("surface {surface} references missing root {root}")]
    MissingRoot {
        /// Owning surface.
        surface: SurfaceId,
        /// Missing root.
        root: RootId,
    },
    /// A reachable graph node was absent.
    #[error("root {root} on surface {surface} references missing node {node:?}")]
    MissingNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Missing node.
        node: NodeId,
    },
    /// A node was encountered twice while deriving constraints.
    #[error("root {root} on surface {surface} reaches node {node:?} more than once")]
    RepeatedNode {
        /// Owning surface.
        surface: SurfaceId,
        /// Owning root.
        root: RootId,
        /// Repeated node.
        node: NodeId,
    },
    /// A tabs leaf had no core-minted minimum answer.
    #[error("root {root} tabs node {tabs:?} has no pane minimum")]
    MissingLeafMinimum {
        /// Owning root.
        root: RootId,
        /// Unmeasured leaf.
        tabs: NodeId,
    },
    /// A semantic tabs leaf had no frozen tab-bar policy requirement.
    #[error("tab bar {id:?} has no frozen presentation requirement")]
    MissingTabBarRequirement {
        /// Missing semantic tab-bar identity.
        id: TabBarSceneId,
    },
    /// A tab-bar requirement omitted one item's static close capability.
    #[error("tab {id:?} has no frozen close capability")]
    MissingTabCloseCapability {
        /// Tab whose close policy fact was missing.
        id: TabSceneId,
    },
    /// Exact-set validation succeeded but the authoritative pane minimum was absent.
    #[error("authoritative pane minimum answer is missing for {key:?}")]
    MissingPaneMinimumMeasurement {
        /// Missing exact key.
        key: PaneMinimumKey,
    },
    /// Exact-set validation succeeded but the authoritative tab answer was absent.
    #[error("authoritative tab intrinsic answer is missing for {key:?}")]
    MissingTabIntrinsicMeasurement {
        /// Missing exact key.
        key: TabIntrinsicKey,
    },
    /// Exact-set validation succeeded but the authoritative strip answer was absent.
    #[error("authoritative tab-strip answer is missing for {key:?}")]
    MissingTabStripMeasurement {
        /// Missing exact key.
        key: TabStripKey,
    },
    /// The active core-owned menu disagreed with the exact live tab roster.
    #[error("active tab-list menu roster does not match {key:?}")]
    TabListMenuRosterMismatch {
        /// Exact strip whose live roster changed without reconciliation.
        key: TabStripStateKey,
    },
    /// The menu anchor was not contained by the authoritative popup plane.
    #[error("tab-list menu anchor for {key:?} is outside its popup plane")]
    TabListMenuAnchorOutsidePopupPlane {
        /// Exact strip whose popup anchor cannot be authorized.
        key: TabStripStateKey,
    },
    /// Neither side of the menu anchor had positive authoritative popup space.
    #[error("tab-list menu for {key:?} has no space in its popup plane")]
    TabListMenuPopupSpaceUnavailable {
        /// Exact strip whose popup cannot be placed without covering its anchor.
        key: TabStripStateKey,
    },
    /// Core state owns an active menu but this contribution could not project it.
    #[error("active tab-list menu {session:?} has no exact record in the compiled plan")]
    ActiveTabListMenuProjectionUnavailable {
        /// Exact active menu which must remain authoritative or settle closed.
        session: crate::tab_strip::TabListMenuSessionId,
    },
    /// The transient popup store disagreed with the manifest frozen for compilation.
    #[error("popup-plane state differs from the exact scene requirement manifest")]
    PopupPlaneStateMismatch,
    /// Layout omitted split-specific projection for a reachable split.
    #[error("root {root} has no projection for split {split:?}")]
    MissingSplitProjection {
        /// Owning root.
        root: RootId,
        /// Missing split projection.
        split: NodeId,
    },
    /// Layout omitted one child rectangle needed by a splitter record.
    #[error("root {root} has no projection for node {node:?}")]
    MissingNodeProjection {
        /// Owning root.
        root: RootId,
        /// Missing projected node.
        node: NodeId,
    },
    /// Layout omitted one child minimum required by a splitter record.
    #[error("layout omitted minimum for child {child:?} of split {split:?} in root {root}")]
    MissingSplitMinimum {
        /// Owning root.
        root: RootId,
        /// Split owning the child.
        split: NodeId,
        /// Child whose axis minimum was absent.
        child: NodeId,
    },
    /// Layout omitted one child maximum required by a splitter record.
    #[error("layout omitted maximum for child {child:?} of split {split:?} in root {root}")]
    MissingSplitMaximum {
        /// Owning root.
        root: RootId,
        /// Split owning the child.
        split: NodeId,
        /// Child whose axis maximum was absent.
        child: NodeId,
    },
    /// A collection cardinality could not be represented for geometry arithmetic.
    #[error("presentation geometry count cannot be represented")]
    GeometryCountOverflow,
    /// Finite measurements overflowed while deriving an aggregate.
    #[error("presentation aggregate `{aggregate}` is non-finite")]
    NonFiniteAggregate {
        /// Aggregate which overflowed.
        aggregate: &'static str,
    },
    /// Valid inputs could not produce finite tab widths above the operable minimum.
    #[error("tab widths cannot be allocated within {available} logical units at minimum {minimum}")]
    InvalidTabWidthAllocation {
        /// Width available to the tab sequence before conditional trailing chrome.
        available: f64,
        /// Required operable width of every tab.
        minimum: f64,
    },
    /// A validated core configuration could not be represented by `DockFraction`.
    #[error("presentation dock fraction cannot be represented by DockFraction")]
    InvalidDockFraction,
    /// A current structural target could not be captured.
    #[error(transparent)]
    Command(#[from] CommandError),
    /// Deterministic root layout failed.
    #[error(transparent)]
    Layout(#[from] LayoutError),
    /// Core presentation metrics could not be converted into layout metrics.
    #[error(transparent)]
    LayoutMetrics(#[from] LayoutMetricsError),
    /// Validated logical geometry could not be constructed.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// The compiler produced a scene record that violated construction rules.
    #[error(transparent)]
    Scene(#[from] SceneBuildError),
    /// The compiler could not preserve one ordered renderer program.
    #[error("surface program construction violated an internal ordering invariant")]
    SurfaceProgramInvariant,
}

impl From<SurfaceProgramBuildError> for SceneCompilationError {
    fn from(_error: SurfaceProgramBuildError) -> Self {
        Self::SurfaceProgramInvariant
    }
}

/// Rejection of one exact adapter measurement contribution.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PresentationCompilationError {
    /// The manifest index belongs to another exact workspace revision.
    #[error("manifest workspace {manifest:?} does not match current workspace {current:?}")]
    WorkspaceVersionMismatch {
        /// Version frozen into the manifest and its structural index.
        manifest: WorkspaceVersion,
        /// Version currently published by the engine.
        current: WorkspaceVersion,
    },
    /// The contribution did not match the current core-derived manifest.
    #[error(transparent)]
    Manifest(#[from] ManifestMeasurementError),
    /// At least one exact-set answer explicitly lacked authoritative data.
    #[error(transparent)]
    Authority(#[from] MeasurementAuthorityError),
    /// The manifest entry disappeared after validation, indicating an internal invariant failure.
    #[error("surface {surface} requirements vanished after validation")]
    RequirementsVanished {
        /// Surface whose current requirement entry was missing.
        surface: SurfaceId,
    },
    /// Core-owned semantic compilation failed.
    #[error(transparent)]
    Scene(#[from] SceneCompilationError),
}
