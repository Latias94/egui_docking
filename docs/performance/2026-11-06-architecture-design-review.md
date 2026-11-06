# Architecture review of the remaining docking performance gap

Date: 2026-11-06. Reviewed implementation: `54840717a2454c957b13a667a939ddf8229e52e9`. ImGui reference: `d88f238d1d363ef387382b2e29ad12cf96f826ac`.

Status: architecture diagnosis complete; proposed implementation experiments have not been run. Three independent, read-only Codex lanes examined state ownership/invalidation, scene/measurement design, and ImGui/adapter responsibilities. They then challenged one another's recommendations. The root agent checked the principal current-source chains and collected separate current-product CPU samples. This round changes documentation and evidence only.

## Decision

A targeted architecture refactor is warranted. The strongest issue is **loss of locality between a domain edit and the work requested from presentation**. Other concrete issues are repeated association through semantic names inside a closed owner, the internal reuse of a detached measurement-submission protocol, and native-framework state transported through a second input/output protocol.

These are design choices with removable work. Their existence does not establish which accounts for most milliseconds, an unavoidable Rust penalty, or a lower bound near the current timings. A wholesale ownership/arena rewrite is not yet justified. The next substantial experiment should make an ordinary local selection actually avoid unrelated construction, while retaining fresh measurement and output authority.

Adding a generation container is insufficient: `SurfacePlanScene` already co-owns the plan, hit manifest, paint-program link, semantic manifest and coordinates (`crates/dockspace/src/scene/lifecycle.rs:119`). A change earns an architectural performance claim only when it eliminates a conversion, traversal, reconstruction or comparison. The cross-review rejected a new owner with the same work as a default destination.

## Evidence and its limits

The latest qualified complete-product reference, retained in [linked targets](2026-11-06-linked-targets.md), is:

| 1,024 panes | dockspace + egui | Dear ImGui | Product ratio |
| --- | ---: | ---: | ---: |
| Idle, one frame | 7.164458 ms | 1.105292 ms | 6.48x |
| Real tab click, press plus release | 25.397584 ms | 2.197958 ms | 11.56x |
| Resize, one frame | 16.857292 ms | 1.121500 ms | 15.03x |

These are complete framework/product ratios. Native styling, fonts, triangle counts, native interaction, and the Rust presentation protocol differ. Both products render every pane, two tabs per pane, the selected text/square content, and complete CPU draw data. The large difference is real for that task; it is not a measured pure docking-core ratio.

The latest renderer-free complete Rust update has process p50 ranges of 1.633–1.655 ms idle and 9.645–9.875 ms semantic selection. It executes an owning recording sink, confirms and commits the full program, and measures settlement separately. It has no egui/font/tessellation work, but it is not a bare reducer. Its values cannot be subtracted from the product values to obtain a framework budget.

Fresh sampling used the frozen current candidate executable, SHA-256 `c00a38ed2552d942dd99048eb06c491d27b0881e41efc122c12d148e7ddfb518`. All 565 recorded Rust/adapter/build/workload inputs were checked against the current tree before sampling. There were two separate serial eight-second `sample` captures: idle with 1,800 workload samples and tab click with 480, both with six warmups at 1,024 panes. No build, allocation profiler or test ran alongside them.

| Inclusive call-tree category | Idle, 6,320 main-thread samples | Click, 6,374 samples |
| --- | ---: | ---: |
| Standalone numerical layout symbols | 0 observed | 0.800% |
| Scene compiler | 0 observed | 10.056% |
| Scene validator | 0.063% | 4.534% |
| Requirement refresh | 0 observed | 2.714% |
| Measurement/resource adapter | 11.472% | 23.753% |
| Measurement-map insertion | 3.228% | 2.165% |
| Receiver queries | 7.722% | 5.271% |
| Whole scene value comparison | 4.288% | 2.887% |
| egui widget creation | 9.842% | 7.138% |
| egui child UI creation | 4.604% | 3.671% |
| CPU tessellation | 11.092% | 8.299% |
| Call trees containing output-proof operations | 15.665% | 11.500% |

**These categories overlap and must not be added.** Each category merges nonoverlapping roots within its own name. Captures include initialization, warmup, untimed click geometry/hover preparation, validation and disposal. Inlining can hide work under callers. Percentages are neither clean timed-frame partitions nor predicted savings.

