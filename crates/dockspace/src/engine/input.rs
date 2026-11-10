//! Public engine input protocol and opaque prepared interaction proofs.

use super::*;
use crate::intent::PointerId;
use crate::scene::{ContainedResizeDirection, SplitterJunctionId, TabSceneId};

/// A document-validated workspace replacement carrying its opaque allocator lineage.
///
/// Values can only be produced by the strict document restore path. This prevents
/// application code from pairing a valid workspace with fabricated presentation
/// tombstones or from silently resetting an engine's retired identity frontier.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedWorkspaceRestore {
    workspace: Workspace,
    presentation_identity_frontier: PresentationIdentityFrontier,
    native_surfaces: Option<crate::frame::NativeSurfaceRestorePlan>,
    root_continuations: Vec<crate::frame::PreparedNativeRootContinuation>,
}

impl ValidatedWorkspaceRestore {
    #[cfg(feature = "serde")]
    pub(crate) fn new(
        workspace: Workspace,
        presentation_identity_frontier: PresentationIdentityFrontier,
    ) -> Self {
        Self {
            workspace,
            presentation_identity_frontier,
            native_surfaces: None,
            root_continuations: Vec::new(),
        }
    }

    /// Returns the strictly validated workspace without exposing allocator mutation.
    #[must_use]
    pub const fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub(crate) const fn presentation_identity_frontier(&self) -> PresentationIdentityFrontier {
        self.presentation_identity_frontier
    }

    #[cfg(feature = "serde")]
    pub(crate) fn with_native_surfaces(
        mut self,
        plan: crate::frame::NativeSurfaceRestorePlan,
    ) -> Self {
        self.native_surfaces = Some(plan);
        self
    }

    pub(crate) fn native_surfaces(&self) -> Option<&crate::frame::NativeSurfaceRestorePlan> {
        self.native_surfaces.as_ref()
    }

    #[cfg(feature = "serde")]
    pub(crate) fn with_root_continuations(
        mut self,
        continuations: Vec<crate::frame::PreparedNativeRootContinuation>,
    ) -> Self {
        self.root_continuations = continuations;
        self
    }

    pub(crate) fn root_continuations(&self) -> &[crate::frame::PreparedNativeRootContinuation] {
        &self.root_continuations
    }

    pub(crate) fn into_parts(self) -> (Workspace, PresentationIdentityFrontier) {
        (self.workspace, self.presentation_identity_frontier)
    }
}

/// Scene-proof-bearing non-pointer activation of one tab-strip control.
///
/// Values are minted only by [`HostFrameView::prepare_tab_strip_control_activation`].
/// The exact presented output, popup-plane revision, structural strip, and
/// control record remain private so callers cannot assemble a stale activation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedTabStripControlActivation {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) control: FrozenTabStripControlClick,
}

impl PreparedTabStripControlActivation {
    /// Returns the stable control identity carried by this prepared activation.
    #[must_use]
    pub const fn control(&self) -> TabStripControlId {
        self.control.record.id()
    }
}

/// Scene-proof-bearing non-pointer activation of one tab-list menu row.
///
/// Values are minted only by [`HostFrameView::prepare_tab_list_menu_row_activation`].
/// The selected item remains inspectable while all mutable authority stays
/// opaque and is revalidated by the reducer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparedTabListMenuRowActivation {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) row: FrozenTabListMenuRowClick,
}

/// Scene-proof-bearing non-pointer dismissal of the exact active tab-list menu.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTabListMenuDismiss {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) session: crate::tab_strip::TabListMenuSessionId,
    pub(super) revision: PopupRoutingRevision,
    pub(super) record: TabListMenuRecord,
    pub(super) backdrop: TabListMenuBackdropRecord,
}

impl PreparedTabListMenuDismiss {
    /// Returns the exact active menu session carried by this proof.
    #[must_use]
    pub const fn session(&self) -> crate::tab_strip::TabListMenuSessionId {
        self.session
    }
}

impl PreparedTabListMenuRowActivation {
    /// Returns the exact active menu session frozen by this activation.
    #[must_use]
    pub const fn session(&self) -> crate::tab_strip::TabListMenuSessionId {
        self.row.session
    }

    /// Returns the stable tab identity selected by this activation.
    #[must_use]
    pub const fn tab(&self) -> crate::scene::TabSceneId {
        self.row.record.tab()
    }
}

/// Validated scroll intent for one core-owned tab strip or tab-list menu.
///
/// The private representation prevents callers from smuggling non-finite
/// deltas into a prepared interaction.
#[derive(Debug, Clone, PartialEq)]
pub struct TabScrollAdjustment(pub(super) TabScrollAdjustmentKind);

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TabScrollAdjustmentKind {
    ScrollByPreserving {
        delta: f64,
        keep_visible: Vec<ItemId>,
    },
    RevealItem(ItemId),
}

