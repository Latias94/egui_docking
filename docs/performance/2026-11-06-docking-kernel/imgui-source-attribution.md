# Source attribution for the pinned renderer-free docking kernel

Date: 2026-11-06. Reference: `repo-ref/imgui`, commit `d88f238d1d363ef387382b2e29ad12cf96f826ac` (1.93.1 WIP). This is a read-only source review of the current matched protocol and Rust/C++ probe boundaries. No compile, test, benchmark, source mutation, or nested agent was performed. The reference worktree had no diff when checked.

**These are source facts and falsifiable predictions, not measured causes.** Native instruction costs, allocator costs, cache effects, optimizer decisions, and relative phase magnitude require root's serial measurements or profiles. Line numbers below refer to the current checkout and may move during integration.

## Scope and notation

The primary matrix has P initial panes, V logical roots, M = 2P real item windows, and N = 2P - V initial nodes. Every initial root contains `P/V` two-item leaves. Let n be the target root's node count, h its depth, K the node registry's entry count, and S the saved-window-settings count. In this fresh dense fixture, K = N, S = 0, h is logarithmic, and no source leaf becomes empty because the moved item is the unselected second tab.

All roots share one context, there are no native OS viewports/host windows/popups, and minima are inactive. Commands are already resolved semantic operations; UI drag hit-testing and tab rendering are excluded. Both probes finalize/layout/export all roots, including untouched ones.

## Native command and data structure facts

### One global reference scan for center, two for edge

`imgui.cpp:18697`, `DockContextProcessDock`, uses the existing node and an ordinary single-window payload in this matrix. Its single-window branch at `18814` saves the old payload DockId, calls `DockNodeAddWindow`, then calls `DockSettingsRenameNodeReferences(old_id, new_id)` when the old ID is nonzero. The source is initially docked, so center and cross-view each enter that rename helper once.

`imgui.cpp:21835`, `DockSettingsRenameNodeReferences`, traverses **all `g.Windows`**, tests `window.DockId == old_id && window.DockNode == nullptr`, and then separately traverses every `g.SettingsWindows` chunk. Its loop is not limited to either affected root. `NoSavedSettings`/`IniFilename = nullptr` makes the settings population empty in the fixture but does not remove the registered-window loop. Thus the abstract scan work is M window entries per center/cross-view command.

`imgui.cpp:20524`, `DockNodeTreeSplit`, also calls the same helper at `20552` when moving the target leaf's identity references to the inheritor child. The later single-window payload branch still performs its own rename. Edge therefore enters the global helper twice, examining 2M window entries and 2S saved settings in total.

All fixture item windows have a non-null DockNode after their immediate transfer. In these matched calls the scan may change **no** window DockId at all; the full search is still present in the source. This distinction matters: the amount of local mutation is not the amount of global observation. An optimizing compiler's final implementation should be checked with a profile or generated code before assigning a timing percentage to this loop.

### Dense node IDs introduce repeated binary searches

`imgui.cpp:18433`, `DockContextGenNodeID`, starts at ID 1 on every call and repeatedly invokes `DockContextFindNodeByID` until it finds a null entry. `DockContextFindNodeByID` at `18428` delegates to `ImGuiStorage::GetVoidPtr`.

The registry is a sorted contiguous `ImVector<ImGuiStoragePair>`, not a hash map:

- `imgui.cpp:3083`, `ImLowerBound`, implements binary search.
- `imgui.cpp:3139`, `ImGuiStorage::GetVoidPtr`, calls that search.
- `imgui.cpp:3201`, `ImGuiStorage::SetVoidPtr`, binary-searches and inserts/updates a sorted entry.
- `imgui.cpp:18444`, `DockContextAddNode`, invokes the generator for ID 0, allocates a real `ImGuiDockNode`, and registers it.

For initially occupied dense IDs 1..N, a single-item edge split creates two new nodes (`20529`, `20532`). The first generator makes N+1 registry queries, and the second makes N+2: **2N+3 `GetVoidPtr` queries inside ID generation**, before accounting for the two registration searches. Each query is logarithmic in registry size. This gives an O(N log N) source bound for this dense-ID allocation path, rather than O(1) ID minting. Sparse IDs or early holes change the search length; fresh dense fixtures avoid hiding it.

Center and cross-view do not create a node in this two-tab source/existing-target fixture and therefore do not enter node-ID generation. Root capture does perform its own V registry lookups in the request phase, a separate O(V log N) cost.

New maximum IDs append logically at the end of sorted storage, so an insertion normally needs no suffix shift for this fixture. `ImVector` capacity growth can still allocate/copy the whole registry. This is a possible allocation branch, not a claim that a particular fixed matrix row reallocates.

