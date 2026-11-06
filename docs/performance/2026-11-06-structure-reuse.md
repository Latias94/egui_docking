# Reusing presentation facts and layout topology

Date: 2026-11-06. Baseline: `f4b12d7ea88d561d717fd3898de8f81c4ce71785`.
This follows [local transactions for tab selection](2026-11-06-local-selection.md).

## Problem and implementation

Selecting a tab changes selection and MRU, but the subsequent presentation work
also repeatedly captures facts that belong to the same immutable workspace.
The engine can reuse those facts without weakening command freshness or treating
a matching version number as proof that an arbitrary workspace is unchanged.

`PresentationAuthorityState` already keeps weak provenance for its manifest's
workspace index. Reuse requires both the exact workspace allocation and the
index's version. The same provenance now permits reuse of its live surface
semantic snapshots. The helper walks the actual live surface roster, clones
each snapshot and fails the entire lookup if any live entry is absent. It
excludes prospective native surfaces, which share the storage map but require
their own current saga checks. Fingerprints remain immutable, shared, complete
snapshots containing selection, MRU, structural rows and presentation ownership.

The original recovery, native admission, policy, popup, revision and influence
checks still execute. A same-version mutable draft changes the allocation
through `Arc::make_mut`; raw publication similarly misses provenance. New
semantics are captured through the existing path. Publishing requirements
updates the index source and semantic snapshots together in their existing
owner, which is also the checkpoint/restore unit. No complete old workspace is
retained by this weak source.

A requirement rebuild also shares an already matching index rather than
deriving it again. When it needs a fresh index, it still builds every semantic
fact and the complete new fingerprints. It can transfer an already initialized
layout topology from the previous index only after matching that topology
against the actual new workspace. Topology stores node IDs, root/central
identity, node kinds, axes, ordered children, weights and postorder rows. It
does not store items, selection, MRU, policy, measurements or owner. Measurement
and geometry are recalculated with the current answers, metrics and projection.

Matching the topology itself matters: a shared old slot can have been filled
while measuring a modified same-version draft. Comparing only the old and new
semantic fingerprints would not prove that the slot matches either of them.
The new-index transfer filters that case; the normal measurement guard remains.
An empty old slot causes no transfer scan. The implementation adds no independent
cache, invalidation history, public protocol or trusted-command flag.

## Cost attribution

A temporary diagnostic build of the baseline ran the existing complete
renderer-free `frame_cost` example at 16/128/1,024 panes, 32 samples and six
warmups. All diagnostic modifications were removed before production changes.
Printing and nested timing scopes perturb these measurements. They attribute
cost; they do not qualify the candidate's latency, and nested rows must not be
added. At 1,024 panes, selection-update p50 attribution was:

| Scope | Baseline diagnostic p50 | Relationship |
| --- | ---: | --- |
| Complete update | 11.811 ms | Parent |
| Selection submission | 1.808 ms | Includes transaction and requirement work |
| Transaction | 0.249 ms | Includes source checking and selection edit |
| Selection edit | 0.189 ms | Includes complete live source verification |
| Requirement refreshes | 1.484 ms | Three calls in the update |
| Surface snapshot capture | 0.417 ms | Nested in those three refreshes |
| Workspace index construction | 0.133 ms | Nested; one call |
| Requirement draft construction | 0.402 ms | Nested; one call |
| Scene compilation | 3.411 ms | Includes layout |
| Root measurement | 0.349 ms | Nested in compilation |
| Cold topology compilation | 0.198 ms | Nested; includes its first measurement |
| Scene validation | 3.251 ms | Includes semantic and target checks |
| Scene installation | 0.719 ms | Includes hit, paint and semantic manifests |
| Host recording | 1.035 ms | Full ordered paint execution |
| Commit | 0.542 ms | Frame commit |

