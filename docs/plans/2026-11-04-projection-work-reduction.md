# Projection work reduction

Date: 2026-11-04. Status: completed. Base: `32a1428`. Implementation: `af22b8f`.

## Outcome

Reduce the remaining complete changed-update cost at 1,024 panes, using the same
ordered recording, measurement and retained-output workloads as the preceding
[compiled-projection qualification](../performance/2026-11-04-compiled-projection-results.md).
Preserve exact authority, complete structural proofs, geometry, error ordering,
atomic publication and native first-live ownership. No second graph, cache or
presentation ledger is introduced. Preserve pre-existing CI and review files.

## Attribution and implementation

- [x] Verify the current release executable against the prior measurement.
  SHA-256: `c6a6af35e1be4451d59b723c11e4d72ab0a983db58ebe8c46445c34db85aa10f`.
- [x] Sample current complete updates without active heap instrumentation.
  The eight-second CPU sample identifies repeated source-proof reconstruction in
  `RootMeasurement::project`, rectangle subtraction buffer churn, and semantic
  uniqueness/record validation as concrete candidates.
- [x] Reuse the existing exact workspace index and one local root proof when
  validating a batch of split overrides. Keep each source's membership, complete
  fingerprint, duplicate, shape and normalization checks in their existing order.
- [x] Remove rectangle-subtraction allocation churn while retaining every ordered
  fragment and bitwise endpoint calculation. Reuse compile-local chrome/base work
  buffers; keep protected cuts, occlusion and receiver precedence unchanged.
- [x] Reduce measured validation allocation work without weakening its rejection
  cases. Do not treat Arc equality as a missing optimization: it already has a
  pointer fast path for Eq payloads.
- [x] Run differential geometry regressions, public host and workspace/native
  consumers serially. Obtain independent read-only review, record full baseline
  and final timing/heap evidence, and make scoped local commits.

Raw attribution and experiment output lives outside the checkout under
`/tmp/dockspace-scale2-01a1a5fb/`. Final measurements and source fingerprints belong
in `docs/performance/`. Main-thread Cargo remains serial with one build job and
the existing target directories. No push or pull request is included.

## Measurement and verification checkpoint

The [complete comparable samples](../performance/2026-11-04-projection-work-results.md)
record changed-update p50 of 22.319–26.305 ms at 1,024 panes. Drag changes from
39.566 to 22.755 ms, with allocation events from 454,420 to 80,980. Other changed
updates improve 15–16%. Idle and retained-output heap remain essentially unchanged.
The complete recordings, workload definition and history are unchanged.

All 2,071 workspace tests passed. The 26 focused cases include exact bitwise
touch subtraction/compilation comparisons, foreign/stale/missing source rejection,
proof work counts and wide-split defect cases. Default public host: 292 passed,
including the three 10,000-depth cases. All consumer/static gates passed serially. Three independent read-only reviews found no evidenced P1/P2.

The public non-indexed projection still uses per-source subtree membership scans
(O(K*N)); it now builds the actual root fingerprint only once per call. The
compiler reuses its existing exact index for both membership and proof. Further
public-path indexing requires its own measurement; no new registry is introduced.

## Final qualification

All serial gates passed: workspace 2,071; public host 292/332; official egui 24;
product egui 114/119; native 556 with one pre-existing ignored case. All three
10,000-depth cases passed under the default host; the persistence run filtered
that already-covered binary. Workspace/native correctness and suspicious Clippy,
strict documentation, ten doctests, format and dependency-boundary checks passed.
The exact 547-file inventory matches the measured implementation. Physical GUI
qualification was not rerun.

The next investigation should sample this reduced implementation again. Remaining
changed updates are 22–26 ms before application/GPU work; present-output heap and
idle work remain essentially unchanged. Do not infer another unmeasured redesign
from the superseded hotspot distribution or replace the complete recording with
a counting sink to improve the reported cost.
