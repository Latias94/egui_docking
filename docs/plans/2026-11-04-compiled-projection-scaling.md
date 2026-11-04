# Compiled projection scaling

Date: 2026-11-04. Status: completed. Base: `d4bdafe`. Implementation: `131043e`.

## Outcome and constraints

Reduce the complete 1,024-pane update cost and retained-output heap cost using
measured attribution. The existing public `frame_cost` workloads and complete
ordered recording remain the comparison surface. Breaking internal or public
changes are allowed when they produce the clearer ownership model.

Preserve the sole canonical graph, transaction draft and presentation ledger.
Immutable projections belong to their existing output owners. Sharing data never
shares input authority: each output retains exact tickets, coordinate capture and
ordered settlement. Keep rollback, stale-input rejection, popup ownership and
native first-live guarantees. Do not introduce a second independently mutable
scene cache. Preserve the pre-existing CI and `docs/reviews/` changes. Commit only
owned files locally; do not push or open a PR.

## Evidence and work

- [x] Verify the frozen baseline executable and its source inventory.
  The binary SHA-256 is
  `43d485b50e755fc2cc4b0e3b4ba7a067474df5e2e7d03f41b042ef5214a1515c`.
  The 542-file inventory matches the current base.
- [x] Attribute timing without active heap-profiler backtraces, and attribute
  retained heap to actual owners and record layouts.
- [x] Compare three independent read-only designs for the compiled-projection
  interface; select the smallest design justified by those measurements.
- [x] Replace the measured repeated work or duplicated storage at its owner;
  remove superseded paths rather than retaining two authorities.
- [x] Maintain public regressions for ownership, stale input and complete output
  traversal. Workspace: 2,062 passed. Public host default: 292 passed, including
  all three 10,000-depth cases. Additional consumers and static gates run serially
  with one Cargo build job.
- [x] Repeat complete timing and heap samples on the final source, record exact
  inputs and limitations, obtain independent review and commit the owned files.

The prior [baseline](../performance/2026-11-04-boundary-repair-results.md)
records 29.411 ms painted idle, 48.337–61.181 ms changed-update p50 and
127.551 MiB retained with four outputs at 1,024 panes. The first sampling run
includes active dhat backtraces and cannot alone attribute uninstrumented CPU
cost; `--timing-only` preserves the workloads while omitting that phase.

Raw experimental output is kept outside the checkout under
`/tmp/dockspace-scale-01a1a5fb/`. Final reproducible evidence belongs under
`docs/performance/`; this plan records durable decisions and completion state.

## Selected design

Keep the existing immutable `SurfacePlanScene` ownership. Consume the program's
identity-based build tape once after canonical sorting, linking it to private
record rows. Production traversal has one linked tape; temporary build identities
are released. Dynamic paint preflight and affine receipts remain in place.

The more flexible geometry/state decomposition is deferred: selection can change
effective minima, strip reveal and target preconditions, so partial reuse would
need additional evidence. A retained-output interface is also deferred; changing
the benchmark to a counting sink or dropping owned records would invalidate the
comparison. The complete renderer record format stays unchanged.

Share immutable measurement maps and one pair of resize arrays per split.
Store touch fragments as private row references into their owning immutable hit
manifest, after filtering, rather than repeating the full structural identity.
Freeze final hit/semantic vectors to exact-length slices. Retain full semantic
identities and every fragment; a private row never grants input authority.

An exact-input fast path may reuse the existing Ready plan before compilation.
It must first validate the token/ticket/policy/coordinates, then compare complete
measurements/config, global popup context, current resize source/weights and the
normalized contained projection. Ready alone is insufficient for same-frame
transients. Native first-live continues its current compilation/admission path.

## Measured checkpoint

The [comparable complete measurements](../performance/2026-11-04-compiled-projection-results.md)
record 3.142 ms idle p50 at 1,024 panes (29.468 ms baseline),
26.864–40.474 ms changed-update p50 (48.461–60.974 ms baseline), and
93.889 MiB retained with four outputs (127.551 MiB baseline). Complete recordings
and all workloads are unchanged. Changed updates still exceed 16.7 ms before
application/GPU work; this checkpoint does not claim real-time qualification.

Three independent read-only reviews found no evidenced P1/P2 in the final
production changes. The full workspace first exposed two outdated compiler-count
assertions and one raw-plan test helper; their updates retain exact geometry,
old-output isolation and complete program validation. Focused reruns and the
complete 2,062-case workspace rerun passed.

## Final qualification

All serial gates passed: workspace 2,062; public host 292 default and 332
persistence (the three depth cases already covered in default were filtered in
persistence); official egui 24; product 114/119; native 556 with one pre-existing
ignored case. Workspace/native correctness and suspicious Clippy, strict docs,
ten workspace doctests, format and dependency-boundary checks passed. Physical
GUI qualification was not rerun. Source inventory: 545 matching inputs.

The next independent optimization should attribute the remaining complete
compilation, validation and touch partition costs. Preserve the same complete
recording and retained-output workload when evaluating any further sharing.
