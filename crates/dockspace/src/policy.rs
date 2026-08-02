//! Revisioned, renderer-neutral docking policy.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;

use thiserror::Error;

use crate::graph::Axis;
use crate::ids::{ItemId, RootId, SurfaceId};

/// Monotonic engine-local identity of one frozen policy value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[repr(transparent)]
pub struct PolicyRevision(u64);

impl PolicyRevision {
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

/// Stable application identity of one docking compatibility class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[repr(transparent)]
pub struct DockClassId(u64);

impl DockClassId {
    /// Creates a class identity from its stable application representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the stable application representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Requested presentation for a root released outside a dock target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TearOffPresentation {
    /// Present the root inside an existing logical surface.
    Contained,
    /// Request a native surface through the platform protocol.
    Native,
}

/// Explicit behavior when a requested native presentation is unavailable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ContainedFallback {
    /// Cancel the operation without changing presentation mode.
    #[default]
    Disabled,
    /// Permit a contained-floating presentation instead.
    Enabled,
}

/// Independently configurable classes of workspace mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockOperation {
    /// Merge or reorder tabs.
    TabMerge,
    /// Split a target at an edge.
    EdgeSplit,
    /// Resize an existing split.
    SplitterResize,
    /// Present a root in a surface's tiled docking graph.
    TiledPresentation,
    /// Present a root inside an existing surface.
    ContainedFloating,
    /// Change geometry or stacking of an existing contained presentation.
    ContainedTransform,
    /// Present a root on a native surface.
    NativeSurface,
}

/// Payload shape whose compatibility is evaluated without renderer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockPayloadKind {
    /// One item from a tabs node.
    Item,
    /// A complete ordered tabs node.
    TabGroup,
    /// A node and all of its descendants.
    Subtree,
    /// A complete docking root.
    Root,
}

/// Renderer-neutral presentation mode of docking content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockPresentationMode {
    /// Content participates in a surface's tiled docking graph.
    Tiled,
    /// Content is a contained floating root inside a logical surface.
    Contained,
    /// Content is owned by an independent native surface.
    Native,
}

/// Exact destination for a presentation policy request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockPresentationTarget {
    /// Join the tiled graph of an existing surface.
    Tiled(SurfaceId),
    /// Float inside an existing surface.
    Contained(SurfaceId),
    /// Create or reuse an independent native surface with this logical identity.
    Native(SurfaceId),
}

impl DockPresentationTarget {
    /// Returns the requested presentation mode.
    #[must_use]
    pub const fn mode(self) -> DockPresentationMode {
        match self {
            Self::Tiled(_) => DockPresentationMode::Tiled,
            Self::Contained(_) => DockPresentationMode::Contained,
            Self::Native(_) => DockPresentationMode::Native,
        }
    }

    /// Returns the exact destination surface identity.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::Tiled(surface) | Self::Contained(surface) | Self::Native(surface) => surface,
        }
    }
}

/// Drop mutation represented by one semantic target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockDropOperation {
    /// Merge into a tabs target or exact tab gap.
    TabMerge,
    /// Split a target at one edge.
    EdgeSplit,
}

impl DockDropOperation {
    const fn operation(self) -> DockOperation {
        match self {
            Self::TabMerge => DockOperation::TabMerge,
            Self::EdgeSplit => DockOperation::EdgeSplit,
        }
    }
}

/// Stable semantic scope used to apply an exact target rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockTargetRuleKey {
    /// The rule applies to the complete target root.
    Root(RootId),
    /// The rule applies to a target represented by one stable item.
    Item(ItemId),
}

/// Static policy for tab-bar presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TabBarVisibility {
    /// The core presentation contains the tab bar.
    #[default]
    Visible,
    /// The core presentation omits the tab bar and all of its hit regions.
    Hidden,
}

/// Static policy for tab-bar interaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum TabBarInteraction {
    /// Tab selection, reordering, and drag activation may be offered.
    #[default]
    Enabled,
    /// The tab bar is paint-only.
    Disabled,
}

/// Effective tab-bar policy for one semantic target.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct TabBarPolicy {
    visibility: TabBarVisibility,
    interaction: TabBarInteraction,
}

impl TabBarPolicy {
    /// Creates an explicit tab-bar policy.
    #[must_use]
    pub const fn new(visibility: TabBarVisibility, interaction: TabBarInteraction) -> Self {
        Self {
            visibility,
            interaction,
        }
    }

    /// Returns whether the bar exists in semantic presentation.
    #[must_use]
    pub const fn visibility(self) -> TabBarVisibility {
        self.visibility
    }

    /// Returns whether the bar may publish semantic interactions.
    #[must_use]
    pub const fn interaction(self) -> TabBarInteraction {
        self.interaction
    }

    fn intersect(self, other: Self) -> Self {
        let visibility = if self.visibility == TabBarVisibility::Hidden
            || other.visibility == TabBarVisibility::Hidden
        {
            TabBarVisibility::Hidden
        } else {
            TabBarVisibility::Visible
        };
        let interaction = if self.interaction == TabBarInteraction::Disabled
            || other.interaction == TabBarInteraction::Disabled
        {
            TabBarInteraction::Disabled
        } else {
            TabBarInteraction::Enabled
        };
        Self::new(visibility, interaction)
    }
}

/// Static close affordance and protocol capability for one pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum CloseCapability {
    /// No close affordance or close plan may be offered.
    Disabled,
    /// A close plan may be offered, but the application must resolve it synchronously.
    #[default]
    Immediate,
    /// A close plan may be resolved immediately or with a core-issued deferred token.
    DeferredAllowed,
}

impl CloseCapability {
    /// Returns whether any close plan may be offered.
    #[must_use]
    pub const fn allows_close(self) -> bool {
        !matches!(self, Self::Disabled)
    }

