# Selection belongs to a current snapshot, geometry belongs to its inputs

Date: 2026-11-06. Design against repository HEAD `34ab23a`, runtime implementation `5484071`. This is a read-only design lane; no implementation, build, test, benchmark, or network work was performed. The proposed qualification target is a user requirement: at least 10% lower complete 1,024-pane press-plus-release click p50 in **each** of two independent serial comparison groups, with complete headless selection improving in the same direction. It is an acceptance gate, not a speed prediction.

## Decision

Make ordinary local selection the natural cheap operation of the session Module. Its Interface is `update.select(item)`, followed by the same complete measurement, painting, settlement, and atomic commit used by every update. The caller does not choose a fast method, capture a root proof, submit a measurement document, select a compiler, or authorize geometry reuse.

The Implementation separates three different lifetimes: stable topology and inventories, mutable selected/MRU state, and the particular output that may authorize future input. A bounded local selection patch updates the second and issues a new instance of the third. If actual geometry dependencies are unchanged, it retains the first without rebuilding it. All other cases use one complete rebuild Implementation producing the same representation. There is no second legacy compiler or external compatibility route.

This deliberately exploits the newly authorized breaking change. Runtime commands are bound to owner-issued snapshot identity. An independently reconstructed equal workspace is a different snapshot; undo/restore does not revive old command authority. Delete value-proof/ABA compatibility and incidental error ordering. Keep the real semantics of source freezes, native lifecycle, stale or unpresented output rejection, rollback, and complete rendering.

## Source facts that the replacement must change

- `operation.rs::WorkspaceSnapshot::prepare_transaction` already recognizes a single Select, but `clone_workspace_draft` still clones the whole `Workspace`; `transaction.rs::into_canonical_parts` discovers change by whole-workspace value equality.
- `runtime/host_frame.rs::select_item_current` is already a useful simple public entry point. Its downstream route, rather than the call spelling, is shallow: product actions capture current sources and pass through the generic command machinery. `engine.rs::reduce_workspace_command` then advances revision and rebuilds requirements.
- `engine/presentation_authority.rs::try_rebuild_presentation_requirements` captures surface semantic snapshots, builds the semantic index, derives a complete requirement draft, and reconciles strip state. `WorkspaceIndex` mixes topology membership with a current-version `NodeFingerprint`.
- `scene_compiler/requirements.rs::derive_root_requirements` requests minimums for every item. It evaluates bar policy through the selected item's target rule. Unchanged policy revision alone does not prove unchanged effective bar policy.
- `runtime/measurement.rs::measure_surface_with` supplies every key itself, assembles three keyed maps, and calls the detached contribution preparation path. The current source then validates membership and performs keyed compiler reads. Keeping callbacks does not require this serialization.
- `engine/surface_contribution.rs::prepare_surface_contribution` explicitly states that local tab-state edits invalidate Ready. A miss calls full `compile_surface_plan(...).finalize()`.
- `scene/lifecycle.rs::install_ready` then compiles hit/touch, paint linkage, and a separate semantic roster. `presentation_hit/geometry_reuse.rs` replays the entire region stream to discover equality. That replay is not selective reconstruction.
- `scene/records/tab.rs::PaneRecord` embeds selected item and `TabRecord` embeds selected boolean. This ties an otherwise unchanged geometry record to local semantic state. Drop targets also contain current proof-bearing commands. Both associations must move to the current snapshot binding.
- `scene/program.rs` already has typed record rows, and `SurfacePlanScene` already owns plan/hit/paint/semantic products. Another aggregate or generation name alone changes no work.
- `drop_resolver.rs::rebind_topology_target` already preserves the frozen target locator while recapturing its live command facts, and checks `presentation_structure_eq` before assessment/preflight. `scene/validation.rs::target_reference_is_current` nevertheless requires each stored target fingerprint to be fully current. Replace that conflicting representation requirement; do not introduce a second rebinding layer.
- `engine/pointer_transaction.rs` performs a tab Select during source press, then captures its post-selection source and arms the drag. An ordinary click can therefore end the press frame in an Armed state even though no drag projection exists. A test of `has_active_gesture()` alone would exclude an important part of the complete product workload.

