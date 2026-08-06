use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The only accepted executable core protocol trace schema.
///
/// This identifier intentionally stays at `/1`: the crate is workspace-private
/// and the old renderer-intent shape was never a public compatibility contract.
pub const CORE_PROTOCOL_TRACE_SCHEMA: &str = "dockspace.core-protocol-trace/1";

/// Local Open GPUI behavior baseline used while extracting docking semantics.
pub const OPEN_GPUI_BASELINE_REVISION: &str = "56604588ee0a047c59e9ef6a2346f4c5839d90de";

macro_rules! string_key {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

macro_rules! numeric_key {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);
    };
}

string_key!(BaselineId);
string_key!(CoreProtocolTraceId);
string_key!(BoundaryId);
string_key!(ProducerId);

numeric_key!(ItemKey);
numeric_key!(RootKey);
numeric_key!(SurfaceKey);
numeric_key!(FloatingPresentationKey);
numeric_key!(WorkAreaKey);
numeric_key!(ReducerTick);
numeric_key!(EffectKey);

/// Complete collection of executable core protocol trace records.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreProtocolTraceSuite {
    pub schema: String,
    pub source_baselines: Vec<SourceBaseline>,
    pub traces: Vec<CoreProtocolTrace>,
}

/// Revision and license evidence shared by one or more traces.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBaseline {
    pub id: BaselineId,
    pub project: String,
    pub repository: String,
    pub revision: String,
    pub license: LicenseProvenance,
}

/// Machine-readable license provenance for copied behavior tests.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseProvenance {
    pub expression: String,
    pub file: String,
}

/// One independently replayable causal trace.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreProtocolTrace {
    pub id: CoreProtocolTraceId,
    pub provenance: CoreProtocolTraceProvenance,
    pub initial_workspace: InitialWorkspace,
    pub boundaries: Vec<CoreProtocolTraceBoundary>,
    pub expected_final: ExpectedCanonicalSnapshot,
}

/// Exact test-level provenance and any deliberate contract strengthening.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreProtocolTraceProvenance {
    pub baseline: BaselineId,
    pub path: String,
    pub test: String,
    pub retained_behavior: String,
    pub deliberate_strengthening: String,
}

/// Workspace topology used to construct the first engine state.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialWorkspace {
    #[serde(default, skip_serializing_if = "InitialPolicyFixture::is_default")]
    pub policy: InitialPolicyFixture,
    pub roots: Vec<InitialRoot>,
    pub surfaces: Vec<SurfaceFixture>,
}

/// Initial application policy applied before the first reducer boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialPolicyFixture {
    pub allow_native_surfaces: bool,
}

impl InitialPolicyFixture {
    const fn is_default(&self) -> bool {
        !self.allow_native_surfaces
    }
}

/// One stable root and its root-local structural topology.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialRoot {
    pub id: RootKey,
    pub central_path: Option<StructuralPath>,
    pub node: NodeFixture,
}

/// One logical surface and its exact back-to-front contained roster.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceFixture {
    pub id: SurfaceKey,
    pub main_root: Option<RootKey>,
    pub contained: Vec<ContainedFixture>,
}

/// One contained presentation in a fixture surface roster.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainedFixture {
    pub id: FloatingPresentationKey,
    pub root: RootKey,
    pub rect: RectFixture,
}

/// Renderer-neutral logical rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RectFixture {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Root-local path through ordered split children. The root itself is `[]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct StructuralPath(pub Vec<usize>);

/// Framework-independent topology node.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NodeFixture {
    Tabs {
        items: Vec<ItemKey>,
        selected: Option<ItemKey>,
        mru: Vec<ItemKey>,
    },
    Split {
        axis: AxisSpec,
        weights: Vec<f32>,
        children: Vec<NodeFixture>,
    },
}

/// Axis encoded as a tagged value rather than a framework-specific enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AxisSpec {
    Horizontal,
    Vertical,
}

/// One atomic reducer boundary.
///
/// [`Self::events`] is the sole causal lane. Its vector order is authoritative:
/// callers cannot supply or forge a reducer ordinal. Presentation observations,
/// dispositions, and surface contributions remain separate complete-frame
/// lanes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreProtocolTraceBoundary {
    pub id: BoundaryId,
    pub provider: Option<PointerProviderIngress>,
    pub presentation_observation: PresentationObservationIngress,
    pub presentation_dispositions: Vec<PresentationDispositionIngress>,
    pub events: Vec<HostFrameEvent>,
    pub surface_contributions: Vec<SurfaceContributionIngress>,
    pub expected: ExpectedTransition,
}

/// Provider lifecycle operation without a caller-supplied lease identity.
///
/// Activation precedes and authorizes the boundary's host frame. Desktop-global
/// retirement and presentation-host retirement publish complete transitions.
/// Surface-local retirement consumes its affine producer and publishes only the
/// explicitly declared maintenance result.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerProviderIngress {
    Activate {
        scope: PointerProviderScopeIngress,
        committed_through: u64,
    },
    RetireDesktopGlobal {},
    RetireSurfaceLocal {
        expected: ExpectedSurfaceLocalPointerMaintenance,
    },
    RetirePresentationHost {},
}

/// Exact adapter-visible maintenance result of draining a surface-local producer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSurfaceLocalPointerMaintenance {
    /// Whether retirement invalidated an interaction or scroll presentation.
    pub interaction_changed: bool,
    /// Whether the adapter must rebuild the affected surface presentation.
    pub repaint_required: bool,
    /// Whether this call retired the active lease or only compacted an earlier tombstone.
    pub disposition: ExpectedSurfaceLocalPointerMaintenanceDisposition,
    /// Number of smooth-scroll sessions which must no longer remain active.
    pub terminated_scroll_sessions: usize,
}

/// Core disposition selected by one affine surface-local producer drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedSurfaceLocalPointerMaintenanceDisposition {
    RetiredActive,
    CompactedPreviouslyRetired,
}

/// Declarative observation lane for one pointer provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerProviderScopeIngress {
    DesktopGlobal {},
    SurfaceLocal { surface: SurfaceKey },
}

/// Complete presentation observation submitted before every other frame fact.
///
/// A batch names every pending stream through stable trace identities. The
/// harness resolves those identities to opaque core streams but never chooses
/// a stream or emission on the fixture's behalf.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationObservationIngress {
    NoUpdate {},
    Batch {
        observations: Vec<PresentationStreamObservationIngress>,
    },
}

/// Stable trace identity for one core-owned presentation stream.
///
/// `sequence` is assigned in first-emission order independently for each
/// logical surface. It is not the opaque core stream serial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationStreamRef {
    pub surface: SurfaceKey,
    pub endpoint: PresentationEndpointRef,
    pub sequence: u64,
}

/// Stable endpoint semantics retained by a presentation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationEndpointRef {
    Headless,
    Native { role: ViewportRoleSpec },
}

/// One explicit provider fact for one exact pending presentation stream.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationStreamObservationIngress {
    pub stream: PresentationStreamRef,
    pub observation: PresentationStreamObservationSpec,
}

/// Provider fact submitted for one exact presentation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationStreamObservationSpec {
    NoUpdate,
    CapturedUnknown {
        generation: u64,
        reason: AuthorityUnavailableReasonSpec,
    },
    Retired {
        generation: u64,
        settled_through: u64,
        presented: RetiredPresentationIngress,
    },
}

/// Final-presentation fact attached to a retired emission range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetiredPresentationIngress {
    Presented {
        emission: u64,
    },
    None,
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Exact disposition of one physical presentation slot in a host frame.
///
/// The slot role is explicit because a live semantic surface and a native
/// staging placeholder are mutually exclusive physical obligations even when
/// they use the same logical surface identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "slot", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationDispositionIngress {
    Surface {
        surface: SurfaceKey,
        disposition: PresentationDispositionSpec,
    },
    NativeStaging {
        surface: SurfaceKey,
        disposition: PresentationDispositionSpec,
    },
}

impl PresentationDispositionIngress {
    /// Returns the logical surface occupied by this physical slot.
    #[must_use]
    pub const fn surface(self) -> SurfaceKey {
        match self {
            Self::Surface { surface, .. } | Self::NativeStaging { surface, .. } => surface,
        }
    }

    /// Returns the explicit disposition for this slot.
    #[must_use]
    pub const fn disposition(self) -> PresentationDispositionSpec {
        match self {
            Self::Surface { disposition, .. } | Self::NativeStaging { disposition, .. } => {
                disposition
            }
        }
    }
}

/// Whether one exact physical presentation slot was painted or unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationDispositionSpec {
    Painted {},
    Unavailable {
        reason: PresentationUnavailableReasonSpec,
    },
}