    /// Returns whether a close plan may enter the deferred state.
    #[must_use]
    pub const fn allows_deferred(self) -> bool {
        matches!(self, Self::DeferredAllowed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct ClassFilter {
    accepted: Option<BTreeSet<DockClassId>>,
    accept_unclassified: bool,
}

impl ClassFilter {
    fn any() -> Self {
        Self {
            accepted: None,
            accept_unclassified: true,
        }
    }

    fn accepts(&self, class: Option<DockClassId>) -> bool {
        match (class, &self.accepted) {
            (None, _) => self.accept_unclassified,
            (Some(_), None) => true,
            (Some(class), Some(accepted)) => accepted.contains(&class),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct RuleFilter<T: Ord> {
    allowed: Option<BTreeSet<T>>,
}

impl<T> RuleFilter<T>
where
    T: Copy + Ord,
{
    fn any() -> Self {
        Self { allowed: None }
    }

    fn allows(&self, value: T) -> bool {
        self.allowed
            .as_ref()
            .is_none_or(|allowed| allowed.contains(&value))
    }
}

/// Per-item policy facts frozen into a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockItemRule {
    dock_class: Option<DockClassId>,
    source_enabled: bool,
    operations: RuleFilter<DockOperation>,
    allow_undocking: bool,
    close: Option<CloseCapability>,
}

impl DockItemRule {
    /// Creates the permissive rule inherited by an explicitly configured item.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the item's optional docking compatibility class.
    #[must_use]
    pub const fn dock_class(&self) -> Option<DockClassId> {
        self.dock_class
    }

    /// Assigns the item's optional docking compatibility class.
    pub fn set_dock_class(&mut self, class: Option<DockClassId>) {
        self.dock_class = class;
    }

    /// Returns whether this item may participate in a docking payload.
    #[must_use]
    pub const fn source_enabled(&self) -> bool {
        self.source_enabled
    }

    /// Enables or disables this item as a docking source.
    pub fn set_source_enabled(&mut self, enabled: bool) {
        self.source_enabled = enabled;
    }

    /// Restricts this item to the supplied operation classes.
    pub fn set_allowed_operations(&mut self, operations: impl IntoIterator<Item = DockOperation>) {
        self.operations.allowed = Some(operations.into_iter().collect());
    }

    /// Removes operation-specific restrictions from this item.
    pub fn allow_all_operations(&mut self) {
        self.operations.allowed = None;
    }

    /// Returns whether this item may leave its current docking owner.
    #[must_use]
    pub const fn allows_undocking(&self) -> bool {
        self.allow_undocking
    }

    /// Enables or disables operations explicitly classified as undocking.
    pub fn set_allow_undocking(&mut self, allowed: bool) {
        self.allow_undocking = allowed;
    }

    /// Returns this item's close override, or `None` to inherit the workspace default.
    #[must_use]
    pub const fn close_capability(&self) -> Option<CloseCapability> {
        self.close
    }

    /// Replaces this item's close capability override.
    pub fn set_close_capability(&mut self, capability: Option<CloseCapability>) {
        self.close = capability;
    }
}

impl Default for DockItemRule {
    fn default() -> Self {
        Self {
            dock_class: None,
            source_enabled: true,
            operations: RuleFilter::any(),
            allow_undocking: true,
            close: None,
        }
    }
}

/// Per-root source compatibility frozen into a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockSourceRule {
    enabled: bool,
    operations: RuleFilter<DockOperation>,
    payloads: RuleFilter<DockPayloadKind>,
    allow_undocking: bool,
}

impl DockSourceRule {
    /// Creates a permissive source rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables the complete root as a docking source.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Restricts this source to the supplied operation classes.
    pub fn set_allowed_operations(&mut self, operations: impl IntoIterator<Item = DockOperation>) {
        self.operations.allowed = Some(operations.into_iter().collect());
    }

    /// Restricts this source to the supplied payload shapes.
    pub fn set_allowed_payloads(&mut self, payloads: impl IntoIterator<Item = DockPayloadKind>) {
        self.payloads.allowed = Some(payloads.into_iter().collect());
    }

    /// Enables or disables operations explicitly classified as undocking.
    pub fn set_allow_undocking(&mut self, allowed: bool) {
        self.allow_undocking = allowed;
    }
}

impl Default for DockSourceRule {
    fn default() -> Self {
        Self {
            enabled: true,
            operations: RuleFilter::any(),
            payloads: RuleFilter::any(),
            allow_undocking: true,
        }
    }
}

/// Per-target compatibility frozen into a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockTargetRule {
    enabled: bool,
    operations: RuleFilter<DockOperation>,
    payloads: RuleFilter<DockPayloadKind>,
    classes: ClassFilter,
    tab_bar: TabBarPolicy,
}

impl DockTargetRule {
    /// Creates a permissive target rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables this semantic target.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Restricts this target to the supplied operation classes.
    pub fn set_allowed_operations(&mut self, operations: impl IntoIterator<Item = DockOperation>) {
        self.operations.allowed = Some(operations.into_iter().collect());
    }

    /// Restricts this target to the supplied payload shapes.
    pub fn set_allowed_payloads(&mut self, payloads: impl IntoIterator<Item = DockPayloadKind>) {
        self.payloads.allowed = Some(payloads.into_iter().collect());
    }

    /// Restricts classified payload items and explicitly chooses unclassified behavior.
    pub fn set_accepted_classes(
        &mut self,
        classes: impl IntoIterator<Item = DockClassId>,
        accept_unclassified: bool,
    ) {
        self.classes.accepted = Some(classes.into_iter().collect());
        self.classes.accept_unclassified = accept_unclassified;
    }

    /// Removes class restrictions from this target.
    pub fn accept_any_class(&mut self) {
        self.classes = ClassFilter::any();
    }

    /// Replaces this target's tab-bar policy.
    pub fn set_tab_bar(&mut self, policy: TabBarPolicy) {
        self.tab_bar = policy;
    }
}

impl Default for DockTargetRule {
    fn default() -> Self {
        Self {
            enabled: true,
            operations: RuleFilter::any(),
            payloads: RuleFilter::any(),
            classes: ClassFilter::any(),
            tab_bar: TabBarPolicy::default(),
        }
    }
}

/// Per-surface source, target, presentation, and lifecycle policy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockSurfaceRule {
    source_enabled: bool,
    target_enabled: bool,
    source_payloads: RuleFilter<DockPayloadKind>,
    target_payloads: RuleFilter<DockPayloadKind>,
    classes: ClassFilter,
    presentations: RuleFilter<DockPresentationMode>,
    resize_axes: BTreeSet<Axis>,
    tab_bar: TabBarPolicy,
    close_enabled: bool,
}

impl DockSurfaceRule {
    /// Creates a permissive surface rule.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables content from this surface as a docking source.
    pub fn set_source_enabled(&mut self, enabled: bool) {
        self.source_enabled = enabled;
    }

    /// Enables or disables this surface as a docking target.
    pub fn set_target_enabled(&mut self, enabled: bool) {
        self.target_enabled = enabled;
    }

    /// Restricts payload shapes originating from this surface.
    pub fn set_allowed_source_payloads(
        &mut self,
        payloads: impl IntoIterator<Item = DockPayloadKind>,
    ) {
        self.source_payloads.allowed = Some(payloads.into_iter().collect());
    }

    /// Restricts payload shapes accepted by this surface.
    pub fn set_allowed_target_payloads(
        &mut self,
        payloads: impl IntoIterator<Item = DockPayloadKind>,
    ) {
        self.target_payloads.allowed = Some(payloads.into_iter().collect());
    }

    /// Restricts classified payload items and explicitly chooses unclassified behavior.
    pub fn set_accepted_classes(
        &mut self,
        classes: impl IntoIterator<Item = DockClassId>,
        accept_unclassified: bool,
    ) {
        self.classes.accepted = Some(classes.into_iter().collect());
        self.classes.accept_unclassified = accept_unclassified;
    }

    /// Removes class restrictions from this surface.
    pub fn accept_any_class(&mut self) {
        self.classes = ClassFilter::any();
    }

    /// Restricts presentation modes admitted by this surface.
    pub fn set_allowed_presentations(
        &mut self,
        presentations: impl IntoIterator<Item = DockPresentationMode>,
    ) {
        self.presentations.allowed = Some(presentations.into_iter().collect());
    }

    /// Enables or disables resizing along one axis on this surface.
    pub fn set_resize_axis(&mut self, axis: Axis, allowed: bool) {
        set_membership(&mut self.resize_axes, axis, allowed);
    }

    /// Replaces this surface's tab-bar policy.
    pub fn set_tab_bar(&mut self, policy: TabBarPolicy) {
        self.tab_bar = policy;
    }

    /// Enables or disables the static surface-close affordance.
    pub fn set_close_enabled(&mut self, enabled: bool) {
        self.close_enabled = enabled;
    }
}

impl Default for DockSurfaceRule {
    fn default() -> Self {
        Self {
            source_enabled: true,
            target_enabled: true,
            source_payloads: RuleFilter::any(),
            target_payloads: RuleFilter::any(),
            classes: ClassFilter::any(),
            presentations: RuleFilter::any(),
            resize_axes: all_axes(),
            tab_bar: TabBarPolicy::default(),
            close_enabled: true,
        }
    }
}

/// Restrictions applied only when the exact target is the central node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct CentralNodePolicy {
    allow_tab_merge: bool,
    allow_edge_split: bool,
}

impl CentralNodePolicy {
    /// Enables or disables tab merging over the central node.
    pub fn set_allow_tab_merge(&mut self, allowed: bool) {
        self.allow_tab_merge = allowed;
    }