### Local item transfer remains local, but is not the complete command

`imgui.cpp:19013`, `DockNodeAddWindow`, removes the window from its old node, appends to the destination Windows vector, maintains docking flags, and adds native tab state. `DockNodeRemoveWindow` at `19072` searches/erases source membership and removes the native tab; `imgui_widgets.cpp:10480`, `TabBarRemoveTab`, erases the tab and clears matching selected/pending/visible IDs.

General vector operations depend on the source/destination tab counts. Here those counts are bounded by 1..3, so these particular local loops are constant-sized with respect to total P. Removing the second source item leaves a singleton and destroys the old non-central TabBar (`imgui.cpp:19115` and `20126`, `DockNodeRemoveTabBar`). This includes real deallocation inside the mutation phase. Native center mutation should not be described as having no memory-management cost merely because no new node is created.

Edge allocates two real nodes and creates/transfers the necessary leaf-window/tab-bar storage. `DockNodeAddTabBar` at `20120` uses `IM_NEW`; native vectors may allocate when their existing capacity is insufficient. No shared immutable output tree or whole-context rollback candidate is allocated by the native command.

## Root finalization is explicit whole-tree work

`imgui.cpp:19260`, `DockNodeUpdateFlagsAndCollapse`, recursively visits children, visits each resident window, combines flags, processes liveness/closure rules, applies hidden-tab-bar changes, and updates visibility. With all windows live and no closure/toggle policy, it still performs those visits. In this valid fixture the traversal is O(N+M) over all requested roots. More general deletion/collapse paths can do additional merge/reference work; they are not triggered by moving the unselected second item.

`imgui.cpp:19376`, `DockNodeUpdateForRootNode`, calls that traversal, then `DockNodeFindInfo` (`19227`) to determine central/root/window-bearing-node metadata. `DockNodeFindInfo` can return early only after it has found both a central node and multiple window-bearing nodes. The matrix deliberately has **no central node**, so this second traversal reaches every node. The function then copies a root window class from the first populated leaf, checking its small window list; its final central-ancestor loop is empty here.

The probe calls this finalizer for every root inside `mutation_ns`. Thus even resize, which has no dock mutation command, has all-root mutation-phase traversal. It is incorrect to interpret that column as only insertion/removal work or to expect resize mutation_ns to be zero.

Edge has an additional intrinsic traversal: `DockNodeTreeSplit` calls `DockNodeUpdateHasCentralNodeChild(DockNodeGetRootNode(parent))` at `20553`. The helper at `19338` recursively clears/recomputes the marker throughout the target root even when no central node exists. It first walks parent pointers to the root, then visits that whole root. Its source bound is O(h+n), in addition to the caller's later all-root finalization. The split also calls `DockNodeTreeUpdatePosSize` immediately at `20554`; for the leaf-target case this is a tiny three-node local projection, before the later full all-root layout.

## Native layout writes existing storage

`imgui.cpp:20615`, `DockNodeTreeUpdatePosSize`, writes node Pos/Size, selects the size policy from existing SizeRef/flags/style, and recurses into visible children. It does not construct maps, projections, tab geometry, or output records. The fixed matrix keeps both children visible and all leaves occupied, so all N nodes (N+2 after edge) are reached. On this valid balanced subset it is Θ(N) work and O(h) call-stack space, with **no explicit heap allocation in this function's executed recursive path**.

The implementation also supports minimum allocation, absolute-size locks, central-node remainder allocation, pixel rounding, and partial single-node projection. These branches establish that its general semantics differ from the Rust constraint solver; the exact-integer, no-central, inactive-minimum subset is what makes the current outputs match.

The function's own comment says it does not affect child Windows. The probe exports node rectangles and does not call the full `DockNodeUpdate`, `DockNodeTreeUpdateSplitter`, `DockNodeUpdateTabBar`, or `NewFrame`. It does not measure physical content bounds, rendered tabs, or native platform lifecycle.

## Atomicity, atomics, and rollback are different claims

The selected native command/finalizer/layout functions operate directly on the global context and its nodes/windows. They return void or direct mutable results, have internal assertions/preconditions, and do not build a transaction candidate, error journal, or original-state restoration mechanism. The probe performs native class/popup filtering (`imgui.cpp:20134`, `DockNodeIsDropAllowedOne`, and `20166`, `DockNodeIsDropAllowed`) before the mutation, but it does not add Rust-style stale-root fingerprint/invariant/rollback guarantees. There is no CPU atomic or mutex in this native call chain's core data structures.