/// Explicit reason why one physical presentation slot was unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationUnavailableReasonSpec {
    FinalPresentationUnobservable,
    OutputNotProduced,
    SupersededBeforePublication,
    RetainedResourceUnavailable,
    TransientVisualNotPainted,
    BackendFailure,
}

/// One independently delivered, exact per-surface measurement callback.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceContributionIngress {
    pub surface: SurfaceKey,
    pub measurements: SurfaceMeasurementIngress,
}

/// Renderer facts supplied by one surface callback.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SurfaceMeasurementIngress {
    Complete {
        bounds: RectFixture,
    },
    Retained {},
    Unavailable {
        reason: MeasurementUnavailableReasonSpec,
    },
    BoundsOnly {
        bounds: RectFixture,
    },
}

/// Why one adapter-owned measurement could not be produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MeasurementUnavailableReasonSpec {
    Unsupported,
    SurfaceUnavailable,
    ContentUnavailable,
    TextMetricsUnavailable,
    Deferred,
}

/// Input identity supplied by one stable producer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IngressRef {
    pub producer: ProducerId,
    pub source_sequence: u64,
}

/// One event in the boundary's sole ordered host-frame lane.
///
/// A journal segment remains a complete contiguous provider interval. Multiple
/// segments may appear in one boundary, interleaved with semantic inputs. The
/// event vector describes arrival order only: the core mints one causal ordinal
/// per edge and one ordinal per semantic input. An empty journal only checks a
/// provider checkpoint and therefore does not consume a causal ordinal.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostFrameEvent {
    SemanticInput {
        producer: ProducerId,
        source_sequence: u64,
        input: CoreProtocolTraceInput,
    },
    PointerJournal {
        previous: u64,
        through: u64,
        edges: Vec<PointerEdgeIngress>,
    },
}

impl HostFrameEvent {
    /// Returns the stable producer-local identity of a semantic input.
    #[must_use]
    pub fn semantic_reference(&self) -> Option<IngressRef> {
        let Self::SemanticInput {
            producer,
            source_sequence,
            ..
        } = self
        else {
            return None;
        };
        Some(IngressRef {
            producer: producer.clone(),
            source_sequence: *source_sequence,
        })
    }
}

/// Adapter-neutral semantic ingress domains.
///
/// Pointer interaction is deliberately absent. It is represented only by
/// [`HostFrameEvent::PointerJournal`] and core-minted receiver receipts.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreProtocolTraceInput {
    WorkspaceCommand {
        command: CoreProtocolTraceCommand,
    },
    /// Cancel the one currently active click session because Escape was
    /// authoritatively received by its semantic input source.
    ///
    /// The trace cannot name the core-minted click session. Replay resolves
    /// this input against the exact active `Pressed` state at this boundary.
    CancelActiveClickWithEscape {},
    /// Activate the exact current overflow-menu control for one structural tab bar.
    /// Replay resolves all runtime scene, popup, and control identities through
    /// the sealed core view; the trace cannot serialize those capabilities.
    OpenTabListMenu {
        surface: SurfaceKey,
        bar: NodeLocation,
    },
    ValidateWorkspace,
    Lifecycle {
        action: LifecycleIngress,
    },
    PlatformObservation {
        observation: PlatformObservationIngress,
    },
    /// Report a delayed destructive-cleanup result through one exact emitted
    /// observation continuation. Replay resolves the opaque token from the
    /// previously observed core emission rather than serializing authority.
    CleanupObservationResult {
        predecessor: EffectKey,
        continuation: EffectKey,
        result: EffectDispatchResultIngress,
    },
}

/// Adapter dispatch result admitted by a cleanup-observation trace event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectDispatchResultIngress {
    DispatchFailed {
        reason: DispatchFailureReasonIngress,
    },
    Unsupported {
        reason: EffectUnsupportedReasonIngress,
    },
    Indeterminate {
        reason: EffectIndeterminateReasonIngress,
    },
}

/// Stable input-side dispatch failure class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchFailureReasonIngress {
    AdapterRejected,
    WindowUnavailable,
    ProviderStopped,
}

/// Stable input-side unsupported class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectUnsupportedReasonIngress {
    BackendUnsupported,
    CapabilityRevoked,
}

/// Stable input-side indeterminate class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectIndeterminateReasonIngress {
    AcknowledgementLost,
    ProviderRestarted,
}

/// Commands name structural paths, never transient runtime node identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreProtocolTraceCommand {
    Select {
        source: ItemLocation,
    },
    OpenCenter {
        item: ItemKey,
        target: NodeLocation,
    },
    OpenTabGap {
        item: ItemKey,
        target: NodeLocation,
        insertion_index: usize,
    },
    Reorder {
        source: ItemLocation,
        insertion_index: usize,
    },
}

/// One item at an exact root-local tabs path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemLocation {
    pub root: RootKey,
    pub path: StructuralPath,
    pub item: ItemKey,
}

/// One node at an exact root-local path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NodeLocation {
    pub root: RootKey,
    pub path: StructuralPath,
}

/// Typed lifecycle input. The binding is always minted by the core and only
/// associated with its logical surface inside the harness.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LifecycleIngress {
    RegisterViewport {
        surface: SurfaceKey,
        token: u64,
        role: ViewportRoleSpec,
    },
}

/// Ownership semantics of one adapter-created native viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewportRoleSpec {
    Root,
    Child,
}

/// One complete pointer-free platform observation published before painting.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformObservationIngress {
    PublishSnapshot { snapshot: PlatformSnapshotFixture },
}

/// Complete typed platform facts required for viewport lifecycle and coordinate
/// authority. Pointer snapshots and derived routes are intentionally absent.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSnapshotFixture {
    pub capability_generation: u64,
    pub focus_generation: u64,
    pub inventory_generation: u64,
    pub capabilities: PlatformCapabilitiesFixture,
    pub windows: Vec<ObservedWindowFixture>,
    pub work_area_observation: WorkAreaRosterObservationFixture,
}

/// One complete causally versioned provider work-area observation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAreaRosterObservationFixture {
    Known {
        generation: u64,
        work_areas: Vec<ObservedWorkAreaFixture>,
    },
    Unknown {
        generation: u64,
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// One provider-owned work area used by core-authored native placement proofs.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedWorkAreaFixture {
    pub token: WorkAreaKey,
    pub bounds: RectFixture,
    pub scale_factor: f64,
}

/// Complete capability roster expressed as explicit supported requirements.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformCapabilitiesFixture {
    pub supported: Vec<PlatformRequirementSpec>,
}

/// Independently degradable platform requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlatformRequirementSpec {
    NativeWindowLifecycle,
    AuthoritativeInventory,
    HoveredWindow,
    DesktopPointerPosition,
    AuthoritativeButtonState,
    GlobalWindowPlacement,
    WorkArea,
    PointerHitTestObservation,
    PointerHitTestControl,
    GlobalFocusObservation,
    WindowActivationControl,
    CloseCancellation,
}

/// Exact provider facts for one registered docking window. `surface` resolves
/// to the current core-minted binding and never serializes a binding token.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedWindowFixture {
    pub surface: SurfaceKey,
    pub coordinate_observation_generation: u64,
    pub content_bounds: RectFixture,
    pub outer_bounds: RectFixture,
    pub native_scale_factor: f64,
    pub presentation_scale_factor: f64,
    pub input_observation_generation: u64,
    pub input_state: WindowInputStateSpec,
    pub presentation_generation: u64,
    pub presentation_state: WindowPresentationStateSpec,
    /// Exact prior platform effect reflected by this presentation sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_presentation_effect: Option<ExpectedEffectRef>,
}

/// Exact native-window pointer-input behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WindowInputStateSpec {
    ReceivesInput,
    PassThrough,
}

/// Exact known native-window presentation state relevant to routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WindowPresentationStateSpec {
    Visible,
    Hidden,
    Minimized,
}

/// One ordered edge and its semantic receiver observation.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PointerEdgeIngress {
    pub sequence: u64,
    pub pointer: u64,
    pub kind: PointerEdgeKindSpec,
    pub location: PointerLocationIngress,
    pub delivery: PointerEventDeliveryIngress,
    pub capture: PointerCaptureIngress,
    pub receiver: PointerReceiverIngress,
}

/// Edge-local endpoint which delivered one pointer transition.
///
/// This is independent from both persistent capture and the hovered route.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerEventDeliveryIngress {
    ProviderEndpoint,
    Native {
        surface: SurfaceKey,
    },
    Foreign,
    None,
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Ordered physical pointer transition.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerEdgeKindSpec {
    Moved,
    PrimaryPressed,
    PrimaryReleased,
    PrimaryContactEnded,
    SecondaryPressed,
    SecondaryReleased,
    SecondaryContactEnded,
    StreamEnded,
    CaptureChanged,
    StreamCancelled {
        reason: PointerStreamCancelReasonSpec,
    },
    Scrolled {
        scroll: ScrollEdgeIngress,
    },
}

