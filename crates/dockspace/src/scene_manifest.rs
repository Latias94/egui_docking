//! Exact, renderer-neutral measurement requirements for scene compilation.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use thiserror::Error;

use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{EngineAuthorityDomainId, ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
pub use crate::policy::PolicyRevision;
use crate::policy::{CloseCapability, TabBarInteraction, TabBarPolicy};
use crate::presentation_config::PresentationConfigRevision;
use crate::scene::{TabBarSceneId, TabSceneId};
use crate::tab_strip::{PopupPlaneRequirement, TabStripStateKey};
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

macro_rules! monotonic_revision {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Creates a revision from its engine-local counter representation.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            /// Returns the engine-local counter representation.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            /// Advances the revision without wrapping.
            #[must_use]
            pub const fn checked_next(self) -> Option<Self> {
                match self.0.checked_add(1) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }
        }
    };
}

monotonic_revision!(
    RequirementRevision,
    "Monotonic identity of one core-derived semantic requirement manifest."
);
monotonic_revision!(
    SurfaceRequirementRevision,
    "Monotonic identity of one surface's independently replaceable measurement requirements."
);
monotonic_revision!(
    SurfaceSceneRevision,
    "Monotonic identity of one compiled presentation for a logical surface."
);

/// Unforgeable-by-convention identity of one exact surface measurement request.
///
/// Only the core can mint a ticket. Adapters may copy it into a contribution,
/// but cannot construct a ticket for a different workspace, configuration, or
/// surface through the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceMeasurementTicket {
    authority_domain: EngineAuthorityDomainId,
    workspace_epoch: WorkspaceEpoch,
    config: PresentationConfigRevision,
    policy: PolicyRevision,
    surface_requirement: SurfaceRequirementRevision,
    surface: SurfaceId,
}

impl SurfaceMeasurementTicket {
    #[must_use]
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        workspace_epoch: WorkspaceEpoch,
        config: PresentationConfigRevision,
        policy: PolicyRevision,
        surface_requirement: SurfaceRequirementRevision,
        surface: SurfaceId,
    ) -> Self {
        Self {
            authority_domain,
            workspace_epoch,
            config,
            policy,
            surface_requirement,
            surface,
        }
    }

    /// Returns the engine authority domain which minted this ticket.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the workspace replacement epoch that owns this ticket.
    #[must_use]
    pub const fn workspace_epoch(self) -> WorkspaceEpoch {
        self.workspace_epoch
    }

    /// Returns the exact presentation configuration revision.
    #[must_use]
    pub const fn config(self) -> PresentationConfigRevision {
        self.config
    }

    /// Returns the exact policy revision used by semantic compilation.
    #[must_use]
    pub const fn policy(self) -> PolicyRevision {
        self.policy
    }

    /// Returns this surface's independently replaceable requirement revision.
    #[must_use]
    pub const fn surface_requirement(self) -> SurfaceRequirementRevision {
        self.surface_requirement
    }

    /// Returns the sole surface this ticket may measure.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Exact key for the dock layout bounds of one surface.
///
/// The rectangle uses the surface-local logical coordinate space. It is not
/// the viewport or work-area extent available to popup presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceBoundsKey {
    surface: SurfaceId,
}

impl SurfaceBoundsKey {
    /// Creates the bounds key for a logical surface.
    #[must_use]
    pub const fn new(surface: SurfaceId) -> Self {
        Self { surface }
    }

    /// Returns the measured surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Exact key for the logical popup plane of one surface.
///
/// The popup plane uses the same surface-local logical coordinate space as
/// [`SurfaceBoundsKey`] and may be larger than the dock layout bounds. For
/// example, an embedded dockspace can occupy only the bottom of a viewport
/// while its popup menu opens into the rest of that viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PopupPlaneBoundsKey {
    surface: SurfaceId,
}

impl PopupPlaneBoundsKey {
    /// Creates the popup-plane bounds key for a logical surface.
    #[must_use]
    pub const fn new(surface: SurfaceId) -> Self {
        Self { surface }
    }

    /// Returns the measured surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }
}

/// Exact key for the intrinsic content minimum of one tabs leaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaneMinimumKey {
    root: RootId,
    tabs: NodeId,
    selected: Option<ItemId>,
}

impl PaneMinimumKey {
    /// Creates a pane-minimum key from core-owned structural identity.
    #[must_use]
    pub const fn new(root: RootId, tabs: NodeId, selected: Option<ItemId>) -> Self {
        Self {
            root,
            tabs,
            selected,
        }
    }

    /// Returns the owning root.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the tabs leaf whose content is constrained.
    #[must_use]
    pub const fn tabs(self) -> NodeId {
        self.tabs
    }

    /// Returns the exact selected pane observed by the core.
    #[must_use]
    pub const fn selected(self) -> Option<ItemId> {
        self.selected
    }
}

/// Exact key for one tab's renderer-measured intrinsic content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabIntrinsicKey {
    surface: SurfaceId,
    tab: TabSceneId,
}

impl TabIntrinsicKey {
    /// Creates an intrinsic measurement key.
    #[must_use]
    pub const fn new(surface: SurfaceId, tab: TabSceneId) -> Self {
        Self { surface, tab }
    }

    /// Returns the surface that must perform this measurement.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the stable semantic tab identity.
    #[must_use]
    pub const fn tab(self) -> TabSceneId {
        self.tab
    }
}

/// Exact key for adapter-owned chrome reserved around one tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabStripKey {
    surface: SurfaceId,
    bar: TabBarSceneId,
}

impl TabStripKey {
    /// Creates a tab-strip measurement key.
    #[must_use]
    pub const fn new(surface: SurfaceId, bar: TabBarSceneId) -> Self {
        Self { surface, bar }
    }

    /// Returns the surface that must perform this measurement.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the stable semantic tab-bar identity.
    #[must_use]
    pub const fn bar(self) -> TabBarSceneId {
        self.bar
    }
}

/// Why an adapter could not provide one required measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasurementUnavailableReason {
    /// The active renderer cannot measure this class of value.
    Unsupported,
    /// The owning surface cannot currently be measured.
    SurfaceUnavailable,
    /// Application content required for measurement is not currently available.
    ContentUnavailable,
    /// Font or text shaping data is not ready for this contribution.
    TextMetricsUnavailable,
    /// The adapter intentionally deferred the measurement to a later callback.
    Deferred,
}

/// One explicit answer to a core-owned measurement requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Measurement<T> {
    /// An authoritative renderer-neutral measurement.
    Measured(T),
    /// The adapter answered the requirement but could not measure it.
    Unavailable(MeasurementUnavailableReason),
}

impl<T> Measurement<T> {
    /// Returns the measured value, if authoritative data was supplied.
    #[must_use]
    pub const fn measured(&self) -> Option<&T> {
        match self {
            Self::Measured(value) => Some(value),
            Self::Unavailable(_) => None,
        }
    }

    /// Returns the explicit reason authority was unavailable.
    #[must_use]
    pub const fn unavailable_reason(&self) -> Option<MeasurementUnavailableReason> {
        match self {
            Self::Measured(_) => None,
            Self::Unavailable(reason) => Some(*reason),
        }
    }
}

/// Renderer-measured intrinsic content for one tab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabIntrinsic {
    content_width: f64,
}

impl TabIntrinsic {
    /// Creates a finite, non-negative tab intrinsic.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementValueError`] when `content_width` is non-finite or negative.
    pub fn new(content_width: f64) -> Result<Self, MeasurementValueError> {
        validate_non_negative("tab content width", content_width)?;
        Ok(Self { content_width })
    }

    /// Returns the measured text-and-icon content width before core padding.
    #[must_use]
    pub const fn content_width(self) -> f64 {
        self.content_width
    }
}

/// Adapter-owned extents reserved before and after a tab strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripMetrics {
    leading_reserved: f64,
    trailing_reserved: f64,
    controls: Option<TabStripControlMetrics>,
    tab_list_menu: Option<TabListMenuMetrics>,
    // Transitional input retained until egui consumes core-owned strip state.
    // The scene compiler deliberately ignores this value.
    scroll_offset: Option<f64>,
}

impl TabStripMetrics {
    /// Creates finite, non-negative tab-strip reservations.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementValueError`] for a non-finite or negative extent.
    pub fn new(
        leading_reserved: f64,
        trailing_reserved: f64,
    ) -> Result<Self, MeasurementValueError> {
        validate_non_negative("tab strip leading reservation", leading_reserved)?;
        validate_non_negative("tab strip trailing reservation", trailing_reserved)?;
        Ok(Self {
            leading_reserved,
            trailing_reserved,
            controls: None,
            tab_list_menu: None,
            scroll_offset: None,
        })
    }

    /// Attaches renderer intrinsics for the three core-ordered controls.
    #[must_use]
    pub const fn with_controls(mut self, metrics: TabStripControlMetrics) -> Self {
        self.controls = Some(metrics);
        self
    }

    /// Attaches renderer measurements required to allocate a tab-list menu.
    #[must_use]
    pub const fn with_tab_list_menu(mut self, metrics: TabListMenuMetrics) -> Self {
        self.tab_list_menu = Some(metrics);
        self
    }

