# Docking kernel implementation review

Reviewed at 2026-11-06T13:31:48.618131+00:00 against root's authoritative `protocol.md`. Base `a9ace585e1e8d812efd35c6149f549043bb7c999`. This review read the actual two native probes, runner, root integration diff and pinned upstream callees. It did not edit source, run Cargo/compiler/tests/benchmarks, invoke binaries, write Git state or spawn an agent. Only this temporary report was written. Compilation/exact-output smoke remain the root's serial responsibility.

## Verdict

The native workloads and owned output are real and match the intended first slice. **Approved after the fix review below: all four findings are closed and no blocking implementation finding remains.** I found no confirmed wrong source/target, geometry, pending-selection, ownership or copied-ImGui-algorithm defect. Existing full-scale short-smoke artifacts independently confirm all 21 common output cases, including both resize widths. This is approval to proceed to the declared formal 48/8 ABBA/BAAB measurements, not a performance or root-cause conclusion.

The original findings are preserved below as history and followed by explicit closure. The kernel is not a full click, native OS-viewport or equal-feature rollback comparison. No new abstraction or proof framework is required.

## Original findings — all closed by follow-up below

### P2 — C++ export includes extra observer validation and Counts work excluded from Rust export

Locations: `integration/product-comparison/imgui_docking_cost.cpp:222-235,252-284,357-360`; compare `crates/dockspace/src/docking_cost.rs:279-331,498-612`. Runner scope: `scripts/compare_docking.py:106` says validation is outside each total timer.

C++ timed export checks TabBar/window identity and full cardinality (`tab.ID == tab.Window->TabId`, `Tabs.Size == Windows.Size`, split Windows emptiness), and accumulates node/leaf/split/item Counts on every visited record. The Rust timed exporter does numeric conversion, native record lookup, binary/schema checks and vector emission, while Counts, item ownership and selected/MRU membership validation are performed after `measure` returns. C++ already has an outside-timer `StateCheck` that independently verifies the same cardinality, order, backlinks and identities.

This does not change the exported state or omit ImGui work. It instead adds benchmark-observer work to C++'s export/total and makes the scope statement inaccurate. Especially for small native handlers, that work should not be attributed to ImGui docking or unavoidable common owned output. It also weakens an ownership-cost explanation if export is later used as an attribution category.

Minimal resolution: retain genuine extraction costs on both sides (actual native TabBar order, real item-ID mapping, required integral/schema conversion, allocation and copying). Move duplicated cross-field/member consistency checks and Counts collection to the existing outside-timer checker, or include exactly the same additional promised metadata/validation on both sides and name that extended contract. Do not remove real native handler/root checks or add equivalent full post-hoc validation into Rust merely to make both slower. The current `StateCheck` already makes the first option small.

### P2 — runner's dump completeness check accepts missing dumps from a process/implementation

Location: `scripts/compare_docking.py:213-223`.

The runner collects a union of dump keys across every process directory, then compares only that union's size with the expected case count. If one implementation produces no dumps, the other can supply every expected key; each set then has one value and both existing checks pass. The intended full-vector cross-product comparison was never performed for the missing side. The same happens if one process omits a subset already supplied by another.

Minimal resolution: after each successful process, require its exact expected `<product>-<panes>-<views>-<case>.txt` filename set in that run's private state directory, then merge bytes for cross-product equality. Keep the existing complete raw/digest checks. There is no need for a native-name resolver, new binary schema or arbitrary graph checker. The current probe source writes the expected files; this is a demonstrated validation-branch hole, not evidence that an actual run has omitted them.

### P3 — accepted iteration range exceeds the Rust driver's supported range

Locations: Rust `docking_cost.rs:636-640`; C++ `imgui_docking_cost.cpp:65-68`; Python `compare_docking.py:79`.

Rust rejects samples/warmup above 10,000, whereas C++ and the runner accept up to 1,000,000. A documented accepted runner invocation can finish compilation and then fail solely on Rust's input limit. Align the runner/driver maximum; defaults 48/8 are unaffected.

### P3 — unset CLI case inherits an unrelated case environment variable

Location: `scripts/compare_docking.py:183-190`.

`dict(os.environ, ...)` retains `DOCKSPACE_DOCK_COST_CASE` when `--case` is absent. The child can run a narrower matrix while the parent expects all cases. Current completeness checks fail safely; there is no false successful result, but this causes a confusing avoidable execution failure. Remove that inherited key when CLI case is absent. Also useful while touching the row gate: require hexadecimal digest syntax and check the four contiguous phase durations sum to the direct total under the recorded nanosecond timer representation.