The repeated surface capture is concrete redundant work. This candidate avoids
the later captures when the workspace is still the same allocation; it does
not remove the whole requirement-refresh budget. The diagnostic idle update
did not call requirement refresh, so this cannot establish an idle improvement.
Transferring topology adds a matching pass before saving cold construction;
the whole 0.198 ms cold scope cannot be counted as savings.

The measurement-change update took 12.317 ms in the diagnostic build. Its
installation scope was 3.624 ms, much larger than selection's 0.719 ms. Bounds,
touch context and geometry-reuse misses deserve separate attribution before
assigning that cost to a particular representation.

## Qualification

Formal, uninstrumented measurements and exact-source verification are recorded
in the [evidence archive](2026-11-06-structure-reuse/README.md).

The baseline executables are the frozen candidate executables from the previous
experiment. Their hashes and full source inventories match committed
`f4b12d7`. The candidate changes only the seven recorded source/test paths.
Benchmark sources and pinned ImGui inputs are byte-identical. Measurements run
serially, separate from builds, tests, review and active heap profiling, using
the existing production ThinLTO product profile and release ThinLTO/one-codegen-
unit headless profile. Cargo uses one job and existing targets.

The frozen product comparison runs one ABBA and one BAAB group with 24 samples
and six warmups per process at 16/128/1,024 panes. Each group pools actual samples
from two processes per condition, giving 48 samples per case. At 1,024 panes:

| Scenario | Group 1 before → after p50 | Change | Group 2 before → after p50 | Change |
| --- | ---: | ---: | ---: | ---: |
| Idle, one frame | 8.369 → 8.358 ms | -0.1% | 8.349 → 8.557 ms | +2.5% |
| Physical tab click, two frames | 29.139 → 28.906 ms | -0.8% | 29.103 → 29.118 ms | +0.1% |
| Surface resize, one frame | 19.029 → 19.118 ms | +0.5% | 19.095 → 19.503 ms | +2.1% |

Product latency is approximately neutral for the targeted click case, rather
than evidence of a repeatable product speedup. The archive retains all p95/max
values and smaller scales. Group 2 candidate idle has a 14.667 ms maximum
against baseline 9.109 ms; no general tail improvement is claimed. Mesh,
content, frame and pass counts match exactly before/after for every sample.

The separate unchanged product-versus-ImGui comparison still shows a large
whole-product gap: candidate 1,024-pane click p50 is 28.957 ms and pinned ImGui
is 2.282 ms. This includes both frameworks' complete rendering and different
draw-command output. It cannot identify a docking-core lower bound or attribute
the whole gap to Rust ownership or layout.

The full renderer-free workload runs the same ABBA/BAAB orders, 48 samples and
six warmups per process. It records every ordered paint step, accepts the
receipt, commits and settles output. Its selection is a semantic action,
distinct from the product's two-frame physical pointer click. At 1,024 panes,
the four process p50 values per condition are:

| Update scenario | Four baseline process p50 values | Four candidate process p50 values |
| --- | --- | --- |
| idle | 1.894, 1.875, 1.909, 1.895 ms | 1.883, 1.913, 1.890, 1.908 ms |
| select | 11.959, 11.820, 11.958, 11.948 ms | 11.295, 11.430, 11.475, 11.486 ms |
| measurement_change | 12.848, 12.643, 12.806, 12.811 ms | 12.678, 12.735, 12.696, 12.796 ms |
| drag | 14.137, 14.087, 14.220, 14.266 ms | 14.202, 14.207, 14.249, 14.178 ms |
| outputs_in_flight | 12.028, 12.020, 12.009, 12.003 ms | 11.482, 11.450, 11.426, 11.473 ms |

These are process summaries, not pooled sample percentiles. Both order groups
show a lower selection median; all four candidate selection medians are below
all four baseline medians. Other scenarios establish no direct benefit. The
candidate selection maximum in group 1 run 3 is 13.553 ms, above both baseline
maxima in that group, despite the median/p95 improvement. Individual sample
timings are unavailable in this example and are not reconstructed.