    /// Enables or disables edge splitting over the central node.
    pub fn set_allow_edge_split(&mut self, allowed: bool) {
        self.allow_edge_split = allowed;
    }

    fn allows(self, operation: DockDropOperation) -> bool {
        match operation {
            DockDropOperation::TabMerge => self.allow_tab_merge,
            DockDropOperation::EdgeSplit => self.allow_edge_split,
        }
    }
}

impl Default for CentralNodePolicy {
    fn default() -> Self {
        Self {
            allow_tab_merge: true,
            allow_edge_split: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
struct DockPolicyRules {
    allowed_operations: BTreeSet<DockOperation>,
    allowed_presentations: BTreeSet<DockPresentationMode>,
    contained_fallback: ContainedFallback,
    resize_axes: BTreeSet<Axis>,
    tab_bar: TabBarPolicy,
    close: CloseCapability,
    central_node: CentralNodePolicy,
    item_rules: BTreeMap<ItemId, DockItemRule>,
    source_rules: BTreeMap<RootId, DockSourceRule>,
    target_rules: BTreeMap<DockTargetRuleKey, DockTargetRule>,
    surface_rules: BTreeMap<SurfaceId, DockSurfaceRule>,
}

impl Default for DockPolicyRules {
    fn default() -> Self {
        Self {
            allowed_operations: BTreeSet::from([
                DockOperation::TabMerge,
                DockOperation::EdgeSplit,
                DockOperation::SplitterResize,
                DockOperation::TiledPresentation,
                DockOperation::ContainedFloating,
                DockOperation::ContainedTransform,
            ]),
            allowed_presentations: BTreeSet::from([
                DockPresentationMode::Tiled,
                DockPresentationMode::Contained,
            ]),
            contained_fallback: ContainedFallback::Disabled,
            resize_axes: all_axes(),
            tab_bar: TabBarPolicy::default(),
            close: CloseCapability::default(),
            central_node: CentralNodePolicy::default(),
            item_rules: BTreeMap::new(),
            source_rules: BTreeMap::new(),
            target_rules: BTreeMap::new(),
            surface_rules: BTreeMap::new(),
        }
    }
}

/// Editable workspace policy from which reducer-boundary snapshots are frozen.
///
/// Platform capabilities remain observed facts. Policy permission never claims that a renderer or
/// native provider can perform an operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockPolicy {
    rules: DockPolicyRules,
}

impl DockPolicy {
    /// Creates the conservative default docking policy.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Freezes this complete policy under an engine-owned monotonic revision.
    #[must_use]
    pub fn snapshot(&self, revision: PolicyRevision) -> DockPolicySnapshot {
        DockPolicySnapshot {
            revision,
            policy: self.clone(),
        }
    }

    /// Returns whether center/tab-gap merges are allowed.
    #[must_use]
    pub fn allows_tab_merge(&self) -> bool {
        self.allows(DockOperation::TabMerge)
    }

    /// Enables or disables center/tab-gap merges.
    pub fn set_allow_tab_merge(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::TabMerge, allowed);
    }

    /// Returns whether edge splits are allowed.
    #[must_use]
    pub fn allows_edge_split(&self) -> bool {
        self.allows(DockOperation::EdgeSplit)
    }

    /// Enables or disables edge splits.
    pub fn set_allow_edge_split(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::EdgeSplit, allowed);
    }

    /// Returns whether splitters may be resized on at least one axis.
    #[must_use]
    pub fn allows_splitter_resize(&self) -> bool {
        self.allows(DockOperation::SplitterResize) && !self.rules.resize_axes.is_empty()
    }

    /// Enables or disables splitter resizing on both axes.
    pub fn set_allow_splitter_resize(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::SplitterResize, allowed);
        self.rules.resize_axes = if allowed { all_axes() } else { BTreeSet::new() };
    }

    /// Returns whether roots may be presented in a surface's tiled graph.
    #[must_use]
    pub fn allows_tiled_presentation(&self) -> bool {
        self.allows(DockOperation::TiledPresentation)
            && self
                .rules
                .allowed_presentations
                .contains(&DockPresentationMode::Tiled)
    }