## Confirmed comparison properties

- **Actual upstream, no rendering.** C++ includes unmodified pinned `imgui.cpp` and links ordinary widgets/draw/tables once. Timed work is operand resolution plus `DockNodeIsDropAllowed`, `DockContextProcessDock`, all-root `DockNodeUpdateForRootNode`, all-root `DockNodeTreeUpdatePosSize`, then owned export. The root flags/info function itself calls native cleanup/info routines without tab/widget/draw submission. No `NewFrame`, `Begin/End`, text/font layout, `DockNodeUpdateTabBar`, splitter widget, render/tessellation/backend or custom docking algorithm is executed. Its outside-timer checker verifies no activated font or generated item-window vertices/indices. This is an internal structural-command endpoint, not UI-final visibility.
- **Proper live state.** C++ setup registers nodes in `DockContext.Nodes` and real `CreateNewWindow` objects in native window storage. Native real IDs remain unchanged. Context exclusively owns these objects. Initial setup uses actual native splitting/tab-add helpers plus fixture selection/active flags, outside timing. Neither side supplies expected post-command state to its timed exporter.
- **Matching operations.** Item2 is initially inactive in root 0's first pane; source retains selected Item1. Center/edge target root 0's last leaf, cross-view targets the last root's last leaf and only runs at four views. Target remains nonempty. C++ root pointers remain valid for these precise cases. Perpendicular half-edge preserves binary topology; both source native transfers and Rust checked transactions are executed. Center/cross move append Item2; edge creates a selected Item2 leaf while the original target keeps its original selection.
- **Geometry.** Both roots have no central flag, zero minimum, splitter 4 and per-view local origin zero. Fresh resize starts from the opposite width and projects every root to 16380/16444 by warmup+sample parity. The six count/view combinations and extra perpendicular split have exact integer halves, including the deepest 1,024-pane case. Both exporters reject nonfinite/fractional/out-of-range values before integer conversion. Full expected-node walkers compare actual geometry and untouched roots outside timing. Same-axis n-ary semantics are not silently normalized into this binary comparison.
- **Selection and tab order.** C++ reads actual `TabBar.Tabs` order and stored `NextSelectedTabId` before `SelectedTabId`; only a real singleton can bypass a TabBar. It never fills missing multi-member selection from the expected request. Rust reads real selected/item order. The value is explicitly `selected_intent`, with no native UI-final/MRU generality claim. The post-command oracle verifies membership, native ID correspondence and order independently.
- **Ownership and disposal.** Both allocate the common fresh vector inside export with the same reservation, retain it beyond the measured endpoint and read it after fixture/context destruction. Rust keeps all richer projections and report through total stop; their destruction is outside. Real transaction temporary/replaced-workspace destruction stays within apply. C++ does not construct a fake immutable projection merely to conceal its in-place native representation; that remaining asymmetry is disclosed.
- **Schema and digest.** Both use exactly nine header words per actual node, view ordinal 1..V, root path 1, left/right paths 2p/2p+1, kinds 0/1/2, then actual ordered members. Full vector length is `9*nodes + 2*initial_panes`. Both FNV1a implementations process all eight little-endian bytes of each word with the specified offset and prime. Digest, expected-state checks, dumps and CSV formatting run after total stop. C++ reruns all-root flags/info **even for resize**, recorded in mutation phase; final prose should state that explicitly.
- **Rust attribution is separate.** `docking_work` requires a build without `DOCKSPACE_BENCH_NO_WORK_COUNTERS`; the timing probe requires its presence. Attribution resets counters after full fixture preparation and snapshots them before post-measure validation. It invokes the same request/apply/layout/export path but publishes counts rather than treating its counter-instrumented times as timing data. Root's `structural_work::update` compile-time bypass prevents clone-volume graph traversals in the timing build; its separate hit lookup counter is outside this kernel. Normal tests must compile without that environment variable.
- **Runner scope.** Actual commands are structured subprocess arguments, profiles are production/ThinLTO and C++ O3/NDEBUG/LTO, binaries are copied/frozen, groups are ABBA then BAAB and children run serially. Raw schema, complete sample identities, counts, width parity and cross-product per-state digests are checked. Source/reference/executable hashes are rechecked after execution. No full headless recorder/receipt/settlement is unfairly added to only one side.

