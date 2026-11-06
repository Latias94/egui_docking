# Retained facts and current bindings: an implementable replacement design

Date: 2026-11-06. Source baseline: documentation HEAD `34ab23a`, runtime `54840717a2454c957b13a667a939ddf8229e52e9`.

This is a read-only design proposal. No source edit, Cargo command, test, benchmark, Git mutation, network request, or nested agent was used. The proposed Interface is intentionally breaking. Existing error priority, independent-workspace value equality, public record constructors, and duplicated proof representations are not design constraints.

## Decision

Make the presentation Module own **retained immutable facts plus small current bindings**, and remove current command authority from geometric records. The first complete slice must select a tab with unchanged effective layout inputs without reconstructing the surface requirements, target records, program, hit inventory, or semantic inventory. Every current measurement request and every paint operation still executes. Changed scroll/policy/minima take explicit larger update paths.

The important seam is not an aggregate named `Generation`. It is the seam between immutable structural/geometric facts and a particular published semantic state or concrete output. Today `SurfacePlanScene` already aggregates plan, hit, paint link, semantics, and coordinates. Putting those same values under another Arc would not achieve this design.

The largest avoidable authority coupling is concrete: `DropTargetRecord.destination` contains `DockTarget`, whose target contains a complete `NodeFingerprint`. A selection changes that fingerprint even when every target rectangle and locator is unchanged. Remove this field. Geometric target facts carry a locator; a binding resolves it against its own immutable source snapshot when an action actually occurs. The resolution produces one checked command handle, not a refreshed proof in every target record.

## Facts established from the current tree

* `scene/lifecycle.rs:119` already co-owns an Arc plan, hit manifest, paint link, semantic manifest, output ticket, and coordinate capture. `install_ready` still compiles/links hit and semantic views when the plan changes (`:1264`).
* `semantic_manifest.rs:19` contains only the output ticket plus receiver IDs. Its output-neutral receiver set is directly shareable. `presentation_hit.rs` already separates its output ticket from Arc-owned hit geometry; the missing step is allowing a new semantic binding without rebuilding the record source that feeds it.
* `scene/records/tab.rs:41` stores selection in `PaneRecord`, and `:99` stores a selected boolean in every `TabRecord`. These are presentation state, not rectangle facts.
* `drop_target.rs:395` stores `DropDestination`, and `DropDestination::Topology` stores a `DockTarget`. `drop_resolver.rs:855`, `engine/presented_input.rs:83`, target-catalog validation, and compiler tests consume that embedded authority.
* Requirement construction measures the minimum for every item in each leaf (`scene_compiler/requirements.rs:151`); it does not merely measure the selected item. Future-drop projection depends on these retained values.
* Effective tab-bar policy intersects workspace, surface, and target rules (`policy.rs:1434`). Selecting a different item can change that rule even without a policy revision.
* Future layout checks the candidate location first and the original item location second (`scene_compiler/future_layout.rs:390`, `:473`). Measures are contextual: storing only `ItemId -> value` would be incorrect.
* Touch protection uses tab-body, close, group-drag, strip-control, title/close regions **and** complete bar/title/anchor bounds (`presentation_hit/touch.rs:787`). Equal bar bounds alone do not prove equal touch dependencies.
* `NativeFirstLiveProjection` is a prospective compiler input, not a topology owner (`engine/native_projection.rs:25`). Publication requires a fresh check of saga, binding, staging resource, ready basis, and the actual first-live presented output.
* `transaction.rs:97` and `engine/reducer.rs:938` infer changes through whole-value comparisons. These may be replaced by owner-issued immutable state identities and exact final publication headers.

## Module and Seam placement

The external Module is the existing docking engine/runtime, made deeper. Its Interface lets an Adapter freeze a frame, answer measurements, consume complete output, settle actual presentation, and submit input from a settled output. The Implementation owns workspace edits, structural compilation, row association, retained facts, output bindings, and rollback together.

Workspace, layout, compiler, and retained data are in-process dependencies. Keep their seams internal; do not introduce trait ports between them. There are already two real Adapters at the renderer seam: egui and the renderer-free owning recorder. Both consume the same current bound view. Native hosting is a real lifecycle Adapter, retaining the existing mailbox/coordinator roles; it does not become an independent source of docking geometry authority.

Depth means one `prepare` call knows how to reuse facts and one `encode` call exposes a complete bound program. Locality means a selection edit, its effective-policy calculation, changed metrics, and its leaf's output facts are reconciled in this one Module. Callers do not supply dirty flags or orchestrate caches.

Do not create a plugin registry, generalized dependency graph, content-addressed cache, compiler proof language, or network abstraction. The following fact classes have a small explicit dependency list.