    /// Attaches a legacy adapter scroll observation.
    ///
    /// The core does not consume this value. It remains temporarily accepted so
    /// adapters can migrate to core-owned tab-strip state without reintroducing
    /// two scroll authorities.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementValueError`] for a non-finite or negative offset.
    pub fn with_scroll_offset(mut self, scroll_offset: f64) -> Result<Self, MeasurementValueError> {
        validate_non_negative("tab strip scroll offset", scroll_offset)?;
        self.scroll_offset = Some(scroll_offset);
        Ok(self)
    }

    /// Returns chrome reserved before the first tab.
    #[must_use]
    pub const fn leading_reserved(self) -> f64 {
        self.leading_reserved
    }

    /// Returns chrome reserved after the last tab.
    #[must_use]
    pub const fn trailing_reserved(self) -> f64 {
        self.trailing_reserved
    }

    /// Returns the adapter's explicit transient scroll offset, when supplied.
    #[must_use]
    pub const fn scroll_offset(self) -> Option<f64> {
        self.scroll_offset
    }

    /// Returns renderer measurements for a tab-list menu, when available.
    #[must_use]
    pub const fn tab_list_menu(self) -> Option<TabListMenuMetrics> {
        self.tab_list_menu
    }

    /// Returns renderer intrinsics for the core-ordered controls.
    #[must_use]
    pub const fn controls(self) -> Option<TabStripControlMetrics> {
        self.controls
    }
}

/// Renderer-measured extents for core-ordered tab-strip controls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripControlMetrics {
    scroll_backward: Option<TabStripControlMetric>,
    scroll_forward: Option<TabStripControlMetric>,
    tab_list_menu: Option<TabStripControlMetric>,
    spacing: f64,
}

impl TabStripControlMetrics {
    /// Creates an empty control roster with finite inter-control spacing.
    pub fn new(spacing: f64) -> Result<Self, MeasurementValueError> {
        validate_non_negative("tab-strip control spacing", spacing)?;
        Ok(Self {
            scroll_backward: None,
            scroll_forward: None,
            tab_list_menu: None,
            spacing,
        })
    }

    /// Attaches the backward-scroll control measurement.
    #[must_use]
    pub const fn with_scroll_backward(mut self, metric: TabStripControlMetric) -> Self {
        self.scroll_backward = Some(metric);
        self
    }

    /// Attaches the forward-scroll control measurement.
    #[must_use]
    pub const fn with_scroll_forward(mut self, metric: TabStripControlMetric) -> Self {
        self.scroll_forward = Some(metric);
        self
    }

    /// Attaches the tab-list menu control measurement.
    #[must_use]
    pub const fn with_tab_list_menu(mut self, metric: TabStripControlMetric) -> Self {
        self.tab_list_menu = Some(metric);
        self
    }

    #[must_use]
    pub const fn scroll_backward(self) -> Option<TabStripControlMetric> {
        self.scroll_backward
    }

    #[must_use]
    pub const fn scroll_forward(self) -> Option<TabStripControlMetric> {
        self.scroll_forward
    }

    #[must_use]
    pub const fn tab_list_menu(self) -> Option<TabStripControlMetric> {
        self.tab_list_menu
    }

    #[must_use]
    pub const fn spacing(self) -> f64 {
        self.spacing
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.scroll_backward.is_none()
            && self.scroll_forward.is_none()
            && self.tab_list_menu.is_none()
    }
}

/// Whether a control consumes strip viewport space or overlays its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabStripControlPlacement {
    ReservedLeading,
    ReservedTrailing,
    OverlayLeading,
    OverlayTrailing,
}

/// One independently optional tab-strip control measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStripControlMetric {
    extent: f64,
    placement: TabStripControlPlacement,
}

impl TabStripControlMetric {
    /// Creates one positive finite control extent with explicit placement.
    pub fn new(
        extent: f64,
        placement: TabStripControlPlacement,
    ) -> Result<Self, MeasurementValueError> {
        validate_positive("tab-strip control extent", extent)?;
        Ok(Self { extent, placement })
    }

    #[must_use]
    pub const fn extent(self) -> f64 {
        self.extent
    }

    #[must_use]
    pub const fn placement(self) -> TabStripControlPlacement {
        self.placement
    }

    pub(crate) const fn can_allocate(self) -> bool {
        self.extent > 0.0
    }
}

/// Renderer-measured scalar inputs used to allocate a core-owned tab-list menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabListMenuMetrics {
    row_height: f64,
    horizontal_padding: f64,
    vertical_padding: f64,
    row_spacing: f64,
    maximum_height: f64,
    scrollbar_extent: f64,
}

impl TabListMenuMetrics {
    /// Creates finite, non-negative menu metrics.
    ///
    /// Zero row height or maximum height is a valid unavailable allocation and
    /// therefore produces no menu geometry.
    pub fn new(
        row_height: f64,
        horizontal_padding: f64,
        vertical_padding: f64,
        row_spacing: f64,
        maximum_height: f64,
        scrollbar_extent: f64,
    ) -> Result<Self, MeasurementValueError> {
        for (field, value) in [
            ("tab-list row height", row_height),
            ("tab-list horizontal padding", horizontal_padding),
            ("tab-list vertical padding", vertical_padding),
            ("tab-list row spacing", row_spacing),
            ("tab-list maximum height", maximum_height),
            ("tab-list scrollbar extent", scrollbar_extent),
        ] {
            validate_non_negative(field, value)?;
        }
        Ok(Self {
            row_height,
            horizontal_padding,
            vertical_padding,
            row_spacing,
            maximum_height,
            scrollbar_extent,
        })
    }

    #[must_use]
    pub const fn row_height(self) -> f64 {
        self.row_height
    }

    #[must_use]
    pub const fn horizontal_padding(self) -> f64 {
        self.horizontal_padding
    }

    #[must_use]
    pub const fn vertical_padding(self) -> f64 {
        self.vertical_padding
    }

    #[must_use]
    pub const fn row_spacing(self) -> f64 {
        self.row_spacing
    }

    #[must_use]
    pub const fn maximum_height(self) -> f64 {
        self.maximum_height
    }

    #[must_use]
    pub const fn scrollbar_extent(self) -> f64 {
        self.scrollbar_extent
    }

    pub(crate) const fn can_allocate(self) -> bool {
        self.row_height > 0.0 && self.maximum_height > 0.0
    }
}

fn validate_non_negative(field: &'static str, value: f64) -> Result<(), MeasurementValueError> {
    if !value.is_finite() {
        return Err(MeasurementValueError::NonFinite { field, value });
    }
    if value < 0.0 {
        return Err(MeasurementValueError::Negative { field, value });
    }
    Ok(())
}

fn validate_positive(field: &'static str, value: f64) -> Result<(), MeasurementValueError> {
    if !value.is_finite() {
        return Err(MeasurementValueError::NonFinite { field, value });
    }
    if value <= 0.0 {
        return Err(MeasurementValueError::NonPositive { field, value });
    }
    Ok(())
}

/// Invalid scalar supplied as an adapter measurement.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum MeasurementValueError {
    /// A scalar was NaN or infinite.
    #[error("measurement `{field}` must be finite, got {value}")]
    NonFinite {
        /// Name of the rejected value.
        field: &'static str,
        /// Rejected scalar.
        value: f64,
    },
    /// A magnitude was negative.
    #[error("measurement `{field}` must be non-negative, got {value}")]
    Negative {
        /// Name of the rejected value.
        field: &'static str,
        /// Rejected scalar.
        value: f64,
    },
    /// A magnitude which represents an explicitly present element was zero or negative.
    #[error("measurement `{field}` must be positive, got {value}")]
    NonPositive {
        /// Name of the rejected value.
        field: &'static str,
        /// Rejected scalar.
        value: f64,
    },
}

/// Complete core-derived measurement requirement set for one surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRequirements {
    ticket: SurfaceMeasurementTicket,
    bounds: SurfaceBoundsKey,
    popup_plane_bounds: Option<PopupPlaneBoundsKey>,
    pane_minimums: BTreeSet<PaneMinimumKey>,
    tab_intrinsics: BTreeSet<TabIntrinsicKey>,
    tab_strips: BTreeSet<TabStripKey>,
    tab_bars: BTreeMap<TabBarSceneId, TabBarRequirement>,
}

/// Frozen presentation policy for one semantic tabs leaf.
///
/// This is core-owned input to adapters and scene compilation. It is not a
/// measurement question: adapters cannot widen interaction or close authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabBarRequirement {
    id: TabBarSceneId,
    policy: TabBarPolicy,
    close_capabilities: BTreeMap<ItemId, CloseCapability>,
}

impl TabBarRequirement {
    pub(crate) fn new(
        id: TabBarSceneId,
        policy: TabBarPolicy,
        close_capabilities: BTreeMap<ItemId, CloseCapability>,
    ) -> Self {
        Self {
            id,
            policy,
            close_capabilities,
        }
    }