impl TabScrollAdjustment {
    /// Creates a signed logical scroll delta. Positive values move toward the
    /// end of the ordered roster.
    pub fn scroll_by(delta: f64) -> Result<Self, TabScrollAdjustmentError> {
        Self::scroll_by_preserving(delta, [])
    }

    /// Creates one signed delta while preserving the largest compatible
    /// prefix of the ordered item roster as operable tab chrome.
    pub fn scroll_by_preserving(
        delta: f64,
        keep_visible: impl IntoIterator<Item = ItemId>,
    ) -> Result<Self, TabScrollAdjustmentError> {
        if !delta.is_finite() {
            return Err(TabScrollAdjustmentError::NonFiniteDelta);
        }
        let keep_visible = keep_visible.into_iter().collect::<Vec<_>>();
        let mut unique = BTreeSet::new();
        for item in &keep_visible {
            if !unique.insert(*item) {
                return Err(TabScrollAdjustmentError::DuplicatePreservedItem { item: *item });
            }
        }
        Ok(Self(TabScrollAdjustmentKind::ScrollByPreserving {
            delta,
            keep_visible,
        }))
    }

    /// Requests the minimum movement needed to reveal one stable item.
    #[must_use]
    pub const fn reveal_item(item: ItemId) -> Self {
        Self(TabScrollAdjustmentKind::RevealItem(item))
    }
}

/// Invalid caller-supplied tab scroll intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TabScrollAdjustmentError {
    /// Scroll deltas must be finite logical values.
    #[error("tab scroll delta must be finite")]
    NonFiniteDelta,
    /// Preserve rosters are ordered sets; duplicate identity is ambiguous.
    #[error("tab scroll preserve roster contains duplicate item {item}")]
    DuplicatePreservedItem {
        /// Duplicated stable item identity.
        item: ItemId,
    },
}

/// Exact presented-authority proof for one tab-strip scroll adjustment.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTabStripScroll {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) key: TabStripStateKey,
    pub(super) record: TabBarRecord,
    pub(super) adjustment: TabScrollAdjustment,
}

impl PreparedTabStripScroll {
    /// Returns the stable structural tab bar carried by this proof.
    #[must_use]
    pub const fn bar(&self) -> crate::scene::TabBarSceneId {
        self.key.bar()
    }
}

/// Exact popup-session proof for one tab-list menu scroll adjustment.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTabListMenuScroll {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) session: crate::tab_strip::TabListMenuSessionId,
    pub(super) revision: PopupRoutingRevision,
    pub(super) record: TabListMenuRecord,
    pub(super) adjustment: TabScrollAdjustment,
}

impl PreparedTabListMenuScroll {
    /// Returns the exact active menu session carried by this proof.
    #[must_use]
    pub const fn session(&self) -> crate::tab_strip::TabListMenuSessionId {
        self.session
    }
}

/// Device-independent navigation intent for the sole active tab-list menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabListMenuNavigation {
    /// Move to the preceding item, clamped at the first item.
    Previous,
    /// Move to the following item, clamped at the last item.
    Next,
    /// Move to the first ordered item.
    First,
    /// Move to the last ordered item.
    Last,
    /// Focus one exact stable item, as requested by accessibility input.
    Focus(ItemId),
}

/// Exact popup-session proof for one menu focus/navigation adjustment.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedTabListMenuNavigation {
    pub(super) presentation: FrozenPresentationAuthority,
    pub(super) session: crate::tab_strip::TabListMenuSessionId,
    pub(super) revision: PopupRoutingRevision,
    pub(super) record: TabListMenuRecord,
    pub(super) target: ItemId,
}

impl PreparedTabListMenuNavigation {
    /// Returns the exact active menu session carried by this proof.
    #[must_use]
    pub const fn session(&self) -> crate::tab_strip::TabListMenuSessionId {
        self.session
    }

    /// Returns the core-resolved focus target.
    #[must_use]
    pub const fn target(&self) -> ItemId {
        self.target
    }
}

/// One current-frame tab-strip or tab-list-menu action prepared from an exact
/// Ready paint candidate.
///
/// The runtime facade keeps this type private. It carries the structural
/// records required for core revalidation without exposing scene or popup
/// authority to renderer adapters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LocalTabChromeAction {
    /// Activate one exact scroll or tab-list-menu control.
    ActivateControl(FrozenTabStripControlClick),
    /// Scroll one exact current tab-strip viewport.
    ScrollStrip {
        key: crate::tab_strip::TabStripStateKey,
        record: TabBarRecord,
        adjustment: TabScrollAdjustment,
    },
    /// Select one exact row from the active tab-list menu.
    ActivateMenuRow(FrozenTabListMenuRowClick),
    /// Dismiss the exact active tab-list menu.
    DismissMenu {
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        record: TabListMenuRecord,
        backdrop: TabListMenuBackdropRecord,
    },
    /// Dismiss the active menu from a non-owner surface's backdrop.
    DismissMenuFromBackdrop(FrozenTabListMenuBackdropClick),
    /// Scroll the exact active tab-list menu.
    ScrollMenu {
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        record: TabListMenuRecord,
        adjustment: TabScrollAdjustment,
    },
    /// Move focus within the exact active tab-list menu.
    NavigateMenu {
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        record: TabListMenuRecord,
        target: ItemId,
    },
}