## The Interface and its two real Adapters

Illustrative types below express ownership and stages; they do not require retaining current public names. Existing engine-owned stages can be renamed in place.

```rust
pub struct DockSession { /* owns the published snapshot */ }
pub struct Update<'session> { /* unpublished candidate and journals */ }
pub struct SnapshotId { /* session identity and monotonic serial */ }
pub struct SurfaceFrame<'update> { /* current binding plus sealed facts */ }
pub struct PaintReceipt { /* minted by complete program execution */ }
pub struct PresentedEmission { /* minted by actual settlement */ }

impl DockSession {
    pub fn update(&mut self, input: HostInput) -> Result<Update<'_>, Error>;
}

impl Update<'_> {
    pub fn select(&mut self, item: ItemId) -> Result<SelectOutcome, Error>;
    pub fn edit(&mut self, request: EditRequest) -> Result<EditOutcome, Error>;
    pub fn surface(
        &mut self,
        surface: SurfaceId,
        measure: impl FnMut(MeasureRequest<'_>) -> MeasureAnswer,
    ) -> Result<SurfaceFrame<'_>, Error>;
    pub fn presented(
        &mut self,
        receipt: PaintReceipt,
        presentation: PresentedEmission,
    ) -> Result<(), Error>;
    pub fn commit(self) -> Result<UpdateReport, Error>;
}

impl SurfaceFrame<'_> {
    pub fn execute<E>(
        self,
        sink: impl FnMut(PaintStep<'_>) -> Result<(), E>,
    ) -> Result<PaintReceipt, PaintError<E>>;
}
```

`HostInput` and `EditRequest` use semantic requests plus an optional expected `SnapshotId` for detached application intent. The local `select` resolves the item in its own candidate and needs no expected-version argument. A presented click still enters through the presented receiver/input machinery; it does **not** become permission to call `select` on an arbitrary stale tab. The reducer converts an admitted click to the same owner-private selection patch.

At the measurement/paint/settlement Seam there are two real Adapters:

1. The egui Adapter answers the complete measurement questions, registers all native widgets/child UIs, executes every paint step, and supplies the completed-pass settlement after final hooks. Preserve the existing output-proof and previous-run native observation mechanisms initially. Repeat passes rebind emission identities; only an accepted terminal pass can publish presented authority. Its `run_single_surface` driver still owns the entire run.
2. The headless Adapter answers the same questions, executes the complete owning recording sink, and supplies its actual recording settlement. It is the second production-quality Adapter, not a no-op test double. It retains normalized records, program order, callback counts, and output receipts.

Do not invent a universal framework run trait to make those two drivers look identical. They vary at the existing measurement, sink, and settlement Seam; their outer run lifecycles can differ. The workspace, index, selection storage, compiler, and authority ledger are in-process dependencies inside one Module, not independent untrusted providers. Their relations are established by construction.

The headless usage is approximately:

```rust
let mut update = session.update(input)?;
update.select(next_item)?;
let surface = update.surface(main_surface, |q| host.measure(q))?;
let receipt = surface.execute(|step| host.record(step))?;
let presented = host.settle(&receipt)?;
update.presented(receipt, presented)?;
let report = update.commit()?;
```

The ordinary egui application continues calling `dockspace.run_single_surface(...)`; its Adapter performs these stages internally. No caller-level reuse flags or special selection protocol exist. `SurfaceFrame` cannot outlive its update; suspend/resume may keep owning the same candidate as today. A callback may fail after emitting output; dropping the update still rolls back docking state and does not authorize that output. General application callback side effects retain their existing practical limits.

Depth comes from this Interface hiding preparation, invalidation, canonical state, complete-Q assembly, linked rows, and publication. Locality comes from making the selection owner the only producer of its invalidation fact; callers and Adapters neither infer nor prove it.

## Representation: keep the existing arena, split its actual dependencies

Use the existing generational `NodeId` arena for topology. Do not introduce a new allocator, dependency graph, Merkle tree, or incremental compiler framework.