    /// Returns the semantic tabs-leaf identity.
    #[must_use]
    pub const fn id(&self) -> TabBarSceneId {
        self.id
    }

    /// Returns the complete effective presentation policy frozen by core.
    #[must_use]
    pub const fn policy(&self) -> TabBarPolicy {
        self.policy
    }

    /// Returns the frozen close capability for an item in this tabs leaf.
    #[must_use]
    pub fn close_capability(&self, item: ItemId) -> Option<CloseCapability> {
        self.close_capabilities.get(&item).copied()
    }

    /// Iterates item close capabilities in stable item identity order.
    #[must_use]
    pub fn close_capabilities(
        &self,
    ) -> impl ExactSizeIterator<Item = (ItemId, CloseCapability)> + '_ {
        self.close_capabilities
            .iter()
            .map(|(item, capability)| (*item, *capability))
    }
}

/// Structural requirement inventory derived before transient popup reconciliation.
///
/// This draft deliberately cannot be submitted to an adapter. The engine first
/// reconciles its transient tab-strip store against these exact structural and
/// policy facts, then finalizes one coherent [`SceneRequirementManifest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneRequirementDraft {
    authority_domain: EngineAuthorityDomainId,
    workspace: WorkspaceVersion,
    workspace_index: Arc<WorkspaceIndex>,
    config: PresentationConfigRevision,
    policy: PolicyRevision,
    revision: RequirementRevision,
    surfaces: BTreeMap<SurfaceId, SurfaceRequirements>,
}

impl SceneRequirementDraft {
    pub(crate) fn new(
        authority_domain: EngineAuthorityDomainId,
        workspace: WorkspaceVersion,
        workspace_index: Arc<WorkspaceIndex>,
        config: PresentationConfigRevision,
        policy: PolicyRevision,
        revision: RequirementRevision,
        surfaces: BTreeMap<SurfaceId, SurfaceRequirements>,
    ) -> Result<Self, ManifestBuildError> {
        validate_manifest_identity(authority_domain, workspace, config, policy, &surfaces)?;
        if workspace_index.version() != workspace {
            return Err(ManifestBuildError::WorkspaceIndexVersionMismatch {
                manifest: workspace,
                index: workspace_index.version(),
            });
        }
        Ok(Self {
            authority_domain,
            workspace,
            workspace_index,
            config,
            policy,
            revision,
            surfaces,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        authority_domain: EngineAuthorityDomainId,
        workspace: WorkspaceVersion,
        config: PresentationConfigRevision,
        policy: PolicyRevision,
        revision: RequirementRevision,
        surfaces: BTreeMap<SurfaceId, SurfaceRequirements>,
    ) -> Result<Self, ManifestBuildError> {
        Self::new(
            authority_domain,
            workspace,
            Arc::new(WorkspaceIndex::empty_for_test(workspace)),
            config,
            policy,
            revision,
            surfaces,
        )
    }

    pub(crate) fn surface(&self, surface: SurfaceId) -> Option<&SurfaceRequirements> {
        self.surfaces.get(&surface)
    }

    pub(crate) fn shared_workspace_index(&self) -> Arc<WorkspaceIndex> {
        Arc::clone(&self.workspace_index)
    }

    pub(crate) fn surfaces(
        &self,
    ) -> impl ExactSizeIterator<Item = (SurfaceId, &SurfaceRequirements)> {
        self.surfaces
            .iter()
            .map(|(surface, requirements)| (*surface, requirements))
    }

    pub(crate) fn finalize(
        mut self,
        popup: PopupPlaneRequirement,
    ) -> Result<SceneRequirementManifest, ManifestBuildError> {
        validate_popup_requirement(&self.surfaces, popup)?;
        if matches!(popup, PopupPlaneRequirement::Active { .. }) {
            for (surface, requirements) in &mut self.surfaces {
                requirements.popup_plane_bounds = Some(PopupPlaneBoundsKey::new(*surface));
            }
        }
        Ok(SceneRequirementManifest {
            authority_domain: self.authority_domain,
            workspace: self.workspace,
            workspace_index: self.workspace_index,
            config: self.config,
            policy: self.policy,
            revision: self.revision,
            popup,
            surfaces: self.surfaces,
        })
    }
}

/// Complete core-derived semantic requirement inventory for one workspace state.
///
/// The surface map is the exact workspace presentation roster. Adapters may
/// submit one surface at a time, but cannot introduce a surface outside this
/// manifest or redefine the ticket attached to an existing surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRequirementManifest {
    authority_domain: EngineAuthorityDomainId,
    workspace: WorkspaceVersion,
    workspace_index: Arc<WorkspaceIndex>,
    config: PresentationConfigRevision,
    policy: PolicyRevision,
    revision: RequirementRevision,
    popup: PopupPlaneRequirement,
    surfaces: BTreeMap<SurfaceId, SurfaceRequirements>,
}

impl SceneRequirementManifest {
    /// Returns the engine authority domain which minted every surface ticket.
    #[must_use]
    pub const fn authority_domain(&self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the exact workspace state from which requirements were derived.
    #[must_use]
    pub const fn workspace(&self) -> WorkspaceVersion {
        self.workspace
    }

    pub(crate) fn workspace_index(&self) -> &WorkspaceIndex {
        &self.workspace_index
    }

    /// Returns the presentation configuration revision used by compilation.
    #[must_use]
    pub const fn config(&self) -> PresentationConfigRevision {
        self.config
    }

    /// Returns the policy revision used to derive every surface requirement.
    #[must_use]
    pub const fn policy(&self) -> PolicyRevision {
        self.policy
    }

    /// Returns this manifest's monotonic revision.
    #[must_use]
    pub const fn revision(&self) -> RequirementRevision {
        self.revision
    }

    /// Returns the exact workspace-global popup presentation requirement.
    #[must_use]
    pub const fn popup(&self) -> PopupPlaneRequirement {
        self.popup
    }

    /// Returns one surface requirement, or `None` outside the exact roster.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceRequirements> {
        self.surfaces.get(&surface)
    }

    /// Iterates the exact surface roster in stable identity order.
    #[must_use]
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (SurfaceId, &SurfaceRequirements)> {
        self.surfaces
            .iter()
            .map(|(surface, requirements)| (*surface, requirements))
    }

    /// Validates one independently delivered surface contribution.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the ticket names a surface outside this
    /// manifest or the contribution fails that surface's exact-set contract.
    pub fn validate_surface<'a>(
        &'a self,
        measurements: &'a SurfaceMeasurements,
    ) -> Result<ValidatedSurfaceMeasurements<'a>, ManifestMeasurementError> {
        let surface = measurements.ticket().surface();
        let requirements = self
            .surfaces
            .get(&surface)
            .ok_or(ManifestMeasurementError::SurfaceOutsideManifest { surface })?;
        requirements
            .validate(measurements)
            .map_err(|source| ManifestMeasurementError::InvalidContribution { surface, source })
    }
}

impl SurfaceRequirements {
    /// Constructs a core-owned exact requirement set.
    pub(crate) fn new(
        ticket: SurfaceMeasurementTicket,
        pane_minimums: BTreeSet<PaneMinimumKey>,
        tab_intrinsics: BTreeSet<TabIntrinsicKey>,
        tab_strips: BTreeSet<TabStripKey>,
        tab_bars: BTreeMap<TabBarSceneId, TabBarRequirement>,
    ) -> Result<Self, RequirementBuildError> {
        let surface = ticket.surface();
        if let Some(key) = tab_intrinsics.iter().find(|key| key.surface() != surface) {
            return Err(RequirementBuildError::TabIntrinsicSurfaceMismatch {
                expected: surface,
                actual: key.surface(),
            });
        }
        if let Some(key) = tab_strips.iter().find(|key| key.surface() != surface) {
            return Err(RequirementBuildError::TabStripSurfaceMismatch {
                expected: surface,
                actual: key.surface(),
            });
        }
        if let Some((map_key, requirement)) = tab_bars
            .iter()
            .find(|(map_key, requirement)| **map_key != requirement.id())
        {
            return Err(RequirementBuildError::TabBarIdentityMismatch {
                map_key: *map_key,
                requirement: requirement.id(),
            });
        }
        Ok(Self {
            ticket,
            bounds: SurfaceBoundsKey::new(surface),
            popup_plane_bounds: None,
            pane_minimums,
            tab_intrinsics,
            tab_strips,
            tab_bars,
        })
    }

    /// Returns the exact ticket adapters must echo in their contribution.
    #[must_use]
    pub const fn ticket(&self) -> SurfaceMeasurementTicket {
        self.ticket
    }

    /// Returns the sole required dock layout bounds key.
    #[must_use]
    pub const fn bounds(&self) -> SurfaceBoundsKey {
        self.bounds
    }

    /// Returns the popup-plane bounds requirement when a workspace popup is active.
    #[must_use]
    pub const fn popup_plane_bounds(&self) -> Option<PopupPlaneBoundsKey> {
        self.popup_plane_bounds
    }

