# Renderer-free docking commands and logical-view attribution

Date: 2026-11-06. Runtime base: `a9ace585e1e8d812efd35c6149f549043bb7c999`.
Reference: Dear ImGui `d88f238d1d363ef387382b2e29ad12cf96f826ac`.
No production algorithm or public API is changed by this experiment.

## Result

The large difference remains after removing both rendering frameworks. At
1,024 total panes and four logical views, pooled p50 for a checked center move,
all-root layout and complete fresh owned export is **2.301 ms for dockspace and
0.067 ms for ImGui**. A perpendicular edge split is **2.342 ms versus 0.103 ms**;
a cross-view move is **2.293 ms versus 0.067 ms**. Complete output vectors match.

The first architectural priority is the checked transaction pipeline. In these
four-view move workloads, mutation accounts for **72.55–73.10%** of Rust's
summed same-execution total time, layout **19.13–19.56%**, current request/proof
capture **6.40–6.52%**, and common export **1.29–1.37%**. For one view, mutation
accounts for **78.16–79.21%**. These shares sum actual phase durations over the
same samples; they are not subtractions of separate medians.

An independent counter build confirms repeated complete-root proofs, four
whole-workspace item-multiset derivations, and one full workspace candidate
clone for every one-item move. This establishes redundant broad work, not a
measured speedup from deleting it. The source and a separate CPU capture also
identify node/vector copying during canonicalization and strict validation.
Their individual removable time has not been isolated by an ablation.

## Matched workload and limits

The [reproduction protocol](../../integration/product-comparison/docking.md)
defines actual command inputs and full output. Both sides use fresh balanced
alternating-axis binary trees, two tabs per pane, inactive size constraints,
four-unit separators, no central leaf, and exact integer geometry. Total panes
are fixed at 16/128/1,024 and divided among one/four logical roots. All roots,
including untouched ones, are laid out and exported for every sample.

`center_move` and `edge_split` move initially unselected Item2 from the first
root's first leaf to that root's last leaf; its source retains selected Item1.
The split is perpendicular to the target's parent and uses half the extent.
`cross_view_move` targets the last root's last leaf and exists only at four
views. `resize` changes every root's external bounds between widths 16,380 and
16,444, height 16,380; the fresh fixture starts at the opposite width.

ImGui executes the real `DockNodeIsDropAllowed`, `DockContextProcessDock`,
all-root `DockNodeUpdateForRootNode`, and `DockNodeTreeUpdatePosSize` from the
unmodified pinned source. Rust builds one shared current `WorkspaceIndex`,
captures the real source/target, executes full `WorkspaceTransaction::apply`,
and calls real `project_root` for all roots. Neither side executes `NewFrame`,
`Begin`/`End`, tab widgets, fonts, draw lists, egui, tessellation, a backend,
scene/hit compilation, or host output settlement. No upstream docking algorithm
is copied into the harness.

Both allocate the same-capacity fresh word vector inside export timing and
export topology, every rectangle, actual ordered members, and selected command
intent. ImGui reads pending `NextSelectedTabId` before `SelectedTabId`; this is
not rendered visibility. The common output survives workspace/context
destruction. Rich Rust projections and reports remain alive through the total
endpoint, with their disposal outside timing. Real transaction temporaries and
replaced workspace destruction stay inside `apply`.

The remaining contract differences are deliberate and visible: Rust retains
stale-proof rejection, atomic failure behavior, MRU handling, canonicalization,
and richer immutable internal projections. ImGui mutates its live tree. Its
mutation phase includes all-root flags/info finalization even for resize.
These are operation/output ratios for this subset, not a Rust/C++ language
ratio, equal failure-contract ratio, complete click, or full product ratio.

Logical views are roots in one workspace/context. Native OS viewport lifecycle,
floating-root undock/redock, minimum-constrained/central layouts and same-axis
canonical n-ary insertion are outside this first matrix. Same-axis native
binary and Rust n-ary geometry must not be rounded or rewritten into agreement.

## Timing

Values are microseconds per complete operation, including common owned export.
Each pooled cell contains 192 samples: 48 per process, two processes per
product in each of two serial groups. Percentiles use nearest rank. The raw
archive retains all samples, p95s and maxima.