impl LocalTabChromeAction {
    fn same_intent(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ActivateControl(a), Self::ActivateControl(b)) => {
                a.key == b.key && a.record.id() == b.record.id()
            }
            (
                Self::ScrollStrip {
                    key: a,
                    adjustment: ad,
                    ..
                },
                Self::ScrollStrip {
                    key: b,
                    adjustment: bd,
                    ..
                },
            ) => a == b && ad == bd,
            (Self::ActivateMenuRow(a), Self::ActivateMenuRow(b)) => {
                a.session == b.session && a.record.tab() == b.record.tab()
            }
            (Self::DismissMenu { session: a, .. }, Self::DismissMenu { session: b, .. }) => a == b,
            (Self::DismissMenuFromBackdrop(a), Self::DismissMenuFromBackdrop(b)) => {
                a.session == b.session
            }
            (Self::DismissMenu { session, .. }, Self::DismissMenuFromBackdrop(backdrop))
            | (Self::DismissMenuFromBackdrop(backdrop), Self::DismissMenu { session, .. }) => {
                *session == backdrop.session
            }
            (
                Self::ScrollMenu {
                    session: a,
                    adjustment: ad,
                    ..
                },
                Self::ScrollMenu {
                    session: b,
                    adjustment: bd,
                    ..
                },
            ) => a == b && ad == bd,
            (
                Self::NavigateMenu {
                    session: a,
                    target: at,
                    ..
                },
                Self::NavigateMenu {
                    session: b,
                    target: bt,
                    ..
                },
            ) => a == b && at == bt,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalTabSelection {
    Activate(TabSceneId),
    Navigate {
        source: TabSceneId,
        target: TabSceneId,
    },
}

impl LocalTabSelection {
    pub(crate) const fn target(self) -> TabSceneId {
        match self {
            Self::Activate(tab) | Self::Navigate { target: tab, .. } => tab,
        }
    }

    pub(crate) const fn receiver(self) -> TabSceneId {
        match self {
            Self::Activate(tab) | Self::Navigate { source: tab, .. } => tab,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LocalSurfaceAction {
    SelectTab {
        scene: SurfaceSceneStamp,
        selection: LocalTabSelection,
    },
    Close {
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
    },
    CloseRoot {
        scene: SurfaceSceneStamp,
        root: crate::ids::RootId,
    },
    PresentationCommand {
        scene: SurfaceSceneStamp,
        action: crate::model::ProductAction,
    },
    TabChrome {
        scene: SurfaceSceneStamp,
        action: LocalTabChromeAction,
    },
    AdjustSplitter {
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
    },
    AdjustSplitterJunction {
        scene: SurfaceSceneStamp,
        junction: SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
    },
    AdjustContainedResize {
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        direction: ContainedResizeDirection,
        delta: f64,
    },
    CancelWithEscape {
        status: InteractionStatus,
        delivery: EscapeDelivery,
    },
}

impl LocalSurfaceAction {
    pub(crate) fn same_intent(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::SelectTab { selection: a, .. }, Self::SelectTab { selection: b, .. }) => a == b,
            (Self::Close { target: a, .. }, Self::Close { target: b, .. }) => a == b,
            (Self::CloseRoot { root: a, .. }, Self::CloseRoot { root: b, .. }) => a == b,
            (
                Self::PresentationCommand { action: a, .. },
                Self::PresentationCommand { action: b, .. },
            ) => a == b,
            (Self::TabChrome { action: a, .. }, Self::TabChrome { action: b, .. }) => {
                a.same_intent(b)
            }
            (
                Self::AdjustSplitter {
                    splitter: a,
                    delta: ad,
                    ..
                },
                Self::AdjustSplitter {
                    splitter: b,
                    delta: bd,
                    ..
                },
            ) => a == b && ad == bd,
            (
                Self::AdjustSplitterJunction {
                    junction: a,
                    axis: aa,
                    delta: ad,
                    ..
                },
                Self::AdjustSplitterJunction {
                    junction: b,
                    axis: ba,
                    delta: bd,
                    ..
                },
            ) => a == b && aa == ba && ad == bd,
            (
                Self::AdjustContainedResize {
                    floating: a,
                    direction: aa,
                    delta: ad,
                    ..
                },
                Self::AdjustContainedResize {
                    floating: b,
                    direction: ba,
                    delta: bd,
                    ..
                },
            ) => a == b && aa == ba && ad == bd,
            (
                Self::CancelWithEscape {
                    status: a,
                    delivery: ad,
                },
                Self::CancelWithEscape {
                    status: b,
                    delivery: bd,
                },
            ) => a == b && ad == bd,
            _ => false,
        }
    }

    pub(super) const fn scene(&self) -> Option<SurfaceSceneStamp> {
        match self {
            Self::SelectTab { scene, .. }
            | Self::Close { scene, .. }
            | Self::CloseRoot { scene, .. }
            | Self::PresentationCommand { scene, .. }
            | Self::TabChrome { scene, .. }
            | Self::AdjustSplitter { scene, .. }
            | Self::AdjustSplitterJunction { scene, .. }
            | Self::AdjustContainedResize { scene, .. } => Some(*scene),
            Self::CancelWithEscape { .. } => None,
        }
    }

    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::SelectTab { .. } => "select-tab",
            Self::Close { .. } => "close",
            Self::CloseRoot { .. } => "close-root",
            Self::PresentationCommand { .. } => "presentation-command",
            Self::TabChrome { .. } => "tab-chrome",
            Self::AdjustSplitter { .. } => "adjust-splitter",
            Self::AdjustSplitterJunction { .. } => "adjust-splitter-junction",
            Self::AdjustContainedResize { .. } => "adjust-contained-resize",
            Self::CancelWithEscape { .. } => "cancel-with-escape",
        }
    }
}