/// Lossless provider facts for one ordered wheel or trackpad edge.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollEdgeIngress {
    pub device: u64,
    pub sequence: Option<u64>,
    pub phase: ScrollPhaseSpec,
    pub delta: Option<ScrollDeltaIngress>,
    pub momentum: ScrollMomentumAuthorityIngress,
    pub modifiers: ScrollModifiersAuthorityIngress,
    pub delivery: ScrollDeliveryEndpointIngress,
}

/// Provider phase of one discrete or phaseful scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollPhaseSpec {
    Discrete,
    Begin,
    Update,
    End,
    Cancel { reason: ScrollCancelReasonSpec },
}

/// Explicit provider reason for cancelling one smooth scroll sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollCancelReasonSpec {
    PlatformCancelled,
    BindingRetired,
    DeviceRemoved,
    ProviderReset,
}

/// Raw two-axis scroll delta retained in its provider unit.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollDeltaIngress {
    PhysicalPixels {
        delta: ScrollVectorFixture,
        surface: SurfaceKey,
        coordinate_generation: CoordinateGenerationIngress,
    },
    LogicalPoints {
        delta: ScrollVectorFixture,
    },
    Lines {
        delta: ScrollVectorFixture,
    },
    Pages {
        delta: ScrollVectorFixture,
    },
}

impl ScrollDeltaIngress {
    pub(crate) const fn vector(self) -> ScrollVectorFixture {
        match self {
            Self::PhysicalPixels { delta, .. }
            | Self::LogicalPoints { delta }
            | Self::Lines { delta }
            | Self::Pages { delta } => delta,
        }
    }
}

/// Finite content-movement vector expressed in the enclosing delta unit.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollVectorFixture {
    pub x: f64,
    pub y: f64,
}

/// Direct-versus-momentum authority retained at event time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollMomentumAuthorityIngress {
    Known {
        momentum: ScrollMomentumSpec,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Whether the platform classified a sample as direct input or momentum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollMomentumSpec {
    Direct,
    Momentum,
}

/// Event-time modifier authority for one scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollModifiersAuthorityIngress {
    Known {
        shift: bool,
        control: bool,
        alt: bool,
        command: bool,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Exact presentation endpoint which physically received one scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScrollDeliveryEndpointIngress {
    Headless {
        surface: SurfaceKey,
        coordinate_generation: CoordinateGenerationIngress,
    },
    Native {
        surface: SurfaceKey,
        coordinate_generation: CoordinateGenerationIngress,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Event-time position, encoded in the provider scope's only lawful lane.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerLocationIngress {
    SurfaceLocal { position: PointAuthorityIngress },
    Desktop { route: DesktopRouteIngress },
}

/// Desktop-global route fact. Core derives every binding and coordinate proof
/// from current state; JSON names just a logical surface and physical facts.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DesktopRouteIngress {
    DockFromDesktop {
        surface: SurfaceKey,
        desktop_position: PointFixture,
    },
    Dock {
        surface: SurfaceKey,
        desktop_position: PointFixture,
        surface_position: PointFixture,
        coordinate_generation: CoordinateGenerationIngress,
    },
    Foreign {
        desktop_position: PointAuthorityIngress,
    },
    OutsideAll {
        desktop_position: PointFixture,
        /// Exact event-time work area. `None` preserves an explicit unavailable fact.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        work_area: Option<WorkAreaKey>,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Whether a trace uses the current coordinate generation or deliberately
/// submits an old one to prove fail-closed validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CoordinateGenerationIngress {
    Current,
    Stale,
}

/// Event-time pointer capture authority.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerCaptureIngress {
    ProviderEndpoint,
    Native {
        surface: SurfaceKey,
    },
    Foreign,
    None,
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Typed stream termination reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerStreamCancelReasonSpec {
    DeviceRemoved,
    ExplicitPlatformCancellation,
    BindingRetired,
}

/// Semantic response to one core-frozen receiver candidate. It never carries
/// output tickets, hit-region IDs, candidate IDs, presentation authority, or
/// adapter-owned route identity.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerReceiverIngress {
    NotApplicable,
    Unknown {
        reason: PointerReceiverUnknownReasonSpec,
    },
    Presented {
        delivery: Option<PointerDeliveryIngress>,
        hover: Option<PointerHoverIngress>,
    },
}

/// Semantic physical-delivery result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerDeliveryIngress {
    Tab {
        item: ItemKey,
    },
    TabClose {
        item: ItemKey,
    },
    Splitter {
        root: RootKey,
        path: StructuralPath,
        index: usize,
    },
    TabStripScroll {
        root: RootKey,
        path: StructuralPath,
    },
    TabListMenuScroll {
        root: RootKey,
        path: StructuralPath,
    },
    Canvas,
    Blocked,
    NoReceiver,
    Unknown {
        reason: PointerReceiverUnknownReasonSpec,
    },
}

/// Semantic point-bound hover-drop result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerHoverIngress {
    DockTarget {
        surface: SurfaceKey,
        target: PointerDropTargetIngress,
    },
    Blocked,
    NoReceiver,
    Unknown {
        reason: PointerReceiverUnknownReasonSpec,
    },
}

/// Stable structural identity of one declared docking target.
///
/// Runtime node and hit-region IDs remain core-owned. A trace identifies the
/// target by logical surface, root, and root-local structural path instead of
/// asking the harness to choose whichever core region is frontmost.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerDropTargetIngress {
    TabGap {
        root: RootKey,
        path: StructuralPath,
        index: usize,
    },
    Center {
        root: RootKey,
        path: StructuralPath,
    },
    InnerEdge {
        root: RootKey,
        path: StructuralPath,
        edge: EdgeSpec,
    },
    OuterEdge {
        root: RootKey,
        path: StructuralPath,
        edge: EdgeSpec,
    },
    SurfaceBackground,
}

/// Physical edge used by a structurally identified drop target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeSpec {
    Left,
    Right,
    Top,
    Bottom,
}

/// Why receiver evidence is unavailable for one edge or probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointerReceiverUnknownReasonSpec {
    FrameworkDeliveryUnavailable,
    EventCorrelationUnavailable,
    LayerAuthorityUnavailable,
    PresentationAuthorityUnavailable,
    NotReported,
}

/// Renderer-neutral point whose coordinate space is defined by its field.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PointFixture {
    pub x: f64,
    pub y: f64,
}

/// Known or unavailable point authority.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PointAuthorityIngress {
    Known {
        point: PointFixture,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Stable projection of provider authority failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorityUnavailableReasonSpec {
    ProviderUnavailable,
    PermissionDenied,
    SurfaceUnavailable,
    CoordinateUnavailable,
    NotReported,
}

/// Exact observable result of one reducer boundary.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedTransition {
    pub tick: ReducerTick,
    pub before: VersionExpectation,
    pub after: VersionExpectation,
    pub reduced: Vec<IngressRef>,
    /// Interaction outcomes produced by semantic inputs, bound to the exact
    /// producer-local ingress which caused them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reduced_interaction_outcomes: Vec<ExpectedReducedInteractionOutcome>,
    pub reduced_pointer_edges: Vec<ExpectedPointerEdge>,
    pub presentation_observations: Vec<ExpectedPresentationObservationOutcome>,
    pub presentation_emissions: usize,
    pub surface_contributions: Vec<ExpectedSurfaceContributionOutcome>,
    /// Commit-only interaction notifications in exact publication order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interaction_events: Vec<ExpectedInteractionEvent>,
    /// Provider-bound platform requests in exact dispatch order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platform_effects: Vec<ExpectedPlatformEffectEmission>,
    /// Complete adapter-facing focus publication for this boundary.
    #[serde(default, skip_serializing_if = "ExpectedFocusDelta::is_empty")]
    pub focus_delta: ExpectedFocusDelta,
    /// Presentation-authority changes in canonical surface order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_scene_deltas: Vec<ExpectedSurfaceSceneDelta>,
    pub interaction: ExpectedInteractionState,
    /// Exact stable roster which can receive interaction after this boundary.
    /// An incomplete popup-plane proof keeps the complete roster empty.
    pub interactive_surface_roster: Vec<SurfaceKey>,
    pub published_state_changed: bool,
}

/// Stable trace-local identity assigned when an effect is first observed.
///
/// This alias preserves effect causality without serializing the engine's
/// private allocation frontier.
pub type ExpectedEffectRef = EffectKey;

/// One committed interaction notification with its exact public cause.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedInteractionEvent {
    pub cause: ExpectedReductionCause,
    pub version: VersionExpectation,
    pub event: ExpectedInteractionEventKind,
}

