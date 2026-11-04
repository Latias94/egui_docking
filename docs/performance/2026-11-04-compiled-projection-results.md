# Compiled-projection scaling measurements

Date: 2026-11-04. Base: `d4bdafe`. Implementation: `131043e`.

## Comparable workload

Both runs use 12 measured samples and three warmups, the same five complete-update
workloads at 16, 128 and 1,024 panes, and three older retained outputs in the
in-flight case. Every pane still contains two tabs. The complete ordered
`OwnedRecord` recording format, payloads and settlement order are unchanged.

- [Fresh baseline CSV](2026-11-04-compiled-projection-baseline.csv)
- [Refactored CSV](2026-11-04-compiled-projection-results.csv)
- [Compiler, machine, commands and executable digests](2026-11-04-compiled-projection-environment.json)
- [Refactored source inventory](2026-11-04-compiled-projection-source.sha256)
- [Workload and metric definitions](2026-11-03-engine-first-methodology.md)

Timing is from release binaries with inactive dhat bookkeeping. Active heap
tracking runs separately. Compilation and CPU sampling are outside the reported
timing windows. This is a shared Apple M4 desktop with no CPU isolation; no
concurrent Cargo was started by this session during either complete run. With
12 samples, p95 is the largest sample, not a reliable latency bound. Results are
local observations, not a cross-machine or GPU frame-rate guarantee.

## Complete update time at 1,024 panes

| Workload | Baseline p50, ms | Refactored p50, ms | Reduction | Refactored p95/max, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 29.468 | 3.142 | 89.3% | 3.534 |
| `select` | 51.746 | 30.893 | 40.3% | 31.712 |
| `drag` | 60.974 | 40.474 | 33.6% | 40.936 |
| `measurement_change` | 48.461 | 26.864 | 44.6% | 27.202 |
| `outputs_in_flight` | 53.073 | 31.455 | 40.7% | 31.887 |

## Tracked heap at the in-flight checkpoint

Requested heap bytes include the session, immutable scenes, host recordings and
spare capacity. They do not represent RSS or GPU memory. Four outputs are retained
after the measured update and zero after settlement.

| Panes | Baseline retained, MiB | Refactored retained, MiB | Baseline settled, MiB | Refactored settled, MiB |
| --- | ---: | ---: | ---: | ---: |
| 16 | 2.045 | 1.499 | 0.343 | 0.234 |
| 128 | 16.405 | 11.737 | 2.637 | 1.703 |
| 1,024 | 127.551 | 93.889 | 20.259 | 13.527 |

At 1,024 panes the retained checkpoint falls from 127.551 to 93.889 MiB (26.4%).
After settlement it falls from 20.259 to 13.527 MiB. The complete CSV also retains
allocation traffic and settlement costs. The case-prefix peak includes setup,
warmups and prior samples; it is not a per-update peak.

## Attribution and implementation

CPU-only sampling identified repeated program traversal and identity joins.
A normal two-tab leaf creates three tab-gap instructions, each of which previously
scanned the whole target catalog on every traversal. Tab operability also scanned
the bar catalog. Hit and semantic compilation and paint preflight/execution
repeated these joins. The final program consumes build identities once after
canonical sorting and executes private record rows. The dynamic paint preflight
and accepting-sink receipt checks remain. Tab/bar queries use canonical search.

A separate one-sample active-dhat backtrace run confirmed that host recording,
hit regions and touch fragments were significant owners. Measured record sizes
were 248 bytes for `OwnedRecord` and receiver descriptors, 112 bytes for visual
IDs and 80 bytes for semantic output tickets. The recording format stays intact.
The engine stores each touch fragment as a private row plus rectangle, freezes
hit/semantic inventories to exact-length slices, shares measurement maps across
snapshots, and shares resize arrays among all gaps of one split. Unchanged drop
target availability no longer forces a copy merely to canonicalize it.

Identical ordinary Ready inputs reuse the existing immutable plan before full
compilation. The guard still compares the exact ticket, coordinates, complete
measurements, config, global popup context, resize inputs and normalized contained
projection. It remains a Ready contribution through the existing reducer; each
actual paint still gets its own emission. Native first-live does not take this
fast path. No independent scene cache, graph or presentation ledger was added.

## Remaining limits

Changed updates still take 26.864–40.474 ms p50 at 1,024 panes, exceeding a 16.7 ms
frame budget before application rendering, text shaping or GPU work. The remaining
full compilation, validation and touch partition work needs fresh attribution
before choosing another architectural change. Idle remains proportional to exact
measurement input and complete recording size. Four genuinely different outputs
still need their historical geometry until the existing settlement rules release
it; this implementation does not claim constant-memory history.

## Verification

The measured implementation passed the all-feature/all-target workspace suite
(2,062 cases), the independent host suite (292 default, including all three
10,000-depth cases; 332 persistence cases with those already-covered depth cases
filtered), official egui (24), product egui (114 default / 119 persistence), and
the fork-backed native adapter (556; one pre-existing ignored case). Workspace
and native correctness/suspicious Clippy and strict documentation passed, as did
the ten workspace documentation examples, formatting and renderer-neutral
dependency boundary. Three independent read-only reviews found no evidenced
P1/P2 in the final production changes. Physical GUI qualification was not rerun
for this internal data and traversal change.