    /// Iterates required intrinsic pane minimums in deterministic identity order.
    #[must_use]
    pub fn pane_minimums(&self) -> impl ExactSizeIterator<Item = PaneMinimumKey> + '_ {
        self.pane_minimums.iter().copied()
    }

    /// Iterates required tab intrinsics in deterministic identity order.
    #[must_use]
    pub fn tab_intrinsics(&self) -> impl ExactSizeIterator<Item = TabIntrinsicKey> + '_ {
        self.tab_intrinsics.iter().copied()
    }

    /// Iterates required tab-strip measurements in deterministic identity order.
    #[must_use]
    pub fn tab_strips(&self) -> impl ExactSizeIterator<Item = TabStripKey> + '_ {
        self.tab_strips.iter().copied()
    }

    /// Returns frozen tab-bar policy for one exact semantic tabs leaf.
    #[must_use]
    pub fn tab_bar(&self, id: TabBarSceneId) -> Option<&TabBarRequirement> {
        self.tab_bars.get(&id)
    }

    /// Iterates every semantic tabs leaf in deterministic identity order.
    #[must_use]
    pub fn tab_bars(&self) -> impl ExactSizeIterator<Item = &TabBarRequirement> {
        self.tab_bars.values()
    }

    /// Validates ticket identity and every required contribution as exact sets.
    ///
    /// Explicitly unavailable answers satisfy set completeness. They remain
    /// non-authoritative and are rejected separately by
    /// [`ValidatedSurfaceMeasurements::require_authoritative`].
    ///
    /// # Errors
    ///
    /// Returns the first stable ticket or exact-set mismatch.
    pub fn validate<'a>(
        &'a self,
        measurements: &'a SurfaceMeasurements,
    ) -> Result<ValidatedSurfaceMeasurements<'a>, MeasurementValidationError> {
        validate_ticket(self.ticket, measurements.ticket)?;
        match measurements.bounds {
            None => return Err(MeasurementValidationError::MissingSurfaceBounds),
            Some((key, _)) if key != self.bounds => {
                return Err(MeasurementValidationError::UnexpectedSurfaceBounds {
                    expected: self.bounds,
                    actual: key,
                });
            }
            Some(_) => {}
        }
        match (self.popup_plane_bounds, measurements.popup_plane_bounds) {
            (Some(_), None) => {
                return Err(MeasurementValidationError::MissingPopupPlaneBounds);
            }
            (Some(expected), Some((actual, _))) if expected != actual => {
                return Err(MeasurementValidationError::UnexpectedPopupPlaneBounds {
                    expected: Some(expected),
                    actual,
                });
            }
            (None, Some((actual, _))) => {
                return Err(MeasurementValidationError::UnexpectedPopupPlaneBounds {
                    expected: None,
                    actual,
                });
            }
            (Some(_), Some(_)) | (None, None) => {}
        }
        validate_pane_minimum_set(&self.pane_minimums, &measurements.pane_minimums)?;
        validate_tab_intrinsic_set(&self.tab_intrinsics, &measurements.tab_intrinsics)?;
        validate_tab_strip_set(&self.tab_strips, &measurements.tab_strips)?;
        Ok(ValidatedSurfaceMeasurements { measurements })
    }
}

/// Adapter contribution for one exact surface requirement ticket.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceMeasurements {
    ticket: SurfaceMeasurementTicket,
    bounds: Option<(SurfaceBoundsKey, Measurement<LogicalRect>)>,
    popup_plane_bounds: Option<(PopupPlaneBoundsKey, Measurement<LogicalRect>)>,
    pane_minimums: BTreeMap<PaneMinimumKey, Measurement<LogicalSize>>,
    tab_intrinsics: BTreeMap<TabIntrinsicKey, Measurement<TabIntrinsic>>,
    tab_strips: BTreeMap<TabStripKey, Measurement<TabStripMetrics>>,
}

impl SurfaceMeasurements {
    /// Starts an empty contribution for an exact core-issued ticket.
    #[must_use]
    pub fn new(ticket: SurfaceMeasurementTicket) -> Self {
        Self {
            ticket,
            bounds: None,
            popup_plane_bounds: None,
            pane_minimums: BTreeMap::new(),
            tab_intrinsics: BTreeMap::new(),
            tab_strips: BTreeMap::new(),
        }
    }

    /// Answers every requirement with one explicit unavailability reason.
    ///
    /// This is the only valid way for a host to say that a rostered surface
    /// participated in the current frame but could not supply measurements.
    /// It preserves the exact-set contract instead of treating an absent
    /// callback as an implicit negative fact.
    #[must_use]
    pub fn unavailable(
        requirements: &SurfaceRequirements,
        reason: MeasurementUnavailableReason,
    ) -> Self {
        Self {
            ticket: requirements.ticket(),
            bounds: Some((requirements.bounds(), Measurement::Unavailable(reason))),
            popup_plane_bounds: requirements
                .popup_plane_bounds()
                .map(|key| (key, Measurement::Unavailable(reason))),
            pane_minimums: requirements
                .pane_minimums()
                .map(|key| (key, Measurement::Unavailable(reason)))
                .collect(),
            tab_intrinsics: requirements
                .tab_intrinsics()
                .map(|key| (key, Measurement::Unavailable(reason)))
                .collect(),
            tab_strips: requirements
                .tab_strips()
                .map(|key| (key, Measurement::Unavailable(reason)))
                .collect(),
        }
    }

    /// Returns the echoed core-issued ticket.
    #[must_use]
    pub const fn ticket(&self) -> SurfaceMeasurementTicket {
        self.ticket
    }

    /// Answers the sole surface dock-layout bounds requirement.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSubmissionError::DuplicateSurfaceBounds`] when it was already answered.
    pub fn set_bounds(
        &mut self,
        key: SurfaceBoundsKey,
        value: Measurement<LogicalRect>,
    ) -> Result<(), MeasurementSubmissionError> {
        if self.bounds.is_some() {
            return Err(MeasurementSubmissionError::DuplicateSurfaceBounds);
        }
        self.bounds = Some((key, value));
        Ok(())
    }

    /// Answers the conditional popup-plane bounds requirement.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementSubmissionError::DuplicatePopupPlaneBounds`] when it was already
    /// answered.
    pub fn set_popup_plane_bounds(
        &mut self,
        key: PopupPlaneBoundsKey,
        value: Measurement<LogicalRect>,
    ) -> Result<(), MeasurementSubmissionError> {
        if self.popup_plane_bounds.is_some() {
            return Err(MeasurementSubmissionError::DuplicatePopupPlaneBounds);
        }
        self.popup_plane_bounds = Some((key, value));
        Ok(())
    }

    /// Answers one intrinsic pane-minimum requirement.
    ///
    /// # Errors
    ///
    /// Returns a typed duplicate error when this key was already answered.
    pub fn insert_pane_minimum(
        &mut self,
        key: PaneMinimumKey,
        value: Measurement<LogicalSize>,
    ) -> Result<(), MeasurementSubmissionError> {
        match self.pane_minimums.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(())
            }
            Entry::Occupied(_) => Err(MeasurementSubmissionError::DuplicatePaneMinimum { key }),
        }
    }

    /// Answers one tab-intrinsic requirement.
    ///
    /// # Errors
    ///
    /// Returns a typed duplicate error when this key was already answered.
    pub fn insert_tab_intrinsic(
        &mut self,
        key: TabIntrinsicKey,
        value: Measurement<TabIntrinsic>,
    ) -> Result<(), MeasurementSubmissionError> {
        match self.tab_intrinsics.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(())
            }
            Entry::Occupied(_) => Err(MeasurementSubmissionError::DuplicateTabIntrinsic { key }),
        }
    }

    /// Answers one tab-strip requirement.
    ///
    /// # Errors
    ///
    /// Returns a typed duplicate error when this key was already answered.
    pub fn insert_tab_strip(
        &mut self,
        key: TabStripKey,
        value: Measurement<TabStripMetrics>,
    ) -> Result<(), MeasurementSubmissionError> {
        match self.tab_strips.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                Ok(())
            }
            Entry::Occupied(_) => Err(MeasurementSubmissionError::DuplicateTabStrip { key }),
        }
    }
}

/// Exact-set-validated measurements for one surface.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedSurfaceMeasurements<'a> {
    measurements: &'a SurfaceMeasurements,
}

