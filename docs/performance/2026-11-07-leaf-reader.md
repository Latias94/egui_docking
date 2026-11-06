# Explicit leaf answers without a forced ordered map

This candidate changes the cold complete-constraint layout entry to consume a
`FnMut(NodeId) -> Option<Constraints>` reader. It keeps one explicit answer for
every actual leaf and retains the reader only during the call. Keyed callers
adapt their existing map with `get`; the minimum-only entry and complete entry
share the same sealed-runtime compiler. There is no new provider framework,
retained reader, topology prewarm or compatibility executor.

In the renderer-free docking probe, the original complete Workspace DFS now
materializes a fresh Vec of `(complete NodeId, Constraints)` rows. The reader
consumes each row only when its full identity matches the actual DFS query.
Traversal, vector allocation/growth, every explicit leaf answer and consumption
remain within the original layout interval. Request, checked mutation, complete
rich projection and fresh complete owned export retain their prior boundaries.

This interface is breaking for the internal complete-constraint entry. Its
contract states once-per-leaf left-to-right DFS order. Missing answers stop at
the exact original leaf Enter. Earlier subtree aggregate errors still occur
before later missing leaves, and public overrides are validated before the
first callback. The minimum-only entry retains its existing opposite order:
measurement precedes override projection checks. The independent frozen
reference, numeric solver, raw draft diagnostics and rich output are unchanged.

The comparison retains the original total panes/view/operation matrix, fixed
ImGui pin, full state-vector equality and two serial ABBA/BAAB groups with 48
samples/eight warmups. The baseline executable is the qualified exact-owner
candidate at `ecf78ba`. The new source also includes the separately committed
resize-output registration fix `5089b4f`; that scene-compiler path is outside
the renderer-free operation. Both exact source and binary receipts are retained.

The new registered tests exercise nonuniform, n-ary, central and recycled
identities, exact callback order and consumption, first/middle/last missing
answers, override precedence and aggregate-error precedence. All original
bitwise oracle, complete rich-output, old-output and registered 10,000-level
tests remain. Two legacy files are mechanically migrated but remain unregistered
under `autotests = false`; no coverage claim relies on them.

Both frozen comparisons preserve every complete state vector and sampled
digest. At 1,024 panes, all complete-operation medians improve 6.74–18.52%
against `ecf78ba`. In that paired comparison, current layout phase medians are
about 108–113 microseconds. Request, mutation and export work remain present.
Phase medians are reported independently and are not an additive budget.

The improvement is not uniform. Seven of 42 group/cell medians regress, by up
to 5.78%; five are 16-pane cells and two are 128-pane cells. Four-view 16-pane
resize regresses in both groups, so the small-input cost cannot be dismissed as
one isolated tail. Eight p95 cells and sixteen max cells regress; the largest
regressions are 123.64% and 586.05% respectively. All observations are retained.
The interface removes the forced map and improves the largest workload, while
small-root fixed costs remain an explicit optimization target.

| Total panes | Complete-operation p50 / ImGui, all cells and groups | Cells at or below 2x |
| --- | --- | --- |
| 16 | 4.60–8.87x | 0/14 |
| 128 | 2.55–4.85x | 0/14 |
| 1,024 | 2.25–3.66x | 0/14 |

Counters-enabled checks on the final candidate pass formatting, 2,224 workspace
tests, four no-default-feature public integrations, correctness/suspicious
Clippy, strict documentation, doctests and the dependency boundary. The earlier
112 focused tests also pass; their receipt precedes the separate scene-compiler
ordering fix. The final complete suite covers that fix, and the independent
public host harness passes 292 no-feature and 335 persistence-feature tests.
The ordinary 21-cell work probe preserves zero root-value builds/item scans and
one full candidate clone per move. No new production-profile full-suite run is
claimed for this slice; the frozen production probe checks all original state
vectors, and the numeric solver/reference are unchanged.

Independent [source review](2026-11-07-leaf-reader/leaf-export-review.md) finds no
remaining blocker in the reader or workload preservation. The
[evidence archive](2026-11-07-leaf-reader/evidence.tar.xz) contains 425 input
files, 16,128 raw rows and 336 complete vectors, exact source/binary/check
receipts, the reconstructible patch and retained CI red/green logs. Every
manifest member was read back byte for byte.
[Comparisons](2026-11-07-leaf-reader/comparisons.csv) include all tails and
negative cells; [SHA256SUMS](2026-11-07-leaf-reader/SHA256SUMS) records the artifact
hashes. The goal remains active. Next isolate solver aggregate reuse, report
ownership, sequential complete-output reads and candidate payload sharing.