## Concrete ownership and data layout

### 1. Durable state and compiled structure

Split the current `Node::Tabs { items, selected }` representation. Structure and current state must have different owners:

```rust
struct RootTopology {
    key: TopologyKey,
    nodes: Box<[NodeSchema]>,
    children: Box<[NodeRow]>,
    items: Box<[ItemId]>,
    preorder: Box<[NodeRow]>,
    postorder: Box<[NodeRow]>,
    parents: Box<[Option<NodeRow>]>,
    lookup: BTreeMap<NodeId, NodeRow>,
    item_locations: BTreeMap<ItemId, ItemLocation>,
}

enum NodeSchema {
    Tabs { id: NodeId, items: Range<u32>, leaf: LeafRow },
    Split { id: NodeId, axis: Axis, children: Range<u32>, split: SplitRow },
}

struct RootSnapshot {
    state: RootStateKey,
    topology: Arc<RootTopology>,
    tabs: SharedPages<TabState>,
    split_weights: SharedPages<SplitWeights>,
    presentation: RootPlacement,
}

struct TabState {
    selected: Option<ItemId>,
    mru: Arc<[ItemId]>,
}
```

`RootStateKey` is owner-issued `(engine domain, workspace epoch, nonreused serial)`. It changes after a real final semantic edit, including selected/MRU, ownership, central designation, item order, or split weights. `TopologyKey` only changes after structural edits affecting its rows. A selection changes one `TabState` and the root state identity, not its topology. Split weights are layout inputs, so changing them need not rebuild parent/child tables.

`SharedPages<T>` is one private storage utility, not an exposed model. Use fixed 32-way Arc directories and 64-entry leaf pages; typed u32 row addresses need at most a handful of directory levels. An update copies only its directory path and one page. `TabState` and leaf facts contain Arc-owned variable arrays, so copying a page does not clone 64 MRU/tab arrays. The same utility holds leaf state/fact slots. It must support only indexed read, checked replace, iteration, and append/rebuild for structural edits. No hashing, observers, automatic invalidation, or dependency tracking.

This explicit small storage choice avoids the trap of `Arc<BTreeMap<NodeId, TabState>>`: `Arc::make_mut` would clone every state on the first local edit. An `Arc<[Arc<LeafFacts>]>` directory would similarly copy the entire surface's pointer array for every selection. Do not accidentally advertise either as bounded-local work. A simpler shallow directory is acceptable for the first measured implementation if its copied volume is reported, but it is not the final layout described here.

The published workspace contains shared root/surface slots plus stable ID directories. It exposes a `NodeView` that combines `NodeSchema` and `TabState`. Serialization/export materializes complete values when explicitly requested. Ordinary frame publication does not materialize the old public `Node` shape. Structural edits can rebuild the affected root's tables in their entirety; no general incremental structural compiler is necessary.

### 2. Measurement schema and answers

Store measurement questions in immutable **per-leaf schema blocks**, with separate surface bounds/popup questions. Each question is a typed row. The root schema maps its nodes/items directly to their question rows. Each leaf block retains all item minima and policy-required intrinsic/strip questions. A surface's frozen question iterator chains the retained blocks in deterministic root/leaf/role order; it does not concatenate a fresh surface inventory.

```rust
struct LeafQuestions {
    key: QuestionSchemaKey,
    minima: Box<[PaneMinimumRequest]>,
    intrinsics: Box<[TabIntrinsicRequest]>,
    strip: Option<TabStripRequest>,
}

struct FrozenMeasureSet {
    binding: MeasureBindingId,
    origin: ProjectionOrigin,
    sources: Arc<SourceSet>,
    surface_questions: SurfaceQuestions,
    leaf_questions: SharedPages<Arc<LeafQuestions>>,
}

struct MeasuredSurface {
    questions: Arc<FrozenMeasureSet>,
    bounds: MeasuredBounds,
    leaves: SharedPages<Arc<LeafMeasures>>,
}

struct LeafMeasures {
    minima: Box<[Measurement<LogicalSize>]>,
    intrinsics: Box<[Measurement<TabIntrinsic>]>,
    strip: Option<Measurement<TabStripMetrics>>,
}
```

The request-to-answer relation is established by construction. The synchronous measurement loop holds the current typed slot and writes exactly its one answer. A completed `MeasuredSurface` cannot be created with a missing or mismatched row. Full Q still runs every successful frame. Compare new answers with retained answers while collecting them; reuse a leaf answer block when all its actual values match. This unavoidable O(Q) callback/value work is separate from structural derivation. Hidden item-minimum changes replace retained measurement facts for future projection even when the selected leaf minimum and present geometry stay unchanged.