impl<'a> ValidatedSurfaceMeasurements<'a> {
    /// Requires every exact-set answer to contain authoritative data.
    ///
    /// # Errors
    ///
    /// Returns the first unavailable answer in stable measurement-class and key order.
    pub fn require_authoritative(
        self,
    ) -> Result<AuthoritativeSurfaceMeasurements<'a>, MeasurementAuthorityError> {
        let bounds = match self.measurements.bounds.as_ref() {
            Some((_, Measurement::Measured(bounds))) => *bounds,
            Some((_, Measurement::Unavailable(reason))) => {
                return Err(MeasurementAuthorityError::SurfaceBounds { reason: *reason });
            }
            None => return Err(MeasurementAuthorityError::ValidatedBoundsMissing),
        };
        let popup_plane_bounds = match self.measurements.popup_plane_bounds.as_ref() {
            Some((_, Measurement::Measured(bounds))) => Some(*bounds),
            Some((_, Measurement::Unavailable(reason))) => {
                return Err(MeasurementAuthorityError::PopupPlaneBounds { reason: *reason });
            }
            None => None,
        };
        for (key, value) in &self.measurements.pane_minimums {
            if let Measurement::Unavailable(reason) = value {
                return Err(MeasurementAuthorityError::PaneMinimum {
                    key: *key,
                    reason: *reason,
                });
            }
        }
        for (key, value) in &self.measurements.tab_intrinsics {
            if let Measurement::Unavailable(reason) = value {
                return Err(MeasurementAuthorityError::TabIntrinsic {
                    key: *key,
                    reason: *reason,
                });
            }
        }
        for (key, value) in &self.measurements.tab_strips {
            if let Measurement::Unavailable(reason) = value {
                return Err(MeasurementAuthorityError::TabStrip {
                    key: *key,
                    reason: *reason,
                });
            }
        }
        Ok(AuthoritativeSurfaceMeasurements {
            measurements: self.measurements,
            bounds,
            popup_plane_bounds,
        })
    }
}

/// Complete authoritative inputs accepted for private scene compilation.
#[derive(Debug, Clone, Copy)]
pub struct AuthoritativeSurfaceMeasurements<'a> {
    measurements: &'a SurfaceMeasurements,
    bounds: LogicalRect,
    popup_plane_bounds: Option<LogicalRect>,
}

impl AuthoritativeSurfaceMeasurements<'_> {
    /// Returns the exact core-issued ticket proven by exact-set validation.
    #[must_use]
    pub fn ticket(self) -> SurfaceMeasurementTicket {
        self.measurements.ticket
    }

    /// Returns the authoritative surface-local dock layout bounds.
    #[must_use]
    pub fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the authoritative popup plane when the manifest required one.
    #[must_use]
    pub fn popup_plane_bounds(self) -> Option<LogicalRect> {
        self.popup_plane_bounds
    }

    /// Returns one exact intrinsic pane minimum.
    #[must_use]
    pub fn pane_minimum(self, key: PaneMinimumKey) -> Option<LogicalSize> {
        match self.measurements.pane_minimums.get(&key) {
            Some(Measurement::Measured(value)) => Some(*value),
            Some(Measurement::Unavailable(_)) | None => None,
        }
    }

    /// Returns one exact tab intrinsic.
    #[must_use]
    pub fn tab_intrinsic(self, key: TabIntrinsicKey) -> Option<TabIntrinsic> {
        match self.measurements.tab_intrinsics.get(&key) {
            Some(Measurement::Measured(value)) => Some(*value),
            Some(Measurement::Unavailable(_)) | None => None,
        }
    }

    /// Returns one exact tab-strip measurement.
    #[must_use]
    pub fn tab_strip(self, key: TabStripKey) -> Option<TabStripMetrics> {
        match self.measurements.tab_strips.get(&key) {
            Some(Measurement::Measured(value)) => Some(*value),
            Some(Measurement::Unavailable(_)) | None => None,
        }
    }
}

fn validate_ticket(
    expected: SurfaceMeasurementTicket,
    actual: SurfaceMeasurementTicket,
) -> Result<(), MeasurementValidationError> {
    if expected.authority_domain() != actual.authority_domain() {
        return Err(MeasurementValidationError::AuthorityDomainMismatch {
            expected: expected.authority_domain(),
            actual: actual.authority_domain(),
        });
    }
    if expected.workspace_epoch() != actual.workspace_epoch() {
        return Err(MeasurementValidationError::WorkspaceEpochMismatch {
            expected: expected.workspace_epoch(),
            actual: actual.workspace_epoch(),
        });
    }
    if expected.config() != actual.config() {
        return Err(MeasurementValidationError::ConfigMismatch {
            expected: expected.config(),
            actual: actual.config(),
        });
    }
    if expected.policy() != actual.policy() {
        return Err(MeasurementValidationError::PolicyMismatch {
            expected: expected.policy(),
            actual: actual.policy(),
        });
    }
    if expected.surface_requirement() != actual.surface_requirement() {
        return Err(MeasurementValidationError::SurfaceRequirementMismatch {
            expected: expected.surface_requirement(),
            actual: actual.surface_requirement(),
        });
    }
    if expected.surface() != actual.surface() {
        return Err(MeasurementValidationError::SurfaceMismatch {
            expected: expected.surface(),
            actual: actual.surface(),
        });
    }
    Ok(())
}

macro_rules! exact_set_validator {
    ($function:ident, $key:ty, $missing:ident, $unexpected:ident) => {
        fn $function<T>(
            required: &BTreeSet<$key>,
            submitted: &BTreeMap<$key, Measurement<T>>,
        ) -> Result<(), MeasurementValidationError> {
            if let Some(key) = required.iter().find(|key| !submitted.contains_key(*key)) {
                return Err(MeasurementValidationError::$missing { key: *key });
            }
            if let Some(key) = submitted.keys().find(|key| !required.contains(*key)) {
                return Err(MeasurementValidationError::$unexpected { key: *key });
            }
            Ok(())
        }
    };
}

exact_set_validator!(
    validate_pane_minimum_set,
    PaneMinimumKey,
    MissingPaneMinimum,
    UnexpectedPaneMinimum
);
exact_set_validator!(
    validate_tab_intrinsic_set,
    TabIntrinsicKey,
    MissingTabIntrinsic,
    UnexpectedTabIntrinsic
);
exact_set_validator!(
    validate_tab_strip_set,
    TabStripKey,
    MissingTabStrip,
    UnexpectedTabStrip
);

fn validate_manifest_identity(
    authority_domain: EngineAuthorityDomainId,
    workspace: WorkspaceVersion,
    config: PresentationConfigRevision,
    policy: PolicyRevision,
    surfaces: &BTreeMap<SurfaceId, SurfaceRequirements>,
) -> Result<(), ManifestBuildError> {
    for (surface, requirements) in surfaces {
        let ticket = requirements.ticket();
        if ticket.surface() != *surface {
            return Err(ManifestBuildError::SurfaceMismatch {
                map_key: *surface,
                ticket: ticket.surface(),
            });
        }
        if ticket.authority_domain() != authority_domain {
            return Err(ManifestBuildError::AuthorityDomainMismatch {
                surface: *surface,
                expected: authority_domain,
                actual: ticket.authority_domain(),
            });
        }
        if ticket.workspace_epoch() != workspace.epoch() {
            return Err(ManifestBuildError::WorkspaceEpochMismatch {
                surface: *surface,
                expected: workspace.epoch(),
                actual: ticket.workspace_epoch(),
            });
        }
        if ticket.config() != config {
            return Err(ManifestBuildError::ConfigMismatch {
                surface: *surface,
                expected: config,
                actual: ticket.config(),
            });
        }
        if ticket.policy() != policy {
            return Err(ManifestBuildError::PolicyMismatch {
                surface: *surface,
                expected: policy,
                actual: ticket.policy(),
            });
        }
    }
    Ok(())
}

fn validate_popup_requirement(
    surfaces: &BTreeMap<SurfaceId, SurfaceRequirements>,
    popup: PopupPlaneRequirement,
) -> Result<(), ManifestBuildError> {
    let PopupPlaneRequirement::Active { session, owner, .. } = popup else {
        return Ok(());
    };
    if session.key() != owner {
        return Err(ManifestBuildError::PopupSessionOwnerMismatch {
            session: session.key(),
            owner,
        });
    }
    let requirements = surfaces
        .get(&owner.surface())
        .ok_or(ManifestBuildError::PopupOwnerSurfaceOutsideManifest { owner })?;
    let live_strip = TabStripKey::new(owner.surface(), owner.bar());
    if !requirements.tab_strips.contains(&live_strip) {
        return Err(ManifestBuildError::PopupOwnerTabStripUnavailable { owner });
    }
    let policy = requirements
        .tab_bar(owner.bar())
        .ok_or(ManifestBuildError::PopupOwnerTabStripUnavailable { owner })?
        .policy();
    if policy.interaction() != TabBarInteraction::Enabled {
        return Err(ManifestBuildError::PopupOwnerInteractionDisabled { owner });
    }
    Ok(())
}

/// Invalid core-owned requirement construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RequirementBuildError {
    /// A tab-bar map key disagreed with the requirement's semantic identity.
    #[error("tab-bar requirement key {map_key:?} does not match {requirement:?}")]
    TabBarIdentityMismatch {
        /// Identity used by the surface requirement map.
        map_key: TabBarSceneId,
        /// Identity carried by the mapped requirement.
        requirement: TabBarSceneId,
    },
    /// A tab intrinsic named a different surface than its ticket.
    #[error("tab intrinsic requirement belongs to surface {actual}, expected {expected}")]
    TabIntrinsicSurfaceMismatch {
        /// Ticket surface.
        expected: SurfaceId,
        /// Key surface.
        actual: SurfaceId,
    },
    /// A tab strip named a different surface than its ticket.
    #[error("tab strip requirement belongs to surface {actual}, expected {expected}")]
    TabStripSurfaceMismatch {
        /// Ticket surface.
        expected: SurfaceId,
        /// Key surface.
        actual: SurfaceId,
    },
}

