//! Public engine input protocol and opaque prepared interaction proofs.

use super::*;
use crate::intent::TabGestureSource;

/// A document-validated workspace replacement carrying its opaque allocator lineage.
///
/// Values can only be produced by the strict document restore path. This prevents
/// application code from pairing a valid workspace with fabricated presentation
/// tombstones or from silently resetting an engine's retired identity frontier.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedWorkspaceRestore {
    workspace: Workspace,
    presentation_identity_frontier: PresentationIdentityFrontier,
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

    pub(super) fn into_parts(self) -> (Workspace, PresentationIdentityFrontier) {
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

/// One exact current-frame splitter gesture phase from a framework response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalSplitterGesturePhase {
    /// Begin one resize from an exact Ready scene and absolute logical points.
    Press {
        /// Exact Ready candidate which owned the pressed splitter target.
        scene: SurfaceSceneStamp,
        /// Absolute logical point at which the drag began.
        initial: LogicalPoint,
        /// Absolute logical point observed by the current response.
        current: LogicalPoint,
    },
    /// Update the active resize at one absolute logical point.
    Move {
        /// Current absolute logical point in the owner surface.
        current: LogicalPoint,
    },
    /// Commit the active resize using this release-time logical point.
    Release {
        /// Exact absolute release point in the owner surface.
        current: LogicalPoint,
    },
    /// Cancel the matching local resize without changing durable layout.
    Cancel,
}

/// One current-frame tab drag fact produced by an egui `Response`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalTabGesturePhase {
    /// Begin a drag from the exact tab response and scene.
    Begin {
        /// Ready scene which owned the source response.
        scene: SurfaceSceneStamp,
        /// Pointer location when the framework crossed its drag threshold.
        initial: LogicalPoint,
        /// Current pointer location in the source surface.
        current: LogicalPoint,
    },
    /// Update the active drag at an absolute logical point.
    Move {
        /// Current Ready scene which still authorizes the local response.
        scene: SurfaceSceneStamp,
        /// Current pointer location in the source surface.
        current: LogicalPoint,
    },
    /// Release the active drag at an absolute logical point.
    Release {
        /// Current Ready scene which still authorizes the local response.
        scene: SurfaceSceneStamp,
        /// Current pointer location in the source surface.
        current: LogicalPoint,
    },
    /// Cancel the active local drag without changing durable layout.
    Cancel,
}

/// One current-frame contained-floating transform fact from a framework response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalContainedGesturePhase {
    /// Begin a transform from an exact Ready scene and absolute logical points.
    Begin {
        /// Ready scene which owned the contained chrome response.
        scene: SurfaceSceneStamp,
        /// Pointer location when the framework crossed its drag threshold.
        initial: LogicalPoint,
        /// Current pointer location in the owner surface.
        current: LogicalPoint,
    },
    /// Update the active transform at an absolute logical point.
    Move {
        /// Current Ready scene which still authorizes the local response.
        scene: SurfaceSceneStamp,
        /// Current pointer location in the owner surface.
        current: LogicalPoint,
    },
    /// Release the active transform against the last painted preview.
    Release {
        /// Current Ready scene which still authorizes the local response.
        scene: SurfaceSceneStamp,
        /// Exact absolute release point in the owner surface.
        current: LogicalPoint,
    },
    /// Cancel the matching local transform without changing durable geometry.
    Cancel,
}