There is no requirement to add undock/redock, arbitrary central/minimum cases, full rendered tab selection or platform viewports to qualify this explicitly limited first matrix. Those remain real requested follow-up scope and must not be represented as already measured. No speedup or root cause is established by this static review.

## Reviewed source receipt

| File | Lines | SHA-256 |
| --- | ---: | --- |
| `crates/dockspace/src/docking_cost.rs` | 797 | `0994c837556c82fa9e3b21b81055a514a402cf24117c4e25edea0e04ca66e2f5` |
| `integration/product-comparison/imgui_docking_cost.cpp` | 623 | `45f355f9f753130437052b76d9000dede48ecf07d80581b0c0c1d049cb09df01` |
| `scripts/compare_docking.py` | 257 | `0ea0ef4073573b561f20df1966a8c0fcc6b3a060093ae7d53b50c0a6b20ae558` |
| `crates/dockspace/src/drop_resolver.rs` | 4218 | `3c4c7156d93da01422e81a1c141b601443a5a6af3fa67d8e1afad856a81beb94` |
| `crates/dockspace/src/lib.rs` | 252 | `4fda49b9273068b0659572b4627e5f631cbb5eb23e6f44349b9d1f62609f4944` |

Protected user files checked: 49; mismatched paths: []. This content receipt is point-in-time; root should bind smoke/formal runs to the final hashes after resolving the above findings.


## Fix review and closure

Reviewed at 2026-11-06T13:39:17.358036+00:00. Read-only source and existing-artifact verification; no compiler/test/benchmark/binary was run by this reviewer. All original findings are **closed**:

- **Timed observer P2 closed.** C++ `export_node` no longer accumulates Counts or repeats the tab/window cardinality/identity and split-membership checks. `selected_intent` no longer repeats the tab/window ID check. The genuine native ID lookup, pending-choice extraction, checked integer/schema conversion, allocation and copying stay inside export. Existing `StateCheck` retains full actual ownership/order/member validation outside the timer, and `run_sample` assigns `snapshot.counts = final_state.counts` only after `measure` returns. Rust's checked apply and all projection/result lifetimes are unchanged. No real native operation or root finalization was removed to fix this observer scope.
- **Dump completeness P2 closed.** Runner checks each private process state directory's exact expected product/count/view/case filename set immediately after successful raw validation, before union/equality. A missing implementation/process set can no longer be supplied by another run. Complete vectors are still compared across all runs.
- **Iteration-limit P3 closed.** The runner now admits samples/warmup only up to 10,000, matching the stricter Rust driver. C++'s looser standalone parser does not make an accepted runner invocation exceed Rust's limit.
- **Inherited-case P3 closed.** With no `--case`, the runner removes inherited `DOCKSPACE_DOCK_COST_CASE`. With an explicit case it sets the exact requested value. The row gate additionally enforces lowercase 16-digit hexadecimal digests and exact four-phase sum equality with the directly measured total.

I also read the completed root-owned `/tmp/dockspace-docking-kernel-20261106-a9ace58/smoke-all` evidence. It is one ABBA group, two samples/one warmup, all three pane counts and both view counts. All four native processes returned 0. I independently checked its **168 existing raw rows** for contiguous phase arithmetic, all **four exact 21-file dump sets**, all **21 complete vector equalities**, its recorded result hashes and every recorded source hash against the current tree. Both resize widths are present by the two-sample parity. These are verification of existing root execution artifacts, not an independent rerun and not formal latency qualification. The 49 protected user files retain their starting hashes.

No new blocker was found in the fixes. Proceed with the root's serial formal measurements; final evidence review should bind raw rows, profile/counter attribution and prose to the final source/binary receipts.

Updated reviewed hashes:

| File | SHA-256 |
| --- | --- |
| `crates/dockspace/src/docking_cost.rs` | `0994c837556c82fa9e3b21b81055a514a402cf24117c4e25edea0e04ca66e2f5` |
| `integration/product-comparison/imgui_docking_cost.cpp` | `4c74fbb088618dff1c71d4eaaa4094889c4516d4ecef3fd2342eeeb9c855f36c` |
| `scripts/compare_docking.py` | `6a099e1f5fdc48da67e730576ec59baff9433d007c01e0f183f96e3c6146ffa5` |
| `crates/dockspace/src/drop_resolver.rs` | `3c4c7156d93da01422e81a1c141b601443a5a6af3fa67d8e1afad856a81beb94` |
| `crates/dockspace/src/lib.rs` | `4fda49b9273068b0659572b4627e5f631cbb5eb23e6f44349b9d1f62609f4944` |