In particular, the output-proof category includes `PaintAudit::check_append -> PaneView::ui -> Label`: it includes real application/content rendering. Its 15.665% is not exclusive auditing overhead or a removable budget. Measurement/resources likewise includes callbacks and real text work. Whole-scene equality is 4.288% inclusive in idle; the earlier 1.139% hit-geometry and 0.411% layout-facts observations are narrower nested categories, not the full comparison cost.

Stable idle already shares a Ready plan and avoids scene compilation. The current samples do not support explaining its cost with full layout recomputation. Click samples support investigating construction and interface work rather than starting with the scalar solver, but do not prove an exclusive phase budget. See the [evidence index](2026-11-06-architecture-design-review/README.md) for raw captures, receipts and reproduction.

## Current causes, ordered by architectural impact

### 1. Selection changes semantic authority and invalidates structural presentation together

**Source fact; highest conditional leverage for click.** An ordinary selection that changes the selected item updates one selected field and its stack MRU. Its published workspace identity/version changes, and `advance_revision` rebuilds presentation requirements (`engine.rs:5841`; `engine/presentation_authority.rs:177`). Exact old workspace/index provenance no longer matches. The refresh captures semantic snapshots, rebuilds the semantic index and complete requirement draft, and reconciles strip rosters (`presentation_authority.rs:258`, `:418`, `:441`, `:588`).

Per-surface revisions already protect unrelated surfaces' Ready state. They do not avoid deriving their requirement inventories. The changed surface loses Ready eligibility, so its next contribution compiles/finalizes the entire surface (`engine/surface_contribution.rs:285`, `:334`, `:362`). Installation then derives hit/touch, execution links and semantic views (`scene/lifecycle.rs:1264`). This is a genuine local-to-surface amplification.

Selection can change effective policy, selected minima, tab reveal/scroll and command proofs. Those dependencies are real. But the complete measurement inventory already includes minima for every item, and many structural records do not depend on which item is selected (`scene_compiler/requirements.rs:151`). A new source fingerprint does not imply all geometry and inventories changed.

**Recommended change:** have the editing owner communicate the actual narrow mutation; retain structural inventory and immutable presentation blocks whose actual inputs remain unchanged; bind new source semantics/output authority separately. First support one ordinary tiled Select, not a general incremental dependency engine. Requirement traversal alone was only 2.714% of click sampling: the material opportunity depends on also avoiding downstream reconstruction.

### 2. One immutable record is repeatedly associated through external semantic keys

**Source fact plus current sampled relevance; recurring opportunity.** The program already resolves typed record rows. Ordinary pane/tab/bar/splitter helpers then extract a stable ID and binary-search the hit inventory again (`scene/program.rs:793`; `runtime/paint.rs:2271`, `:2405`, `:2616`; `presentation_hit.rs:482`). Only DropTarget execution has already been linked by `5484071`.

**Recommended change:** extend the existing private program/record association to role-specific optional receiver rows, with a checked generation binding and the current output ticket. Keep public IDs, absent/passive roles and foreign-wrapper behavior. No new top-level owner is required. This removes recurring joins, rather than another cache lookup.

The egui DropTarget visitor is empty while the complete program still consumes those nonvisual instructions (`product_render/mod.rs:516`). A later visual-stream/query-service split could avoid inert adapter consumption while preserving complete program obligations and future input queries. The existing installation link already removed their repeated identity search; that solved work is not being proposed again.

### 3. The internal synchronous measurement producer serializes a relation it already owns

**Source fact; removable share and migration value not established.** `measure_surface_with` owns the frozen request loop, supplies each key, receives its answer, fills three ordered maps, and sends them through the detached contribution route (`runtime/measurement.rs:201`). When a new plan is needed, set validation, authoritative-value traversal and keyed compiler reads follow (`scene_manifest.rs:1067`, `:1295`, `:1376`, `:1445`). Matching Ready reuse instead compares the new maps with retained answers and returns before those compiler checks (`engine/surface_contribution.rs:285`). Map assembly remains on that idle path; repeated compiler exact-set validation does not. This is a protocol-conversion opportunity, but its replacement is not simply three Vecs.

The counter-review found important compatibility constraints. Compiler consumption follows workspace item order rather than frozen request order. Future drop layout queries retained measurements across source/candidate positions (`scene_compiler/future_layout.rs:390`, `:473`). Public detached batches may be stale, incomplete or malformed. Unavailable answers do not stop the synchronous loop, while later invalid variants can fail immediately; detached missing-before-unexpected error order is also observable.