    /// Enables or disables presentation into a surface's tiled graph.
    pub fn set_allow_tiled_presentation(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::TiledPresentation, allowed);
    }

    /// Enables or disables splitter resizing along one axis.
    pub fn set_allow_resize_axis(&mut self, axis: Axis, allowed: bool) {
        set_membership(&mut self.rules.resize_axes, axis, allowed);
    }

    /// Returns whether contained-floating presentation is allowed.
    #[must_use]
    pub fn allows_contained_floating(&self) -> bool {
        self.allows(DockOperation::ContainedFloating)
            && self
                .rules
                .allowed_presentations
                .contains(&DockPresentationMode::Contained)
    }

    /// Enables or disables contained-floating presentation.
    pub fn set_allow_contained_floating(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::ContainedFloating, allowed);
    }

    /// Returns whether existing contained presentations may change geometry or stacking.
    #[must_use]
    pub fn allows_contained_transform(&self) -> bool {
        self.allows(DockOperation::ContainedTransform)
    }

    /// Enables or disables geometry and stacking changes for existing contained presentations.
    pub fn set_allow_contained_transform(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::ContainedTransform, allowed);
    }

    /// Returns whether native-surface requests are allowed by application policy.
    #[must_use]
    pub fn allows_native_surfaces(&self) -> bool {
        self.allows(DockOperation::NativeSurface)
            && self
                .rules
                .allowed_presentations
                .contains(&DockPresentationMode::Native)
    }

    /// Enables or disables native-surface requests.
    pub fn set_allow_native_surfaces(&mut self, allowed: bool) {
        self.set_allowed(DockOperation::NativeSurface, allowed);
    }

    /// Returns the explicit native-to-contained fallback setting.
    #[must_use]
    pub const fn contained_fallback(&self) -> ContainedFallback {
        self.rules.contained_fallback
    }

    /// Sets explicit native-to-contained fallback behavior.
    pub fn set_contained_fallback(&mut self, fallback: ContainedFallback) {
        self.rules.contained_fallback = fallback;
    }

    /// Returns whether one operation class is enabled.
    #[must_use]
    pub fn allows(&self, operation: DockOperation) -> bool {
        self.rules.allowed_operations.contains(&operation)
    }

    /// Enables or disables one operation class.
    pub fn set_allowed(&mut self, operation: DockOperation, allowed: bool) {
        set_membership(&mut self.rules.allowed_operations, operation, allowed);
        match operation {
            DockOperation::TiledPresentation => set_membership(
                &mut self.rules.allowed_presentations,
                DockPresentationMode::Tiled,
                allowed,
            ),
            DockOperation::ContainedFloating => set_membership(
                &mut self.rules.allowed_presentations,
                DockPresentationMode::Contained,
                allowed,
            ),
            DockOperation::NativeSurface => set_membership(
                &mut self.rules.allowed_presentations,
                DockPresentationMode::Native,
                allowed,
            ),
            DockOperation::TabMerge
            | DockOperation::EdgeSplit
            | DockOperation::SplitterResize
            | DockOperation::ContainedTransform => {}
        }
    }

    /// Enables or disables one presentation mode.
    pub fn set_presentation_allowed(&mut self, mode: DockPresentationMode, allowed: bool) {
        set_membership(&mut self.rules.allowed_presentations, mode, allowed);
        match mode {
            DockPresentationMode::Tiled => {
                set_membership(
                    &mut self.rules.allowed_operations,
                    DockOperation::TiledPresentation,
                    allowed,
                );
            }
            DockPresentationMode::Contained => {
                set_membership(
                    &mut self.rules.allowed_operations,
                    DockOperation::ContainedFloating,
                    allowed,
                );
            }
            DockPresentationMode::Native => {
                set_membership(
                    &mut self.rules.allowed_operations,
                    DockOperation::NativeSurface,
                    allowed,
                );
            }
        }
    }

    /// Replaces the workspace-wide tab-bar policy.
    pub fn set_tab_bar(&mut self, policy: TabBarPolicy) {
        self.rules.tab_bar = policy;
    }

    /// Replaces the workspace-wide pane close capability.
    pub fn set_close_capability(&mut self, capability: CloseCapability) {
        self.rules.close = capability;
    }

    /// Returns mutable central-node restrictions.
    #[must_use]
    pub fn central_node_mut(&mut self) -> &mut CentralNodePolicy {
        &mut self.rules.central_node
    }

    /// Inserts or replaces the rule for one stable item.
    pub fn set_item_rule(&mut self, item: ItemId, rule: DockItemRule) -> Option<DockItemRule> {
        self.rules.item_rules.insert(item, rule)
    }

    /// Inserts or replaces the source rule for one root.
    pub fn set_source_rule(
        &mut self,
        root: RootId,
        rule: DockSourceRule,
    ) -> Option<DockSourceRule> {
        self.rules.source_rules.insert(root, rule)
    }

    /// Inserts or replaces one exact target rule.
    pub fn set_target_rule(
        &mut self,
        target: DockTargetRuleKey,
        rule: DockTargetRule,
    ) -> Option<DockTargetRule> {
        self.rules.target_rules.insert(target, rule)
    }

    /// Inserts or replaces the rule for one logical surface.
    pub fn set_surface_rule(
        &mut self,
        surface: SurfaceId,
        rule: DockSurfaceRule,
    ) -> Option<DockSurfaceRule> {
        self.rules.surface_rules.insert(surface, rule)
    }

    /// Checks a tab merge before constructing an unscoped command.
    ///
    /// Context-bearing interaction paths should use [`DockPolicySnapshot::evaluate`].
    pub fn check_tab_merge(&self) -> Result<(), PolicyRejection> {
        self.check_operation(DockOperation::TabMerge)
    }

    /// Checks an edge split before constructing an unscoped command.
    ///
    /// Context-bearing interaction paths should use [`DockPolicySnapshot::evaluate`].
    pub fn check_edge_split(&self) -> Result<(), PolicyRejection> {
        self.check_operation(DockOperation::EdgeSplit)
    }

    /// Checks splitter resizing before constructing an unscoped command.
    ///
    /// Context-bearing interaction paths should use [`DockPolicySnapshot::evaluate`].
    pub fn check_splitter_resize(&self) -> Result<(), PolicyRejection> {
        if self.allows_splitter_resize() {
            Ok(())
        } else {
            Err(PolicyRejection::SplitterResizeDisabled)
        }
    }

    /// Checks one explicitly requested tear-off presentation.
    pub fn check_tear_off(&self, presentation: TearOffPresentation) -> Result<(), PolicyRejection> {
        match presentation {
            TearOffPresentation::Contained if self.allows_contained_floating() => Ok(()),
            TearOffPresentation::Contained => Err(PolicyRejection::ContainedFloatingDisabled),
            TearOffPresentation::Native if self.allows_native_surfaces() => Ok(()),
            TearOffPresentation::Native => Err(PolicyRejection::NativeSurfacesDisabled),
        }
    }

    /// Resolves policy-only fallback after capability rejected a native request.
    ///
    /// This function never inspects pointer position, elapsed time, focus, or window geometry.
    pub fn native_unavailable_fallback(&self) -> Result<TearOffPresentation, PolicyRejection> {
        if self.rules.contained_fallback != ContainedFallback::Enabled {
            return Err(PolicyRejection::ContainedFallbackDisabled);
        }
        self.check_tear_off(TearOffPresentation::Contained)?;
        Ok(TearOffPresentation::Contained)
    }

    fn check_operation(&self, operation: DockOperation) -> Result<(), PolicyRejection> {
        if self.allows(operation) {
            Ok(())
        } else {
            Err(operation_disabled(operation))
        }
    }
}

/// Exact source and payload facts supplied to the pure evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockPayloadSourceFacts {
    root: RootId,
    surface: SurfaceId,
    presentation: DockPresentationMode,
    undocks: bool,
}

impl DockPayloadSourceFacts {
    /// Creates exact facts for content already owned by the workspace.
    #[must_use]
    pub const fn new(
        root: RootId,
        surface: SurfaceId,
        presentation: DockPresentationMode,
        undocks: bool,
    ) -> Self {
        Self {
            root,
            surface,
            presentation,
            undocks,
        }
    }

    /// Returns the source root.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the source surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the source presentation mode.
    #[must_use]
    pub const fn presentation(self) -> DockPresentationMode {
        self.presentation
    }

    /// Returns whether the mutation leaves the current docking owner.
    #[must_use]
    pub const fn undocks(self) -> bool {
        self.undocks
    }
}

/// Exact payload facts supplied to the pure evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockPayloadPolicyFacts {
    kind: DockPayloadKind,
    items: Vec<ItemId>,
    source: Option<DockPayloadSourceFacts>,
}

impl DockPayloadPolicyFacts {
    /// Creates exact payload facts. Empty or duplicate item lists are rejected by evaluation.
    #[must_use]
    pub fn new(
        kind: DockPayloadKind,
        items: impl IntoIterator<Item = ItemId>,
        source_root: RootId,
        source_surface: SurfaceId,
        source_presentation: DockPresentationMode,
        undocks_source: bool,
    ) -> Self {
        Self {
            kind,
            items: items.into_iter().collect(),
            source: Some(DockPayloadSourceFacts::new(
                source_root,
                source_surface,
                source_presentation,
                undocks_source,
            )),
        }
    }

    /// Creates facts for newly opened content with no existing docking owner.
    #[must_use]
    pub fn opened(kind: DockPayloadKind, items: impl IntoIterator<Item = ItemId>) -> Self {
        Self {
            kind,
            items: items.into_iter().collect(),
            source: None,
        }
    }

    /// Returns the payload shape.
    #[must_use]
    pub const fn kind(&self) -> DockPayloadKind {
        self.kind
    }

    /// Returns ordered stable item identities.
    #[must_use]
    pub fn items(&self) -> &[ItemId] {
        &self.items
    }

    /// Returns exact existing-owner facts, or `None` for newly opened content.
    #[must_use]
    pub const fn source(&self) -> Option<DockPayloadSourceFacts> {
        self.source
    }
}

/// Exact target facts supplied to drop policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockDropTargetFacts {
    surface: SurfaceId,
    rule: Option<DockTargetRuleKey>,
    central: bool,
}

impl DockDropTargetFacts {
    /// Creates target facts from core-owned semantic identity.
    #[must_use]
    pub const fn new(surface: SurfaceId, rule: Option<DockTargetRuleKey>, central: bool) -> Self {
        Self {
            surface,
            rule,
            central,
        }
    }

    /// Returns the destination surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact optional target-rule identity.
    #[must_use]
    pub const fn rule(self) -> Option<DockTargetRuleKey> {
        self.rule
    }

    /// Returns whether the core classified this as the central node.
    #[must_use]
    pub const fn is_central(self) -> bool {
        self.central
    }
}

/// Complete drop request for one pure policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockDropPolicyRequest {
    operation: DockDropOperation,
    payload: DockPayloadPolicyFacts,
    target: DockDropTargetFacts,
}

impl DockDropPolicyRequest {
    /// Creates a complete renderer-neutral drop policy request.
    #[must_use]
    pub const fn new(
        operation: DockDropOperation,
        payload: DockPayloadPolicyFacts,
        target: DockDropTargetFacts,
    ) -> Self {
        Self {
            operation,
            payload,
            target,
        }
    }
}