/// Stable projection of the core-minted cause attached to a published event.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedReductionCause {
    Input { ingress: IngressRef },
    PointerEdge { sequence: u64 },
    PointerProviderRetirement,
    PlatformProviderReplacement,
    SurfaceContribution { surface: SurfaceKey },
    SurfacePresentationObservationBatch,
    PresentationObservation { stream: PresentationStreamRef },
    SurfaceContributionBatch,
    PresentationHostRetirement,
}

/// Stable adapter-visible payload of one committed interaction event.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedInteractionEventKind {
    ScrollTerminated {
        receiver: Option<ExpectedScrollReceiver>,
        reason: ExpectedScrollTerminationReason,
    },
    SemanticFocusRequested {
        root: RootKey,
        path: StructuralPath,
        item: ItemKey,
    },
    PreviewPublished {
        visual: ExpectedPreviewVisual,
    },
    PreviewCleared,
    Cancelled {
        status: ExpectedInteractionState,
        reason: ExpectedInteractionCancelReason,
    },
    Delivered {
        delivery: ExpectedWorkspaceDeliveryKind,
    },
    ResizeDelivered,
    ContainedTransformPreviewPublished {
        surface: SurfaceKey,
        root: RootKey,
        floating: FloatingPresentationKey,
        rect: RectFixture,
    },
    ContainedTransformDelivered,
    NativePresentationRequested {
        surface: SurfaceKey,
        effect: ExpectedEffectRef,
    },
}

/// Renderer-neutral preview visual published to an adapter.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPreviewVisual {
    Dock {
        surface: SurfaceKey,
        target: PointerDropTargetIngress,
        rect: RectFixture,
    },
    Contained {
        surface: SurfaceKey,
        rect: RectFixture,
        fallback: bool,
    },
    Native {
        host_surface: SurfaceKey,
        target_surface: SurfaceKey,
        placement: RectFixture,
    },
}

/// Semantic class of one committed workspace delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedWorkspaceDeliveryKind {
    Dock,
    Contained,
    ContainedFallback,
}

/// One effect emitted to the active platform provider.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPlatformEffectEmission {
    pub id: ExpectedEffectRef,
    pub epoch: u64,
    pub inventory_generation: u64,
    pub effect: ExpectedPlatformEffect,
}

/// Stable semantic projection of a platform request.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPlatformEffect {
    CreateWindow {
        surface: SurfaceKey,
        placement: RectFixture,
        role: ViewportRoleSpec,
    },
    ShowWindow {
        surface: SurfaceKey,
        after_hidden_generation: u64,
        after_pre_show_stream: PresentationStreamRef,
        after_pre_show_emission: u64,
    },
    CompensatingClose {
        surface: SurfaceKey,
        compensates: ExpectedEffectRef,
    },
    CancelRootClose {
        surface: SurfaceKey,
    },
    RetainChild {
        surface: SurfaceKey,
    },
    ReleaseChild {
        surface: SurfaceKey,
    },
    ContinueCleanup {
        surface: SurfaceKey,
        predecessor: ExpectedEffectRef,
        after: Option<ExpectedEffectRef>,
    },
    RequestRootClose {
        surface: SurfaceKey,
    },
    SetPointerPassthrough {
        surface: SurfaceKey,
        enabled: bool,
        after: Option<ExpectedEffectRef>,
    },
    RequestFocus {
        surface: SurfaceKey,
        after: Option<ExpectedEffectRef>,
    },
    RequestReplacement {
        surface: SurfaceKey,
        placement: RectFixture,
        role: ViewportRoleSpec,
    },
    ResolveNativeClose {
        surface: SurfaceKey,
        observed_at: u64,
        received_at: u64,
        resolution: NativeCloseResolutionSpec,
    },
}

/// Stable native-close decision requested from the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeCloseResolutionSpec {
    Accept,
    Cancel,
}

/// Adapter-facing focus changes produced by one atomic boundary.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedFocusDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_observation: Option<ExpectedFocusValueChange<ExpectedGlobalFocusObservation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_activation: Option<ExpectedFocusValueChange<ExpectedPendingViewportActivation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe_only_activation:
        Option<ExpectedFocusValueChange<ExpectedRecordedObserveOnlyActivation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_intent: Option<ExpectedFocusValueChange<ExpectedPaneFocusIntent>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_focus: Vec<ExpectedSurfaceFocusChange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ExpectedFocusEffectChange>,
}

impl ExpectedFocusDelta {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.global_observation.is_none()
            && self.pending_activation.is_none()
            && self.observe_only_activation.is_none()
            && self.pane_intent.is_none()
            && self.surface_focus.is_empty()
            && self.effects.is_empty()
    }
}

/// Before-and-after values of one focus publication slot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedFocusValueChange<T> {
    pub before: Option<T>,
    pub after: Option<T>,
}

/// One provider-owned global focus fact.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedGlobalFocusObservation {
    pub generation: u64,
    pub focused: ExpectedGlobalFocusAuthority,
    pub acknowledged_effect: ExpectedAcknowledgedEffectAuthority,
}

/// Authoritative focused-window value, or its typed unavailability.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedGlobalFocusAuthority {
    Dock {
        surface: SurfaceKey,
    },
    Foreign,
    None,
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Provider authority for the optional focus-effect acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedAcknowledgedEffectAuthority {
    Known {
        effect: Option<ExpectedEffectRef>,
    },
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Pending explicit activation visible to an adapter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPendingViewportActivation {
    pub request: ExpectedViewportActivationRequest,
    pub observation_baseline: u64,
    pub platform_focus: ExpectedPendingPlatformFocus,
}

/// Observe-only activation visible to an adapter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedRecordedObserveOnlyActivation {
    pub request: ExpectedViewportActivationRequest,
    pub observation_baseline: Option<u64>,
}

/// Stable semantic content of one viewport activation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedViewportActivationRequest {
    pub target: SurfaceKey,
    pub pane: ExpectedPaneFocusDisposition,
    pub cause: ExpectedViewportActivationCause,
}

/// Pane-focus component of an activation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPaneFocusDisposition {
    Preserve,
    Set { item: ItemKey },
    Clear,
}

/// Semantic origin of a viewport activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedViewportActivationCause {
    Explicit,
    DropCommitted,
    TearOffCommitted,
    PointerTabGesture,
    CloseRecovery,
    RecoveryReplacement,
    PlatformObservation,
}

/// Platform-focus phase retained by a pending activation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPendingPlatformFocus {
    EffectRequired,
    Requested {
        effect: ExpectedEffectRef,
    },
    Indeterminate {
        effect: ExpectedEffectRef,
        reason: ExpectedEffectIndeterminateReason,
    },
    ObservedAwaitingTarget {
        effect: ExpectedEffectRef,
        acknowledged_at: u64,
    },
}

/// One exact pane-focus command retained for adapter acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPaneFocusIntent {
    pub target: SurfaceKey,
    pub focus: ExpectedPanelFocus,
    pub source: ExpectedPaneFocusIntentSource,
    pub cause: Option<ExpectedViewportActivationCause>,
    pub focus_observation_baseline: u64,
    pub pane_observation_baseline: Option<u64>,
}

/// Exact pane focus requested or observed by an adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPanelFocus {
    Item { item: ItemKey },
    None,
}

/// Precedence source of a pane-focus intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPaneFocusIntentSource {
    PlatformActivation,
    ExplicitViewportActivation,
    PointerTabGesture,
    CloseRecovery,
}

/// Net pane-focus publication for one logical surface.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSurfaceFocusChange {
    pub surface: SurfaceKey,
    pub state: ExpectedFocusValueChange<ExpectedSurfaceFocusState>,
}

/// Published pane-focus history and latest provider observation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSurfaceFocusState {
    pub panel: ExpectedPanelFocusRecord,
    pub observation: Option<ExpectedPaneFocusObservation>,
}

/// Stable pane-focus history value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPanelFocusRecord {
    NoHistory,
    Item { item: ItemKey },
    None,
}

/// One exact pane-focus observation without its opaque acknowledgement token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPaneFocusObservation {
    pub generation: u64,
    pub focus: ExpectedPanelFocus,
    pub acknowledges_intent: bool,
}

/// Net phase change for one exact global focus effect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedFocusEffectChange {
    pub effect: ExpectedEffectRef,
    pub surface: SurfaceKey,
    pub phase: ExpectedFocusValueChange<ExpectedEffectPhase>,
    pub observed: Option<ExpectedObservedPlatformFocusEffect>,
}

