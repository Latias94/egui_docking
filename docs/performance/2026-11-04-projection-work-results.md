# Projection work reduction measurements

Date: 2026-11-04. Base: `32a1428`. Implementation: `af22b8f`.

## Method

Both complete runs use 12 samples and three warmups, 16/128/1,024 panes, two
tabs per pane, and three older outputs in the retained-output workload. The
complete ordered recording and reverse settlement remain unchanged. Timing uses
the inactive-dhat release allocator; active heap samples are separate. No Cargo
or CPU sampler ran concurrently with the reported benchmark from this session.
The shared Apple M4 desktop is not CPU-isolated. With 12 samples p95 equals max;
these are local observations, not tail-latency or GPU frame-rate bounds.

- [Fresh baseline CSV](2026-11-04-projection-work-baseline.csv)
- [Final complete CSV](2026-11-04-projection-work-results.csv)
- [Environment, commands and executable digests](2026-11-04-projection-work-environment.json)
- [Exact final source inventory](2026-11-04-projection-work-source.sha256)
- [Workload definitions](2026-11-03-engine-first-methodology.md)

## Complete update time at 1,024 panes

| Workload | Baseline p50, ms | Final p50, ms | Reduction | Final p95/max, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 3.027 | 3.096 | -2.3% | 3.575 |
| `select` | 30.277 | 25.601 | 15.4% | 26.256 |
| `drag` | 39.566 | 22.755 | 42.5% | 23.506 |
| `measurement_change` | 26.652 | 22.319 | 16.3% | 26.991 |
| `outputs_in_flight` | 31.305 | 26.305 | 16.0% | 26.939 |

Idle has the same allocation count and requested bytes; its 2.3% timing increase
is small in this shared-desktop sample. The intended reduction is in changed
updates. No per-change causal attribution is inferred from the combined result.

## Allocation traffic at 1,024 panes

| Workload | Baseline allocations | Final allocations | Baseline requested MiB | Final requested MiB |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 4,840 | 4,840 | 17.451 | 17.451 |
| `select` | 238,503 | 133,762 | 133.043 | 94.492 |
| `drag` | 454,420 | 80,980 | 151.899 | 88.941 |
| `measurement_change` | 176,023 | 73,058 | 127.007 | 88.239 |
| `outputs_in_flight` | 238,523 | 133,782 | 133.046 | 94.495 |

Drag-update allocation events fall 82.2%, while requested allocation traffic
falls 41.4%. Other changed workloads also allocate less. Requested traffic is
cumulative allocation volume, not retained memory or RSS.

## Retained heap

| Panes | Baseline four outputs, MiB | Final four outputs, MiB | Baseline settled, MiB | Final settled, MiB |
| --- | ---: | ---: | ---: | ---: |
| 16 | 1.499 | 1.499 | 0.234 | 0.234 |
| 128 | 11.737 | 11.738 | 1.703 | 1.704 |
| 1,024 | 93.889 | 93.891 | 13.527 | 13.528 |

At 1,024 panes the retained checkpoints differ by about 1 KiB; this change
reduces construction work and traffic rather than historical output payloads.
The full CSV retains case-prefix peaks, which include setup, warmups and prior
samples rather than one update alone. Rectangle subtraction now retains an
intermediate high-water capacity until the current compile phase ends; it can
temporarily use more capacity for an individual cut, even while reducing repeated
allocations across cuts. Final inventories remain exact-length immutable slices.

## Changes and proof obligations

The CPU-only sample showed the compiler repeatedly rebuilding a root fingerprint
for each split override. Compiled projection now uses the existing exact
WorkspaceIndex and version. Every source still proves existence and membership;
the complete fingerprint is compared once for each distinct immutable proof in
that projection. A call-local identity set only records already-verified proofs.
The public projection path builds its actual root fingerprint once, but preserves
its existing per-source subtree membership scan. It still has an O(K*N) membership
cost without an index; the compiler path uses indexed membership. No persistent
cache, graph or authority registry was added.

Touch subtraction appends the transformed suffix in its existing Vec, moves it
into place and truncates. It preserves the untouched prefix and exact ordered
top/bottom/left/right fragments. Chrome visibility and base-restoration buffers
are reused only inside the current compile. Test-only differential checks compare
the old subtraction algorithm with every cut and existing complete touch fixture
using ordered IDs and exact endpoint bits, including empty and extreme geometry.

Semantic uniqueness uses sorted temporary vectors while retaining the smallest
duplicate ID and record-class rejection order. Existing private workspace node
membership uses a HashSet only for exact insert/contains; no hash-set iteration
participates in canonical fingerprint order. A wide split reuses array-validity
proofs only for identical immutable slices and lengths; every gap still checks its
own geometry, axis, constraints, membership, central index and operability.

## Remaining limits

Changed-update p50 remains 22.319–26.305 ms at 1,024 panes, above a 16.7 ms budget
before application rendering, text shaping, GPU submission and physical windows.
Idle and retained scene payloads are essentially unchanged. Further changes need
fresh attribution of the reduced implementation rather than assuming the previous
hotspot distribution still applies. No physical GUI qualification is claimed.

## Verification

Fresh serial gates passed on the measured source: 2,071 workspace tests; public
host 292 default cases including all three 10,000-depth cases, plus 332 persistence
cases with that already-covered depth binary filtered; official egui 24; product
egui 114/119; fork-backed native 556 with one pre-existing ignored case. Workspace
and native correctness/suspicious Clippy, strict docs, ten documentation examples,
formatting, diff checks and renderer-neutral dependency validation passed.
Three independent read-only reviews found no evidenced P1/P2. Physical GUI
qualification was not rerun for this internal computation/allocation change.