Delete the keyed `SurfaceMeasurements` public construction protocol rather than maintain an old/new pair. A detached Adapter can retain a `MeasureCollector` created from the frozen set, accept only opaque request handles from it, and `finish()` once. Collector checks owner/schema binding, duplicate/missing answers, answer type, finite values, and unavailable states. Numeric row indices alone are never accepted from external code. There is no conversion back to three BTreeMaps before compilation.

Unavailable answers are stored without changing the complete question inventory. Successful batches answer every Q. Invalid data may abort; exact missing-before-unexpected/variant error ordering is deliberately not preserved. Result categories remain meaningful and deterministic. Both egui and headless callers migrate directly.

### 3. Immutable geometric and receiver facts

Separate the following retained blocks:

| Block | Layout and reuse unit | Actual dependencies |
| --- | --- | --- |
| `RootTopology` | One structural root | Nodes, order, parent/child ownership, central structure |
| `LeafQuestions` | One tabs leaf | Item roster, effective bar policy and request-visible context |
| `RootLayout` | One root | Topology, split weights, root bounds, effective selected leaf minima, layout configuration |
| `LeafGeometry` | One tabs leaf | Allocated rectangle, effective bar policy, tab metrics, actual scroll/reveal result, close chrome |
| `RootChrome` | One root | Layout, split/junction inputs, root containment and chrome configuration |
| `LayerFacts` | One surface composition | Contained order/rectangles, occlusions, popup and anchor placement |
| `PointerFacts` | Leaf/root/composition blocks | Actual local receiver regions, lanes, precedence, clipping and layer inputs |
| `SemanticFacts` | Leaf/root/popup blocks | Exposed semantic actions; independent of pointer clipping |
| `TouchFacts` | Initially one surface | Expanded resize allocations, all protected regions/chrome, occlusions, configuration |
| `ProgramSchema` | Structural root + local overlays | Program order and typed row associations, including optional role rows |

`RootLayout` is retained if actual effective selected minima, bounds, weights and layout configuration match. A changed selected minimum can affect siblings and ancestors; initially run the full solver for that root and rebuild the resulting affected geometric blocks. Other roots and surfaces remain shared. Do not build an incremental numeric solver first.

`LeafGeometry` contains rectangle and chrome data, not selected booleans or selected content IDs. `PaneView::selected()` reads its binding's `TabState`; `TabView::selected()` compares its row's item with that same state. Selection-dependent reveal is still an explicit leaf-geometry input. Recalculate reveal/scroll for the changed leaf; if the resulting geometry matches, retain its geometry identity. This compares the changed leaf, not every surface record.

Pure target facts contain `TargetLocator` plus geometric availability/region/visual/layer facts. Locators name center, tab gap, inner edge, outer edge, and surface background using typed root/node/edge/gap rows. They contain no `DockTarget`, `ItemSource`, `NodeFingerprint`, workspace version, selected-item rule, or output ticket. Any policy-derived availability that actually changes is a local target-status/chrome fact, not an authority proof.

All program operations carry private role rows directly: pane body, tab body/close, bar scroll/group drag/control, splitter/junction, contained roles, and drop target. Each optional role preserves absent/passive/disabled behavior. egui's bound view constructs the current descriptor from those rows; ordinary painting performs no stable-ID binary search. Stable IDs remain available for widget keys, events, export, and diagnostics.

Do not reflatten retained blocks into new contiguous public arrays. Public methods return iterators and bound views. `visit_program` walks structural order and resolves per-leaf local blocks without assembling or relinking the whole tape. Global hit comparison uses the existing explicit total stack order; local sorted blocks can be queried and compared without recreating one concatenated sorted table. A structural edit may rebuild its root's schedule and directories. Popup/contained composition has its own retained block.

Touch fragments cannot retain flattened global receiver row numbers, because replacing a leaf might shift all later rows. Store `ReceiverAddress { owner_block, local_row }`; touch ownership points to immutable splitter/root blocks. Keep the current full protection algorithm initially. Reuse touch facts only when all actual dependency blocks remain identical or an explicit comparison of **changed** protection/resize/occlusion inputs proves equality. Tab scrolling may change protected regions even with equal bar bounds. If it does, rebuild touch once for the surface; this is a documented conservative fallback, not a silent assumption. Future localized touch recomputation is optional later work.

### 4. Current binding and output lifetime