**Recommended change:** isolate the assembly/association cost before migrating all consumers. A private request-bound batch or contiguous keyed storage may remove tree work, provided the relation survives into compiler/retained consumers. Converting it back to the same maps before compilation merely relocates work. Keep the full Q and the current callback/error behavior. An optional cross-frame metadata revision could reduce repeated application work, but it is a separate API contract, not permission to skip today's callbacks.

### 4. Shared storage still participates in value-based change detection and broad transaction copies

**Source fact, with direct equality samples; bounded opportunity.** At commit, `reducer.rs:938` compares complete scene values. Derived equality descends through shared plans and hit geometry despite an unchanged allocation. The explicit `Arc::ptr_eq` in `install_ready` does not protect this caller. A private published-state comparison can short-circuit identical immutable owners while checking mutable binding/header facts. Keep public value equality for independent objects.

The existing public scene delta compares only stamp/kind/interaction authority. It is not a complete replacement for whole equality: candidate/fallback, confirmation, native provenance, popup and revision-frontier distinctions also matter. A future owner-issued mutation journal must compare initial/final state and cover rollback/restart; simply OR-ing changed flags can report a change that a later operation canceled.

Selection also clones the whole workspace (`operation.rs:83`; `transaction.rs:14`), then infers change by value comparison (`transaction.rs:97`). A fresh source proof is captured and then checked through another capture. Atomic rollback and public stale-proof semantics are necessary; these whole-value mechanisms are choices. Current attribution does not establish copying as the dominant term. A persistent-arena rewrite should follow direct clone attribution, not precede it.

### 5. A separate docking core bridges a second native input/output system

**Source fact; mix of required behavior and replaceable representation.** Each selected pane creates native widget and child-UI state. Tab/splitter controls have both egui registration and core receiver bindings. Before application callbacks, the run freezes previous-pass widget/area/interactive facts and later reconciles them against exact presented bindings (`pass_settlement.rs:673`; `product_pointer.rs:36`, `:531`). Capture is once per run, including no-event runs; repeat passes share it through Arc. It is not a full snapshot per pane or per repeat pass.

ImGui consumes retained native windows, tab bars, hover and active-ID state directly. The current Rust adapter transports those facts through renderer-neutral scene/output identities and a host journal. This is adapter interoperability work, not uniformly an inherent framework cost.

Native widget registration supports blockers, focus and hover. General pane callbacks can mutate previous shapes, other layers and final hooks, so output proof checks real obligations. A renderer-owned completed-pass snapshot or output mutation fact might replace copying/descriptor scans, but that is an integration prototype with unproven savings. Skipping these checks without an equivalent mechanism changes the product contract.

## Concrete refactor and experiment order

The architectural destination is **reusable facts with current semantic and output bindings**, expressed inside the existing owners where possible. A generation name, another global counter, more content fingerprints, or a generic dependency graph is not the deliverable.

| Experiment | Work that must actually disappear | Required behavior / falsifier |
| --- | --- | --- |
| Ordinary receiver linkage; independent small control | Repeated semantic searches for current-plan pane/tab/bar/splitter roles | Same complete descriptors, program order and current ticket; foreign/passive/absent cases unchanged. Reject if memory cost outweighs repeatable complete-frame gain. |
| **Local-selection dependency pilot; primary architecture experiment** | Unrelated requirement inventory and at least one complete derived construction pass | New selected/MRU/source authority, full current Q, all panes/content and output settlement remain. Equal-input selection must skip construction, not rebuild then compare. Changed minima/policy/scroll must match conservative output. |
| Published-state identity comparison; independent small control | Deep scans of exact shared immutable bodies | Same published-state and scene-delta results across fallback, confirmation, native, retirement, rollback and encoding restart. Disappearing samples with negligible frame gain limit its priority. |
| Measurement storage/association counterfactual | Assembly/set-relation/readback cost, without dropping real measurements | Preserve callback sequence, early errors, unavailable handling, public malformed batches and cross-source future-layout lookup. Defer a broad migration if remaining key directories and callbacks dominate. |
| Framework snapshot/output ownership prototype | Demonstrable copying or descriptor capture/scans | Keep callback timing, unknown native blockers/overlays/transforms, final hooks, repeat passes and exact presented authority. Inclusive audit samples alone are not acceptance evidence. |