Separate active-DHAT runs use six samples and two warmups. At 1,024 panes:

| Selection update metric | Baseline | Candidate | Reduction |
| --- | ---: | ---: | ---: |
| Allocation events | 84,659 | 76,351 | 8,308 (9.8%) |
| Requested allocation bytes | 51,861,180 | 50,508,388 | 1,352,792 (2.6%) |
| Case-prefix peak tracked bytes | 32,191,264 | 31,871,904 | 319,360 (1.0%) |

Selection live-before/live-after accounting is identical, and settlement still
uses 1,832 allocation events and 587,296 requested bytes. Idle, drag and
measurement-change allocation counts and requested bytes are identical at
every scale. Outputs-in-flight selection saves the same 8,308 events and
1,352,792 requested bytes. These are allocator accounting metrics, not RSS;
active-profile timings do not qualify latency. The candidate is retained for
the repeatable renderer-free selection improvement and allocation reduction,
with the neutral product result explicitly preserved.

## Correctness checks

Four new tests and expanded existing topology cases cover exact provenance,
prospective-key filtering, missing-live-key fallback through an actual rebuild,
version mismatch, same-version draft edits and raw publication. They also cover
sharing topology while deriving fresh selection/MRU/item fingerprints,
re-measuring changed answers/metrics/projection against the frozen layout
oracle, structural mismatches, and an old slot filled from a modified draft.
Legal structural edits explicitly require a successful new index; deletion
accepts only the expected missing-node error.

All 2,162 workspace tests, 292 default-host tests, 335 serde-host tests, 556
native tests and 10 doctests pass on the final source inventory. The nine
focused tests are included in the workspace total. The normal opt-in layout
probe and existing native wall-time test remain skipped. Workspace/native
correctness and suspicious Clippy checks, formatting, dependency boundaries
and whitespace checks pass.

Independent architecture and implementation reviews found no blocking issue.
The implementation review identified the overly permissive test assertion
described above; it was tightened, formatted and rerun before qualification.
Reviewers did not run the tests. Native/checkpoint safety was checked through
the existing suites and static ownership/call-chain inspection; the new focused
tests are not a new exhaustive native-lifecycle model.

## Repeated proofs still present

These are remaining opportunities, not measured benefits of this change:

1. `try_rebuild_presentation_requirements` still captures semantic snapshots
   before constructing a fresh index. Both capture complete root fingerprints.
   Reusing the fresh index for that first capture would remove another scan,
   but requires an explicit decision about existing failure ordering and native
   projection handling. It should not introduce a second invalidation system.
2. `capture_product_item` captures a complete live source fingerprint for an
   internally generated command; selection then verifies the complete source
   again. `ReferenceAuthority::Indexed` already exists for other checked paths.
   Passing exact immutable provenance through this seam could share the
   fingerprint while preserving old-source rejection. A version number alone,
   or an assumption that all internal commands are current, is insufficient.
3. `validate_semantic_uniqueness` sorts temporary ID vectors;
   `expected_pane_records` traverses roots into a tree map;
   `validate_pane_and_tab_records` builds more maps; `canonicalize_ready` sorts
   the records again. A directory built once per validation could serve
   duplicate detection, joins and canonical order. A directory-only refactor
   can keep stable duplicate/error precedence. Reusing this local work needs
   no persistent cache or compiler-generated-is-trusted switch.
4. Compilation and validation each derive splitter junctions. A smaller typed
   construction interface may reduce the proof surface. Simply feeding a
   compiler's own answers back as the validator's expected answers would remove
   independent checks without proving that construction is correct.
5. Requirement revision changes can rebuild inventories just to change tickets.
   `exact_set_validator` also makes two directions of per-key tree lookups. A
   sorted merge can retain the existing missing-before-unexpected error order
   with linear work. Selection can change tab-bar policy; that dependency must
   remain explicit before sharing an inventory.

