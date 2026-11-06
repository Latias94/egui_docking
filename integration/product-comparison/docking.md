# Renderer-free docking commands and logical views

Run from the repository root:

```text
python3 scripts/compare_docking.py --out /tmp/dockspace-docking-comparison
```

The runner builds both probes, freezes their executables, and runs two serial
groups in ABBA and BAAB order. Defaults are 48 measured samples and eight
warmups per process, total pane counts 16/128/1,024, and one/four logical views.
Use an unused output directory. A small qualification run is:

```text
python3 scripts/compare_docking.py --samples 2 --warmup 1 --panes 16 --groups 1 --out /tmp/dockspace-docking-smoke
```

## Boundary

This compares actual structural docking requests, all-root layout, and a fresh
owned complete state export. It excludes egui, `NewFrame`, `Begin`/`End`, text,
fonts, widgets, drawing, tessellation, GPU work, scene/hit compilation, and host
output settlement. Logical views mean several dockspace roots in one
workspace/ImGui context. They do not include native OS viewport creation,
platform events, window ownership transfer, or renderer viewport management.

The C++ probe includes the pinned, unmodified `imgui.cpp` and calls its real
private handlers. The Rust probe is an ignored in-crate test calling the
existing private index, transaction, and layout functions. No public benchmark
API or replacement docking algorithm is introduced. The upstream revision is
`d88f238d1d363ef387382b2e29ad12cf96f826ac` (Dear ImGui 1.93.1 WIP).

Rust uses the `production` profile with ThinLTO and one codegen unit. The build
sets `DOCKSPACE_BENCH_NO_WORK_COUNTERS=1` to compile out the test-only structural
ledger, including its otherwise additional clone-volume graph traversals.
Ordinary tests and the separate work-count probe must omit that variable.
C++ uses C++17, O3/NDEBUG and LTO (MSVC O2/LTCG). Cargo builds use one job and
never overlap measurement processes.

## Fixture and commands

Each sample constructs a fresh complete fixture outside timing. Total panes
are divided equally among views. Each root is a balanced binary tree with
alternating axes, starting horizontal, equal split weights, no central node,
four-unit separators, and inactive minimum/maximum constraints. Each initial
leaf has two tabs, globally numbered Item1 through Item(2N); its first item is
selected. All views use local origin (0, 0).

| Case | Actual request |
| --- | --- |
| `resize` | Change every root's bounds between widths 16,380 and 16,444, height 16,380. Each fresh fixture starts at the opposite width. |
| `center_move` | Move initially unselected Item2 from root 1's first leaf to root 1's last leaf. |
| `edge_split` | Move Item2 into a new half-sized leaf before root 1's last leaf, with an axis perpendicular to that leaf's parent. |
| `cross_view_move` | At four views only, move Item2 from the first root's first leaf to the last root's last leaf. |

Non-resize cases use 16,380 square bounds. The source always retains selected
Item1, so it does not require empty-pane collapse. Initial and final state,
all members, tab order, selection intent, complete topology, and exact integer
geometry are checked outside timing for every warmup and sample.

The perpendicular edge case keeps both trees binary with the same geometry.
Same-axis insertion has different native binary versus canonical n-ary
semantics and must be tested separately. Undock/redock requires matching the
intermediate floating state and is outside this first matrix.

## Timed phases

All phases belong to the same execution. `operation_owned_ns` is measured
directly from its first to last timestamp; it is not reconstructed from
separate percentile measurements.

| Phase | dockspace | Dear ImGui |
| --- | --- | --- |
| `request_ns` | Build one current `WorkspaceIndex`, resolve current leaves, capture shared checked source/target proofs, and form the real transaction. Resize only captures bounds. | Resolve current roots and windows, determine split direction, and call real `DockNodeIsDropAllowed`. Resize only resolves roots. |
| `mutation_ns` | Full `WorkspaceTransaction::apply`: policy, live proofs, atomic candidate, canonicalization, invariants and publication. Resize installs external bounds. | Real `DockContextProcessDock`, then `DockNodeUpdateForRootNode` on every root, including resize. |
| `layout_ns` | Build leaf constraint maps and run actual `project_root` for every root. | Run actual `DockNodeTreeUpdatePosSize` for every root. |
| `export_ns` | Allocate and fill a fresh owned `Vec<u64>` from actual workspace/projection state. | Allocate and fill a fresh owned `vector<uint64_t>` from actual native node/tab state. |

The common vector reserves `panes*32 + views*16` words inside export timing.
Both sides export every root, including untouched roots. Native ImGui IDs are
mapped to stable fixture item IDs; native IDs and native tab order are not
rewritten. Selected intent uses `TabBar.NextSelectedTabId` before
`TabBar.SelectedTabId`. It is the command's pending selection, not visibility
after a rendered tab bar or a complete physical click.

The rich Rust projections and transaction report remain alive through the
total timer's endpoint. Their disposal, common vector disposal, fixture/context
destruction, checks, digest, dump, and CSV IO are outside timing. Actual
transaction temporaries and the replaced workspace retain their production
lifetimes inside `apply`. Both common vectors are read after the corresponding
workspace/context has been destroyed.

The common result does not make the implementations feature-equivalent.
Rust's atomic rollback and stale-proof contract, MRU handling, immutable richer
projections, and actual allocations remain included. ImGui mutates live nodes
in place. Ratios describe this explicit operation/output boundary, not language
performance, full docking feature parity, or complete product performance.

## Complete output and evidence

Roots use ordinal order; nodes use geometric preorder. Each node emits nine
words `[view, path, kind, x, y, width, height, item_count, selected_intent]`, then
its ordered item IDs. Root path is 1, child paths are 2p/2p+1, and kind is
leaf/horizontal/vertical = 0/1/2. Geometry must be finite, nonnegative, and
exactly integral before conversion. Fractional geometry is rejected rather
than rounded into agreement.

The runner checks exact sample identities, complete node/leaf/split/item/root
counts, resize parity, and all-state digests across products and processes.
Each process must also supply every sample-zero full-vector dump, and all
corresponding dumps must match byte for byte. Digests use FNV-1a64 over eight
little-endian bytes per word.

The output directory retains raw CSVs, per-group summaries, all process output,
full-vector dumps, build logs, executable hashes, source/reference hashes,
toolchain versions, commands, and timestamps. Source and executable hashes are
checked again after measurement. Summary percentiles use nearest rank and
retain all tail samples. Very small request times approach clock overhead.

For work attribution, build without `DOCKSPACE_BENCH_NO_WORK_COUNTERS` and run
the ignored `docking_cost::docking_work` test with
`DOCKSPACE_DOCK_COST_OUT` pointing to an unused CSV path. It uses the same
driver, resets counters after fixture setup, and snapshots them before external
validation. Its instrumented clock readings are discarded. Counts are not
formal timing results.