| Total panes | Views | Case | dockspace p50 | ImGui p50 | Ratio |
| --- | --- | --- | ---: | ---: | ---: |
| 16 | 1 | `resize` | 6.500 | 0.625 | 10.40× |
| 16 | 1 | `center_move` | 43.458 | 0.667 | 65.15× |
| 16 | 1 | `edge_split` | 47.791 | 1.042 | 45.86× |
| 16 | 4 | `resize` | 6.292 | 0.584 | 10.77× |
| 16 | 4 | `center_move` | 29.583 | 0.625 | 47.33× |
| 16 | 4 | `edge_split` | 32.167 | 0.958 | 33.58× |
| 16 | 4 | `cross_view_move` | 30.125 | 0.625 | 48.20× |
| 128 | 1 | `resize` | 53.875 | 6.917 | 7.79× |
| 128 | 1 | `center_move` | 361.333 | 6.958 | 51.93× |
| 128 | 1 | `edge_split` | 383.292 | 10.125 | 37.86× |
| 128 | 4 | `resize` | 53.417 | 6.666 | 8.01× |
| 128 | 4 | `center_move` | 249.875 | 6.750 | 37.02× |
| 128 | 4 | `edge_split` | 256.917 | 9.708 | 26.46× |
| 128 | 4 | `cross_view_move` | 249.167 | 6.750 | 36.91× |
| 1,024 | 1 | `resize` | 574.416 | 67.209 | 8.55× |
| 1,024 | 1 | `center_move` | 3411.958 | 68.166 | 50.05× |
| 1,024 | 1 | `edge_split` | 3600.291 | 108.000 | 33.34× |
| 1,024 | 4 | `resize` | 469.459 | 65.416 | 7.18× |
| 1,024 | 4 | `center_move` | 2300.916 | 67.416 | 34.13× |
| 1,024 | 4 | `edge_split` | 2342.083 | 103.000 | 22.74× |
| 1,024 | 4 | `cross_view_move` | 2293.458 | 66.542 | 34.47× |

The direction repeats in both serial groups. Each entry below is a group p50,
with 96 samples per product/cell; no group is dropped.

| 1,024-pane case | Views | Group 1 Rust / ImGui | Group 2 Rust / ImGui |
| --- | ---: | ---: | ---: |
| `resize` | 1 | 576.292 / 66.625 | 572.791 / 67.625 |
| `center_move` | 1 | 3349.084 / 67.458 | 3444.125 / 69.292 |
| `edge_split` | 1 | 3578.916 / 107.084 | 3634.208 / 109.292 |
| `resize` | 4 | 464.666 / 64.584 | 472.458 / 66.500 |
| `center_move` | 4 | 2298.167 / 66.000 | 2304.292 / 68.084 |
| `edge_split` | 4 | 2328.541 / 102.208 | 2347.750 / 103.917 |
| `cross_view_move` | 4 | 2271.334 / 66.000 | 2306.625 / 67.250 |

Tails are retained, not presented as a guarantee: pooled 1,024-pane one-view
edge p95/max is 3.969/4.368 ms for Rust and 0.124/0.597 ms for ImGui. Four-view
center p95/max is 2.663/3.013 ms and 0.078/0.125 ms respectively. Single-call
small phases approach the clock's resolution; integer nanoseconds do not imply
independent nanosecond precision.

## Attribution

The following phase p50s are independent summaries of directly recorded nested
durations. They must not be summed to reconstruct a p50 total.

| 1,024 panes | Views | Product | Request | Mutation | Layout | Export |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| `resize` | 4 | dockspace | 0.041 | 0.041 | 442.000 | 28.125 |
| `resize` | 4 | ImGui | 0.125 | 14.333 | 6.500 | 44.167 |
| `center_move` | 4 | dockspace | 149.208 | 1671.125 | 444.542 | 28.917 |
| `center_move` | 4 | ImGui | 0.167 | 15.916 | 6.500 | 44.500 |
| `edge_split` | 4 | dockspace | 148.292 | 1708.084 | 446.500 | 28.917 |
| `edge_split` | 4 | ImGui | 0.167 | 51.542 | 6.500 | 44.250 |
| `cross_view_move` | 4 | dockspace | 143.958 | 1671.583 | 443.000 | 28.792 |
| `cross_view_move` | 4 | ImGui | 0.166 | 15.792 | 6.500 | 43.875 |