/// Public phase of one effect ledger record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedEffectPhase {
    Requested,
    DispatchFailed {
        reason: ExpectedDispatchFailureReason,
    },
    ObservationDispatchFailed {
        reason: ExpectedDispatchFailureReason,
    },
    ObservedApplied {
        inventory_generation: u64,
    },
    CleanupObservationIndeterminate {
        predecessor: ExpectedEffectRef,
        reason: ExpectedEffectIndeterminateReason,
    },
    CleanupResultObserved {
        predecessor: ExpectedEffectRef,
    },
    CleanupObservationSuperseded {
        predecessor: ExpectedEffectRef,
        successor: ExpectedEffectRef,
    },
    Unsupported {
        reason: ExpectedEffectUnsupportedReason,
    },
    ObservationUnsupported {
        reason: ExpectedEffectUnsupportedReason,
    },
    Indeterminate {
        reason: ExpectedEffectIndeterminateReason,
    },
    Destroyed {
        inventory_generation: u64,
    },
    Invalidated {
        reason: ExpectedEffectInvalidation,
    },
}

/// Stable effect dispatch failure class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedDispatchFailureReason {
    AdapterRejected,
    WindowUnavailable,
    ProviderStopped,
}

/// Stable unsupported-effect class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedEffectUnsupportedReason {
    BackendUnsupported,
    CapabilityRevoked,
}

/// Stable indeterminate-effect class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedEffectIndeterminateReason {
    AcknowledgementLost,
    ProviderRestarted,
}

/// Stable pre-dispatch invalidation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedEffectInvalidation {
    WorkspaceReplaced { replacement_epoch: u64 },
    NativeCreateAborted,
    PreAdmissionCloseCleared,
    PlatformProviderReplaced,
}

/// Provider proof which settled one focus effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedObservedPlatformFocusEffect {
    pub surface: SurfaceKey,
    pub generation: u64,
    pub evidence: ExpectedPlatformFocusEvidence,
}

/// Stable evidence class for an observed platform focus effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPlatformFocusEvidence {
    ExactEffectAcknowledgement,
    NewerMatchingObservation,
}

/// One presentation-authority change in canonical surface order.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSurfaceSceneDelta {
    pub surface: SurfaceKey,
    pub before: Option<ExpectedSurfaceSceneState>,
    pub after: Option<ExpectedSurfaceSceneState>,
}

/// Stable adapter-visible authority of one surface scene.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSurfaceSceneState {
    pub state: ExpectedSurfaceSceneStateKind,
    pub presented: Option<ExpectedPresentedSurfaceAuthority>,
}

/// Public scene availability class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedSurfaceSceneStateKind {
    Ready,
    Stale,
    Bootstrap,
}

/// Stable presentation stream and emission authorizing interaction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPresentedSurfaceAuthority {
    pub stream: PresentationStreamRef,
    pub emission: u64,
    pub coordinate_generation: u64,
}

/// Stable observable result of reducing one stream presentation fact.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPresentationObservationOutcome {
    NoUpdate {
        stream: PresentationStreamRef,
    },
    CapturedUnknown {
        stream: PresentationStreamRef,
        generation: u64,
        reason: AuthorityUnavailableReasonSpec,
    },
    Presented {
        stream: PresentationStreamRef,
        generation: u64,
        settled_through: u64,
        presented: u64,
        retired_output_count: usize,
        promotion_eligible: bool,
    },
    Retired {
        stream: PresentationStreamRef,
        generation: u64,
        settled_through: u64,
        presented: ExpectedRetiredPresentation,
        retired_output_count: usize,
        promotion_eligible: bool,
    },
    Rejected {
        stream: PresentationStreamRef,
        reason: ExpectedPresentationObservationRejection,
    },
}

/// Stable non-presented result retained with one terminal stream range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedRetiredPresentation {
    None,
    Unknown {
        reason: AuthorityUnavailableReasonSpec,
    },
}

/// Stable class of a non-fatal presentation observation rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPresentationObservationRejection {
    CaptureGenerationNotIncreasing,
    SettlementFromDifferentStream,
    SettlementNotIncreasing,
    SettlementNotPending,
    PresentedFromDifferentStream,
    PresentedOutsideRetirementRange,
    PresentedNotPending,
}

/// Expected journal edge result. Sequence comes from provider input, while the
/// stream and ticket remain core-owned and are asserted only by their causal
/// reduction presence.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedPointerEdge {
    pub cause: ExpectedPointerEdgeCause,
    /// Core-minted provider incarnation which owned the reduced stream.
    pub provider_incarnation: u64,
    /// Core-minted pointer-stream incarnation which consumed this edge.
    pub stream_incarnation: u64,
    /// Provider sequence identifying the exact ingress edge. Replay verifies
    /// pointer, button, location, and capture against that complete fixture.
    pub sequence: u64,
    pub outcomes: Vec<ExpectedInteractionOutcome>,
}

/// Stable interaction result produced by one reduced semantic input.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedReducedInteractionOutcome {
    pub ingress: IngressRef,
    pub outcome: ExpectedInteractionOutcome,
}

/// Stable public close target without core-owned request or decision tokens.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedCloseTarget {
    Item {
        item: ItemKey,
    },
    Root {
        root: RootKey,
    },
    Surface {
        surface: SurfaceKey,
        disposition: ExpectedSurfaceCloseDisposition,
    },
}

/// Stable surface-close disposition carried by a close target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedSurfaceCloseDisposition {
    RetainLayout,
    RehomeAll { target: SurfaceKey },
    CloseContent,
}

/// Stable observable class of a reduced pointer edge's causal authority.
///
/// Pointer-edge traces deliberately do not serialize core-minted stream or
/// ticket identities. They do require the transition to prove that the edge
/// was reduced through the pointer journal rather than another reducer path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPointerEdgeCause {
    PointerEdge,
}

/// Stable observable projection of a reduced interaction result.
///
/// Opaque session IDs, close-plan tokens, and command payloads remain
/// core-owned. Variant identity and externally meaningful booleans remain
/// exact so a trace cannot silently reinterpret one state-machine branch as
/// another.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedInteractionOutcome {
    ScrollBegan {
        receiver: ExpectedScrollReceiver,
    },
    ScrollApplied {
        session: bool,
        receiver: ExpectedScrollReceiver,
        phase: ScrollPhaseSpec,
        requested_delta: f64,
        applied_delta: f64,
        unapplied_delta: f64,
        offset: f64,
    },
    ScrollSuppressed {
        session: bool,
        sequence: Option<u64>,
        phase: ScrollPhaseSpec,
        reason: ExpectedScrollSuppressionReason,
    },
    ScrollTerminated {
        receiver: Option<ExpectedScrollReceiver>,
        reason: ExpectedScrollTerminationReason,
    },
    CloseRequested {
        target: ExpectedCloseTarget,
        items: Vec<ItemKey>,
        reused: bool,
    },
    TabStripControlActivated {
        control: ExpectedTabStripControl,
        changed: bool,
        menu_open: bool,
    },
    TabListMenuItemSelected {
        item: ItemKey,
        changed: bool,
    },
    TabListMenuDismissed,
    TabListMenuFrameConsumed,
    TabStripScrolled {
        changed: bool,
    },
    TabListMenuScrolled {
        changed: bool,
    },
    TabListMenuFocusMoved {
        changed: bool,
    },
    DragArmed,
    DragBegan,
    Preview {
        status: ExpectedPreviewResolutionStatus,
    },
    PreviewAcknowledged {
        changed: bool,
    },
    DragDelivered,
    /// A drag release was consumed and now awaits proof that its exact
    /// core-owned preview was presented.
    ///
    /// The session and preview identities remain opaque to the trace. Replay
    /// distinguishes this obligation from both immediate delivery and a
    /// contained-transform obligation through the variant itself.
    PendingDragRelease,
    Cancelled {
        reason: ExpectedInteractionCancelReason,
    },
    ResizeBegan,
    ResizeUpdated,
    SplitterAdjusted {
        changed: bool,
    },
    ResizeDelivered {
        changed: bool,
    },
    ContainedPlacementApplied {
        changed: bool,
    },
    ContainedTransformBegan,
    ContainedTransformPreviewUpdated,
    ContainedTransformPreviewAcknowledged {
        changed: bool,
    },
    /// A contained-transform release was consumed and now awaits proof that
    /// its exact core-owned rectangle preview was presented.
    ///
    /// The trace deliberately records no session or preview token because
    /// neither identity can be supplied by a protocol fixture.
    PendingContainedTransformRelease,
    ContainedTransformDelivered {
        changed: bool,
    },
    Rejected,
}

/// Stable structural identity of one core-owned docking scroll receiver.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedScrollReceiver {
    TabStrip {
        surface: SurfaceKey,
        root: RootKey,
        path: StructuralPath,
    },
    TabListMenu {
        surface: SurfaceKey,
        root: RootKey,
        path: StructuralPath,
    },
}

/// Stable class of a fail-closed scroll delivery result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedScrollSuppressionReason {
    ReceiverUnknown,
    FrameworkBlocked,
    NoReceiver,
    DockCanvas,
    DockBlocker { blocker: ExpectedScrollBlocker },
}