/// Input accepted by the U3 engine boundary.
#[allow(
    private_interfaces,
    reason = "test-only raw engine exposure retains sealed product action payloads"
)]
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    /// Apply an owned local action using its core-issued frozen semantic witness.
    ApplyDeferredSurfaceAction {
        expected: WorkspaceVersion,
        witness: Arc<crate::scene::SurfacePlanScene>,
        action: LocalSurfaceAction,
    },
    /// Attach an empty central logical surface using declared semantic identities.
    AttachEmptySurface {
        expected: WorkspaceVersion,
        surface: SurfaceId,
        root: RootId,
    },
    /// Retire an item-free logical surface without replacing the workspace epoch.
    RetireEmptySurface {
        expected: WorkspaceVersion,
        surface: SurfaceId,
    },

    /// Changes targeting intent for one current host-local pointer owner.
    UpdatePointerMagneticTargeting {
        /// Exact provider whose ordered input contains this directive.
        provider: PointerInputLease,
        /// Provider-owned pointer identity, without inventing a new stream.
        pointer: PointerId,
        /// Whether magnetic targets may be acquired; explicit targets remain valid.
        enabled: bool,
    },
    /// Changes targeting intent from one exact managed window callback.
    UpdateNativeMagneticTargeting {
        /// Joined provider which captured the source callback in its total order.
        provider: BackendIngressLease,
        /// Exact source or known keyboard-focus incarnation, revalidated at reduction.
        binding: ViewportBinding,
        /// Whether magnetic targets may be acquired.
        enabled: bool,
    },
    /// Ends managed input at the current fully reduced host-frame boundary.
    ///
    /// This terminal host control must not enter a backend ingress recorder.
    CancelNativeInputForOverload {
        /// Exact joined provider and presentation host whose input is cancelled.
        provider: BackendIngressLease,
    },
    /// Observes native input device removal without ending the host-frame input phase.
    ObserveNativeInputDeviceRemoval {
        /// Exact joined provider whose live pointer and scroll streams are cancelled.
        provider: BackendIngressLease,
    },
    /// Ends input depending on one exact retired native binding.
    ObserveNativeInputBindingRetired {
        /// Exact joined provider which observed the binding's terminal input boundary.
        provider: BackendIngressLease,
        /// Complete window incarnation, independent of its logical surface's successor.
        binding: ViewportBinding,
    },
    /// Bind an existing adapter window to a current logical surface.
    RegisterViewport {
        /// Exact platform provider enrolling this native binding.
        provider: PlatformObservationLease,
        /// Workspace version whose surface roster was inspected.
        expected: WorkspaceVersion,
        /// Stable logical surface already present in the workspace.
        surface: crate::ids::SurfaceId,
        /// Opaque adapter token, never an operating-system handle.
        token: WindowToken,
        /// Root or docking-owned child close semantics.
        role: ViewportRole,
        /// Exact lifecycle recovery target required for docking-owned child windows.
        recovery_target: Option<SurfaceRecoveryTarget>,
    },
    /// Enroll an existing docking-owned child while letting the core mint its recovery identity.
    BootstrapChildViewport {
        /// Exact platform provider enrolling this native binding.
        provider: PlatformObservationLease,
        /// Workspace version whose surface roster was inspected.
        expected: WorkspaceVersion,
        /// Stable logical child surface already present in the workspace.
        surface: crate::ids::SurfaceId,
        /// Opaque adapter token, never an operating-system handle.
        token: WindowToken,
        /// Adapter facts from which the core constructs the exact recovery target.
        recovery: SurfaceRecoveryBootstrap,
    },
    /// Creates native hosting for a complete logical surface without moving its content.
    CreateNativeSurface {
        /// Published workspace version whose logical surface was inspected.
        expected: WorkspaceVersion,
        /// Existing logical surface to materialize.
        surface: SurfaceId,
        /// Current application-root surface which receives failed/destroyed content.
        recovery_host: SurfaceId,
        /// Explicit desktop-physical placement, independent of current coordinate authority.
        placement: crate::geometry::PhysicalRect,
    },
    /// Advances one previously committed document-owned initial creation intent.
    AdvanceNativeSurfaceCreate {
        expected: WorkspaceVersion,
        surface: SurfaceId,
    },
    /// Cancels an unbound creation intent without allocating a window or effect.
    CancelNativeSurfaceCreateIntent {
        expected: WorkspaceVersion,
        surface: SurfaceId,
    },
    /// Explicitly cancel one initial hosting attempt without discarding logical content.
    CancelNativeSurface {
        /// Workspace version whose pending binding was inspected.
        expected: WorkspaceVersion,
        /// Exact initial establishment owner; replacements are not interchangeable.
        binding: ViewportBinding,
    },
    /// Publish one complete frame-before-paint platform snapshot.
    PublishPlatformSnapshot {
        /// Exact platform provider which captured this complete batch.
        provider: PlatformObservationLease,
        /// Workspace epoch against which adapter bindings were observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Complete capabilities, inventory, and pointer facts.
        snapshot: PlatformSnapshot,
    },
    /// Publish one exact binding-scoped native-close observation.
    PublishNativeCloseObservation {
        /// Exact platform provider which captured the close fact.
        provider: PlatformObservationLease,
        /// Workspace epoch whose binding incarnation was observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Provider-owned close fact for one exact native binding.
        observation: WindowCloseObservation,
    },
    /// Publish one globally consistent native-focus observation.
    PublishGlobalFocusObservation {
        /// Exact platform provider which captured this focus fact.
        provider: PlatformObservationLease,
        /// Workspace epoch whose binding incarnation was observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Provider-owned global focus fact.
        observation: crate::viewport_focus::FocusObservationEnvelope,
    },
    /// Report an adapter dispatch result without claiming the effect was observed applied.
    ReportPlatformEffect {
        /// Exact platform provider which received and dispatched the effect.
        provider: PlatformObservationLease,
        /// Workspace epoch in which the result was received.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Exact effect identity and non-observational dispatch result.
        result: EffectResult,
    },
    /// Request explicit activation of one exact current docking viewport.
    ActivateViewport {
        /// Workspace version whose binding and pane were inspected.
        expected: WorkspaceVersion,
        /// Exact-incarnation activation with explicit item-or-none pane focus.
        request: ViewportActivationRequest,
    },
    /// Publish one adapter-observed pane-focus fact.
    PublishPaneFocusObservation {
        /// Workspace epoch in which the exact binding was observed.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Provider-owned pane focus observation and optional intent acknowledgement.
        observation: PaneFocusObservation,
    },
    /// Publish one exact result for a committed pane-focus request.
    PublishPaneFocusRequestObservation {
        /// Workspace epoch in which the request target was resolved.
        expected_epoch: crate::ids::WorkspaceEpoch,
        /// Opaque request identity, target, native guard, and exact result.
        observation: PaneFocusRequestObservation,
    },
    /// Reports the host's current UI focus independently of pane history.
    PublishHostFocusObservation {
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: crate::viewport_focus::HostFocusObservation,
    },
    /// Explicitly cancel one unresolved native-create saga.
    ///
    /// All pending phases are cancellable, including first-live presentation.
    /// Source ownership stays unchanged until the exact first-live output is presented.
    CancelNativeCreate {
        /// Workspace version against which the saga was inspected.
        expected: WorkspaceVersion,
        /// Exact create saga to cancel without a timeout heuristic.
        saga: NativeCreateSagaId,
    },
    /// Explicitly retry one exact failed cleanup under its phase-specific protocol.
    ///
    /// A definitively failed destructive cleanup is retried only under its original
    /// cleanup-specific guards. `ObservationDispatchFailed` and `ObservationUnsupported` may
    /// retry only an observation-only `ContinueCleanup` which keeps the same destructive
    /// predecessor; they never redispatch that predecessor.
    RetryViewportCleanup {
        /// Workspace version against which the failed cleanup was inspected.
        expected: WorkspaceVersion,
        /// Exact failed cleanup effect; indeterminate and all other phases are not retryable.
        failed_effect: crate::effect::EffectId,
    },
    /// Authoritatively replace the complete workspace.
    ReplaceWorkspace(Workspace),
    /// Replaces the workspace while atomically continuing exact external Root windows.
    ReplaceWorkspaceContinuingRoots {
        /// Validated replacement graph with final presentation identities.
        workspace: Workspace,
        /// Complete coordinator preparation for the exact predecessors and targets.
        continuations: Vec<crate::frame::PreparedNativeRootContinuation>,
    },
    /// Restore one complete workspace and allocator lineage validated as one document.
    RestoreWorkspace(ValidatedWorkspaceRestore),
    /// Apply one checked command derived from an exact engine version.
    WorkspaceCommand {
        /// Version from which source and target references were captured.
        expected: WorkspaceVersion,
        /// Checked durable mutation.
        command: WorkspaceCommand,
    },
    /// Select one item by stable application identity.
    SelectItem {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item.
        item: ItemId,
    },
    /// Open one item at a stable product placement.
    OpenItem {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item.
        item: ItemId,
        /// Item-, root-, or surface-centric destination.
        placement: crate::model::DockPlacement,
    },
    /// Move one open item to a stable product placement.
    DockItem {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item.
        item: ItemId,
        /// Item-, root-, or surface-centric destination.
        placement: crate::model::DockPlacement,
    },
    /// Move one complete root to a stable product placement.
    DockRoot {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable root moved as one payload.
        root: RootId,
        /// Item-, root-, or surface-centric destination.
        placement: crate::model::DockPlacement,
    },
    /// Dock one complete contained root into the default target selected by core.
    DockBackRoot {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable root moved as one payload.
        root: RootId,
    },
    /// Starts one native child-window lifecycle without early source transfer.
    TearOffNative {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Exact item, frozen tabs group, or stable complete root.
        source: crate::model::NativeTearOffSource,
        /// Explicit outer placement, or `None` for the core-derived default.
        placement: Option<crate::model::NativeWindowPlacement>,
    },
    /// Move one complete root into a contained presentation on an existing surface.
    FloatRoot {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable complete root moved as one payload.
        root: RootId,
        /// Existing target surface.
        surface: SurfaceId,
        /// Optional explicit durable bounds; `None` delegates placement to core.
        rect: Option<crate::geometry::LogicalRect>,
    },
    /// Move one open item into a contained presentation on an existing surface.
    FloatItem {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item.
        item: ItemId,
        /// Existing target surface.
        surface: SurfaceId,
    },
    /// Raise the contained presentation owning one item.
    RaiseContained {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item inside the contained root.
        item: ItemId,
    },
    /// Clamp the contained presentation owning one item into exact current surface bounds.
    BringContainedIntoView {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item inside the contained root.
        item: ItemId,
    },
    /// Request a core-owned close plan for stable application content.
    RequestContentClose {
        /// Version from which the stable target was selected.
        expected: WorkspaceVersion,
        /// Item or complete root identity; all mutable facts are captured by the core.
        target: ContentCloseTarget,
    },
    /// Request a close plan from one exact scene-bound semantic activation.
    ///
    /// Pointer close activation remains part of the legacy pointer reducer until
    /// the pointer journal owns close-control receivers. This input cannot forge
    /// pointer hit authority.
    RequestSceneClose {
        /// Workspace version from which the scene action was captured.
        expected: WorkspaceVersion,
        /// Exact last-painted scene authority used for the activation.
        scene: SurfaceSceneStamp,
        /// Stable close-control identity exposed by that scene.
        target: CloseSceneTarget,
    },
    /// Request a close from one exact current-frame framework response.
    RequestLocalSceneClose {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Stable close-control identity exposed by that candidate.
        target: CloseSceneTarget,
    },
    /// Request application approval to close a root from its exact menu anchor.
    RequestLocalRootClose {
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        root: RootId,
    },
    /// Select one exact tab activated by a current-frame framework response.
    SelectLocalSceneTab {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Activation, or navigation from an operable tab to a member of its strip.
        selection: LocalTabSelection,
    },
    /// Apply one current-frame tab-strip or popup action from an exact Ready candidate.
    ApplyLocalTabChromeAction {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Opaque control, row, dismissal, scroll, or navigation action.
        action: LocalTabChromeAction,
    },
    /// Apply one root presentation command from an exact current-frame candidate.
    ApplyLocalPresentationCommand {
        /// Workspace version from which the command availability was derived.
        expected: WorkspaceVersion,
        /// Exact Ready candidate which exposed the command.
        scene: SurfaceSceneStamp,
        /// Core-minted product action hidden by the renderer-facing record.
        action: crate::model::ProductAction,
    },
    /// Deliver one keyboard or accessibility action to an exact presented receiver.
    ActivateSemanticReceiver {
        /// Workspace version current when the host captured the semantic event.
        expected: WorkspaceVersion,
        /// Exact output, emission, receiver, and device-independent action.
        event: SemanticReceiverEvent,
    },
    /// Apply one scene-bound keyboard or accessibility splitter adjustment.
    AdjustSplitterResize {
        /// Workspace version from which the scene action was captured.
        expected: WorkspaceVersion,
        /// Exact last-painted scene authority that exposed the splitter action.
        scene: SurfaceSceneStamp,
        /// Stable structural splitter identity exposed by that scene.
        splitter: SplitterSceneId,
        /// Signed displacement along the split axis in logical surface units.
        delta: f64,
    },
    /// Apply one splitter adjustment captured from an exact current-frame framework response.
    AdjustLocalSplitterResize {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Stable structural splitter identity exposed by that candidate.
        splitter: SplitterSceneId,
        /// Signed displacement along the split axis in logical surface units.
        delta: f64,
    },
    /// Apply one axis of a splitter junction captured from an exact current-frame response.
    AdjustLocalSplitterJunctionResize {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Complete junction identity exposed by that candidate.
        junction: crate::scene::SplitterJunctionId,
        /// Logical split axis adjusted by the semantic control.
        axis: crate::model::DockspaceAxis,
        /// Signed displacement shared by every incident splitter on `axis`.
        delta: f64,
    },
    /// Apply one cardinal contained resize captured from an exact current-frame response.
    AdjustLocalContainedResize {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Stable contained presentation identity.
        floating: FloatingPresentationId,
        /// Exact cardinal edge exposed by that candidate.
        direction: crate::scene::ContainedResizeDirection,
        /// Signed displacement of that edge in logical surface units.
        delta: f64,
    },
    /// Activate one exact current tab-strip control without fabricating pointer delivery.
    ActivateTabStripControl {
        /// Opaque proof prepared from a sealed, receiver-authoritative presentation.
        prepared: PreparedTabStripControlActivation,
    },
    /// Activate one exact current tab-list menu row without fabricating pointer delivery.
    ActivateTabListMenuRow {
        /// Opaque proof prepared from the sole active popup session and presented row.
        prepared: PreparedTabListMenuRowActivation,
    },
    /// Dismiss the exact current tab-list menu without fabricating a pointer backdrop click.
    DismissTabListMenu {
        /// Opaque proof prepared from the active popup session and presented menu.
        prepared: PreparedTabListMenuDismiss,
    },
    /// Apply one exact current tab-strip scroll adjustment.
    AdjustTabStripScroll {
        /// Opaque proof prepared from the exact presented bar geometry.
        prepared: PreparedTabStripScroll,
    },
    /// Apply one exact current tab-list menu scroll adjustment.
    AdjustTabListMenuScroll {
        /// Opaque proof prepared from the active popup session and menu geometry.
        prepared: PreparedTabListMenuScroll,
    },
    /// Navigate or focus one exact current tab-list menu row.
    NavigateTabListMenu {
        /// Opaque proof prepared from the active popup session and ordered roster.
        prepared: PreparedTabListMenuNavigation,
    },
    /// Apply one exact keyboard or programmatic contained placement.
    ApplyContainedPlacement {
        /// Workspace version from which the placement proof was requested.
        expected: WorkspaceVersion,
        /// Root presented by the contained floating.
        root: RootId,
        /// Exact contained presentation identity.
        floating: FloatingPresentationId,
        /// Exact previous rectangle captured from the workspace.
        expected_rect: crate::geometry::LogicalRect,
        /// Current scene-bound placement authorization.
        placement: ContainedPlacementProof,
    },
    /// Cancel whichever docking gesture is active at this exact reducer position.
    CancelActiveInteractionWithEscape {
        /// Workspace version current when this key edge entered the host frame.
        expected: WorkspaceVersion,
        /// Authoritative endpoint which delivered the Escape press.
        delivery: EscapeDelivery,
    },
    /// Cancel only the gesture captured by an exact current-candidate action.
    ///
    /// Unlike the raw Escape edge, this action retains its gesture identity and
    /// frozen surface authority. Its workspace version must still match exactly.
    CancelPreparedInteractionWithEscape {
        expected: WorkspaceVersion,
        witness: Arc<crate::scene::SurfacePlanScene>,
        status: InteractionStatus,
        delivery: EscapeDelivery,
    },
    /// Resolve one exact provider-observed native close edge with a complete
    /// core-frozen surface operation. The request cannot name an arbitrary
    /// surface or rely on the current viewport; the edge carries the exact
    /// binding incarnation and observation generations that opened it.
    RequestSurfaceClose {
        /// Version from which the caller inspected the close edge.
        expected: WorkspaceVersion,
        /// Exact provider close edge published by this engine domain.
        edge: NativeCloseEdge,
        /// Complete explicit surface disposition.
        request: SurfaceCloseRequest,
    },
    /// Explicitly cancel one exact unresolved native close edge.
    ///
    /// This only asks the core to issue a compensating native effect. It does
    /// not infer that cancellation succeeded; a later authoritative clear
    /// observation with the exact effect frontier is still required.
    CancelSurfaceClose {
        /// Version from which the caller inspected the close edge.
        expected: WorkspaceVersion,
        /// Exact provider close edge which remains live until settled.
        edge: NativeCloseEdge,
    },
    /// Resolve one item's exact initial token in a core-owned close plan.
    ResolveClose {
        /// Exact plan request returned by the engine.
        request: CloseRequestId,
        /// Single-use token for one stable item.
        token: CloseDecisionToken,
        /// Immediate application decision.
        decision: CloseDecision,
    },
    /// Resolve one exact deferred close continuation.
    ContinueDeferredClose {
        /// Exact plan request which minted the continuation.
        request: CloseRequestId,
        /// Single-use deferred continuation token.
        token: DeferredCloseToken,
        /// Terminal continuation decision.
        decision: DeferredCloseDecision,
    },
    /// Replace application policy if the input is still current.
    ReplacePolicy {
        /// Version observed when the application chose the policy.
        expected: WorkspaceVersion,
        /// Complete replacement policy.
        policy: DockPolicy,
    },
    /// Replace renderer-neutral semantic presentation geometry if still current.
    ReplacePresentationConfig {
        /// Workspace version observed when the application chose the configuration.
        expected: WorkspaceVersion,
        /// Complete validated semantic geometry replacement.
        config: DockPresentationConfig,
    },
    /// Re-run strict validation without changing state.
    ValidateWorkspace,
}