```rust
struct SurfaceBinding {
    id: OutputBindingId,
    surface: SurfaceId,
    facts: Arc<SurfaceFacts>,
    measurements: Arc<MeasuredSurface>,
    sources: Arc<SourceSet>,
    coordinates: SurfaceCoordinateCapture,
    origin: ProjectionOrigin,
}

enum ProjectionOrigin {
    Live,
    NativeFirstLive(NativeProjectionProvenance),
    Counterfactual(CandidateProvenance),
}

struct SourceSet {
    roots: SharedPages<Arc<RootSnapshot>>,
    placement: Arc<SurfacePlacementSnapshot>,
    policy: Arc<DockPolicySnapshot>,
    config: Arc<DockPresentationConfig>,
}

struct SurfaceSlot {
    candidate: Option<Arc<SurfaceBinding>>,
    paint_fallback: Option<Arc<SurfaceBinding>>,
    interaction: Option<PresentedEmission>,
    native_projection: Option<NativeProjectionProvenance>,
}
```

`SurfaceFacts` is a small root/leaf/composition directory into retained blocks. New sources/measurement provenance/output identity do not require a new copy of those blocks. A binding retains its own measured answers even when the same geometry was produced from different answers. This is necessary for future projection. Never use a geometry owner as a substitute for the measurement/source owner.

There are three independent identities: retained fact identity, output binding identity, and concrete emission identity. A binding may encode multiple emissions. Every actual pass/repeat pass receives a new emission key attached to its exact host stream/endpoint/coordinate generation. Sharing facts or even a binding does not confirm an emission. Only complete encoding and final Presented settlement creates `PresentedEmission`; Dropped/cancelled/aborted work cannot authorize input.

The existing output ledger remains the owner of final presentation authority. Candidate, confirmed paint fallback, in-flight emissions, and stream-retirement records retain Arcs to exact bindings. They do not mutate old bindings to point at new source snapshots. After an output becomes ineligible, it may remain paintable but cannot produce an accepted semantic command. Retirement, coordinate rebinding and workspace replacement revoke authority independently of fact storage reuse.

Issue binding/emission/root-state serials from the existing nonrollbackable issuer. Aborted frames consume serials rather than allowing ABA. All mutable candidate state, sources, leaf updates and slot changes publish atomically at frame commit. A dropped frame releases its staged Arcs without touching the original state. Initial/final publication headers, not OR-ed intermediate dirty bits, decide whether final state changed. The staging owner keeps original values for touched slots. At commit it compares final versus original values only for those slots, including MRU, and restores the original immutable references/state key when the complete touched scope is unchanged. A sequence that selects A, then B, then A may still change MRU; compare the actual final state rather than selected item alone. Unused allocated serials remain consumed. Structural batches can conservatively compare/rebuild their affected roots; local selection never needs a full-workspace equality scan to normalize a cancelled edit.

## Interface sketch

Names below are illustrative Rust contracts. `MeasureCollector`, binding keys, and row constructors have private fields. Intermediate fact types stay private.

```rust
impl DockspaceHostFrame<'_> {
    pub fn measure_surface_with(
        &mut self,
        surface: SurfaceId,
        resolve: impl FnMut(SurfaceMeasureRequest<'_>) -> SurfaceMeasureAnswer,
    ) -> Result<(), FrameError>;

    pub fn prepare_surface(
        &mut self,
        surface: SurfaceId,
    ) -> Result<PreparedSurface<'_>, FrameError>;

    pub fn commit(self) -> Result<FrameCommit, FrameError>;
}

impl PreparedSurface<'_> {
    pub fn encode<S: SurfaceSink>(
        &self,
        sink: &mut S,
    ) -> Result<EncodedSurface, EncodeError<S::Error>>;
}

impl SurfaceView<'_> {
    pub fn panes(&self) -> impl Iterator<Item = PaneView<'_>>;
    pub fn tabs(&self) -> impl Iterator<Item = TabView<'_>>;
    pub fn targets(&self) -> impl Iterator<Item = TargetView<'_>>;
    pub fn visit_program<V: SurfaceVisitor>(&self, visitor: &mut V)
        -> Result<(), V::Error>;
}

impl TargetView<'_> {
    pub fn id(&self) -> DropTargetId;
    pub fn region(&self) -> HitRegion;
    pub fn locator(&self) -> TargetLocator;
}

impl DockEngine {
    pub fn settle(&mut self, observation: PresentationObservation)
        -> Result<(), PresentationError>;
    pub fn submit(&mut self, input: PresentedInput)
        -> Result<InputOutcome, InputError>;
}
```

`SurfaceSink`/`SurfaceVisitor` already has real egui/headless uses; adapt the existing interfaces instead of creating a second rendering protocol. A `PaneView` or `TargetView` is bound to one output. Receiver descriptors are obtained through that view; the Interface does not accept `other_output.receiver(view_from_first_output)`. If a long-lived handle is necessary, it contains binding ID + block identity + typed row and is validated. A shared Rust lifetime alone cannot identify its owner.

Typical egui/headless use is still one complete measurement and one complete output:

```rust
let mut frame = runtime.begin_frame(input)?;
let surfaces: Vec<_> = frame.surface_ids().collect();
for surface in surfaces {
    frame.measure_surface_with(surface, |request| adapter.measure(request))?;
    let prepared = frame.prepare_surface(surface)?;
    let encoded = prepared.encode(adapter.sink(surface))?;
    adapter.retain_emission(encoded);
}
let committed = frame.commit()?;
adapter.publish(committed);
```

Final observations arrive through the existing settlement path after the Adapter knows whether each exact encoded emission was Presented or Dropped. `EncodedSurface` cannot itself claim to be presented. The egui callback/final-hook/output audit remains complete. The recorder still owns and consumes the entire program. This design changes construction/association, not what gets rendered.

## Replacing embedded command proofs

Replace `NodeFingerprint`-carrying public sources/targets with owner-minted snapshot handles:

```rust
struct SourceHandle {
    root: RootStateKey,
    address: SourceAddress,
}

struct TargetHandle {
    root: RootStateKey,
    placement: PlacementStateKey,
    locator: TargetLocator,
}

impl BoundSources<'_> {
    fn resolve_target(&self, locator: TargetLocator) -> Result<TargetHandle, CommandError>;
    fn resolve_source(&self, locator: SourceLocator) -> Result<SourceHandle, CommandError>;
}
```

Resolving checks row ownership, existence, central/placement facts and the selected-item-derived target rule against **this binding's** snapshots. The command evaluator then checks engine domain/epoch and current root/placement state keys, current policy and native barriers. It never silently upgrades an old handle to the newest source state. Item membership and row type are read directly from the frozen topology. Center/gap/edge locators use the same checked resolver.

The input path first validates exact presented emission, stream, endpoint, coordinates, and current authority eligibility, then interprets its receiver/target locator and produces a checked command. A naked locator cannot authorize a command. A direct non-output command captures its handles from the current engine read view and follows ordinary checked execution. Full-root/subtree/native reservation handles retain the required root snapshot identity instead of a full value fingerprint. Invalidation is deliberately conservative at root scope for source validity; this does not force root geometry reconstruction.

This changes ABA behavior: a handle from root state A is stale after A -> B -> A even if the values are equal. Accepting a separately reconstructed equal workspace's old proof is no longer promised. Stable domain IDs remain serializable, but live capabilities are recaptured after load. Preserve native source-freeze meaning using those exact immutable root keys: selecting a frozen source root changes the key and blocks/invalidates the pending operation according to lifecycle rules. Do not weaken this check merely because geometry was retained.

Different roots on one surface have distinct keys, so selection in root A does not invalidate a handle for root B unless shared placement/policy authority changed. A surface background target uses `PlacementStateKey`, preventing a formerly rootless target from accepting a second root. Root rehome changes placement and root state identity. Workspace replacement changes epoch and rejects all earlier handles.

`DropDestination::Topology(DockTarget)` disappears from records. Replace `DropTargetRecord::destination() -> &DropDestination` with a locator view and a separate **bound command-resolution** method at the engine/input seam. The geometric validator checks that locator and target ID agree and that rectangles/coverage are valid. It does not recapture full command proofs for every record. The command evaluator validates the one actual command.

## Local selection: concrete update algorithm

1. Resolve the selected item once from the current or valid presented source. Check exact membership, policy and native source barriers. Build a new `TabState` with the selected item first and the rest of MRU preserving order. Replace one page path in the staged root; retain topology and all unchanged state pages.
2. Mint the new root state key and replace that root in the staged source set. This rebinds future target/source resolution without touching any target fact. Other root source states remain the same.
3. Compute the changed leaf's new **effective** bar policy from the newly selected item. If it matches, retain the leaf question schema. If it changes, replace only its questions and policy/chrome facts; question iteration still returns the complete current Q. No all-surface requirement draft is built.
4. Run all current measurement callbacks. Compare/store values in their owned typed leaf rows. Retain unchanged answer blocks. Accumulate a bounded list of actually changed leaf inputs while collecting the fresh answers; the Adapter does not assert them.
5. Compute selected content + bar outer minimum for the changed leaf. If all active layout dependencies match, retain `RootLayout` without numerical projection. A changed hidden-item minimum updates measurement retention only. If an effective minimum changes, solve the affected root conservatively, retaining other roots.
6. Apply reveal/scroll for the changed bar using the new selected item and current intrinsic/strip metrics. Rebuild only that leaf's geometry/pointer/semantic facts when its actual inputs change. With stable geometry/action availability, retain all three. Build views that read current selected/MRU through the binding; never rewrite selected flags in all tab records.
7. Retain root chrome, target locator geometry, and program schema whenever their explicit dependencies match. Reuse touch only with exact protection/resize/occlusion dependencies; otherwise do the one conservative surface touch rebuild. No re-emission of all regions is needed just to discover unchanged leaf facts.
8. Create the new small output binding with current sources, complete measurement snapshot and exact coordinate/projection provenance. Encode every pane/tab/content/operation and settle every concrete output normally. The previously presented binding is retained separately; old output receives no current-source authority by sharing geometry.