/// Complete presentation request for one pure policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockPresentationPolicyRequest {
    payload: DockPayloadPolicyFacts,
    target: DockPresentationTarget,
}

/// Complete request for mutating one existing contained presentation in place.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockContainedTransformPolicyRequest {
    payload: DockPayloadPolicyFacts,
    surface: SurfaceId,
}

impl DockContainedTransformPolicyRequest {
    /// Creates a renderer-neutral in-place contained transform request.
    #[must_use]
    pub const fn new(payload: DockPayloadPolicyFacts, surface: SurfaceId) -> Self {
        Self { payload, surface }
    }
}

impl DockPresentationPolicyRequest {
    /// Creates a renderer-neutral presentation policy request.
    #[must_use]
    pub const fn new(payload: DockPayloadPolicyFacts, target: DockPresentationTarget) -> Self {
        Self { payload, target }
    }
}

/// Complete resize request for one pure policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockResizePolicyRequest {
    axis: Axis,
    surface: Option<SurfaceId>,
}

impl DockResizePolicyRequest {
    /// Creates a resize request, optionally scoped to one surface.
    #[must_use]
    pub const fn new(axis: Axis, surface: Option<SurfaceId>) -> Self {
        Self { axis, surface }
    }
}

/// Complete tab-bar request for one pure policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockTabBarPolicyRequest {
    surface: SurfaceId,
    target: Option<DockTargetRuleKey>,
}

/// Normalized policy facts for one root owned by a recoverable child surface.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockSurfaceRecoveryRootFacts {
    source_presentation: DockPresentationMode,
    items: BTreeSet<ItemId>,
}

impl DockSurfaceRecoveryRootFacts {
    /// Creates exact renderer-neutral facts for one complete recovery root.
    #[must_use]
    pub fn new(
        source_presentation: DockPresentationMode,
        items: impl IntoIterator<Item = ItemId>,
    ) -> Self {
        Self {
            source_presentation,
            items: items.into_iter().collect(),
        }
    }

    /// Returns the presentation mode owned by the source child surface.
    #[must_use]
    pub const fn source_presentation(&self) -> DockPresentationMode {
        self.source_presentation
    }

    /// Returns the normalized complete item set of this root.
    #[must_use]
    pub const fn items(&self) -> &BTreeSet<ItemId> {
        &self.items
    }
}

/// Complete renderer-neutral authorization request for one child-surface recovery obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockSurfaceRecoveryPolicyRequest {
    source_surface: SurfaceId,
    host_surface: SurfaceId,
    roots: BTreeMap<RootId, DockSurfaceRecoveryRootFacts>,
}

impl DockSurfaceRecoveryPolicyRequest {
    /// Creates a normalized complete-roster recovery request.
    #[must_use]
    pub const fn new(
        source_surface: SurfaceId,
        host_surface: SurfaceId,
        roots: BTreeMap<RootId, DockSurfaceRecoveryRootFacts>,
    ) -> Self {
        Self {
            source_surface,
            host_surface,
            roots,
        }
    }

    /// Returns the child surface whose complete roster is recoverable.
    #[must_use]
    pub const fn source_surface(&self) -> SurfaceId {
        self.source_surface
    }

    /// Returns the exact, unchanged recovery host.
    #[must_use]
    pub const fn host_surface(&self) -> SurfaceId {
        self.host_surface
    }

    /// Returns every normalized source root fact keyed by stable root identity.
    #[must_use]
    pub const fn roots(&self) -> &BTreeMap<RootId, DockSurfaceRecoveryRootFacts> {
        &self.roots
    }
}

impl DockTabBarPolicyRequest {
    /// Creates a tab-bar request for one semantic target.
    #[must_use]
    pub const fn new(surface: SurfaceId, target: Option<DockTargetRuleKey>) -> Self {
        Self { surface, target }
    }
}

/// Renderer-neutral policy query evaluated against exactly one frozen revision.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum DockPolicyRequest {
    /// Evaluate one unique geometric drop winner.
    Drop(DockDropPolicyRequest),
    /// Evaluate one contained or native presentation transition.
    Present(DockPresentationPolicyRequest),
    /// Evaluate an in-place change to an existing contained presentation.
    TransformContained(DockContainedTransformPolicyRequest),
    /// Authorize the durable recovery obligation of one child surface.
    RecoverSurface(DockSurfaceRecoveryPolicyRequest),
    /// Evaluate one split resize.
    Resize(DockResizePolicyRequest),
    /// Evaluate tab-bar interaction availability.
    InteractWithTabBar(DockTabBarPolicyRequest),
    /// Evaluate the static pane-close capability.
    ClosePane {
        /// Stable item being closed.
        item: ItemId,
        /// Whether the application requests deferred resolution.
        deferred: bool,
    },
    /// Evaluate the static surface-close capability.
    CloseSurface {
        /// Logical surface being closed.
        surface: SurfaceId,
    },
}

/// Immutable policy captured at one reducer boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct DockPolicySnapshot {
    revision: PolicyRevision,
    policy: DockPolicy,
}

impl DockPolicySnapshot {
    /// Returns the exact revision carried by all proofs derived from this snapshot.
    #[must_use]
    pub const fn revision(&self) -> PolicyRevision {
        self.revision
    }

    /// Returns the frozen global operation set.
    #[must_use]
    pub fn operations(&self) -> &BTreeSet<DockOperation> {
        &self.policy.rules.allowed_operations
    }

    /// Returns all explicitly configured item rules.
    #[must_use]
    pub fn item_rules(&self) -> &BTreeMap<ItemId, DockItemRule> {
        &self.policy.rules.item_rules
    }

    /// Returns all explicitly configured source-root rules.
    #[must_use]
    pub fn source_rules(&self) -> &BTreeMap<RootId, DockSourceRule> {
        &self.policy.rules.source_rules
    }

    /// Returns all explicitly configured target rules.
    #[must_use]
    pub fn target_rules(&self) -> &BTreeMap<DockTargetRuleKey, DockTargetRule> {
        &self.policy.rules.target_rules
    }

    /// Returns all explicitly configured surface rules.
    #[must_use]
    pub fn surface_rules(&self) -> &BTreeMap<SurfaceId, DockSurfaceRule> {
        &self.policy.rules.surface_rules
    }

    /// Returns an editable copy suitable for constructing the next policy revision.
    #[must_use]
    pub fn to_policy(&self) -> DockPolicy {
        self.policy.clone()
    }

    /// Returns the immutable policy value carried by this revision.
    #[must_use]
    pub const fn policy(&self) -> &DockPolicy {
        &self.policy
    }

    pub(crate) fn has_same_rules(&self, policy: &DockPolicy) -> bool {
        self.policy == *policy
    }

    /// Evaluates one request using no mutable state, framework values, or inferred geometry.
    #[must_use]
    pub fn evaluate(&self, request: &DockPolicyRequest) -> PolicyDecision {
        let result = match request {
            DockPolicyRequest::Drop(request) => self.evaluate_drop(request),
            DockPolicyRequest::Present(request) => self.evaluate_presentation(request),
            DockPolicyRequest::TransformContained(request) => {
                self.evaluate_contained_transform(request)
            }
            DockPolicyRequest::RecoverSurface(request) => self.evaluate_surface_recovery(request),
            DockPolicyRequest::Resize(request) => self.evaluate_resize(*request),
            DockPolicyRequest::InteractWithTabBar(request) => self.evaluate_tab_bar(*request),
            DockPolicyRequest::ClosePane { item, deferred } => {
                self.evaluate_pane_close(*item, *deferred)
            }
            DockPolicyRequest::CloseSurface { surface } => self.evaluate_surface_close(*surface),
        };
        PolicyDecision::from_result(result)
    }