```rust
struct Snapshot {
    id: SnapshotId,
    topology: Arc<Topology>,
    selection: SelectionPages,
    root_semantics: RootSemanticStamps,
}

struct Topology {
    nodes: SlotMap<NodeId, NodeTopology>,
    roots: RootRoster,
    surfaces: SurfaceRoster,
    index: TopologyIndex,
}

enum NodeTopology {
    Tabs { items: Arc<[ItemId]>, selection_slot: SelectionSlot },
    Split { axis: Axis, children: Arc<[NodeId]>, weights: Arc<[SplitWeight]> },
}

struct StackSelection {
    selected: Option<ItemId>,
    mru: Arc<[ItemId]>,
}

struct SurfaceFacts {
    structure: Arc<SurfaceStructure>,
    geometry: Arc<SurfaceGeometry>,
    queries: Arc<SurfaceQueries>,
}

struct CurrentSurface {
    facts: Arc<SurfaceFacts>,
    snapshot: Snapshot,
    measurements: Arc<MeasuredRows>,
    strip_state: StripStateSnapshot,
    output: OutputBinding,
}
```

`SurfaceStructure` stores topology-linked leaf/item slots, complete request directory, root/program order, static close capabilities, and the mapping from each compiler leaf to its measurement rows. `SurfaceGeometry` stores selection-independent pane/tab/bar/splitter/drop geometry and executable row associations. `SurfaceQueries` stores pointer geometry, touch fragments and semantic membership independently; semantic membership must not be inferred from pointer clipping. `OutputBinding` is small: candidate/coordinate identity and current output/emission authority. Every descriptor combines a row with that binding at use, rather than storing the binding in every immutable row.

`SelectionPages` is a deliberately small paged copy-on-write table, for example 64 stack states per page with a shallow page directory. Copying an update shares all topology and inventory storage; a Select copies one page of `Arc`/small entries and the changed stack's exact MRU permutation. The directory copy is O(number of pages), explicitly **not** asymptotically constant time. At 1,024 leaves it is 16 page references, not all nodes/items/rosters. Use this bounded mechanism before contemplating a general persistent collection. Page count and bytes are measurable implementation costs.

Selected state has exactly one source of truth: remove `selected` from durable topology storage and remove its duplicate MRU map. Expose a borrowed `NodeView::Tabs { items, selected }` for consumers instead of preserving `Workspace::node() -> &Node`. Builders/serialization read or materialize this view at their actual seam. They are not hot-path workspace materialization steps. General edits may create a complete mutable draft, normalize/validate it, and freeze the same `Topology` plus selection representation again.

Likewise, `PaneView::selected()` and `TabView::selected()` read the current selection slot. The immutable geometry row does not change solely to flip a boolean. Targets store stable root/node/edge roles plus their immutable structural owner, not captured full-root proof objects; policy and command authority are attached from `CurrentSurface` when consumed. Refactor the existing `drop_resolver.rs::rebind_topology_target` responsibility into `bind_target(locator, structural_owner, current_snapshot)`: this is the existing rebinding function with a better input representation, not a new layer before it. Preserve its exact root/node/surface/edge/tab-gap/fraction checks. The structural owner proves what its former `presentation_structure_eq` comparison proved; it does not claim selected/MRU or drop policy are unchanged. Replace the validator's exact-current-fingerprint requirement with actual locator/structural-owner validity. The existing `DropEligibilityContext::check_target` then evaluates the newly bound destination through `DropCommandEligibility::check_target_indexed` and actual drop policy, as it already does. Public semantic IDs remain useful names and widget IDs, but ordinary paint execution consumes linked row handles directly. No per-step stable-ID join is required.

## Exact local invalidation contract

This first implementation handles one successful Select in an otherwise ordinary main-root surface. The current product case is one tiled surface with two tabs per leaf. Eligibility is explicit and bounded, not inferred by comparing whole workspaces.

### Before collecting measurements