/// Incoherent core-owned aggregate manifest construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ManifestBuildError {
    /// The structural index was derived from another workspace revision.
    #[error("workspace index version {index:?} does not match manifest version {manifest:?}")]
    WorkspaceIndexVersionMismatch {
        /// Exact version named by the manifest.
        manifest: WorkspaceVersion,
        /// Exact version frozen into the structural index.
        index: WorkspaceVersion,
    },
    /// The active session and explicit owner disagree.
    #[error("active popup session strip {session:?} does not match explicit owner {owner:?}")]
    PopupSessionOwnerMismatch {
        /// Strip frozen into the core-minted session.
        session: TabStripStateKey,
        /// Explicit global popup owner.
        owner: TabStripStateKey,
    },
    /// The active popup owner names a surface outside the exact manifest roster.
    #[error("active popup owner {owner:?} names a surface outside the exact manifest roster")]
    PopupOwnerSurfaceOutsideManifest {
        /// Orphan popup owner.
        owner: TabStripStateKey,
    },
    /// The active popup owner is not a live visible tab strip on its surface.
    #[error("active popup owner {owner:?} is not a live visible tab strip")]
    PopupOwnerTabStripUnavailable {
        /// Orphan popup owner.
        owner: TabStripStateKey,
    },
    /// The active popup owner is paint-only under the frozen tab-bar policy.
    #[error("active popup owner {owner:?} has disabled tab-bar interaction")]
    PopupOwnerInteractionDisabled {
        /// Policy-disabled popup owner.
        owner: TabStripStateKey,
    },
    /// A map key and its surface ticket disagree.
    #[error("manifest key surface {map_key} does not match ticket surface {ticket}")]
    SurfaceMismatch {
        /// Surface map key.
        map_key: SurfaceId,
        /// Surface named by the ticket.
        ticket: SurfaceId,
    },
    /// One surface ticket was minted by another engine authority domain.
    #[error("surface {surface} ticket authority {actual:?} does not match {expected:?}")]
    AuthorityDomainMismatch {
        /// Incoherent surface.
        surface: SurfaceId,
        /// Aggregate engine authority domain.
        expected: EngineAuthorityDomainId,
        /// Ticket engine authority domain.
        actual: EngineAuthorityDomainId,
    },
    /// One surface ticket came from another workspace replacement epoch.
    #[error("surface {surface} ticket workspace epoch {actual:?} does not match {expected:?}")]
    WorkspaceEpochMismatch {
        /// Incoherent surface.
        surface: SurfaceId,
        /// Aggregate workspace epoch.
        expected: WorkspaceEpoch,
        /// Ticket workspace epoch.
        actual: WorkspaceEpoch,
    },
    /// One surface ticket came from another presentation configuration.
    #[error("surface {surface} ticket config {actual:?} does not match {expected:?}")]
    ConfigMismatch {
        /// Incoherent surface.
        surface: SurfaceId,
        /// Aggregate configuration revision.
        expected: PresentationConfigRevision,
        /// Ticket configuration revision.
        actual: PresentationConfigRevision,
    },
    /// One surface ticket came from another policy revision.
    #[error("surface {surface} ticket policy {actual:?} does not match {expected:?}")]
    PolicyMismatch {
        /// Incoherent surface.
        surface: SurfaceId,
        /// Aggregate policy revision.
        expected: PolicyRevision,
        /// Ticket policy revision.
        actual: PolicyRevision,
    },
}

/// Rejection of one independently delivered surface contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ManifestMeasurementError {
    /// The contribution ticket names a surface outside the exact manifest roster.
    #[error("measurement surface {surface} is outside the requirement manifest")]
    SurfaceOutsideManifest {
        /// Unexpected surface.
        surface: SurfaceId,
    },
    /// The contribution violated its surface's ticket or exact-set contract.
    #[error("measurement contribution for surface {surface} is invalid: {source}")]
    InvalidContribution {
        /// Surface selected from the contribution ticket.
        surface: SurfaceId,
        /// Exact ticket or set mismatch.
        #[source]
        source: MeasurementValidationError,
    },
}

/// Duplicate answers within one adapter contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MeasurementSubmissionError {
    /// Surface bounds were answered more than once.
    #[error("surface bounds were submitted more than once")]
    DuplicateSurfaceBounds,
    /// Popup-plane bounds were answered more than once.
    #[error("popup-plane bounds were submitted more than once")]
    DuplicatePopupPlaneBounds,
    /// One pane minimum key was answered more than once.
    #[error("pane minimum for {key:?} was submitted more than once")]
    DuplicatePaneMinimum {
        /// Repeated key.
        key: PaneMinimumKey,
    },
    /// One tab intrinsic key was answered more than once.
    #[error("tab intrinsic for {key:?} was submitted more than once")]
    DuplicateTabIntrinsic {
        /// Repeated key.
        key: TabIntrinsicKey,
    },
    /// One tab strip key was answered more than once.
    #[error("tab strip for {key:?} was submitted more than once")]
    DuplicateTabStrip {
        /// Repeated key.
        key: TabStripKey,
    },
}

/// Ticket or exact-set mismatch in one surface contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MeasurementValidationError {
    /// The contribution was minted by a different engine authority domain.
    #[error("measurement authority {actual:?} does not match {expected:?}")]
    AuthorityDomainMismatch {
        /// Required engine authority domain.
        expected: EngineAuthorityDomainId,
        /// Submitted engine authority domain.
        actual: EngineAuthorityDomainId,
    },
    /// The contribution came from a different workspace replacement epoch.
    #[error("measurement workspace epoch {actual:?} does not match {expected:?}")]
    WorkspaceEpochMismatch {
        /// Required workspace epoch.
        expected: WorkspaceEpoch,
        /// Submitted workspace epoch.
        actual: WorkspaceEpoch,
    },
    /// The contribution used a stale or foreign geometry configuration.
    #[error("measurement config {actual:?} does not match {expected:?}")]
    ConfigMismatch {
        /// Required configuration revision.
        expected: PresentationConfigRevision,
        /// Submitted configuration revision.
        actual: PresentationConfigRevision,
    },
    /// The contribution used a stale or foreign policy revision.
    #[error("measurement policy {actual:?} does not match {expected:?}")]
    PolicyMismatch {
        /// Required policy revision.
        expected: PolicyRevision,
        /// Submitted policy revision.
        actual: PolicyRevision,
    },
    /// The contribution used a stale independently published surface requirement.
    #[error("surface requirement {actual:?} does not match {expected:?}")]
    SurfaceRequirementMismatch {
        /// Required surface revision.
        expected: SurfaceRequirementRevision,
        /// Submitted surface revision.
        actual: SurfaceRequirementRevision,
    },
    /// The contribution belongs to another surface.
    #[error("measurement surface {actual} does not match {expected}")]
    SurfaceMismatch {
        /// Required surface.
        expected: SurfaceId,
        /// Submitted surface.
        actual: SurfaceId,
    },
    /// The sole bounds requirement was omitted.
    #[error("surface bounds measurement is missing")]
    MissingSurfaceBounds,
    /// Bounds were submitted under a different key.
    #[error("surface bounds key {actual:?} does not match {expected:?}")]
    UnexpectedSurfaceBounds {
        /// Required key.
        expected: SurfaceBoundsKey,
        /// Submitted key.
        actual: SurfaceBoundsKey,
    },
    /// The conditional popup-plane bounds requirement was omitted.
    #[error("popup-plane bounds measurement is missing")]
    MissingPopupPlaneBounds,
    /// Popup-plane bounds were submitted under a different or inactive key.
    #[error("popup-plane bounds key {actual:?} does not match {expected:?}")]
    UnexpectedPopupPlaneBounds {
        /// Required key, or `None` when no popup plane was requested.
        expected: Option<PopupPlaneBoundsKey>,
        /// Submitted key.
        actual: PopupPlaneBoundsKey,
    },
    /// One required pane minimum was omitted.
    #[error("pane minimum for {key:?} is missing")]
    MissingPaneMinimum {
        /// First missing key.
        key: PaneMinimumKey,
    },
    /// One unrequested pane minimum was submitted.
    #[error("pane minimum for {key:?} was not requested")]
    UnexpectedPaneMinimum {
        /// First unexpected key.
        key: PaneMinimumKey,
    },
    /// One required tab intrinsic was omitted.
    #[error("tab intrinsic for {key:?} is missing")]
    MissingTabIntrinsic {
        /// First missing key.
        key: TabIntrinsicKey,
    },
    /// One unrequested tab intrinsic was submitted.
    #[error("tab intrinsic for {key:?} was not requested")]
    UnexpectedTabIntrinsic {
        /// First unexpected key.
        key: TabIntrinsicKey,
    },
    /// One required tab-strip measurement was omitted.
    #[error("tab strip for {key:?} is missing")]
    MissingTabStrip {
        /// First missing key.
        key: TabStripKey,
    },
    /// One unrequested tab-strip measurement was submitted.
    #[error("tab strip for {key:?} was not requested")]
    UnexpectedTabStrip {
        /// First unexpected key.
        key: TabStripKey,
    },
}