    pub(crate) fn splitter_resize_is_allowed(&self, axis: Axis, surface: SurfaceId) -> bool {
        matches!(
            self.evaluate(&DockPolicyRequest::Resize(DockResizePolicyRequest::new(
                axis,
                Some(surface),
            ))),
            PolicyDecision::Allow
        )
    }

    pub(crate) fn evaluate_drop_facts(
        &self,
        operation: DockDropOperation,
        payload: &DockPayloadPolicyFacts,
        target: DockDropTargetFacts,
    ) -> PolicyDecision {
        PolicyDecision::from_result(self.evaluate_drop_parts(operation, payload, target))
    }

    /// Computes the effective tab-bar policy by intersecting workspace, surface, and target rules.
    #[must_use]
    pub fn tab_bar_policy(&self, request: DockTabBarPolicyRequest) -> TabBarPolicy {
        let mut policy = self.policy.rules.tab_bar;
        if let Some(surface) = self.policy.rules.surface_rules.get(&request.surface) {
            policy = policy.intersect(surface.tab_bar);
        }
        if let Some(target) = request
            .target
            .and_then(|key| self.policy.rules.target_rules.get(&key))
        {
            policy = policy.intersect(target.tab_bar);
        }
        policy
    }

    /// Returns the effective static pane-close capability.
    #[must_use]
    pub fn pane_close_capability(&self, item: ItemId) -> CloseCapability {
        self.policy
            .rules
            .item_rules
            .get(&item)
            .and_then(DockItemRule::close_capability)
            .unwrap_or(self.policy.rules.close)
    }

    fn evaluate_drop(&self, request: &DockDropPolicyRequest) -> Result<(), PolicyRejection> {
        self.evaluate_drop_parts(request.operation, &request.payload, request.target)
    }

    fn evaluate_drop_parts(
        &self,
        request: DockDropOperation,
        payload: &DockPayloadPolicyFacts,
        target: DockDropTargetFacts,
    ) -> Result<(), PolicyRejection> {
        let operation = request.operation();
        self.check_operation(operation)?;
        if target.central && !self.policy.rules.central_node.allows(request) {
            return Err(PolicyRejection::CentralNodeOperationDisabled { operation: request });
        }
        self.evaluate_source(payload, Some(operation))?;
        self.evaluate_target(payload, target, operation)
    }

    fn evaluate_presentation(
        &self,
        request: &DockPresentationPolicyRequest,
    ) -> Result<(), PolicyRejection> {
        let mode = request.target.mode();
        if !self.policy.rules.allowed_presentations.contains(&mode) {
            return Err(PolicyRejection::PresentationModeDisabled { mode });
        }
        let operation = match mode {
            DockPresentationMode::Tiled => Some(DockOperation::TiledPresentation),
            DockPresentationMode::Contained => Some(DockOperation::ContainedFloating),
            DockPresentationMode::Native => Some(DockOperation::NativeSurface),
        };
        if let Some(operation) = operation {
            self.check_operation(operation)?;
        }
        self.evaluate_source(&request.payload, operation)?;
        let surface = request.target.surface();
        self.evaluate_surface_target(&request.payload, surface, Some(mode))
    }

    fn evaluate_contained_transform(
        &self,
        request: &DockContainedTransformPolicyRequest,
    ) -> Result<(), PolicyRejection> {
        let Some(source) = request.payload.source else {
            return Err(PolicyRejection::ContainedTransformRequiresExistingSource);
        };
        if source.presentation != DockPresentationMode::Contained {
            return Err(PolicyRejection::ContainedTransformSourceMismatch {
                source_surface: source.surface,
                source_presentation: source.presentation,
                target_surface: request.surface,
            });
        }

        self.check_operation(DockOperation::ContainedTransform)?;
        self.evaluate_source(&request.payload, Some(DockOperation::ContainedTransform))?;
        self.evaluate_surface_target(&request.payload, request.surface, None)
    }

    fn evaluate_surface_recovery(
        &self,
        request: &DockSurfaceRecoveryPolicyRequest,
    ) -> Result<(), PolicyRejection> {
        if request.source_surface == request.host_surface {
            return Err(PolicyRejection::SurfaceRecoverySourceIsHost {
                surface: request.source_surface,
            });
        }
        for (root, facts) in &request.roots {
            let payload = DockPayloadPolicyFacts::new(
                DockPayloadKind::Root,
                facts.items.iter().copied(),
                *root,
                request.source_surface,
                facts.source_presentation,
                true,
            );
            self.evaluate_presentation(&DockPresentationPolicyRequest::new(
                payload,
                DockPresentationTarget::Contained(request.host_surface),
            ))?;
        }
        Ok(())
    }

    fn evaluate_resize(&self, request: DockResizePolicyRequest) -> Result<(), PolicyRejection> {
        self.check_operation(DockOperation::SplitterResize)?;
        if !self.policy.rules.resize_axes.contains(&request.axis) {
            return Err(PolicyRejection::ResizeAxisDisabled { axis: request.axis });
        }
        if let Some(surface) = request.surface
            && self
                .policy
                .rules
                .surface_rules
                .get(&surface)
                .is_some_and(|rule| !rule.resize_axes.contains(&request.axis))
        {
            return Err(PolicyRejection::SurfaceResizeAxisDisabled {
                surface,
                axis: request.axis,
            });
        }
        Ok(())
    }

    fn evaluate_tab_bar(&self, request: DockTabBarPolicyRequest) -> Result<(), PolicyRejection> {
        let policy = self.tab_bar_policy(request);
        if policy.visibility == TabBarVisibility::Hidden {
            return Err(PolicyRejection::TabBarHidden);
        }
        if policy.interaction == TabBarInteraction::Disabled {
            return Err(PolicyRejection::TabBarInteractionDisabled);
        }
        Ok(())
    }

    fn evaluate_pane_close(&self, item: ItemId, deferred: bool) -> Result<(), PolicyRejection> {
        let capability = self.pane_close_capability(item);
        if !capability.allows_close() {
            return Err(PolicyRejection::PaneCloseDisabled { item });
        }
        if deferred && !capability.allows_deferred() {
            return Err(PolicyRejection::DeferredPaneCloseDisabled { item });
        }
        Ok(())
    }

    fn evaluate_surface_close(&self, surface: SurfaceId) -> Result<(), PolicyRejection> {
        if self
            .policy
            .rules
            .surface_rules
            .get(&surface)
            .is_none_or(|rule| rule.close_enabled)
        {
            Ok(())
        } else {
            Err(PolicyRejection::SurfaceCloseDisabled { surface })
        }
    }

    fn evaluate_source(
        &self,
        payload: &DockPayloadPolicyFacts,
        operation: Option<DockOperation>,
    ) -> Result<(), PolicyRejection> {
        validate_payload_items(payload)?;
        if let Some(source_facts) = payload.source {
            if let Some(surface) = self.policy.rules.surface_rules.get(&source_facts.surface) {
                if !surface.source_enabled {
                    return Err(PolicyRejection::SurfaceSourceDisabled {
                        surface: source_facts.surface,
                    });
                }
                if !surface.source_payloads.allows(payload.kind) {
                    return Err(PolicyRejection::SurfaceSourcePayloadRejected {
                        surface: source_facts.surface,
                        payload: payload.kind,
                    });
                }
            }
            if let Some(source) = self.policy.rules.source_rules.get(&source_facts.root) {
                if !source.enabled {
                    return Err(PolicyRejection::SourceDisabled {
                        root: source_facts.root,
                    });
                }
                if !source.payloads.allows(payload.kind) {
                    return Err(PolicyRejection::SourcePayloadRejected {
                        root: source_facts.root,
                        payload: payload.kind,
                    });
                }
                if let Some(operation) = operation
                    && !source.operations.allows(operation)
                {
                    return Err(PolicyRejection::SourceOperationRejected {
                        root: source_facts.root,
                        operation,
                    });
                }
                if source_facts.undocks && !source.allow_undocking {
                    return Err(PolicyRejection::SourceUndockingDisabled {
                        root: source_facts.root,
                    });
                }
            }
            for item in &payload.items {
                let Some(rule) = self.policy.rules.item_rules.get(item) else {
                    continue;
                };
                if !rule.source_enabled {
                    return Err(PolicyRejection::ItemSourceDisabled { item: *item });
                }
                if let Some(operation) = operation
                    && !rule.operations.allows(operation)
                {
                    return Err(PolicyRejection::ItemOperationRejected {
                        item: *item,
                        operation,
                    });
                }
                if source_facts.undocks && !rule.allow_undocking {
                    return Err(PolicyRejection::ItemUndockingDisabled { item: *item });
                }
            }
        }
        Ok(())
    }