1. Resolve the item through `TopologyIndex`; ensure current canonical membership and effective selection permission. Update selected and the exact MRU permutation, issue a new snapshot identity and the changed root's semantic stamp, record focus/events, and record `LocalSelection { stack, old, new }`. A selected no-op obeys the same established MRU/focus semantics and records no fake domain change.
2. Initial eligibility requires one affected stack, unchanged topology/config/policy/coordinates, the same main-root ownership and ordering, no popup, no contained arrangement on this surface, no projected drag/transform/resize preview, and no pending create/rehome/recovery affecting it. A source press that arms an ordinary main-root tab without producing a projection may retain geometry; it is not excluded merely because the interaction status is Armed. Preserve its capture/continuation and bind its armed source to the post-selection snapshot, matching the existing source-press ordering. Unsupported batches or a second incompatible mutation mark `Rebuild` permanently for this update. Ordinary hover facts, a nonprojecting tab press/arm, and a completed click are allowed; disqualifying every press/release input would miss the product gate. A drag that crosses activation threshold or changes projected geometry uses full rebuilding.
3. Recompute the changed leaf's **effective** bar policy using its new selected-item target rule, via the same policy function used by full rebuilding. Compare the full policy value, not just its revision. A different effective policy uses the full rebuild; this also derives the proper new measurement inventory before callbacks. Reuse other leaves' policy/capability inventory only while the frozen policy and topology owner are unchanged.

### While collecting complete current Q

4. Ask every current bounds, optional popup, pane-minimum, intrinsic, and strip question. In particular, minima for **every item** remain present for future drop projection. Fill typed value arrays tied directly to the immutable request directory. Validate answer variant, finite/positive geometry and unavailable status. There is no re-created keyed submission or exact-set proof for this owner-issued loop. Callback failures still fail the update; unavailable results remain explicit. Historical incidental error priority need not survive.
5. Compare each fresh answer to the retained answer at its known row while filling the array. This is an O(Q) value read accompanying required callbacks, not a second full-set or scene traversal. The initial local path requires every answer unchanged. Any changed value, including unrelated title widths or a non-selected pane minimum, uses the complete rebuild. This is conservative and easy to verify; finer invalidation is not needed for the qualification attempt.
6. Compare old versus new selected **actual effective outer minimum**, using the same `pane_outer_minimum` calculation as full rebuilding: componentwise max with configured pane minimum, plus bar height only when effective visibility is Visible. Equal raw or topology values are insufficient. Distinct effective minima use the full rebuild even though all per-item answers may be unchanged.

### Before retaining geometry

7. Run the changed strip's selection resolution against retained width/chrome inputs and **current** strip state. Apply clamp/reveal with the actual selected range, max scroll, and viewport. It may read O(tabs in that bar), never all bars. Record the new resolved selection. If its actual scroll offset or any resulting visible/operable tab/control membership changes, use the full rebuild. There is no attempt in the shortest path to patch the global hit arena for a scrolling bar.
8. When these checks pass, selection changes no layout rectangle, chrome rectangle, hit region, protected-chrome rectangle, touch fragment, semantic membership, splitter constraint, drop geometry or program order. Keep the exact same `SurfaceFacts`; replace only current snapshot/selection, measured-row owner, changed strip's resolved selection, and output binding. Complete paint still visits all records and all selected pane contents.

This is the smallest useful chunk boundary: the complete sealed surface fact block can survive this class of semantic edits. It skips **all** layout solving, pane/bar/tab record construction, plan geometry validation, hit-region emission/canonicalization, touch compilation, semantic-roster traversal/sort/dedup, and execution-link installation for an eligible Select. Requirement/index construction also disappears. It does not replay those streams to prove equality. Fresh answers, one effective-policy calculation, the changed leaf minimum, and the changed strip's reveal calculation establish the exact closed dependency case.

A change of selected target rule may change prospective drop **policy** without changing geometry. Equal effective bar policy is not equal effective drop policy. Keep current drop evaluation in the existing assessment/winner route: its live destination must carry the new selected-item `DockTargetRuleKey`, and `evaluate_drop_facts` must use the current payload, operation, surface/root/target rules and arrangement lock. Add a case where bar policy remains equal but A allows a drop and B denies it. Retained geometry must yield the new denial. Do not cache a previous drop decision or rely on bar-policy equality as authorization. That rule is read from the new snapshot when drop queries or emitted command descriptors are bound. It must not remain in a retained old target. Active drag is outside the local case, so a change affecting displayed drag guides always uses rebuilding.