Fresh measurement answers can change any leaf, not only the leaf selected by the command. Process every changed leaf collected in step 4 through the same explicit minimum/chrome dependencies. If another leaf's selected minimum or surface bounds changed, solve that affected root even though the domain edit was a local Select. The unchanged-facts claim below is conditional on the actual complete answer set, not merely a selection mutation label.

On the common stable-metrics/no-reveal-change case, structural requirement visits, target-proof cloning, full compiler/finalizer calls, hit compilation, semantic compilation, and paint linking all become zero for the selection update. Full Q callbacks and full paint execution remain proportional to the actual workload. A root-level source identity refresh costs no traversal.

This is an explicit update switch driven by a small owner-issued edit result (`Selection`, `Weights`, `Topology`, `Placement`, `Policy`, `Transient`). It is not an open dependency inference system. Transactions can conservatively rebuild all affected roots when their edit combination is unsupported. The output behavior, not a particular fast-path eligibility mask, is the public promise.

## Multi-surface, native and future-drop semantics

**Independent surfaces.** Per-surface slots/coordinates/output bindings remain separate. The frame can carry a new binding for A and a retained binding for B without deriving B's schema or facts. One publication version is useful for user events; it is not a geometry cache key or a reason to invalidate every root. A native retirement revokes only the relevant host/endpoint, with existing ledger consequences.

**Native first-live.** Freeze a prospective `SourceSet` with its native provenance: saga, source root-state keys, binding, staging resource, ready basis, config/policy and coordinate authority. Compile using the same immutable topology/questions/measures/facts representations. Its output can be encoded and Presented, but cannot publish durable ownership or issue ordinary live semantic commands. First-live commit revalidates the exact provenance and source barriers, executes the checked ownership transaction, and publishes a new live source/binding. Fact blocks may survive that transition if actual inputs match; projection provenance never disappears just because their geometry matches.

**Future drop.** `Counterfactual` retains original source snapshots and uses a staged candidate root snapshot. The projection Interface accepts explicit measurement origins, not an ambient map from the newest workspace. For a candidate item/context, it first checks the exact candidate measurement address, then obtains the original item location through the original topology's `item_locations` index and reads the source measurement slot. Source/target contexts include surface, root, leaf, item, role, config and request-visible measurement context. Preserve conflicting-answer detection where two retained outputs claim different values for the same contextual origin. Do not collapse tab widths/minima by ItemId.

Counterfactual results have no live output or command authority. The caller receives candidate geometry/preview facts. If it becomes a first-live native output, it goes through the native origin described above; a boolean `is_native` or a live source ticket is insufficient.

**Old outputs in flight.** Output E1 retains binding B1 and its source/measurement provenance even while B2 shares the same rectangles. A late E1 Presented observation is evaluated using the ledger's exact current retention/authority rules; it cannot confirm B2. A B1 receiver cannot be rebound to B2 or retargeted through B2's source set. A retained paint fallback may be encoded again with an exact emission identity but remains source-stale for interaction where the lifecycle says so. Arc reachability controls memory; the ledger controls permission.

## Migration and deletion list

Implement replacements directly and delete superseded routes after each complete slice. Do not retain wrappers that reconstruct the old values each frame.