The next larger budget is the scene validation/compilation path, especially
repeated inventories. Transaction ownership is lower priority in this workload:
the complete transaction is already about 0.249 ms in the diagnostic build.
Changing all workspace storage before addressing the larger full-scene passes
would be a poor use of the measured budget.

### The production construction seam is narrower than the validator

In a non-test build, `scene` is a private module. `PresentationPlan` fields,
its measurement constructor and record mutation methods are private or
crate-private. The regular preparation path compiles a plan and immediately
validates it in `engine/surface_contribution.rs`; the native projection path
does the same in `engine/native_projection.rs`. A host cannot submit arbitrary
pane/tab/splitter records through these interfaces. Raw synthetic plans are
test fixtures. The external inputs at this seam are measurements and the
engine's current authority/state, rather than a host-supplied scene graph.

This distinction makes the compiler/validator pair a legitimate architectural
target. Checking that each internally emitted pane has the ID, selection and
layer just read from the same workspace can be a redundant assertion of the
compiler's construction. The mere existence of an error variant or an invalid
synthetic-plan test does not establish a production trust boundary. A future
refactor should list which invariants construction guarantees, restrict the
constructors that establish them, and keep the general validator as a differential
oracle while replacing redundant production checks. It need not preserve every
internal defensive branch forever.

That is different from omitting all post-compilation work. The current validator
also canonicalizes records and target availability; downstream code needs those
results. Arbitrary measurement values can affect geometry, and current/native
authority can become stale between preparation and reduction. Those dependencies
need a construction proof or an actual check at the appropriate seam. This
candidate changes neither interface. It establishes a measured baseline for
shrinking that larger proof surface rather than adding another global cache.

## Complexity and the absence of a millisecond lower bound

Let `N` be nodes, `T` all items, `k` tabs in the selected leaf, `Q` the required
measurement answers, `K` complete scene/output records, and `A` explicit
rectangles that actually change. Root-local sizes are `Nr` and `Tr`.

| Work | Necessary work under the current contract | Avoidable work or limitation |
| --- | --- | --- |
| Select one item | Constant selected-field write; current Vec MRU is worst-case `O(k)` | Cloning the entire workspace and scanning its source root are representation/contract costs, not selection's intrinsic lower bound |
| Complete collision-free fingerprint | Fresh flat capture is `Omega(Nr + Tr)`; independently captured exact comparison is worst-case the same | Immutable sharing can avoid repeated capture; collision freedom does not require rebuilding an unchanged value |
| Complete measurement input | Arbitrary new answers require `Omega(Q)` input work | This contract requests pane minima for every item, including hidden items; measuring only the selected item changes the workload |
| Geometry propagation | Explicit updates need `Omega(A)`; worst case `A = Theta(N)` | If selected minimum, policy and bounds do not change, selection need not invalidate geometry; changed minima can affect ancestors and many rectangles |
| Fresh complete scene/output | Producing `K` explicit records requires `Omega(K)` | Multiple sorts, tree tables, scans and allocations are additional costs |
| Complete host recording | Executing every required output step requires `Omega(K)` | Lookup, wrapping and temporary ownership costs can still improve |

This does not establish an overall linear upper bound. The current tab validator
has a member-to-visible-record linear search, which can contribute `O(k * v)`
inside a leaf. High-fanout layout can redistribute repeatedly, and ordered maps
and sorting introduce logarithmic terms. Persistent or chunked workspace
ownership could shrink local cloning, but would not eliminate the cost of a
complete explicit output.

Neither these bounds nor a whole-product ImGui/egui ratio proves that 1,024
leaves require approximately 12 ms. The abstractions differ, and whole-product
rendering is not a docking-core lower bound. Any repeatable improvement with the
same complete output, geometry and authorization behavior disproves the claim
that the previous elapsed time was irreducible. The current measurements still
show avoidable repeated derivation; an architectural/theoretical stop is not
justified.