Full rebuilding is **one** function, for example `build_surface(snapshot, inventory, measured, strip_state, projections) -> SurfaceFacts`. Bootstrap, structural edits, changed measurement, changed minima/policy, reveal movement, resize, popup and native first-live all call it. Factor its strip/minimum calculations for reuse by the bounded local check. Never keep a second old full compiler alongside this function.

## Publication, rollback, and native semantics

A snapshot ID is a current-state capability, not a content fingerprint. Snapshot-bound commands from another session or a replaced state fail once at admission. Within the admitted in-process operation there is no capture-source, then recapture-and-validate-source sequence. Structural edits still check real membership, item ownership, policy, canonicality and geometry exactly where they are created.

The prepared candidate owns its selection pages and strip changes. Commit exchanges the published snapshot only after existing required measurement, encoding, lifecycle and presentation conditions hold. Drop, encoding failure and suspended-update drop discard the candidate and its journals. Repeat-pass restart rolls back only derived encoding/presentation state while preserving the sole input candidate as today. IDs may be burned by failed attempts; they are never reused to revive authority.

Retaining `SurfaceFacts` never retains input authority. Every successful changed candidate receives a new small current binding; every concrete emission receives its own exact emission identity. Preserve candidate/fallback/confirmed/interactive distinctions and the existing presented ledger. Old output remains paint fallback where appropriate, but cannot authorize commands against the new snapshot. Unpresented output cannot become current merely because it shares geometry with a presented one. Coordinate changes always require current coordinate binding and rule out this local optimization.

Pending native operations hold an owner-issued immutable frozen root source and its semantic stamp. That stamp covers membership, selection, MRU, presentation ownership and relevant root semantics. Any actual source edit, including a Select, must pass the same lifecycle freeze decision before adoption; a fresh snapshot ID is not permission to mutate the frozen source. Updating an unrelated root does not automatically mutate that source. The first local optimization falls back whenever native state is relevant, retaining the real full native path and its barrier. Replacing `NodeFingerprint` with owner-issued stamps is authorized; weakening native source semantics is not.

Change reporting comes from explicit mutation/publication outcomes. Track initial/final compact identities for touched scene slots, including fallback, confirmation, interaction authority, coordinates, native first-live provenance, popup and revision frontier. Do not OR together arbitrary changed flags; a sequence can cancel. Public value comparisons may remain diagnostic, but are not the publication protocol. This supports the main design; merely inserting `Arc::ptr_eq` would not meet its avoided-work requirement.

## Operable migration and deletion list