/// Stable structural identity of a core-owned blocker in the scroll lane.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedScrollBlocker {
    TabListMenuFrame {
        surface: SurfaceKey,
        root: RootKey,
        path: StructuralPath,
    },
    TabListMenuBackdrop {
        surface: SurfaceKey,
        root: RootKey,
        path: StructuralPath,
    },
    ContainedFrame {
        surface: SurfaceKey,
        floating: FloatingPresentationKey,
    },
}

/// Stable terminal reason for one core-owned smooth-scroll session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedScrollTerminationReason {
    Completed,
    Cancelled { reason: ScrollCancelReasonSpec },
    StreamCancelled,
    StreamEnded,
    ProviderRetired,
    ReceiverLost,
    DeliveryEndpointChanged,
    BindingRetired,
    PresentationHostRetired,
    SurfaceRemoved,
    PopupRoutingChanged,
    PolicyChanged,
    PresentationConfigChanged,
}

/// Stable semantic class of a core-owned tab-strip control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedTabStripControl {
    ScrollBackward,
    ScrollForward,
    TabListMenu,
}

/// Stable public summary of the current interaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedInteractionState {
    Idle,
    Pressed,
    Armed,
    Dragging,
    Resizing,
    ContainedTransforming,
}

/// Public result class of resolving one drag observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedPreviewResolutionStatus {
    Resolved,
    KnownNone,
    Rejected,
    Unavailable,
    UnknownAuthority,
    OpaqueBlocker,
    NativeCapabilityUnknown,
    NativePlacementUnavailable,
}

/// Stable cancellation cause shared by journal and semantic interaction outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedInteractionCancelReason {
    Escape,
    ReleasedBeforeDrag,
    CaptureLost,
    CaptureAuthorityUnavailable,
    DeliveryAuthorityUnavailable,
    DeliveryOwnerLost,
    PointerStreamCancelled,
    PointerStreamEnded,
    UnknownButtonState,
    UnknownTargetAuthority,
    OpaquePointerBlocker,
    ClickReceiverMismatch,
    NativeCapabilityUnknown,
    NativeCapabilityUnavailable,
    NativePlacementUnavailable,
    SourceVanished,
    WorkspaceChanged,
    PolicyChanged,
    SurfaceClosed,
    SceneUnavailable,
    PointerProviderRetired,
    ReplacedByNewGesture,
    WorkspaceRestored,
}

/// Reducer contribution outcomes in canonical surface order.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpectedSurfaceContributionOutcome {
    Ready { surface: SurfaceKey },
    Retained { surface: SurfaceKey },
    Unavailable { surface: SurfaceKey },
    Rejected { surface: SurfaceKey },
}

/// Workspace version used at a boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionExpectation {
    pub epoch: u64,
    pub revision: u64,
}

/// Canonical durable state with structural paths and exact item ownership.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectedCanonicalSnapshot {
    pub workspace: CanonicalWorkspace,
}

/// Canonical workspace state.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalWorkspace {
    pub version: VersionExpectation,
    pub roots: Vec<CanonicalRoot>,
    pub surfaces: Vec<CanonicalSurface>,
    pub item_multiset: Vec<ItemCount>,
    pub item_owners: Vec<ItemOwner>,
}

/// One canonical stable root.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalRoot {
    pub id: RootKey,
    pub central_path: Option<StructuralPath>,
    pub owner: RootOwner,
    pub node: NodeFixture,
}

/// Exact presentation carrier for a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RootOwner {
    Main {
        surface: SurfaceKey,
    },
    Contained {
        surface: SurfaceKey,
        floating: FloatingPresentationKey,
    },
}

/// One canonical logical surface.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSurface {
    pub id: SurfaceKey,
    pub main_root: Option<RootKey>,
    pub contained: Vec<CanonicalContained>,
}

/// One canonical contained presentation.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalContained {
    pub id: FloatingPresentationKey,
    pub root: RootKey,
    pub rect: RectFixture,
}

/// Exact multiplicity for one stable item identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemCount {
    pub item: ItemKey,
    pub count: usize,
}

/// Exact structural and presentation owner of one item occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ItemOwner {
    pub item: ItemKey,
    pub root: RootKey,
    pub path: StructuralPath,
    pub owner: RootOwner,
}

/// Decode or replay failure with trace-local context.
#[derive(Debug)]
pub enum CoreProtocolTraceError {
    Json(serde_json::Error),
    UnsupportedSchema { actual: String },
    Invalid(String),
    Replay(String),
}

impl fmt::Display for CoreProtocolTraceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid core protocol JSON: {error}"),
            Self::UnsupportedSchema { actual } => {
                write!(
                    formatter,
                    "unsupported core protocol trace schema `{actual}`"
                )
            }
            Self::Invalid(detail) => write!(formatter, "invalid core protocol trace: {detail}"),
            Self::Replay(detail) => {
                write!(formatter, "core protocol trace replay failed: {detail}")
            }
        }
    }
}

impl Error for CoreProtocolTraceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::UnsupportedSchema { .. } | Self::Invalid(_) | Self::Replay(_) => None,
        }
    }
}

impl From<serde_json::Error> for CoreProtocolTraceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Decodes and validates one complete trace suite.
///
/// # Errors
///
/// Returns [`CoreProtocolTraceError`] for malformed JSON, an unknown schema,
/// or an invalid trace contract.
pub fn decode_core_protocol_trace_suite(
    input: &str,
) -> Result<CoreProtocolTraceSuite, CoreProtocolTraceError> {
    let suite: CoreProtocolTraceSuite = serde_json::from_str(input)?;
    validate_core_protocol_trace_suite(&suite)?;
    Ok(suite)
}