/// First exact requirement that was answered without authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MeasurementAuthorityError {
    /// Exact-set validation produced no bounds entry, indicating an internal protocol defect.
    #[error("validated surface contribution lost its bounds answer")]
    ValidatedBoundsMissing,
    /// Surface bounds were unavailable.
    #[error("surface bounds are unavailable: {reason:?}")]
    SurfaceBounds {
        /// Adapter-provided reason.
        reason: MeasurementUnavailableReason,
    },
    /// Popup-plane bounds were unavailable.
    #[error("popup-plane bounds are unavailable: {reason:?}")]
    PopupPlaneBounds {
        /// Adapter-provided reason.
        reason: MeasurementUnavailableReason,
    },
    /// An intrinsic pane minimum was unavailable.
    #[error("pane minimum for {key:?} is unavailable: {reason:?}")]
    PaneMinimum {
        /// Requirement key.
        key: PaneMinimumKey,
        /// Adapter-provided reason.
        reason: MeasurementUnavailableReason,
    },
    /// A tab intrinsic was unavailable.
    #[error("tab intrinsic for {key:?} is unavailable: {reason:?}")]
    TabIntrinsic {
        /// Requirement key.
        key: TabIntrinsicKey,
        /// Adapter-provided reason.
        reason: MeasurementUnavailableReason,
    },
    /// Tab-strip metrics were unavailable.
    #[error("tab strip for {key:?} is unavailable: {reason:?}")]
    TabStrip {
        /// Requirement key.
        key: TabStripKey,
        /// Adapter-provided reason.
        reason: MeasurementUnavailableReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LogicalSize;
    use crate::ids::{WorkspaceEpoch, WorkspaceRevision};
    use crate::policy::{TabBarInteraction, TabBarPolicy, TabBarVisibility};
    use crate::tab_strip::{PopupRoutingRevision, TabListMenuSessionId};
    use slotmap::SlotMap;

    fn version(epoch: u64, revision: u64) -> WorkspaceVersion {
        WorkspaceVersion::new(WorkspaceEpoch::new(epoch), WorkspaceRevision::new(revision))
    }

    fn authority_domain() -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(1)
    }

    fn keys() -> (
        SurfaceMeasurementTicket,
        PaneMinimumKey,
        TabIntrinsicKey,
        TabStripKey,
    ) {
        let mut nodes = SlotMap::with_key();
        let tabs = nodes.insert(());
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let tab = TabSceneId {
            root,
            tabs,
            item: ItemId::new(3),
        };
        (
            SurfaceMeasurementTicket::new(
                authority_domain(),
                WorkspaceEpoch::new(4),
                PresentationConfigRevision::new(6),
                PolicyRevision::new(7),
                SurfaceRequirementRevision::new(8),
                surface,
            ),
            PaneMinimumKey::new(root, tabs, Some(tab.item)),
            TabIntrinsicKey::new(surface, tab),
            TabStripKey::new(surface, TabBarSceneId { root, tabs }),
        )
    }

    fn requirements() -> SurfaceRequirements {
        let (ticket, pane, tab, strip) = keys();
        SurfaceRequirements::new(
            ticket,
            BTreeSet::from([pane]),
            BTreeSet::from([tab]),
            BTreeSet::from([strip]),
            BTreeMap::new(),
        )
        .expect("fixture requirements should be coherent")
    }

    fn active_requirements(interaction: TabBarInteraction) -> SurfaceRequirements {
        let (ticket, _, _, strip) = keys();
        let bar = strip.bar();
        SurfaceRequirements::new(
            ticket,
            BTreeSet::new(),
            BTreeSet::new(),
            BTreeSet::from([strip]),
            BTreeMap::from([(
                bar,
                TabBarRequirement::new(
                    bar,
                    TabBarPolicy::new(TabBarVisibility::Visible, interaction),
                    BTreeMap::new(),
                ),
            )]),
        )
        .expect("active popup requirements should be coherent")
    }

    fn active_popup(owner: TabStripStateKey) -> PopupPlaneRequirement {
        PopupPlaneRequirement::Active {
            revision: PopupRoutingRevision::default(),
            session: TabListMenuSessionId::new_for_test(owner, 1),
            owner,
        }
    }

    fn manifest_with_popup(
        requirements: SurfaceRequirements,
        popup: PopupPlaneRequirement,
    ) -> Result<SceneRequirementManifest, ManifestBuildError> {
        let ticket = requirements.ticket();
        SceneRequirementDraft::new_for_test(
            ticket.authority_domain(),
            version(4, 5),
            ticket.config(),
            ticket.policy(),
            RequirementRevision::new(9),
            BTreeMap::from([(ticket.surface(), requirements)]),
        )
        .and_then(|draft| draft.finalize(popup))
    }

    fn manifest() -> SceneRequirementManifest {
        let requirements = requirements();
        let ticket = requirements.ticket();
        SceneRequirementDraft::new_for_test(
            ticket.authority_domain(),
            version(4, 5),
            ticket.config(),
            ticket.policy(),
            RequirementRevision::new(9),
            BTreeMap::from([(ticket.surface(), requirements)]),
        )
        .and_then(|draft| draft.finalize(PopupPlaneRequirement::default()))
        .expect("fixture manifest should be coherent")
    }

    #[test]
    fn final_manifest_rejects_orphan_popup_surface_and_tab_bar() {
        let requirements = active_requirements(TabBarInteraction::Enabled);
        let ticket = requirements.ticket();
        let live_bar = requirements
            .tab_strips()
            .next()
            .expect("fixture has one live strip")
            .bar();
        let orphan_surface = TabStripStateKey::new(SurfaceId::new(99), live_bar);
        assert_eq!(
            manifest_with_popup(requirements.clone(), active_popup(orphan_surface)),
            Err(ManifestBuildError::PopupOwnerSurfaceOutsideManifest {
                owner: orphan_surface,
            })
        );

        let orphan_bar = TabStripStateKey::new(
            ticket.surface(),
            TabBarSceneId {
                root: RootId::new(99),
                tabs: live_bar.tabs,
            },
        );
        assert_eq!(
            manifest_with_popup(requirements, active_popup(orphan_bar)),
            Err(ManifestBuildError::PopupOwnerTabStripUnavailable { owner: orphan_bar })
        );
    }

    #[test]
    fn final_manifest_rejects_policy_disabled_popup_owner() {
        let requirements = active_requirements(TabBarInteraction::Disabled);
        let ticket = requirements.ticket();
        let owner = TabStripStateKey::new(
            ticket.surface(),
            requirements
                .tab_strips()
                .next()
                .expect("fixture has one live strip")
                .bar(),
        );
        assert_eq!(
            manifest_with_popup(requirements, active_popup(owner)),
            Err(ManifestBuildError::PopupOwnerInteractionDisabled { owner })
        );
    }

    #[test]
    fn popup_plane_measurement_is_an_active_only_exact_requirement() {
        let active_seed = active_requirements(TabBarInteraction::Enabled);
        let ticket = active_seed.ticket();
        let owner = TabStripStateKey::new(
            ticket.surface(),
            active_seed
                .tab_strips()
                .next()
                .expect("fixture has one live strip")
                .bar(),
        );
        let manifest = manifest_with_popup(active_seed, active_popup(owner))
            .expect("active popup manifest should be coherent");
        let active_requirements = manifest
            .surface(ticket.surface())
            .expect("active owner surface should remain rostered");
        let popup_key = active_requirements
            .popup_plane_bounds()
            .expect("active popup requires an exact plane on every surface");
        assert_eq!(popup_key.surface(), ticket.surface());

        let mut contribution = SurfaceMeasurements::new(active_requirements.ticket());
        contribution
            .set_bounds(
                active_requirements.bounds(),
                Measurement::Measured(
                    LogicalRect::new(20.0, 440.0, 600.0, 120.0)
                        .expect("layout bounds should be valid"),
                ),
            )
            .expect("layout bounds answer should be unique");
        for key in active_requirements.tab_strips() {
            contribution
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        TabStripMetrics::new(0.0, 0.0).expect("strip metrics should be valid"),
                    ),
                )
                .expect("strip answer should be unique");
        }
        assert!(matches!(
            active_requirements.validate(&contribution),
            Err(MeasurementValidationError::MissingPopupPlaneBounds)
        ));

        let popup_plane =
            LogicalRect::new(0.0, 0.0, 640.0, 600.0).expect("popup plane should be valid");
        contribution
            .set_popup_plane_bounds(popup_key, Measurement::Measured(popup_plane))
            .expect("popup-plane answer should be unique");
        let authoritative = active_requirements
            .validate(&contribution)
            .expect("complete exact set should validate")
            .require_authoritative()
            .expect("measured popup plane should be authoritative");
        assert_eq!(authoritative.popup_plane_bounds(), Some(popup_plane));

        let inactive = requirements();
        let mut unexpected = measured_contribution(&inactive);
        let actual = PopupPlaneBoundsKey::new(inactive.ticket().surface());
        unexpected
            .set_popup_plane_bounds(actual, Measurement::Measured(popup_plane))
            .expect("first extra answer should be accepted by the builder");
        assert!(matches!(
            inactive.validate(&unexpected),
            Err(MeasurementValidationError::UnexpectedPopupPlaneBounds {
                expected: None,
                actual: submitted,
            }) if submitted == actual
        ));
    }

    fn measured_contribution(requirements: &SurfaceRequirements) -> SurfaceMeasurements {
        let (_, pane, tab, strip) = keys();
        let mut contribution = SurfaceMeasurements::new(requirements.ticket());
        contribution
            .set_bounds(
                requirements.bounds(),
                Measurement::Measured(
                    LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("valid bounds"),
                ),
            )
            .expect("single bounds answer");
        contribution
            .insert_pane_minimum(
                pane,
                Measurement::Measured(LogicalSize::new(80.0, 60.0).expect("valid minimum")),
            )
            .expect("unique pane answer");
        contribution
            .insert_tab_intrinsic(
                tab,
                Measurement::Measured(TabIntrinsic::new(72.0).expect("valid intrinsic")),
            )
            .expect("unique tab answer");
        contribution
            .insert_tab_strip(
                strip,
                Measurement::Measured(TabStripMetrics::new(0.0, 20.0).expect("valid metrics")),
            )
            .expect("unique strip answer");
        contribution
    }

    #[test]
    fn revisions_advance_without_wrapping() {
        assert_eq!(
            RequirementRevision::new(8).checked_next(),
            Some(RequirementRevision::new(9))
        );
        assert_eq!(RequirementRevision::new(u64::MAX).checked_next(), None);
        assert_eq!(
            SurfaceSceneRevision::new(10).checked_next(),
            Some(SurfaceSceneRevision::new(11))
        );
        assert_eq!(SurfaceSceneRevision::new(u64::MAX).checked_next(), None);
    }

    #[test]
    fn exact_complete_contribution_becomes_authoritative() {
        let manifest = manifest();
        let requirements = manifest
            .surface(SurfaceId::new(1))
            .expect("fixture surface should exist");
        let contribution = measured_contribution(requirements);
        let accepted = manifest
            .validate_surface(&contribution)
            .expect("exact contribution should validate")
            .require_authoritative()
            .expect("all values are measured");

        assert_eq!(accepted.bounds().width().to_bits(), 800.0_f64.to_bits());
        assert!(accepted.pane_minimum(keys().1).is_some());
        assert!(accepted.tab_intrinsic(keys().2).is_some());
        assert!(accepted.tab_strip(keys().3).is_some());
    }

    #[test]
    fn missing_and_unavailable_are_distinct() {
        let requirements = requirements();
        let mut missing = measured_contribution(&requirements);
        missing.tab_intrinsics.clear();
        assert!(matches!(
            requirements.validate(&missing),
            Err(MeasurementValidationError::MissingTabIntrinsic { key }) if key == keys().2
        ));

        let mut unavailable = measured_contribution(&requirements);
        unavailable.tab_intrinsics.insert(
            keys().2,
            Measurement::Unavailable(MeasurementUnavailableReason::TextMetricsUnavailable),
        );
        let validated = requirements
            .validate(&unavailable)
            .expect("unavailable is still an exact answer");
        assert!(matches!(
            validated.require_authoritative(),
            Err(MeasurementAuthorityError::TabIntrinsic { key, reason })
                if key == keys().2
                    && reason == MeasurementUnavailableReason::TextMetricsUnavailable
        ));
    }

    #[test]
    fn foreign_engine_ticket_fails_before_set_checks() {
        let requirements = requirements();
        let foreign_ticket = SurfaceMeasurementTicket::new(
            EngineAuthorityDomainId::new_for_test(2),
            WorkspaceEpoch::new(4),
            PresentationConfigRevision::new(6),
            PolicyRevision::new(7),
            SurfaceRequirementRevision::new(8),
            SurfaceId::new(1),
        );
        let foreign = SurfaceMeasurements::new(foreign_ticket);
        assert!(matches!(
            requirements.validate(&foreign),
            Err(MeasurementValidationError::AuthorityDomainMismatch { .. })
        ));
    }

    #[test]
    fn missing_precedes_unexpected_in_stable_exact_set_order() {
        let requirements = requirements();
        let mut contribution = measured_contribution(&requirements);
        contribution.pane_minimums.clear();
        let (_, pane, _, _) = keys();
        let unexpected = PaneMinimumKey::new(RootId::new(99), pane.tabs(), None);
        contribution.pane_minimums.insert(
            unexpected,
            Measurement::Unavailable(MeasurementUnavailableReason::Deferred),
        );

        assert!(matches!(
            requirements.validate(&contribution),
            Err(MeasurementValidationError::MissingPaneMinimum { key }) if key == pane
        ));
    }

    #[test]
    fn unexpected_answer_is_rejected_by_the_exact_set_contract() {
        let requirements = requirements();
        let mut contribution = measured_contribution(&requirements);
        let (_, pane, _, _) = keys();
        let unexpected = PaneMinimumKey::new(RootId::new(99), pane.tabs(), None);
        contribution.pane_minimums.insert(
            unexpected,
            Measurement::Unavailable(MeasurementUnavailableReason::Deferred),
        );

        assert!(matches!(
            requirements.validate(&contribution),
            Err(MeasurementValidationError::UnexpectedPaneMinimum { key })
                if key == unexpected
        ));
    }

    #[test]
    fn duplicate_answers_are_rejected_without_silent_overwrite() {
        let requirements = requirements();
        let mut contribution = SurfaceMeasurements::new(requirements.ticket());
        let key = requirements.bounds();
        contribution
            .set_bounds(
                key,
                Measurement::Unavailable(MeasurementUnavailableReason::Deferred),
            )
            .expect("first answer should be accepted");
        assert_eq!(
            contribution.set_bounds(
                key,
                Measurement::Unavailable(MeasurementUnavailableReason::SurfaceUnavailable),
            ),
            Err(MeasurementSubmissionError::DuplicateSurfaceBounds)
        );
    }

    #[test]
    fn requirement_construction_rejects_cross_surface_keys() {
        let (ticket, pane, tab, strip) = keys();
        let foreign = TabIntrinsicKey::new(SurfaceId::new(99), tab.tab());
        assert_eq!(
            SurfaceRequirements::new(
                ticket,
                BTreeSet::from([pane]),
                BTreeSet::from([foreign]),
                BTreeSet::from([strip]),
                BTreeMap::new(),
            ),
            Err(RequirementBuildError::TabIntrinsicSurfaceMismatch {
                expected: SurfaceId::new(1),
                actual: SurfaceId::new(99),
            })
        );
    }

    #[test]
    fn manifest_rejects_a_surface_outside_its_exact_roster() {
        let manifest = manifest();
        let foreign_ticket = SurfaceMeasurementTicket::new(
            manifest.authority_domain(),
            manifest.workspace().epoch(),
            manifest.config(),
            manifest.policy(),
            SurfaceRequirementRevision::new(99),
            SurfaceId::new(99),
        );
        assert!(matches!(
            manifest.validate_surface(&SurfaceMeasurements::new(foreign_ticket)),
            Err(ManifestMeasurementError::SurfaceOutsideManifest { surface })
                if surface == SurfaceId::new(99)
        ));
    }

    #[test]
    fn manifest_rejects_a_foreign_engine_ticket_for_an_existing_surface() {
        let manifest = manifest();
        let requirements = manifest
            .surface(SurfaceId::new(1))
            .expect("fixture surface should exist");
        let expected = requirements.ticket();
        let foreign_ticket = SurfaceMeasurementTicket::new(
            EngineAuthorityDomainId::new_for_test(2),
            expected.workspace_epoch(),
            expected.config(),
            expected.policy(),
            expected.surface_requirement(),
            expected.surface(),
        );

        assert!(matches!(
            manifest.validate_surface(&SurfaceMeasurements::new(foreign_ticket)),
            Err(ManifestMeasurementError::InvalidContribution {
                surface,
                source: MeasurementValidationError::AuthorityDomainMismatch { .. },
            }) if surface == SurfaceId::new(1)
        ));
    }

    #[test]
    fn scalar_measurements_reject_non_finite_and_negative_values() {
        assert!(matches!(
            TabIntrinsic::new(f64::NAN),
            Err(MeasurementValueError::NonFinite { .. })
        ));
        assert!(matches!(
            TabStripMetrics::new(-1.0, 0.0),
            Err(MeasurementValueError::Negative { .. })
        ));
        for extent in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                TabStripControlMetric::new(extent, TabStripControlPlacement::ReservedTrailing,),
                Err(MeasurementValueError::NonFinite { .. })
            ));
        }
        assert!(matches!(
            TabStripControlMetric::new(0.0, TabStripControlPlacement::ReservedTrailing),
            Err(MeasurementValueError::NonPositive { .. })
        ));
        assert!(matches!(
            TabStripControlMetric::new(-1.0, TabStripControlPlacement::ReservedTrailing),
            Err(MeasurementValueError::NonPositive { .. })
        ));
    }
}