| Current location | Replacement and required deletion |
| --- | --- |
| `graph.rs`, `workspace.rs` | Split topology from selection/MRU. Keep existing generational arena; replace semantic `WorkspaceIndex` with topology/item/parent ownership rows. Delete version-coupled fingerprint data from that index. Migrate borrowed node views instead of recreating a full `Workspace` on selection. |
| `operation.rs`, `transaction.rs` | One owner-private `apply_select` returns a bounded patch outcome and canonical preservation. Delete whole-workspace selection draft cloning and whole-value comparison from that route. General transaction preparation remains the sole conservative edit/normalization implementation. |
| `command.rs`, `workspace.rs` capture/verify helpers | Replace proof-bearing runtime sources/targets with snapshot-bound semantic references minted by the owner or resolved from current semantic requests. Delete `NodeFingerprint`, root fingerprint construction, recapture validators and obsolete public capture/verify exports after callers move. Migrate persistence deliberately; do not retain a compatibility proof route. |
| `engine/product_action.rs`, `engine.rs`, admitted pointer and semantic reducers | Route current Select and admitted tab activation to the same owner patch. Do not materialize `WorkspaceCommand::Select` only to send it back through generic capture/validate. Preserve admission, policy, focus, MRU, events and lifecycle barriers. |
| `engine/presentation_authority.rs`, `scene_compiler/requirements.rs` | Selection retains structure/request-directory owner and updates one effective bar policy/current binding. Delete full semantic snapshot/index/draft derivation for this case. Keep one complete inventory builder for invalidation cases. |
| `runtime/measurement.rs`, `scene_manifest.rs`, `engine/surface_contribution.rs` | Make the synchronous row-bound measurement call the normal path. Delete detached tokens/prepared-contribution/map-submission protocol and associated public malformed-batch compatibility after all internal/adapter callers move. Keep one complete answer representation consumed by full compilation, Ready matching, future layout and native first-live. |
| `scene_compiler/surface.rs`, `scene.rs`, `scene/records/tab.rs` | Share a single complete `build_surface`; split selected fields and proof-bearing semantic targets out of immutable geometry. Introduce the bounded retain decision above; no whole-plan re-emission to discover equality. |
| `scene/records/layout.rs`, `scene_compiler/future_layout.rs` | Retain the request directory plus measured rows. Current compiler leaves use linked rows. Cross-source/candidate future-layout queries keep semantic key-to-row lookup through the directory; deleting answer maps must not delete those queries or full-Q data. |
| `drop_resolver.rs`, `scene/validation.rs::target_reference_is_current` | Reuse the existing target-rebind/assessment responsibility with typed geometry locators and a structural owner. Delete embedded full-current fingerprint requirements and the redundant full presentation-structure comparison where owner identity establishes it. Keep exact locator checks and current actual drop-policy evaluation; bar-policy equivalence never substitutes for drop policy. |
| `scene/lifecycle.rs`, `presentation_hit.rs`, `semantic_manifest.rs`, `scene/paint_program.rs` | Separate facts from output wrapper. Retain hit/touch/semantic/paint links as one immutable fact group in the local case. Full building still produces distinct pointer and semantic inventories. Remove the obsolete full-stream equality-replay reuse path once true input-bound retention replaces it. |
| `runtime/paint.rs`, `scene/program.rs` | Execute directly linked geometry/receiver roles; bind selected state and output identity from the current surface. Delete repeated ID joins for owned ordinary records and any runtime reliance on foreign proof-bearing wrappers. Stable semantic names remain at diagnostics/events/adapter widget seams. |
| `engine/reducer.rs`, presentation publication methods | Replace broad scene equality with owner-produced compact initial/final publication changes. Preserve every meaningful header distinction. Delete obsolete comparison-driven hot-path change inference. |
| egui/native/headless adapters and harnesses | Migrate to the one synchronous Interface. Retain actual native registration, complete recording/program counts, output verification, final hooks, lifecycle settlement and complete product workload. Do not ship compatibility adapters for the removed protocols. |

Do not try to preserve all old error enums or wrapper shapes. Compile failures after deleting an obsolete type are a useful inventory of remaining callers; fix those callers rather than adding an alias that keeps the old protocol alive.

## Shortest implementation path and qualification

1. Change the workspace owner first: split selection storage, retain the existing topology arena/index, and implement the one selection patch including MRU, events, focus, snapshot/root stamps and lifecycle check. Move current item selection and admitted click to it. General edits can still materialize a draft, but selection must not. Do not start by rewriting every structural operation incrementally.
2. Freeze structure and measurement directories, convert synchronous answers into row-bound storage all the way through compiler and retained future-layout use, and remove the detached submission route. Link compiler request rows once. The core owns all of these in-process relations.
3. Separate selection and current output binding from immutable geometry/query facts. Implement **exactly** the bounded retain checks above and one full fallback. Let both full and local paths return the same `CurrentSurface`; install it without rebuilding views already sealed in its facts. The initial objective is whole-fact retention on unchanged scroll, not a leaf-by-leaf hit arena editor.
4. Migrate egui and headless callers to the Interface, remove replaced public protocol/types and update docs/examples. Preserve output ledger and native proof behavior first; this refactor does not require simultaneously redesigning egui terminal output auditing.
5. Verify through the same Interface with the two actual Adapters. Compare complete normalized state, MRU, all measurements, geometry, hit/touch, semantic membership, bound descriptor policy, program order, full contents, emitted output counts and settlement. Include unequal minima, selected-item bar policy, equal-bar/different-drop policy, reveal/clipping, drop-source lookup, source-press selection with nonprojecting arm and subsequent release/drag-threshold activation, stale/unpresented output, aborted update/restart, and native freeze/first-live/retirement.