impl EngineInput {
    /// Returns the diagnostic source class recorded for this input.
    ///
    /// Input classes never reorder a host frame. The frame capability preserves
    /// append order within its explicit semantic and configuration phases.
    #[must_use]
    pub const fn priority(&self) -> InputPriority {
        match self {
            Self::RegisterViewport { .. }
            | Self::BootstrapChildViewport { .. }
            | Self::CreateNativeSurface { .. }
            | Self::AdvanceNativeSurfaceCreate { .. }
            | Self::CancelNativeSurfaceCreateIntent { .. }
            | Self::CancelNativeSurface { .. }
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_)
            | Self::ReplaceWorkspaceContinuingRoots { .. }
            | Self::RestoreWorkspace(_) => InputPriority::LifecycleControl,
            Self::PublishPlatformSnapshot { .. }
            | Self::PublishNativeCloseObservation { .. }
            | Self::PublishGlobalFocusObservation { .. }
            | Self::ObserveNativeInputDeviceRemoval { .. }
            | Self::ObserveNativeInputBindingRetired { .. }
            | Self::ReportPlatformEffect { .. } => InputPriority::PlatformObservation,
            Self::PublishPaneFocusObservation { .. }
            | Self::PublishPaneFocusRequestObservation { .. }
            | Self::PublishHostFocusObservation { .. } => InputPriority::PlatformObservation,
            Self::ActivateViewport { .. }
            | Self::UpdatePointerMagneticTargeting { .. }
            | Self::UpdateNativeMagneticTargeting { .. }
            | Self::CancelNativeInputForOverload { .. }
            | Self::AttachEmptySurface { .. }
            | Self::RetireEmptySurface { .. }
            | Self::WorkspaceCommand { .. }
            | Self::SelectItem { .. }
            | Self::OpenItem { .. }
            | Self::DockItem { .. }
            | Self::DockRoot { .. }
            | Self::DockBackRoot { .. }
            | Self::TearOffNative { .. }
            | Self::FloatRoot { .. }
            | Self::FloatItem { .. }
            | Self::RaiseContained { .. }
            | Self::BringContainedIntoView { .. }
            | Self::RequestContentClose { .. }
            | Self::RequestSceneClose { .. }
            | Self::ApplyDeferredSurfaceAction { .. }
            | Self::RequestLocalSceneClose { .. }
            | Self::RequestLocalRootClose { .. }
            | Self::SelectLocalSceneTab { .. }
            | Self::ApplyLocalTabChromeAction { .. }
            | Self::ApplyLocalPresentationCommand { .. }
            | Self::ActivateSemanticReceiver { .. }
            | Self::AdjustSplitterResize { .. }
            | Self::AdjustLocalSplitterResize { .. }
            | Self::AdjustLocalSplitterJunctionResize { .. }
            | Self::AdjustLocalContainedResize { .. }
            | Self::ActivateTabStripControl { .. }
            | Self::ActivateTabListMenuRow { .. }
            | Self::DismissTabListMenu { .. }
            | Self::AdjustTabStripScroll { .. }
            | Self::AdjustTabListMenuScroll { .. }
            | Self::NavigateTabListMenu { .. }
            | Self::ApplyContainedPlacement { .. }
            | Self::CancelActiveInteractionWithEscape { .. }
            | Self::CancelPreparedInteractionWithEscape { .. }
            | Self::RequestSurfaceClose { .. }
            | Self::CancelSurfaceClose { .. }
            | Self::ResolveClose { .. }
            | Self::ContinueDeferredClose { .. } => InputPriority::ApplicationCommand,
            Self::ReplacePolicy { .. } | Self::ReplacePresentationConfig { .. } => {
                InputPriority::ConfigurationCommit
            }
            Self::ValidateWorkspace => InputPriority::Maintenance,
        }
    }

    pub(crate) const fn is_configuration_commit(&self) -> bool {
        matches!(
            self,
            Self::ReplacePolicy { .. } | Self::ReplacePresentationConfig { .. }
        )
    }

    pub(crate) const fn is_backend_ingress_fact(&self) -> bool {
        matches!(
            self,
            Self::PublishPlatformSnapshot { .. }
                | Self::PublishNativeCloseObservation { .. }
                | Self::PublishGlobalFocusObservation { .. }
                | Self::ObserveNativeInputDeviceRemoval { .. }
                | Self::ObserveNativeInputBindingRetired { .. }
                | Self::ReportPlatformEffect { .. }
        )
    }
}