### Checked publication does broad work for a local move

The separate ordinary counter build runs the same driver once per cell. Setup
is complete before reset; external correctness checks occur after the snapshot.
Its times are discarded. All 21 observed rows match predictions derived from
the source before reading measurements.

For total panes T and roots V, the request's current index captures V roots.
Authorization then rebuilds one source-root proof and two target-root proofs;
application repeats the same sequence before any mutation. With equal-sized
roots, this predicts V+6 fingerprints and `(2T-V) + 6*(2T/V-1)` node visits.

| Per successful 1,024-pane move | One view | Four views |
| --- | ---: | ---: |
| Fingerprint builds | 7 | 10 |
| Fingerprint node visits | 14,329 | 5,110 |
| Full item-multiset scans | 4 | 4 |
| Item occurrences visited by those scans | 8,192 | 8,192 |
| Full transaction candidates | 1 | 1 |
| Nodes copied into the candidate | 2,047 | 2,044 |
| Tab items copied into the candidate | 2,048 | 2,048 |

Resize has zero in all these counters. Moving across views does not avoid the
global clone/multiset work. Splitting one tree into four smaller roots reduces
fingerprint-node visits while leaving the global clone/item inventories almost
unchanged; the timing changes are consistent with that scope difference. This
is not a single-variable ablation and cannot assign a causal saved percentage.

The [transaction source trace](2026-11-06-docking-kernel/transaction-source-attribution.md)
also identifies work outside the clone ledger: canonicalization clones each
post-move Node on both entry and exit; the final strict validator clones each
visited Node again. All of these copies include owned tab/split vectors. Root
cleanup collects every remaining source-root item only to test emptiness, and
edge locates the target parent three times. Two workspace equality calls stop
at changed MRU in this fixture; they must not be called two full node scans.

A separate eight-second CPU capture of the frozen one-view center executable
shows `Node::clone` and allocation below the strict validator under
`prepare_transaction_inner`, plus repeated `build_root_index` call paths. The
process also includes fixture setup, external validation, output checking and
disposal. Its flat symbol percentages cannot be used as timed-phase shares.
The formal phase timers, counters and source trace provide the attribution;
the capture corroborates that the identified mechanisms execute.

### Layout and intermediate ownership remain a second cost

Resize omits checked commands, yet Rust still takes 469 us at four views and
574 us at one view including export. Its layout phase is about 442/517 us;
ImGui's native all-root bounds update is about 6.5 us in both shapes. Rust
layout consumes 93.64% of its summed four-view resize total and 89.63% at one
view. The shared export does not explain that gap.

The current Rust implementation already has dense structural rows and scratch
reuse internally. It should not be described as an entirely map-based solver
that has not received those improvements. The indexed scene path also already
reuses topology. This experiment measures the standalone full `project_root`
path; its construction cost does not describe every retained product frame.
This public full projection path
still recompiles topology, constructs leaf-constraint maps, measures the tree,
and allocates three keyed result maps plus per-split result vectors. Native
layout writes existing node fields without a fresh intermediate projection.
General constraint handling and richer owned results are real extra contracts;
neither contract proves that their current repeated allocation is minimal.

The fresh common vector costs about 29 us for Rust and 44 us for ImGui in the
four-view move cases. The C++ export includes native-pointer-to-stable-item
mapping; it is adapter overhead, explicitly charged. Removing output ownership
alone would leave the observed transaction and layout phases. No allocation
event counts or bytes are inferred from this time measurement.

### ImGui also has nonlocal work

The [native source trace](2026-11-06-docking-kernel/imgui-source-attribution.md)
shows a context-wide window-reference scan for center/cross moves and two such
scans for edge. Root flags/info finalization traverses all roots. In this dense
ID fixture, edge's two allocations cause `2N+3` registry queries during ID
generation, each a binary search; edge also refreshes target-root metadata and
performs a small local layout before the later all-root update. These are
source counts/bounds, not measured shares. The larger observed native edge
mutation phase is consistent with this additional work.

