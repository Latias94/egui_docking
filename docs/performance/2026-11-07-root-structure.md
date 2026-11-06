# Share the graph's actual split payloads with layout

The graph now completes an immutable root structure from its actual NodeStore
payloads. Split rows share the same `Arc<Node>` allocation as the live arena;
tabs rows retain only complete generational IDs. Layout reads that owned structure
and computes fresh measurements and complete owned results. Remove the runtime
topology compiler, copied axis/weight-bit representation, `OnceLock/matches`
reuse chain, cache transfer and their obsolete call sites.

RootStructure retains root-local row joins, postorder, sorted identity lookup and
split/output offsets. Those indexes refer to the single immutable split payload,
rather than keeping another editable child roster or weight source. Current
central metadata and constraints remain live per-call inputs. Existing public
`Workspace::node`, NodeMap/SplitMap reads, all result slices and complete NodeId
semantics remain. Old projections own their historical structure after every
Workspace has been destroyed.

Concrete writers withdraw a root's current body before changing split payloads.
Successful completion installs the actual new body and placements together.
Nonempty center moves, selection, reorder, presentation changes and rehome share
unchanged structures. Real edge/removal/resize edits rebuild only their affected
roots. Same-value Resize skips mutable access before COW. Item completion runs
after local collapse/flatten, weight diagnostics and placement repair. A normalized
equal general Move restores only original command references, preserving its
current body and exact arena payload pairing.

Move prefixes complete source and distinct target, Open and donors complete their
explicit root, and birth constructs the actual new root. They add no peer-root
scan per command. The final full canonicalizer temporarily owns all bodies,
restores only actually unchanged roots and completes the remaining bodies once
its original validation succeeds. General structural prefixes may still rebuild
again after final canonicalization. A failed candidate remains isolated and its
withdrawn private body cannot be projected. The new UnfinishedRoot error is an
internal classification, not an alternative authorization path.

This changes where structural ownership lives. Builder/import construction still
precedes the primary operation timer, just as fixture construction did before.
Every real mutation's body reconstruction and disposal of the replaced live
Workspace remain inside checked mutation. Fresh leaf collection, N actual leaf
answers, all-root numeric projection, rich output and fresh complete export keep
their existing timers. No prewarmed layout call, reduced workload or deferred
publication substitutes for that work.

There are genuine costs. A changed root rebuilds all its rows, shared split handles
and placements, then sorts its identity directory. Immutable graph bodies consume
memory before the first layout and retain historical split payloads while outputs
live. The full candidate allocator/maps still fork. Neither zero fingerprints nor
shared center bodies imply constant-cost mutation or zero graph traversal.

The ordinary work probe retains its previous 20 columns and appends successful
body completions, constructed rows and shared split handles. The hook runs once
after a successful graph-owned build. Failed partial construction, raw diagnostic
adapters, allocator events, capacity and additional placement maintenance are
excluded. A successful build subsequently rolled back still counts. The ledger
is separate from the unchanged formal timing schema and disabled through the
existing test-only bypass for timing.

Independent reviews cover the exact original 16-path candidate and the three-file
counter augmentation. The first compiler run found one missing Arc qualification
in a migrated test; the one-line `std::sync::Arc::clone` correction preserves all
assertions, and its failed receipt/log is retained. Subsequent counters-enabled
qualification passes 408 focused cases, four work regressions, 2,064 production
core, 2,240 workspace, four no-default public integrations and 292/335 public host
tests. The three opt-in probes remain skipped in ordinary suites. Formatting,
correctness/suspicious Clippy, strict docs, doctests and dependency checks pass.
Both registered 10,000-depth tests pass. The existing production recovery-scale
test reports about 0.007 seconds versus the previous coarse 0.008 seconds; this
does not establish a paired cold-path speedup or an exact mutation budget.

The independent solver/traversal oracle, numeric core, override validator and
formal docking measurement are unchanged. The ordinary 21-cell work probe
confirms zero successful body builds for resize, center and cross-view center.
Each edge builds one affected root, with `2 * (panes / views) + 1` rows and
`panes / views` shared split handles. This counts body output, not all graph
walks, allocations or placement maintenance.

## Complete paired measurements

The frozen candidate binary is SHA-256
`d0facb66482a23377bcdaad5e3bb5be04de9fe6ee35f521a51d78e24cfdf9ba2`.
Its control is the actual `80403cb` cold-effects binary, SHA-256
`daf9156d7f2b87612f05abd3c364097d06e09c42dc8a32a1c352b4206a759a21`.
Each pairing separately runs ABBA and BAAB, 48 samples and eight warmups per
process/cell. Each group therefore includes 96 observations per condition for
every original 21-cell workload. Request capture, checked mutation, every root's
fresh numeric layout and fresh complete owned export retain their timer bounds.

Against `80403cb`, complete-operation p50 improves in 36 of 42 group/cells,
ties once and regresses in five. The full reduction range is -15.48% to +45.23%.
At 1024 panes, resize/center/cross-view center improve 24.96–30.91%, and four-view
edge improves 14.28–16.23%. Single-view edge regresses 10.84–15.48%; its smaller
16/128-pane counterparts range from a tie to a 4.29% regression. The complete
p95 reduction range is -46.50% to +48.30%, with six negative cells. The max
reduction range is -221.30% to +93.31%, with 14 negatives. These tails and all
raw samples remain in the archive.

One group illustrates the edge cost boundary. At 1024 panes and one view,
layout p50 changes from 111,791 ns to 74,166 ns, while mutation changes from
18,791 ns to 79,084 ns. The independently measured complete-operation p50
changes from 145,875 ns to 168,458 ns. This localizes the regression to mutation;
it does not measure how much of that phase belongs to row construction,
placement insertion, directory sorting, Arc ownership or other calls. Phase
medians are not added to manufacture a total.

The same full protocol compares pinned ImGui
`d88f238d1d363ef387382b2e29ad12cf96f826ac`:

| Total panes | Complete-operation p50 / ImGui, both groups | Group/cells at most 2x |
| --- | --- | --- |
| 16 | 2.60–5.47x | 0 / 14 |
| 128 | 1.44–2.27x | 7 / 14 |
| 1024 | 1.18–1.58x | 14 / 14 |

All 42 p50 and p95 ratios remain slower than ImGui; 27 max ratios are slower.
The twofold goal is still incomplete. This checkpoint is retained for shared
structural ownership and its broad measured gains, with the single-view edge
regression explicitly carried forward. Further work must address both small
root layout costs and changed-root completion rather than treating this as a
uniform performance win.

## Reproducibility

[comparisons.csv](2026-11-07-root-structure/comparisons.csv) includes every
group/cell's complete-operation p50, p95 and max for both pairings.
[evidence.tar.xz](2026-11-07-root-structure/evidence.tar.xz) contains 16,128 raw
rows, 336 complete vectors, all original run receipts, qualification logs,
successful-build work counts, the failed initial compile attempt, the reviewed
original patch and exact changed control/candidate file pairs. Every complete
vector and digest agrees; both binaries remain unchanged. Each member was read
back and compared with its input bytes.

The original author's set-hash format text says lexical order, but its set hash
uses the existing manifest-array order. Independent review reproduces that
order and verifies each file hash and the patch. The frozen original receipt is
retained with this explicit correction; the formal build receipt independently
binds every measured source file.