| Current source | Direct replacement / deletion |
| --- | --- |
| `graph.rs::Node::Tabs.selected`, `Workspace.tab_mru` and public borrowed node shape | Immutable topology + shared selected/MRU pages; expose `NodeView`; adapt builder/serde/export at explicit materialization points |
| `command.rs::NodeFingerprint`, proof-bearing `ItemSource`/`NodeSource`/`TabTarget`/edge sources | Snapshot-scoped source/target handles; remove full fingerprint capture/value equality from ordinary commands and native reservations |
| `workspace.rs::WorkspaceIndex` tied to full version/allocation | Topology-owned immutable index, plus source snapshots for current state/placement; remove rebuild caused solely by selection |
| `operation.rs`, `transaction.rs` local Select | Checked local state patch with atomic staged publication; delete full clone/canonicalization/item-multiset comparison for Select; keep structural edit validation |
| `engine/presentation_authority.rs::surface_semantic_snapshots`, root fingerprint capture, `workspace_index_source`, aggregate draft refresh on selection | Root snapshot identities and edit-domain switch; delete all-surface requirement derivation for nonstructural local edits |
| `scene_manifest.rs::SurfaceRequirements`, keyed `SurfaceMeasurements`, `AuthoritativeSurfaceMeasurements` exact-set round trips | Question blocks + request-bound collector + typed measured values; delete the internal map/set assembly and readback protocol |
| `scene_compiler/requirements.rs` whole-workspace requirement draft | Compile topology schema once; refresh questions only for changed policy/leaf blocks |
| `scene.rs::PresentationPlan` monolithic public Vec records and provenance | Retained fact directories + bound iterators; no compatibility flattening on prepare/paint |
| `scene/records/tab.rs` selected fields | Selected/MRU read from bound source state; retain geometry independently |
| `drop_target.rs::DropDestination::Topology`, `DropTargetRecord::destination/new` | Locator-only target facts and checked bound resolution; migrate constructors/tests/export consumers |
| `drop_resolver.rs`, `engine/presented_input.rs` record destinations | Resolve the chosen locator using its exact binding before creating one command |
| `scene/validation.rs`, `scene/validation/target_catalog.rs` proof agreement joins | Construction-time typed row/locator invariants plus real geometry/coverage checks; reference tests cover external malformed input |
| `scene/program.rs`, `scene/paint_program.rs` plan-wide link rebuilding | Structural program schema + local role rows established by fact builders; no per-paint semantic joins |
| `presentation_hit.rs` output-bearing aggregate compilation | Output-neutral chunked pointer/touch facts; output descriptors are views from binding |
| `semantic_manifest.rs` output + full sorted receiver list | Output-neutral semantic blocks with binding views; retain pointer/semantic differences |
| `scene/lifecycle.rs::SurfacePlanScene` and nested candidate/fallback duplication | Small exact binding references in surface slot; ledger retention keeps historical bindings |
| `engine/reducer.rs` whole scene comparison | Final binding/header identities including candidate/fallback, authority, confirmation, coordinates, native provenance and revision frontier |
| `scene/records/layout.rs` retained measurement maps | Contextual typed measurement snapshots retained independently of geometry |
| `scene_compiler/future_layout.rs` temporary minimum-map merge and source recapture | Origin-aware typed lookup via frozen source/candidate contexts; retain conflict/config checks |
| `engine/native_projection.rs`, native admission/reservation/first-live code | Explicit prospective source/binding provenance with exact root-state freeze; same lifecycle checks and no geometry-authority conflation |
| `runtime/measurement.rs`, `runtime/paint.rs`, `egui_dockspace/product_render/*`, host recorder/harness | Direct question/view/receiver-row consumption; adapt native callbacks and conformance fixtures to the new bound views |

Search and migrate every `destination()`, `fingerprint()`, direct `Node::Tabs` destructure, `pane_records()/tab_records()` borrowed-slice consumer, ticket getter and plan-wide equality consumer. Public views may keep familiar method names, but changing slices to iterators and proof-bearing records to locators is intentional. Tests that assert old missing-key priority or cross-owner value-proof acceptance should be deleted/replaced, not emulated.

## Shortest implementable slices

1. **Remove target authority from facts end to end.** Add root-state/placement keys and bound source resolution; replace record destinations with locators; migrate drop resolution, command capture, target validation and native source freeze. Keep the existing conservative compiler during this slice. This independently removes repeated embedded full-root proofs and establishes the indispensable seam. It is not yet the complete selection optimization.
2. **Separate binding from facts and selected state.** Move output/coordinate/native/source/measurement provenance into exact bindings. Make hit/semantic bodies output-neutral and expose bound views; remove selected fields from pane/tab geometry. Use source-state selection in render views and link ordinary role rows directly. Adopt header/identity publication checks. Keep full construction as a valid conservative producer.
3. **Complete the selection slice.** Split immutable topology/index from shared selection/MRU state, freeze per-leaf question blocks, and retain root/leaf fact blocks. Implement the explicit local selection algorithm above. Full Q runs and all program operations encode, but unchanged geometry selection does not call full-surface compiler/hit/semantic builders. This is the minimum useful performance deliverable, not an inventory-only checkpoint.
4. **Replace measurement storage.** Feed typed question rows and retained answer blocks all the way into ordinary compilation, native first-live and future projection. Delete maps/exact-set conversions. It can be combined with slice 3 where retaining dual answer representations would otherwise grow a compatibility path.
5. **Extend larger updates using the same representations.** Root-level minima/weights solves, selected-policy changes, scroll, contained composition and popup updates use their explicit dependency blocks. No second cache architecture is added. Structural edits can still rebuild affected roots. Delete old all-surface emit-and-compare reuse attempts once all consumers use fact ownership.