The probe's `timestamp()` uses `std::atomic_signal_fence` around `Clock::now`; this is a compiler ordering boundary, not a transactional rollback mechanism or an inter-thread atomic counter. Compiler/allocator implementation details should not be inferred from the word `atomic` in that helper.

Rust's `WorkspaceTransaction::apply` (`transaction.rs:51`) calls checked preparation and only publishes the successful candidate. `clone_workspace_draft` (`transaction.rs:11`) uses `workspace.clone()`. `Workspace` (`graph.rs:356`) contains a node SlotMap, MRU BTreeMap/vectors, and presentation maps, so this is a full durable-workspace draft clone, not an Arc clone of the graph. `PreparedTransaction::publish` (`transaction.rs:102`) replaces the old workspace after successful preparation; destruction of the replaced workspace occurs inside that actual apply. Failed preparation discards the candidate and preserves the original.

This is atomicity under an exclusive mutable Rust API and a failure contract. It does not mean publication is implemented by a CPU compare-and-swap. The kernel uses plain `Workspace` and `WorkspaceTransaction::apply`, not the engine's concurrent host/input publication machinery.

Rust does use `Arc` for immutable fingerprint records (`command.rs:41`, `137`), root-index membership (`workspace.rs:56`), and temporary topology (`layout.rs:763`). Their shared-pointer clone/drop paths can involve reference-count atomics; Arc allocation/initialization is distinct from an atomic increment. `ids.rs:18` has an AtomicU64 for engine authority minting, and host/pointer-receiver modules have other atomics, but this command/layout probe does not instantiate those engine/native/receiver paths. Do not attribute unrelated runtime atomics to this benchmark. No source evidence here establishes refcount atomics as the dominant cost.

## Rust's checked command and projection contract

The current request phase (`docking_cost.rs:139`, `capture_request`) builds **one** current `WorkspaceIndex` and shares its fingerprints between source/target captures. `WorkspaceIndex::build` (`workspace.rs:82`) derives ownership and indexes every root; `build_root_index` (`workspace.rs:824`) visits its complete subtree, captures item order/selection/MRU and split structure, and creates immutable authority records. This constructs whole-root authority that the native pointer/class path does not construct; its relative elapsed-time cost is not established by source inspection.

The mutation phase invokes the real checked pipeline:

- `operation.rs:644`, `prepare_transaction_inner`: clone a complete draft, build the initial item multiset, authorize/apply commands, prune vacant surfaces, canonicalize, compare/report changed state, and reconcile the final item multiset.
- `operation.rs:908`, `DropCommandEligibility::new`, and `971`, `check_target_with`: validate source/target authority and evaluate actual drop facts.
- `operation.rs:1522`, `move_payload`: validates payload and target again before detach/insert. The plain transaction path uses `ReferenceAuthority::Live` (`operation.rs:2426`, `2508`, `2613`); the request's prebuilt index is not automatically an apply-time authority cache.
- `workspace.rs:777`, `verify_reference`, re-captures current root facts; `validate_tab_target`/`validate_edge_target` also rederive target semantics. These paths can rebuild whole-root fingerprints even with exact-current request inputs.
- `canonical.rs:205`, `canonicalize_draft`: validates rewrite input, traverses/rewrites roots, checks reachability/item ownership, and runs the complete candidate validator (`canonical.rs:210..245`). It works on the one already cloned transaction draft; it does **not** add another full workspace draft clone in this inner wrapper.
- `graph.rs:447`, `item_multiset`: visits the arena's item occurrences and inserts/counts them in a BTreeMap. Canonicalization and final reconciliation perform additional whole-workspace passes; BTreeMap work is not identical to a flat native window scan.

These are genuine checked API costs and stronger contracts. The current successful workload does not trigger a failed transaction, stale proof, or complex selected-item fallback; it still pays the success-path machinery that protects them. Conversely, matching normalized output does not establish that native and Rust commands promise the same failure behavior.

Rust layout also creates a richer owned internal output, in addition to the common vector:

- `docking_cost.rs:222`, `project_all_roots`, rebuilds per-root leaf-constraint BTreeMaps and calls `project_root` for every root.
- `layout.rs:441/463`, `project_root` / `project_root_with_overrides`, uses full topology compilation and measurement (`layout/topology.rs:42`) for this non-prepared call; it does not reuse the request WorkspaceIndex as a prepared layout topology.
- `layout.rs:167`, `LayoutProjection`, owns node rectangles, composed node constraints, and per-split outcomes in three BTreeMaps.
- `layout.rs:542`, `project_with_weights`, creates those output maps. `prepare_split_projection` (`layout.rs:966`) constructs per-split minimum/maximum vectors, solved child sizes, and splitter rectangles; `advance_split_projection` (`1082`) retains them in SplitProjection.
- `solve_axis` (`layout.rs:335`) validates weights/constraints and creates numerical size vectors even in the ordinary satisfiable, no-central case. Map insertion/constraint-map construction adds logarithmic keyed operations to the otherwise bounded-degree traversal.