/// Validates the static shape of one trace suite.
///
/// # Errors
///
/// Returns [`CoreProtocolTraceError`] when identifiers, causal order, journal
/// watermarks, or semantic receiver facts are malformed.
pub fn validate_core_protocol_trace_suite(
    suite: &CoreProtocolTraceSuite,
) -> Result<(), CoreProtocolTraceError> {
    if suite.schema != CORE_PROTOCOL_TRACE_SCHEMA {
        return Err(CoreProtocolTraceError::UnsupportedSchema {
            actual: suite.schema.clone(),
        });
    }
    if suite.source_baselines.is_empty() {
        return Err(CoreProtocolTraceError::Invalid(
            "source_baselines is empty".into(),
        ));
    }
    let baseline_ids = suite
        .source_baselines
        .iter()
        .map(|baseline| baseline.id.clone())
        .collect::<BTreeSet<_>>();
    if baseline_ids.len() != suite.source_baselines.len() {
        return Err(CoreProtocolTraceError::Invalid(
            "source_baselines contains duplicate ids".into(),
        ));
    }
    if suite.traces.is_empty() {
        return Err(CoreProtocolTraceError::Invalid("traces is empty".into()));
    }
    let mut trace_ids = BTreeSet::new();
    for trace in &suite.traces {
        if !trace_ids.insert(trace.id.clone()) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace id `{}` is duplicated",
                trace.id.as_str()
            )));
        }
        if !baseline_ids.contains(&trace.provenance.baseline) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` names unknown baseline `{}`",
                trace.id.as_str(),
                trace.provenance.baseline.as_str()
            )));
        }
        validate_trace(trace)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TracePointerProviderState {
    Absent,
    Active(PointerProviderScopeIngress),
    RetiredAwaitingDrain,
}

fn validate_trace(trace: &CoreProtocolTrace) -> Result<(), CoreProtocolTraceError> {
    if trace.boundaries.is_empty() {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` has no boundaries",
            trace.id.as_str()
        )));
    }
    let root_ids = trace
        .initial_workspace
        .roots
        .iter()
        .map(|root| root.id)
        .collect::<BTreeSet<_>>();
    if root_ids.len() != trace.initial_workspace.roots.len() {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` has duplicate root ids",
            trace.id.as_str()
        )));
    }
    let mut surface_ids = trace
        .initial_workspace
        .surfaces
        .iter()
        .map(|surface| surface.id)
        .collect::<BTreeSet<_>>();
    if surface_ids.len() != trace.initial_workspace.surfaces.len() {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` has duplicate surface ids",
            trace.id.as_str()
        )));
    }
    for surface in &trace.initial_workspace.surfaces {
        if let Some(root) = surface.main_root
            && !root_ids.contains(&root)
        {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` surface {} names missing main root {}",
                trace.id.as_str(),
                surface.id.0,
                root.0
            )));
        }
    }

    let mut boundary_ids = BTreeSet::new();
    let mut producer_sequences = BTreeMap::<ProducerId, u64>::new();
    let mut provider_state = TracePointerProviderState::Absent;
    let mut previous_tick = 0_u64;
    for boundary in &trace.boundaries {
        if !boundary_ids.insert(boundary.id.clone()) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` repeats boundary `{}`",
                trace.id.as_str(),
                boundary.id.as_str()
            )));
        }
        let expected_tick = match &boundary.provider {
            Some(PointerProviderIngress::RetireSurfaceLocal { expected })
                if expected.disposition
                    == ExpectedSurfaceLocalPointerMaintenanceDisposition::CompactedPreviouslyRetired =>
            {
                previous_tick
            }
            _ => previous_tick.checked_add(1).ok_or_else(|| {
                CoreProtocolTraceError::Invalid(format!(
                    "trace `{}` exhausts the reducer tick frontier",
                    trace.id.as_str()
                ))
            })?,
        };
        if boundary.expected.tick.0 != expected_tick {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` expects reducer tick {}, but this boundary class requires {}",
                trace.id.as_str(),
                boundary.id.as_str(),
                boundary.expected.tick.0,
                expected_tick
            )));
        }
        previous_tick = boundary.expected.tick.0;
        validate_provider_boundary_contract(trace, boundary)?;
        if let Some(provider) = &boundary.provider {
            match provider {
                PointerProviderIngress::Activate { scope, .. } => {
                    if provider_state != TracePointerProviderState::Absent {
                        return Err(CoreProtocolTraceError::Invalid(format!(
                            "trace `{}` activates a second pointer provider before draining its predecessor at `{}`",
                            trace.id.as_str(),
                            boundary.id.as_str()
                        )));
                    }
                    provider_state = TracePointerProviderState::Active(*scope);
                }
                PointerProviderIngress::RetireDesktopGlobal {} => {
                    if provider_state
                        != TracePointerProviderState::Active(
                            PointerProviderScopeIngress::DesktopGlobal {},
                        )
                    {
                        return Err(CoreProtocolTraceError::Invalid(format!(
                            "trace `{}` retires a missing or non-desktop pointer provider at `{}`",
                            trace.id.as_str(),
                            boundary.id.as_str()
                        )));
                    }
                    provider_state = TracePointerProviderState::Absent;
                }
                PointerProviderIngress::RetireSurfaceLocal { expected } => {
                    let valid = matches!(
                        (provider_state, expected.disposition),
                        (
                            TracePointerProviderState::Active(
                                PointerProviderScopeIngress::SurfaceLocal { .. }
                            ),
                            ExpectedSurfaceLocalPointerMaintenanceDisposition::RetiredActive
                        ) | (
                            TracePointerProviderState::RetiredAwaitingDrain,
                            ExpectedSurfaceLocalPointerMaintenanceDisposition::CompactedPreviouslyRetired
                        )
                    );
                    if !valid {
                        return Err(CoreProtocolTraceError::Invalid(format!(
                            "trace `{}` surface-local retirement at `{}` does not match provider state {:?} and disposition {:?}",
                            trace.id.as_str(),
                            boundary.id.as_str(),
                            provider_state,
                            expected.disposition
                        )));
                    }
                    provider_state = TracePointerProviderState::Absent;
                }
                PointerProviderIngress::RetirePresentationHost {} => {
                    if !matches!(
                        provider_state,
                        TracePointerProviderState::Active(
                            PointerProviderScopeIngress::SurfaceLocal { .. }
                        )
                    ) {
                        return Err(CoreProtocolTraceError::Invalid(format!(
                            "trace `{}` retires a presentation host without an active surface-local producer at `{}`",
                            trace.id.as_str(),
                            boundary.id.as_str()
                        )));
                    }
                    provider_state = TracePointerProviderState::RetiredAwaitingDrain;
                }
            }
        }
        let pointer_segment_count = boundary
            .events
            .iter()
            .filter(|event| matches!(event, HostFrameEvent::PointerJournal { .. }))
            .count();
        let provider_active = matches!(provider_state, TracePointerProviderState::Active(_));
        if provider_active != (pointer_segment_count > 0) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` must {} at least one complete pointer journal segment",
                trace.id.as_str(),
                boundary.id.as_str(),
                if provider_active {
                    "contain"
                } else {
                    "not contain"
                }
            )));
        }
        validate_presentation_contract(trace, boundary, &surface_ids)?;
        validate_events(trace, boundary, &mut producer_sequences, &surface_ids)?;
        validate_expected_interaction_outcomes(trace, boundary)?;
        for surface in boundary
            .expected
            .platform_effects
            .iter()
            .filter_map(|emission| match &emission.effect {
                ExpectedPlatformEffect::CreateWindow { surface, .. } => Some(*surface),
                _ => None,
            })
        {
            if !surface_ids.insert(surface) {
                return Err(CoreProtocolTraceError::Invalid(format!(
                    "trace `{}` boundary `{}` creates already-known surface {}",
                    trace.id.as_str(),
                    boundary.id.as_str(),
                    surface.0
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_provider_boundary_contract(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
) -> Result<(), CoreProtocolTraceError> {
    if !matches!(
        boundary.provider,
        Some(
            PointerProviderIngress::RetireDesktopGlobal {}
                | PointerProviderIngress::RetireSurfaceLocal { .. }
                | PointerProviderIngress::RetirePresentationHost {}
        )
    ) {
        return Ok(());
    }
    if !boundary.events.is_empty() {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` pointer-provider retirement boundary `{}` cannot contain host-frame events",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    if !matches!(
        boundary.presentation_observation,
        PresentationObservationIngress::NoUpdate {}
    ) || !boundary.presentation_dispositions.is_empty()
        || !boundary.surface_contributions.is_empty()
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` pointer-provider retirement boundary `{}` cannot contain host-frame presentation or surface facts",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    Ok(())
}

fn validate_presentation_contract(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    let ingress_streams = match &boundary.presentation_observation {
        PresentationObservationIngress::NoUpdate {} => BTreeSet::new(),
        PresentationObservationIngress::Batch { observations } => {
            let mut streams = BTreeSet::new();
            for observation in observations {
                validate_presentation_stream_ref(trace, boundary, observation.stream, surfaces)?;
                if !streams.insert(observation.stream) {
                    return Err(CoreProtocolTraceError::Invalid(format!(
                        "trace `{}` boundary `{}` repeats presentation stream {:?}",
                        trace.id.as_str(),
                        boundary.id.as_str(),
                        observation.stream
                    )));
                }
                match observation.observation {
                    PresentationStreamObservationSpec::NoUpdate
                    | PresentationStreamObservationSpec::CapturedUnknown { .. } => {}
                    PresentationStreamObservationSpec::Retired {
                        settled_through,
                        presented,
                        ..
                    } => {
                        if settled_through == 0 {
                            return Err(CoreProtocolTraceError::Invalid(format!(
                                "trace `{}` boundary `{}` uses zero as a settled emission sequence",
                                trace.id.as_str(),
                                boundary.id.as_str()
                            )));
                        }
                        match presented {
                            RetiredPresentationIngress::Presented { emission } if emission == 0 => {
                                return Err(CoreProtocolTraceError::Invalid(format!(
                                    "trace `{}` boundary `{}` uses zero as a presented emission sequence",
                                    trace.id.as_str(),
                                    boundary.id.as_str()
                                )));
                            }
                            RetiredPresentationIngress::Presented { .. }
                            | RetiredPresentationIngress::None
                            | RetiredPresentationIngress::Unknown { .. } => {}
                        }
                    }
                }
            }
            streams
        }
    };

    let mut expected_streams = BTreeSet::new();
    for outcome in &boundary.expected.presentation_observations {
        let stream = expected_presentation_stream(outcome);
        validate_presentation_stream_ref(trace, boundary, stream, surfaces)?;
        if !expected_streams.insert(stream) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` repeats expected presentation stream {:?}",
                trace.id.as_str(),
                boundary.id.as_str(),
                stream
            )));
        }
    }
    if ingress_streams != expected_streams {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` presentation ingress and expected outcome stream sets differ",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }

    let mut disposition_slots = BTreeSet::new();
    for disposition in &boundary.presentation_dispositions {
        let (surface, role) = match disposition {
            PresentationDispositionIngress::Surface { surface, .. } => (*surface, "surface"),
            PresentationDispositionIngress::NativeStaging { surface, .. } => {
                (*surface, "native_staging")
            }
        };
        if !surfaces.contains(&surface) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` presentation disposition names unknown surface {}",
                trace.id.as_str(),
                boundary.id.as_str(),
                surface.0
            )));
        }
        if !disposition_slots.insert((surface, role)) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` repeats `{role}` presentation slot for surface {}",
                trace.id.as_str(),
                boundary.id.as_str(),
                surface.0
            )));
        }
    }

    let roster = &boundary.expected.interactive_surface_roster;
    if roster.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` interaction surface roster is not canonical",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    if let Some(surface) = roster.iter().find(|surface| !surfaces.contains(surface)) {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` interaction roster names unknown surface {}",
            trace.id.as_str(),
            boundary.id.as_str(),
            surface.0
        )));
    }
    Ok(())
}