The order prevents building a new generation container that still embeds old fingerprint-bearing targets. A smaller root-only structure clone does not complete slice 3 for a 1,024-leaf single root. Avoid building a persistent graph for every structural operation before local selected state and fact sharing work.

## Counterexamples and checks through the Interface

| Scenario | Required result / bug exposed |
| --- | --- |
| Select one of two tabs with equal minima, stable effective policy and no reveal movement | New selected/MRU and current source; same geometry/hit/semantic owners; complete Q and complete paint; no full compiler/link/manifest walk |
| Select an offscreen tab | Reveal/scroll recomputed; correct clipping, controls, pointer receivers and touch dependencies; unrelated leaf blocks retained |
| Select item with larger/smaller actual minimum | Conservative affected-root solve and sibling rectangles match independent full compilation |
| Same policy revision, different selected-item target rule | Updated effective bar policy, questions, chrome and operability; no stale rule bound from a target record |
| Change only a hidden item's minimum | Current geometry may remain shared, but future-drop candidate uses the new complete measurement snapshot |
| Same ItemId measured in different source/destination contexts | Candidate-first/source-fallback lookup with correct context; no item-only cache substitution |
| Conflicting retained measurements for the same origin | Explicit rejection/unavailability; no arbitrary merge winner |
| Open/clipped popup row | Row remains semantic-reachable where appropriate, even without a pointer region |
| Changed tab regions but unchanged bar bounds | No accidental touch reuse unless full relevant protection facts match |
| Multiple contained occlusions | Preserve drop coverage and touch subtraction; retain known validator counterexamples |
| B1 and B2 share all geometry; only B1 was Presented | B2 cannot accept input; B1 does not become current by sharing geometry |
| Late Presented/Dropped for old stream/binding | Exact ledger disposition; no candidate/fallback/native state confused with newer output |
| Foreign view/row with same stable IDs and same Rust lifetime | Rejected through owner binding or made unexpressible by view Interface |
| Same root values restored after A -> B -> A | Old source handle stays stale; new documented identity semantics |
| Workspace replacement / native binding reattach | Old epoch/coordinate/endpoint authority rejected even if geometry and stable IDs match |
| Pending native source then local selection | Exact source-freeze/barrier semantics preserved; fact reuse cannot authorize transfer |
| Native first-live callback races source/config/binding change | Prospective output cannot publish source ownership; revalidation fails safely |
| Dropped prepared frame / failed second batch command | All workspace, MRU, sources, bindings and ledger-visible publication remain unchanged; allocation serials are not reused |
| An intermediate operation is later cancelled in the same frame | Final publication delta reflects initial versus final state, not sticky dirty bits |
| egui final hooks/repeat passes mutate output | Existing full output validation still decides settlement; same facts do not excuse incomplete output |

Use the same public frame/measure/encode/settle/submit Interface for correctness tests. The conservative full compiler may remain as a test-only reference builder for normalized geometry/receiver/program comparison; it is not a second production pipeline. Replace obsolete internal map/fingerprint-unit tests with these observable cases rather than stacking both suites indefinitely.

For the one architectural acceptance check, instrument fact construction counts/visited records and copied state volume. They establish that work disappeared; they do not replace semantic tests. Run the existing 16/128/1,024-pane complete headless and product workloads serially after implementation, with fresh all-Q measurement and owning recording/real content. Separate allocation accounting and CPU timing. Include current + fallback + in-flight + native prospective retained memory, not only latest scene memory. No speedup is claimed by this proposal.

## Why this is materially different from another generation container

A new outer generation Arc with the current `PresentationPlan`, output-bearing manifests, selected fields and full `DockTarget` fingerprints would still need replacement on every selection. Its installer would still emit records, reconstruct regions, sort semantic IDs and refresh target proofs. It changes a name and a clone path.

This design deletes specific work:

* Full target/source proof payloads disappear from geometric records; command binding happens once for the actual selected action.
* Selected/MRU state changes independently of topology and rectangle storage; rendering resolves selection through the exact binding.
* Complete measurement answers remain request-bound from collection through future projection, without rebuilding their relation in ordered maps.
* Retained leaf/root/composition blocks are selected from known actual dependencies before compilation; unchanged facts are not re-emitted to discover equality.
* Program roles address local receiver rows directly; output descriptors bind those rows to the current emission without semantic-ID joins.
* Source, measurement, coordinate, prospective-native and concrete output identities remain independent. Historical bindings survive safely while their immutable facts are shared.
* Publication compares the small final state the owner published, rather than recursively comparing immutable record contents.

That is the Depth gained at the engine Interface and the Locality gained by its maintainers. The retained fact store is useful precisely because binding no longer lives inside every fact.