    fn evaluate_target(
        &self,
        payload: &DockPayloadPolicyFacts,
        target: DockDropTargetFacts,
        operation: DockOperation,
    ) -> Result<(), PolicyRejection> {
        self.evaluate_surface_target(payload, target.surface, None)?;
        let Some(target_key) = target.rule else {
            return Ok(());
        };
        let Some(rule) = self.policy.rules.target_rules.get(&target_key) else {
            return Ok(());
        };
        if !rule.enabled {
            return Err(PolicyRejection::TargetDisabled { target: target_key });
        }
        if !rule.operations.allows(operation) {
            return Err(PolicyRejection::TargetOperationRejected {
                target: target_key,
                operation,
            });
        }
        if !rule.payloads.allows(payload.kind) {
            return Err(PolicyRejection::TargetPayloadRejected {
                target: target_key,
                payload: payload.kind,
            });
        }
        self.evaluate_class_filter(payload, &rule.classes, PolicyRuleScope::Target(target_key))
    }

    fn evaluate_surface_target(
        &self,
        payload: &DockPayloadPolicyFacts,
        surface: SurfaceId,
        presentation: Option<DockPresentationMode>,
    ) -> Result<(), PolicyRejection> {
        let Some(rule) = self.policy.rules.surface_rules.get(&surface) else {
            return Ok(());
        };
        if !rule.target_enabled {
            return Err(PolicyRejection::SurfaceTargetDisabled { surface });
        }
        if let Some(mode) = presentation
            && !rule.presentations.allows(mode)
        {
            return Err(PolicyRejection::SurfacePresentationModeRejected { surface, mode });
        }
        if !rule.target_payloads.allows(payload.kind) {
            return Err(PolicyRejection::SurfaceTargetPayloadRejected {
                surface,
                payload: payload.kind,
            });
        }
        self.evaluate_class_filter(payload, &rule.classes, PolicyRuleScope::Surface(surface))
    }

    fn evaluate_class_filter(
        &self,
        payload: &DockPayloadPolicyFacts,
        filter: &ClassFilter,
        scope: PolicyRuleScope,
    ) -> Result<(), PolicyRejection> {
        for item in &payload.items {
            let class = self
                .policy
                .rules
                .item_rules
                .get(item)
                .and_then(DockItemRule::dock_class);
            if !filter.accepts(class) {
                return Err(PolicyRejection::DockClassRejected {
                    item: *item,
                    class,
                    scope,
                });
            }
        }
        Ok(())
    }

    fn check_operation(&self, operation: DockOperation) -> Result<(), PolicyRejection> {
        if self.policy.rules.allowed_operations.contains(&operation) {
            Ok(())
        } else {
            Err(operation_disabled(operation))
        }
    }
}

impl Default for DockPolicySnapshot {
    fn default() -> Self {
        DockPolicy::default().snapshot(PolicyRevision::default())
    }
}

impl Deref for DockPolicySnapshot {
    type Target = DockPolicy;

    fn deref(&self) -> &Self::Target {
        &self.policy
    }
}

/// Scope whose exact compatibility rule rejected a payload item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum PolicyRuleScope {
    /// An exact semantic target rule.
    Target(DockTargetRuleKey),
    /// A logical surface rule.
    Surface(SurfaceId),
}

/// Deterministic result of one pure policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum PolicyDecision {
    /// Every applicable rule accepts the request.
    Allow,
    /// One exact rule rejects the request for the contained reason.
    Reject(PolicyRejection),
}

impl PolicyDecision {
    fn from_result(result: Result<(), PolicyRejection>) -> Self {
        match result {
            Ok(()) => Self::Allow,
            Err(reason) => Self::Reject(reason),
        }
    }

    /// Returns the rejection reason, if this decision rejected the request.
    #[must_use]
    pub const fn rejection(self) -> Option<PolicyRejection> {
        match self {
            Self::Allow => None,
            Self::Reject(reason) => Some(reason),
        }
    }
}