fn validate_presentation_stream_ref(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    stream: PresentationStreamRef,
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    if stream.sequence == 0 {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` uses zero as a presentation stream sequence",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    if !surfaces.contains(&stream.surface) {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` presentation stream names unknown surface {}",
            trace.id.as_str(),
            boundary.id.as_str(),
            stream.surface.0
        )));
    }
    Ok(())
}

const fn expected_presentation_stream(
    outcome: &ExpectedPresentationObservationOutcome,
) -> PresentationStreamRef {
    match outcome {
        ExpectedPresentationObservationOutcome::NoUpdate { stream }
        | ExpectedPresentationObservationOutcome::CapturedUnknown { stream, .. }
        | ExpectedPresentationObservationOutcome::Presented { stream, .. }
        | ExpectedPresentationObservationOutcome::Retired { stream, .. }
        | ExpectedPresentationObservationOutcome::Rejected { stream, .. } => *stream,
    }
}

fn validate_expected_interaction_outcomes(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
) -> Result<(), CoreProtocolTraceError> {
    let mut outcomes = BTreeSet::new();
    for expected in &boundary.expected.reduced_interaction_outcomes {
        if !boundary.expected.reduced.contains(&expected.ingress) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` expects an interaction outcome for an unreduced ingress",
                trace.id.as_str(),
                boundary.id.as_str()
            )));
        }
        if !outcomes.insert(expected.ingress.clone()) {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` repeats one semantic interaction outcome ingress",
                trace.id.as_str(),
                boundary.id.as_str()
            )));
        }
    }
    Ok(())
}

fn validate_events(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    producer_sequences: &mut BTreeMap<ProducerId, u64>,
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    for event in &boundary.events {
        match event {
            HostFrameEvent::SemanticInput {
                producer,
                source_sequence,
                ..
            } => {
                if let Some(previous) = producer_sequences.get(producer)
                    && source_sequence <= previous
                {
                    return Err(CoreProtocolTraceError::Invalid(format!(
                        "trace `{}` producer `{}` regressed source sequence at `{}`",
                        trace.id.as_str(),
                        producer.as_str(),
                        boundary.id.as_str()
                    )));
                }
                producer_sequences.insert(producer.clone(), *source_sequence);
            }
            HostFrameEvent::PointerJournal {
                previous,
                through,
                edges,
            } => validate_pointer_journal(trace, boundary, *previous, *through, edges, surfaces)?,
        }
    }
    Ok(())
}

fn validate_pointer_journal(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    previous: u64,
    through: u64,
    edges: &[PointerEdgeIngress],
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    if through < previous {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` reverses pointer watermark",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    if through.saturating_sub(previous) != u64::try_from(edges.len()).unwrap_or(u64::MAX) {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "trace `{}` boundary `{}` journal does not cover its complete interval",
            trace.id.as_str(),
            boundary.id.as_str()
        )));
    }
    for (offset, edge) in edges.iter().enumerate() {
        let expected = previous + u64::try_from(offset).unwrap_or(u64::MAX) + 1;
        if edge.sequence != expected {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "trace `{}` boundary `{}` journal has a gap or reordered edge",
                trace.id.as_str(),
                boundary.id.as_str()
            )));
        }
        validate_pointer_edge(trace, boundary, edge, surfaces)?;
    }
    Ok(())
}

fn validate_pointer_edge(
    trace: &CoreProtocolTrace,
    boundary: &CoreProtocolTraceBoundary,
    edge: &PointerEdgeIngress,
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    let context = || {
        format!(
            "trace `{}` boundary `{}`",
            trace.id.as_str(),
            boundary.id.as_str()
        )
    };
    match &edge.location {
        PointerLocationIngress::SurfaceLocal { position } => validate_point_authority(position)?,
        PointerLocationIngress::Desktop { route } => match route {
            DesktopRouteIngress::DockFromDesktop {
                surface,
                desktop_position,
            } => {
                if !surfaces.contains(surface) {
                    return Err(CoreProtocolTraceError::Invalid(format!(
                        "{} names unknown desktop target surface {}",
                        context(),
                        surface.0
                    )));
                }
                validate_point(*desktop_position)?;
            }
            DesktopRouteIngress::Dock {
                surface,
                desktop_position,
                surface_position,
                ..
            } => {
                if !surfaces.contains(surface) {
                    return Err(CoreProtocolTraceError::Invalid(format!(
                        "{} names unknown desktop target surface {}",
                        context(),
                        surface.0
                    )));
                }
                validate_point(*desktop_position)?;
                validate_point(*surface_position)?;
            }
            DesktopRouteIngress::Foreign { desktop_position } => {
                validate_point_authority(desktop_position)?;
            }
            DesktopRouteIngress::OutsideAll {
                desktop_position, ..
            } => validate_point(*desktop_position)?,
            DesktopRouteIngress::Unknown { .. } => {}
        },
    }
    if let PointerEdgeKindSpec::Scrolled { scroll } = edge.kind {
        validate_scroll_edge(&context(), scroll, surfaces)?;
        if matches!(edge.receiver, PointerReceiverIngress::NotApplicable)
            || matches!(
                edge.receiver,
                PointerReceiverIngress::Presented { delivery: None, .. }
            )
        {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "{} scroll edge has no explicit delivery receipt",
                context()
            )));
        }
    }
    if let PointerCaptureIngress::Native { surface } = edge.capture
        && !surfaces.contains(&surface)
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{} names unknown native capture surface {}",
            context(),
            surface.0
        )));
    }
    if let PointerEventDeliveryIngress::Native { surface } = edge.delivery
        && !surfaces.contains(&surface)
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{} names unknown native delivery surface {}",
            context(),
            surface.0
        )));
    }
    if let PointerReceiverIngress::Presented {
        hover: Some(PointerHoverIngress::DockTarget { surface, .. }),
        ..
    } = &edge.receiver
        && !surfaces.contains(surface)
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{} names unknown hover receiver surface {}",
            context(),
            surface.0
        )));
    }
    Ok(())
}

fn validate_scroll_edge(
    context: &str,
    scroll: ScrollEdgeIngress,
    surfaces: &BTreeSet<SurfaceKey>,
) -> Result<(), CoreProtocolTraceError> {
    let legal_shape = match scroll.phase {
        ScrollPhaseSpec::Discrete => scroll.sequence.is_none() && scroll.delta.is_some(),
        ScrollPhaseSpec::Begin | ScrollPhaseSpec::End => scroll.sequence.is_some(),
        ScrollPhaseSpec::Update => scroll.sequence.is_some() && scroll.delta.is_some(),
        ScrollPhaseSpec::Cancel { .. } => scroll.sequence.is_some() && scroll.delta.is_none(),
    };
    if !legal_shape {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{context} scroll edge has an invalid phase/token/delta shape"
        )));
    }
    if let Some(delta) = scroll.delta {
        let vector = delta.vector();
        if !vector.x.is_finite() || !vector.y.is_finite() {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "{context} scroll edge contains a non-finite delta"
            )));
        }
        if let ScrollDeltaIngress::PhysicalPixels { surface, .. } = delta
            && !surfaces.contains(&surface)
        {
            return Err(CoreProtocolTraceError::Invalid(format!(
                "{context} physical scroll delta names unknown surface {}",
                surface.0
            )));
        }
    }
    let endpoint = match scroll.delivery {
        ScrollDeliveryEndpointIngress::Headless { surface, .. }
        | ScrollDeliveryEndpointIngress::Native { surface, .. } => Some(surface),
        ScrollDeliveryEndpointIngress::Unknown { .. } => None,
    };
    if let Some(surface) = endpoint
        && !surfaces.contains(&surface)
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{context} scroll delivery endpoint names unknown surface {}",
            surface.0
        )));
    }
    if let (
        Some(ScrollDeltaIngress::PhysicalPixels {
            surface: delta_surface,
            coordinate_generation: delta_generation,
            ..
        }),
        ScrollDeliveryEndpointIngress::Native {
            surface: endpoint_surface,
            coordinate_generation: endpoint_generation,
        },
    ) = (scroll.delta, scroll.delivery)
        && (delta_surface != endpoint_surface || delta_generation != endpoint_generation)
    {
        return Err(CoreProtocolTraceError::Invalid(format!(
            "{context} physical scroll delta and native delivery endpoint disagree"
        )));
    }
    Ok(())
}

fn validate_point_authority(point: &PointAuthorityIngress) -> Result<(), CoreProtocolTraceError> {
    if let PointAuthorityIngress::Known { point } = point {
        validate_point(*point)?;
    }
    Ok(())
}

fn validate_point(point: PointFixture) -> Result<(), CoreProtocolTraceError> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err(CoreProtocolTraceError::Invalid(
            "point contains a non-finite coordinate".into(),
        ));
    }
    Ok(())
}