The richer projections remain alive through common export and the final timestamp (`docking_cost.rs:349`, `measure`). Their destruction is outside the total timer as required. Their allocation, measurement, and construction remain inside layout_ns. Native tree layout has no comparable owned intermediate projection. Both sides count the fresh common vector allocation, but that does not erase this internal result-contract difference.

## Cost model for this exact matrix

| Work | Native source bound / scope | Important qualifier |
| --- | --- | --- |
| Request, no popup | O(V log N) root registry lookup + bounded operand/class checks | No full current-root proof construction |
| Center/cross mutation | O(M+S+N) from rename and all-root flags/info; small leaf-vector changes | Source tab-bar deallocation is inside the phase |
| Edge mutation | O(N log N + M+S+N+n) for dense IDs, two renames, target-root marker update, all-root flags/info | Two new nodes; local three-node layout also occurs here |
| Resize mutation | O(N+M) all-root flags/info | No docking command, but deliberately not an empty phase |
| All-root native layout | Θ(N), Θ(N+2) after edge | Existing node storage, no explicit output allocation |
| Common owned export | Θ(N+M) expected identity-map work plus output allocation | Pointer-to-stable-item mapping is adapter normalization overhead |

The exact normalized word count before edge is `9N + M = 20P - 9V`; edge adds 18 words, with total item count unchanged. Both sides reserve the common requested capacity `32P + 16V` words inside export. Digest, correctness checks, optional dumps, and result destruction are excluded from timed totals.

This model describes source-level operation counts and normal container bounds, not elapsed-time measurements. In particular, allocator internals, hash-map behavior, and CPU cache costs need direct evidence.

## Falsifiable predictions for root's measured attribution

1. **Global command scan despite a small payload.** Center/cross native mutation should grow with total context size even though only one item moves. A profile/call observation should show one rename call and the all-root finalizers; no writes from rename are required to explain its search scope. Absence of actual visits in optimized code, or negligible sampled cost, would refute a claim that this loop materially explains measured time.
2. **Edge has a distinct dense-ID component.** Native edge should expose `DockContextGenNodeID`/`ImLowerBound` work absent from center/cross. The dense fresh fixture predicts 2N+3 generator lookups. A diagnostic with an early reusable ID hole would reduce lookup iterations without changing layout size, but must be labeled a separate control, not silently substituted into the primary matrix. If generation has little measured weight, its source complexity is not the measured bottleneck.
3. **Four logical views do not imply four times the native command work at fixed P.** M is unchanged and N differs by only three nodes between V=1 and V=4. Global rename and full output scope stay essentially constant. Edge's extra target-root marker traversal becomes smaller with four views, while root-handle/metadata overhead grows with V. Large four-view timing changes need cache/data-shape or other phase evidence; they cannot be inferred solely from a root count.
4. **Native intra-root versus cross-root center move should have similar dominant work with V=4.** Both use one rename scan, no new node, the same all-root traversals/layout, and equal output sizes. Different local metadata/cache effects remain possible. A large systematic gap would warrant specific profiling; this call graph does not itself predict a new cross-root global algorithm.
5. **Common ownership does not make the layout contracts equal.** A Rust layout-phase allocation/profile should include constraint maps, compiled dense topology, rectangle/constraint/split maps, and split vectors; native layout should principally write existing node fields. If measurements do not show those costs materially, do not promote allocation count alone into a dominant-cause claim. The older prepared-layout case is useful attribution context, not an implementation change to the matched checked kernel.
6. **Resize separates command machinery from layout/export.** Rust resize omits index capture and checked transaction apply; native resize still runs all-root flags/info in mutation. Strong command-case Rust mutation growth that disappears in resize would be consistent with draft/proof/canonicalization costs, but nested measured phase evidence is required before assigning responsibility among them.
7. **Transaction atomicity is not evidence of CPU-atomic contention.** A valid profile should not charge engine-authority/native-receiver atomics to this probe's call chain. Observed refcount instructions, allocator synchronization, or fences must be identified separately; their presence alone does not establish contention or dominance.

No performance conclusion or runtime refactor recommendation follows until root's full-state smoke comparison and serial measurements qualify this matched kernel.