Before timing, use a small test-only or diagnostic avoided-work ledger on the qualified 1,024-pane Select: zero full workspace candidate clones, zero root fingerprint builds, zero requirement/index derivation visits, zero full surface compilations, zero hit emissions, zero touch compilations, zero semantic-roster builds, zero execution relinking. Complete-Q and full paint counts must remain equal to baseline. A diagnostic ledger is not a new publication proof script.

Then freeze baseline and candidate inputs/build settings and run two independent **serial** comparison groups, with both real press and release and every pane/tab/content/CPU draw product present. Each group's 1,024-pane complete click p50 improvement must be at least 10%; complete headless selection must improve in the same direction. The gate is not satisfied by averaging a winning and losing group, a semantic select microbenchmark, or skipping hover/settlement/product work. Check idle/resize and 16/128-pane behavior, and retained current-plus-fallback/in-flight memory. Numerical layout, framework work and output work remain real costs.

If this bounded pilot misses the user gate, first inspect its avoided-work ledger and actual product eligibility. Do not expand to a general dependency framework in hope of hiding a failure. Local hit-arena editing for changed scroll is a separate optional investment only if workload evidence shows fallback there is the dominant lost opportunity.

## Counterexamples that overturn or materially narrow this design

- The real press/release path mutates another presentation dependency. In particular, current source press selects and then arms; rejecting all Armed states excludes the press frame. If ordinary source press/release cannot retain facts while preserving its exact capture, continuation and threshold-activation behavior, the proposed optimization may not address the user gate. Classify nonprojecting arm versus projected manipulation, not merely `has_active_gesture()` or presence of pointer events; verify this before broad migration.
- A selected-item rule changes bar visibility or enabled behavior while policy revision stays fixed. The local retain route must reject it. A design using only revision equality is invalid.
- Every measurement answer is unchanged, but the two selected items have different measured minima. Selection changes effective root constraints and must rebuild. All-answer equality alone is invalid.
- The new tab is clipped or offscreen, so reveal changes scroll, visible tabs or controls. This first local route must rebuild; patching selected booleans alone is invalid.
- Retained drop commands or magnetic fields contain the previous selected item's rule, fingerprint, payload or authority. Late current binding must remove that dependency. If a field mixes geometry and selected policy too tightly to separate without full rebuilding, fix that representation or narrow the retain route; do not silently reuse stale actions.
- An unexpected geometry/semantic dependency on selected content exists outside the explicit policy/minimum/reveal list, for example a selected-dependent presentation-menu anchor, protected chrome, or root-scoped action membership. Either express it as a current binding or add its actual check/full fallback. The current root anchor is chosen using layout/chrome; it cannot be assumed stable without checking that full builder consumes only retained inputs.
- Removing `selected` from geometry forces the egui Adapter to perform a binary search or allocate a new wrapper per operation. That moves work to every painted frame. Current views must use direct selection/receiver slots; profile the full workload to reject excessive indirection.
- A frozen native source can change selected/MRU through the patch, or a dropped update modifies published page data. Either is a semantic failure regardless of speed. Copy-on-write ownership and lifecycle admission must be tested through the normal Interface.
- A previous presented output becomes current solely because it shares facts with the new snapshot. Shared immutable geometry is never current input authority; any such acceptance invalidates the design.
- Complete measurement callbacks, application contents, egui native work and settlement dominate enough that eliminating all identified rebuild work still misses 10% in one comparison group. Then the architecture may be cleaner, but it has not met this performance objective. The evidence supports moving investigation to the measured remainder, not declaring a lower bound or promising a larger rewrite.