/// A deterministic policy rejection, independent of renderer capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum PolicyRejection {
    /// Center/tab-gap merge is disabled.
    #[error("tab merging is disabled by workspace policy")]
    TabMergeDisabled,
    /// Edge splitting is disabled.
    #[error("edge splitting is disabled by workspace policy")]
    EdgeSplitDisabled,
    /// Tiled presentation is disabled.
    #[error("tiled presentation is disabled by workspace policy")]
    TiledPresentationDisabled,
    /// Splitter resizing is disabled.
    #[error("splitter resizing is disabled by workspace policy")]
    SplitterResizeDisabled,
    /// Contained-floating presentation is disabled.
    #[error("contained-floating presentation is disabled by workspace policy")]
    ContainedFloatingDisabled,
    /// Existing contained presentations may not change geometry or stacking.
    #[error("contained presentation transforms are disabled by workspace policy")]
    ContainedTransformDisabled,
    /// Native surfaces are disabled by application policy.
    #[error("native surfaces are disabled by workspace policy")]
    NativeSurfacesDisabled,
    /// Native-to-contained fallback was not explicitly enabled.
    #[error("contained fallback for unavailable native presentation is disabled")]
    ContainedFallbackDisabled,
    /// A child surface cannot recover into itself.
    #[error("surface {surface} cannot be its own recovery host")]
    SurfaceRecoverySourceIsHost {
        /// Invalid source and host surface.
        surface: SurfaceId,
    },
    /// The core supplied an empty payload.
    #[error("policy payload is empty")]
    EmptyPayload,
    /// The core supplied the same item more than once.
    #[error("policy payload contains duplicate item {item}")]
    DuplicatePayloadItem {
        /// Repeated stable item identity.
        item: ItemId,
    },
    /// One presentation mode is disabled globally.
    #[error("presentation mode {mode:?} is disabled by workspace policy")]
    PresentationModeDisabled {
        /// Rejected presentation mode.
        mode: DockPresentationMode,
    },
    /// A contained transform was requested for content without an existing owner.
    #[error("contained presentation transforms require an existing contained source")]
    ContainedTransformRequiresExistingSource,
    /// A contained transform was requested for a source in another presentation mode.
    #[error(
        "contained transform from {source_presentation:?} surface {source_surface} cannot target contained surface {target_surface}"
    )]
    ContainedTransformSourceMismatch {
        /// Exact source surface.
        source_surface: SurfaceId,
        /// Exact source presentation mode.
        source_presentation: DockPresentationMode,
        /// Requested in-place target surface.
        target_surface: SurfaceId,
    },
    /// One resize axis is disabled globally.
    #[error("resize axis {axis:?} is disabled by workspace policy")]
    ResizeAxisDisabled {
        /// Rejected axis.
        axis: Axis,
    },
    /// One resize axis is disabled for a surface.
    #[error("resize axis {axis:?} is disabled on surface {surface}")]
    SurfaceResizeAxisDisabled {
        /// Rejected surface.
        surface: SurfaceId,
        /// Rejected axis.
        axis: Axis,
    },
    /// A central-node restriction rejects the exact operation.
    #[error("operation {operation:?} is disabled over the central node")]
    CentralNodeOperationDisabled {
        /// Rejected drop operation.
        operation: DockDropOperation,
    },
    /// The source surface is disabled.
    #[error("surface {surface} is disabled as a docking source")]
    SurfaceSourceDisabled {
        /// Rejected source surface.
        surface: SurfaceId,
    },
    /// The source surface rejects the payload shape.
    #[error("surface {surface} rejects source payload {payload:?}")]
    SurfaceSourcePayloadRejected {
        /// Rejected source surface.
        surface: SurfaceId,
        /// Rejected payload shape.
        payload: DockPayloadKind,
    },
    /// The source root is disabled.
    #[error("root {root} is disabled as a docking source")]
    SourceDisabled {
        /// Rejected source root.
        root: RootId,
    },
    /// The source root rejects the payload shape.
    #[error("root {root} rejects payload {payload:?}")]
    SourcePayloadRejected {
        /// Rejected source root.
        root: RootId,
        /// Rejected payload shape.
        payload: DockPayloadKind,
    },
    /// The source root rejects the operation.
    #[error("root {root} rejects operation {operation:?}")]
    SourceOperationRejected {
        /// Rejected source root.
        root: RootId,
        /// Rejected operation.
        operation: DockOperation,
    },
    /// The source root may not be undocked.
    #[error("root {root} may not be undocked")]
    SourceUndockingDisabled {
        /// Rejected source root.
        root: RootId,
    },
    /// One item is disabled as a docking source.
    #[error("item {item} is disabled as a docking source")]
    ItemSourceDisabled {
        /// Rejected item.
        item: ItemId,
    },
    /// One item rejects the operation.
    #[error("item {item} rejects operation {operation:?}")]
    ItemOperationRejected {
        /// Rejected item.
        item: ItemId,
        /// Rejected operation.
        operation: DockOperation,
    },
    /// One item may not be undocked.
    #[error("item {item} may not be undocked")]
    ItemUndockingDisabled {
        /// Rejected item.
        item: ItemId,
    },
    /// The destination surface is disabled.
    #[error("surface {surface} is disabled as a docking target")]
    SurfaceTargetDisabled {
        /// Rejected destination surface.
        surface: SurfaceId,
    },
    /// The destination surface rejects the payload shape.
    #[error("surface {surface} rejects target payload {payload:?}")]
    SurfaceTargetPayloadRejected {
        /// Rejected destination surface.
        surface: SurfaceId,
        /// Rejected payload shape.
        payload: DockPayloadKind,
    },
    /// The destination surface rejects the presentation mode.
    #[error("surface {surface} rejects presentation mode {mode:?}")]
    SurfacePresentationModeRejected {
        /// Rejected destination surface.
        surface: SurfaceId,
        /// Rejected presentation mode.
        mode: DockPresentationMode,
    },
    /// The exact target is disabled.
    #[error("target {target:?} is disabled by workspace policy")]
    TargetDisabled {
        /// Rejected exact target.
        target: DockTargetRuleKey,
    },
    /// The exact target rejects the operation.
    #[error("target {target:?} rejects operation {operation:?}")]
    TargetOperationRejected {
        /// Rejected exact target.
        target: DockTargetRuleKey,
        /// Rejected operation.
        operation: DockOperation,
    },
    /// The exact target rejects the payload shape.
    #[error("target {target:?} rejects payload {payload:?}")]
    TargetPayloadRejected {
        /// Rejected exact target.
        target: DockTargetRuleKey,
        /// Rejected payload shape.
        payload: DockPayloadKind,
    },
    /// A target or surface rejects one item's docking class.
    #[error("item {item} with class {class:?} is rejected by {scope:?}")]
    DockClassRejected {
        /// Rejected payload item.
        item: ItemId,
        /// Item class, or `None` for an explicitly unclassified item.
        class: Option<DockClassId>,
        /// Exact rejecting rule scope.
        scope: PolicyRuleScope,
    },
    /// The semantic tab bar is hidden.
    #[error("tab bar is hidden by workspace policy")]
    TabBarHidden,
    /// The semantic tab bar is paint-only.
    #[error("tab bar interaction is disabled by workspace policy")]
    TabBarInteractionDisabled,
    /// One pane has no static close capability.
    #[error("pane item {item} cannot be closed by workspace policy")]
    PaneCloseDisabled {
        /// Rejected pane item.
        item: ItemId,
    },
    /// One pane does not allow deferred close resolution.
    #[error("pane item {item} cannot defer close resolution")]
    DeferredPaneCloseDisabled {
        /// Rejected pane item.
        item: ItemId,
    },
    /// One surface has no static close capability.
    #[error("surface {surface} cannot be closed by workspace policy")]
    SurfaceCloseDisabled {
        /// Rejected surface.
        surface: SurfaceId,
    },
}

fn all_axes() -> BTreeSet<Axis> {
    BTreeSet::from([Axis::Horizontal, Axis::Vertical])
}

fn set_membership<T>(set: &mut BTreeSet<T>, value: T, present: bool)
where
    T: Ord,
{
    if present {
        set.insert(value);
    } else {
        set.remove(&value);
    }
}

fn validate_payload_items(payload: &DockPayloadPolicyFacts) -> Result<(), PolicyRejection> {
    if payload.items.is_empty() && payload.kind != DockPayloadKind::Root {
        return Err(PolicyRejection::EmptyPayload);
    }
    let mut unique = BTreeSet::new();
    for item in &payload.items {
        if !unique.insert(*item) {
            return Err(PolicyRejection::DuplicatePayloadItem { item: *item });
        }
    }
    Ok(())
}

fn operation_disabled(operation: DockOperation) -> PolicyRejection {
    match operation {
        DockOperation::TabMerge => PolicyRejection::TabMergeDisabled,
        DockOperation::EdgeSplit => PolicyRejection::EdgeSplitDisabled,
        DockOperation::TiledPresentation => PolicyRejection::TiledPresentationDisabled,
        DockOperation::SplitterResize => PolicyRejection::SplitterResizeDisabled,
        DockOperation::ContainedFloating => PolicyRejection::ContainedFloatingDisabled,
        DockOperation::ContainedTransform => PolicyRejection::ContainedTransformDisabled,
        DockOperation::NativeSurface => PolicyRejection::NativeSurfacesDisabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_contained_policies_are_independent() {
        let mut policy = DockPolicy::default();

        assert_eq!(
            policy.check_tear_off(TearOffPresentation::Native),
            Err(PolicyRejection::NativeSurfacesDisabled)
        );
        assert!(
            policy
                .check_tear_off(TearOffPresentation::Contained)
                .is_ok()
        );

        policy.set_allow_native_surfaces(true);
        policy.set_allow_contained_floating(false);
        assert!(policy.check_tear_off(TearOffPresentation::Native).is_ok());
        assert_eq!(
            policy.check_tear_off(TearOffPresentation::Contained),
            Err(PolicyRejection::ContainedFloatingDisabled)
        );
    }

    #[test]
    fn contained_fallback_requires_two_explicit_permissions() {
        let mut policy = DockPolicy::default();
        assert_eq!(
            policy.native_unavailable_fallback(),
            Err(PolicyRejection::ContainedFallbackDisabled)
        );

        policy.set_contained_fallback(ContainedFallback::Enabled);
        assert_eq!(
            policy.native_unavailable_fallback(),
            Ok(TearOffPresentation::Contained)
        );

        policy.set_allow_contained_floating(false);
        assert_eq!(
            policy.native_unavailable_fallback(),
            Err(PolicyRejection::ContainedFloatingDisabled)
        );
    }
}