ImGui does not clone a context for rollback, reconstruct whole-root value
proofs, or repeatedly canonicalize a trusted graph through an error-returning
transaction. Its native mutation accepts stronger live-context preconditions.
Rust's transaction atomicity is distinct from CPU atomics. This probe does not
instantiate the engine authority/receiver paths, and there is no evidence that
Arc reference-count instructions or Rust ownership rules are the dominant
cause of the measured difference.

## Refactor priority from this evidence

1. **Make one owner validate a move once per unchanged candidate phase.** Derive
   source/target facts and execution operands from one candidate-bound check,
   then consume that checked result for mutation. Reuse each distinct current
   root proof within that phase; stop reuse immediately after mutation. This
   removes the current authorize/revalidate/recapture layering without
   weakening stale-reference rejection. A caller-supplied version alone cannot
   authorize reuse across internal same-version edits or later batch commands.
2. **Separate untrusted draft/import canonicalization from commands on an
   already valid owned workspace.** Keep a complete validator at that boundary
   and in regression/property tests. Have ordinary commands preserve local
   topology, MRU and item conservation through explicit updates, rather than
   repairing and re-proving every unaffected root after each command. The
   current duplicate roster/node passes are an implementation choice, not the
   definition of correctness. This requires a trusted valid baseline, with each
   command in a batch checked against the actual staged state produced by all
   preceding commands.
3. **Replace whole-workspace rollback storage once those invariants have one
   owner.** A touched-root candidate or bounded staged mutation can retain
   atomic failure behavior while avoiding unrelated-root copies. Cross-root
   moves, node IDs, late batch failures, MRU, orphan cleanup and presentation
   changes must commit together. Simply mutating live state early would lose
   the existing contract. Start by removing avoidable per-Node clones in
   validation; do not add another generic proof/cache framework.
4. **Then change layout result/storage ownership.** Retain valid topology at its
   actual owner, reuse numeric/result buffers, and expose immutable views or
   compact owned results where callers need them. Preserve the general solver
   and old/in-flight output semantics. The current internal dense topology
   should be extended at that seam rather than duplicated by a second solver.

The full owned export itself requires visiting all output nodes/items, so this
measured operation has an unavoidable linear lower bound. That does not justify
seven complete root proofs, four ordered multisets, repeated vector clones or
new keyed intermediate results for a one-item edit. The data does not establish
that any current residual gap is theoretically irreducible, nor predict a
specific post-refactor ratio. Keep this matched harness as the regression and
investment test for the next implementation.

## Evidence and validation

The [archive](2026-11-06-docking-kernel/) retains 8,064 formal raw rows, 84 group
summaries, 42 pooled phase summaries, per-process output, 168 sample-zero full-vector dumps,
build/environment/source receipts, separate work counts and CPU capture, and
independent implementation/evidence reviews. Eight processes run serially in
ABBA then BAAB order, with eight warmups per cell. Full dump vectors match in
all 21 matrix cells; every sample's complete-state digest matches across
products/processes at the corresponding dimensions. All four phase durations
sum exactly to the directly measured total for every raw sample.

Rust 1.95.0 and Apple Clang 21 use production optimization/ThinLTO on ARM64
macOS 27.0. No build overlaps formal measurement. Formal timing disables the
test structural ledger at compile time; ordinary tests and counter attribution
compile without that variable. Executable, measured source and pinned upstream
hashes remain unchanged through the formal run. Preliminary smoke runs are
qualification only and are not pooled into formal results.

The ordinary production-profile core suite passes 1,943 tests, with the three
opt-in probes skipped. All four public-facade integration tests pass. The
independent enabled-counter probe passes its complete 21-cell matrix. Formatting,
Python compilation, the all-features normal/build dependency boundary and diff
checks pass. Clippy correctness/suspicious checks pass; all/pedantic warnings
remain, including the probe's long external validator and deliberately exact
binary floating-point assertions. This is not a warning-free Clippy claim.
Independent implementation, formal-evidence and root-cause reviews have no
remaining blocking findings. User CI and review files retain their 49 starting
hashes and are excluded from this change.