/// Input accepted by the U3 engine boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
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
    /// Explicitly cancel one unresolved native-create saga.
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
    /// Move one open item into a contained presentation on an existing surface.
    FloatItem {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item.
        item: ItemId,
        /// Existing target surface.
        surface: SurfaceId,
        /// Durable surface-local contained bounds.
        rect: crate::geometry::LogicalRect,
    },
    /// Replace the durable bounds of the contained presentation owning one item.
    SetContainedRect {
        /// Exact workspace version from which this product action was derived.
        expected: WorkspaceVersion,
        /// Stable application item inside the contained root.
        item: ItemId,
        /// New durable surface-local bounds.
        rect: crate::geometry::LogicalRect,
    },
    /// Raise the contained presentation owning one item.
    RaiseContained {
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
    /// Select one exact tab activated by a current-frame framework response.
    SelectLocalSceneTab {
        /// Workspace version from which the local response was captured.
        expected: WorkspaceVersion,
        /// Exact Ready candidate painted by the framework callback.
        scene: SurfaceSceneStamp,
        /// Stable tab identity exposed by that candidate.
        tab: crate::scene::TabSceneId,
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
    /// Drive one local-response splitter or junction gesture through the core resize FSM.
    LocalSplitterGesture {
        /// Workspace version current when the framework response was captured.
        expected: WorkspaceVersion,
        /// Logical surface which owns the framework response.
        surface: SurfaceId,
        /// Exact structural handle or junction identity owned by the response.
        target: SplitterResizeTarget,
        /// Press, motion, release, or explicit cancellation fact.
        phase: LocalSplitterGesturePhase,
    },
    /// Drive one local-response tab drag through the core drag/drop resolver.
    LocalTabGesture {
        /// Workspace version current when the framework response was captured.
        expected: WorkspaceVersion,
        /// Logical surface which owns the framework response.
        surface: SurfaceId,
        /// Stable tab or tab-group identity which owns the response.
        source: TabGestureSource,
        /// Begin, move, release, or cancellation fact.
        phase: LocalTabGesturePhase,
    },
    /// Reduce one current-frame contained-floating resize through the core transform FSM.
    LocalContainedGesture {
        /// Workspace version from which the response was captured.
        expected: WorkspaceVersion,
        /// Surface whose Ready scene owned the response.
        surface: SurfaceId,
        /// Stable contained presentation identity.
        floating: FloatingPresentationId,
        /// Exact contained chrome gesture represented by the response.
        kind: crate::intent::ContainedGestureKind,
        /// Current framework gesture phase.
        phase: LocalContainedGesturePhase,
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
    /// Confirm that the adapter actually painted one exact drag preview.
    AcknowledgePreview {
        /// Workspace version from which the preview was painted.
        expected: WorkspaceVersion,
        /// Core-minted preview identity returned by the painted scene.
        acknowledgement: PaintAcknowledgement,
    },
    /// Confirm that the adapter actually painted one exact contained-transform preview.
    AcknowledgeContainedTransformPreview {
        /// Workspace version from which the preview was painted.
        expected: WorkspaceVersion,
        /// Core-minted preview identity returned by the painted scene.
        acknowledgement: ContainedTransformPaintAcknowledgement,
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
            | Self::CancelNativeCreate { .. }
            | Self::RetryViewportCleanup { .. }
            | Self::ReplaceWorkspace(_)
            | Self::RestoreWorkspace(_) => InputPriority::LifecycleControl,
            Self::PublishPlatformSnapshot { .. }
            | Self::PublishNativeCloseObservation { .. }
            | Self::ReportPlatformEffect { .. } => InputPriority::PlatformObservation,
            Self::PublishPaneFocusObservation { .. } => InputPriority::PlatformObservation,
            Self::ActivateViewport { .. }
            | Self::WorkspaceCommand { .. }
            | Self::SelectItem { .. }
            | Self::OpenItem { .. }
            | Self::DockItem { .. }
            | Self::DockRoot { .. }
            | Self::FloatItem { .. }
            | Self::SetContainedRect { .. }
            | Self::RaiseContained { .. }
            | Self::RequestContentClose { .. }
            | Self::RequestSceneClose { .. }
            | Self::RequestLocalSceneClose { .. }
            | Self::SelectLocalSceneTab { .. }
            | Self::ActivateSemanticReceiver { .. }
            | Self::AdjustSplitterResize { .. }
            | Self::AdjustLocalSplitterResize { .. }
            | Self::LocalSplitterGesture { .. }
            | Self::LocalTabGesture { .. }
            | Self::LocalContainedGesture { .. }
            | Self::ActivateTabStripControl { .. }
            | Self::ActivateTabListMenuRow { .. }
            | Self::DismissTabListMenu { .. }
            | Self::AdjustTabStripScroll { .. }
            | Self::AdjustTabListMenuScroll { .. }
            | Self::NavigateTabListMenu { .. }
            | Self::ApplyContainedPlacement { .. }
            | Self::CancelActiveInteractionWithEscape { .. }
            | Self::AcknowledgePreview { .. }
            | Self::AcknowledgeContainedTransformPreview { .. }
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
                | Self::ReportPlatformEffect { .. }
        )
    }
}
