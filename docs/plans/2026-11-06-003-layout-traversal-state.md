# One owner for layout traversal state

The checked-move refactor at `fa5fb1e` meets its move latency target, but its
resize measurements include a small repeated one-view shift. Resize never
executes the changed transaction code. Preserve those results and investigate
the preparation and layout phases separately.

## Diagnosis and bounded change

Use the frozen binaries for an unchanged-history resize comparison, and a
separate single-executable control with interleaved current and historical
fixture preparation. These controls retain fresh fixtures, complete all-root
projection, owned exports and outside-timer validation. Controls cannot replace
the original formal matrix or prove a unique cause merely from unchanged source.

Layout compilation currently owns two hash collections: completed node rows and
the set of nodes on the active traversal. Its existing measurement vector already
distinguishes an entered, unfinished row (`None`) from a completed row (`Some`).
Keep one node-to-row map and use that existing distinction for repeat visits.
This removes the second collection and its duplicate hashing without adding a
cache, skipping validation, changing public types or reducing projection output.

Preserve the original depth-first order: completed rows can be reused, an active
row is a cycle, missing leaf constraints still precede later structural failures,
and exit requires every child measurement. Topology hints must keep their exact
same-version structural checks. No transaction, canonicalization, rollback,
solver, output or benchmark-boundary change belongs in this slice.

## Verification and acceptance

- Exercise a completed shared subtree followed by a back-edge, including the
  missing-constraint precedence, against the existing frozen traversal oracle.
- Run the existing layout properties and all-feature core tests with ordinary
  counters, formatting and applicable integration/documentation checks.
- Freeze a production timing executable with counters disabled. Run two serial
  ABBA/BAAB groups of the unchanged full 16/128/1024 by 1/4-view matrix against
  both the original baseline and the submitted checked-move executable.
- Retain complete state equality and same-execution phase sums. The original
  baseline's 1024-pane center cells must still improve at least 20% in each
  group, edge/cross must improve, and resize must have no unexplained repeated
  regression in the final candidate.
- Obtain independent source and evidence review, record negative results,
  commit only owned changes, and push the existing branch.

## Result

The [qualified result](../performance/2026-11-07-layout-traversal.md) retains all
historical and current negative observations. Against both frozen baselines,
every resize p50 improves in both complete groups. The original baseline's
1,024-pane center reductions are 44.68%/44.56% at one view and 28.27%/28.17% at
four views; all original-baseline move cells improve. Incremental smaller move
cells and tail statistics remain mixed and are reported explicitly. Ordinary
verification passes 2,024 core, four integration and nine documentation tests.
