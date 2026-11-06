# Sealed transaction ownership and dense complete layout

This qualified checkpoint removes full settlement from an ordinary single Item
move and replaces per-split projection allocations with flat complete output.
The fixed renderer-free operation is substantially faster, but **no matrix cell
meets the twofold ImGui objective**. Further architectural work remains justified.

The starting reference is `ea4161e`; the candidate also includes the separately
qualified arena bookkeeping checkpoint `dbac00f`. ImGui remains the unmodified
`d88f238d1d363ef387382b2e29ad12cf96f826ac` source. The fixture, operation,
request-capture interval, all-root layout, fresh owned export, full oracle and
serial ABBA/BAAB protocol are unchanged. See the
[measurement boundary](../../integration/product-comparison/docking.md).

## Results

Both comparisons use 48 samples and eight warmups per process over all 21
applicable cells. Each has two serial groups, 8,064 raw rows and 168 complete
sample-zero vectors. Every corresponding full vector and state digest matches.
Both frozen executables and the candidate source receipt remain unchanged.

For 1,024 total panes, the two groups give these complete-operation p50 results:

| Views | Operation | Reduction versus ea4161e | Candidate / ImGui |
| --- | --- | --- | --- |
| 1 | Resize | 37.40–38.91% | 4.60–4.64x |
| 1 | Center move | 61.66–61.66% | 10.06–10.20x |
| 1 | Perpendicular edge | 56.06–56.15% | 8.43–8.43x |
| 4 | Resize | 33.56–33.81% | 4.21–4.23x |
| 4 | Center move | 65.69–65.93% | 7.95–8.12x |
| 4 | Perpendicular edge | 64.05–64.34% | 5.74–5.84x |
| 4 | Cross-view move | 63.59–63.61% | 8.91–9.07x |

Each range is the two actual group results, not a confidence interval. Candidate
times in the ea4161e and ImGui comparisons come from their respective paired
runs; subtracting percentiles from different runs would not yield a valid phase
budget. The 16-pane cells still range up to 16.20x ImGui. The full
[comparison table](2026-11-07-sealed-layout/comparisons.csv) retains every
16/128/1,024-pane result, p95, max, and negative tail comparison.
All 42 paired p50 results improve on ea4161e, but two p95 and four max results
regress; the largest max regression is 108.17%. Tail improvement is not uniform.

## Ownership and successful work

`Workspace` now seals canonical storage; only the graph implementation can
complete and publish edits. Builder owns raw uncommitted data. The old mutable
committed escape, `selection_ready` state and redundant `CanonicalWorkspace`
wrapper are removed. Single Select remains an invariant-preserving edit.

Exactly one ordinary `Move(Item)` uses the same checked executor followed by
source-path/outer-wrapper completion. A populated source Center move does not
run a structural normalization pass. Last-item repair retires the actual empty,
collapsed and flattened nodes while preserving central identity and presentation
ownership. Every other program selects the complete general route before any
execution. Mixed batches, recovery, builder/import, strict validation, staged
staleness and atomic publication retain their broader settlement rules.

The ordinary counter probe confirms zero global item-multiset scans for all
matched moves, down from four at the initial reference, and one complete
candidate clone. The independent local/general matrix also requires zero full
canonical passes and strict workspace validations on successful local moves.
It strictly validates each successful candidate outside its work snapshot.

The root-value proof cost is still present. At 1,024 panes/one view a move builds
two fingerprints and visits 4,094 nodes across capture/checking, while cloning
2,047 nodes and 2,048 tab members once. At four views Center builds five
fingerprints/visits 2,555 nodes; Cross builds six/visits 3,066. These are measured
work counts from a normal instrumented build, not formal timings. See
[work.csv](2026-11-07-sealed-layout/work.csv).

## Numeric correctness boundary

Independent review found two reachable f32 cases which the first local
implementation incorrectly accepted: same-axis insertion can push an accepted
near-tolerance input sum just outside `1e-5`, and normalization of a legal
2,000-child source split can produce a sum outside that same strict bound.
Both regressions were run red before the correction and green afterward.

The existing executor now reports the actual inserted/modified split. The local
finisher checks that input roster before rewriting, then checks all final live
split outputs on the actual repair frontier. It retains the original accumulation
order, epsilon and normalizer. Failed frontier gates use the original complete
validators for the same typed diagnostics and order; successful operations do
not restore a global validation pass. Output checking follows all rewrites, so
retired intermediate nodes and later canonical failures keep their original
semantics. Both tests compare the independent full-route error and complete
rollback, starting from checked valid Builder input.

## Complete layout representation

The projection now retains root-local dense rectangles/constraints, a shared
immutable topology and a complete identity-sorted directory. Queries compare the
full generational NodeId. Complete split minima, maxima, extents and gaps live in
flat arrays, with borrowed `SplitProjection` slices. The solver writes into the
owned output segment and reuses temporary scratch rather than giving each split
its own output buffers. All logical values, constraints, overflow, unallocated
extent and complete output lifetimes remain observable.

The reference test implementation retains the old numerical helpers and old
BTreeMap/Vec output representation independently. Randomized bit comparisons,
raw malformed diagnostics, sparse/recycled identity checks and simultaneous old
outputs qualify the new representation. The 10,000-level regression was moved
into the actually registered library test module; an unregistered legacy copy
was not counted as coverage. It projects, reads and drops all 20,001 node results
and 10,000 complete split outputs after dropping the workspace.

## Verification and remaining attribution

Ordinary, counters-enabled serial checks passed: 32 focused tests, 2,035
production core tests, four no-default-feature public integrations, 2,211 full
workspace tests, workspace correctness/suspicious Clippy, strict documentation,
workspace doctests, formatting and the renderer-neutral dependency boundary.
The three opt-in performance probes were skipped by normal suites; the separate
work probe passed. Source hashes match the formal frozen candidate. Independent
transaction correction and layout reviews found no remaining blocker in their
inspected scope. Full commands, logs, source/binary receipts and reviews are in
the archive; [checks.json](2026-11-07-sealed-layout/checks.json) lists the gates.

The residual cost still warrants structural changes: request capture and live
checking construct root-sized values; candidates deep-copy every node/MRU;
edge parent queries rediscover paths; cold layout rebuilds a visited directory,
looks up each child again and converts fresh leaf inputs. Complete export also
performs keyed rectangle lookups. These are implementation choices, not a proved
Rust or renderer-neutral lower bound. The next slices should use sealed runtime
ownership to avoid rediscovery, retain the raw diagnostic path, and measure
owner-issued references and smaller candidate copies separately. No twofold
completion or irreducible limit is claimed here.

The earlier CI allocation failure was infrastructure-only; its retry and the
expanded native-test/cancel-close coverage commit `aa3c99b` both passed all six
jobs. This local runtime checkpoint still requires its own pushed-head CI run.

The [evidence archive](2026-11-07-sealed-layout/evidence.tar.xz) includes 16,128
raw rows and 336 complete vectors across both comparisons, checks, exact source
receipts and the reconstructible patch/new files. Its manifest and every member
were read back byte for byte. [SHA256SUMS](2026-11-07-sealed-layout/SHA256SUMS)
records the published evidence hashes. Executables are identified by hash and
kept locally rather than committed.