Start the selection pilot with exactly one ordinary tiled Select and stable topology/configuration/coordinates. Recompute the changed bar's **effective** policy using the newly selected item's rule; a stable policy revision is insufficient. Preserve the complete current measurement answer set. Compare actual effective leaf minima. Rebuild the changed bar's selection/reveal/scroll/chrome even if minima match (`scene_compiler/surface.rs:1297`, `:2024`). Share only independently unaffected blocks.

Use the existing conservative path for batches, structural edits, contained raise/select, popup, active drag/resize, recovery and pending native/rehome. Preserve native source-freeze checks and newly published full semantic source proofs. This bounded eligibility is preferable to inventing a universal proof or cache-invalidation framework.

Record avoided visits/allocations separately from CPU timing, then compare the unchanged 16/128/1,024-pane complete headless and product workloads in independent serial groups. Evaluate current-plus-fallback/in-flight retained memory. A broad split should require a material repeatable complete-click win beyond accepted narrow improvements. The scene reviewer proposed 10% as an investment gate; that is a proposed engineering threshold, not a measured result, a user requirement or a speedup forecast. Stop at the smaller design if the prototype still re-emits every view to discover reuse, or if the remainder is mostly required callbacks/native rendering/output work.

## What to borrow from ImGui and how to compare it

Pinned ImGui retains `ImGuiDockNode`/TabBar/window associations and updates local selection in place. It rejects dormant drag-target work early and gathers detailed splitter constraint lists when active. It still walks trees, submits native controls and measures text. It has release operational checks and assert-based invariants; the benchmark's `NDEBUG` removes the latter. It does not provide an equivalent general historical-output/presentation-settlement contract.

Three comparison boundaries should remain distinct:

1. **Current layout plus equal rectangle export.** Extend the existing extraction of the actual unmodified `DockNodeTreeUpdatePosSize` to include complete normalized owned rectangle export, for both fresh and reused output buffers. Compare current Rust output and explicitly list unmatched constraint/split metadata. The old `d6dc6d7` layout result is historical and is not current timing.
2. **Command-to-normalized-state.** Use real `DockContextProcessDock`/undock handlers plus necessary normal finalization, and matching Rust semantic commands. Verify the complete tree, tab order, selection/visibility, assignments and common-subset geometry. Give both implementations equal export ownership. Exclude egui child UIs, widget submission, pointer freeze and output audit from this private kernel; report bridge experiments separately. The existing complete recording/settlement example remains a separate workload.
3. **Complete product.** Retain real press/release, hover/blocker behavior, content, native controls and CPU draw data. This is the final end-to-end check. A selection field assignment is not a click benchmark.

`DockContextNewFrameUpdateDocking` alone omits normal dockspace-root update, which goes through `DockSpace -> DockNodeUpdate`. Full interactive docking is interwoven with tab widgets, input/focus and drawing. A faithful drawing-suppressed extraction is possible but is a higher-cost later experiment; it should not become a second incomplete docking implementation or an elaborate proof script.

## Required checks, avoidable checks, and limits

The best current example of excessive internal validation is re-establishing request/answer membership inside the loop that owns that membership. Repeated identity joins and value comparisons are similarly avoidable representation work. External measurement submissions, stale receipts, arbitrary callbacks and computed geometry still require their actual checks or equivalent construction invariants.

Production geometry validation has known genuine counterexamples, including out-of-bar viewports, Visible tabs without drawable records, and lost drop coverage under combined contained-window occlusions. Semantic and pointer rosters differ intentionally: clipped menu rows remain keyboard/accessibility reachable. Geometry identity cannot authorize a newly presented emission. Removing these distinctions would change correctness, not merely improve implementation.

With the current contract, successful measurement batches must read the full Q, visible output must encode its actual records/vertices, and touch results must represent their actual fragments. These give output/callback-count lower bounds. They do not imply repeated structural derivation, internal semantic joins, full-scene equality, or a millisecond floor. No present experiment proves the residual gap irreducible.

All three lanes and their counter-reviews are preserved in the [evidence directory](2026-11-06-architecture-design-review/README.md). The state and ImGui lanes also independently reviewed this synthesis against current source and found no blockers; the selected-item wording above incorporates the state reviewer's final precision fix. The principal unresolved question is how much current authority-bearing data can be rebound without rebuilding unrelated facts. The proposed local-selection experiment answers that directly. This report establishes where to change the architecture and where evidence is still missing; it claims no new speedup.
