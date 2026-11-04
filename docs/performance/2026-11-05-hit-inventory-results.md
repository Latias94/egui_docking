# Hit inventory compilation measurements

Date: 2026-11-05. Base: `c62d757`. Implementation: `d7d8b65`.

## Method

Both complete runs use 12 samples and three warmups, 16/128/1,024 panes, two
tabs per pane, and three older outputs in the retained-output workload. The
complete ordered recording, pointer authority checks and reverse settlement
remain unchanged. Timing uses the inactive-dhat release allocator; active heap
samples are separate. No Cargo build/test or CPU sampler from this session runs
concurrently with the complete measurements. The shared Apple M4 desktop is not
CPU-isolated. With 12 samples p95 equals max; these observations do not establish
tail latency, GPU frame rates or physical window behavior.

- [Fresh complete baseline CSV](2026-11-05-hit-inventory-baseline.csv)
- [Final complete CSV](2026-11-05-hit-inventory-results.csv)
- [Environment, commands and executable digests](2026-11-05-hit-inventory-environment.json)
- [Exact source inventory](2026-11-05-hit-inventory-source.sha256)
- [Workload definitions](2026-11-03-engine-first-methodology.md)

## Complete update time at 1,024 panes

| Workload | Baseline p50, ms | Final p50, ms | Reduction | Final p95/max, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 3.030 | 2.997 | 1.1% | 4.091 |
| `select` | 25.495 | 22.364 | 12.3% | 23.508 |
| `drag` | 22.478 | 19.598 | 12.8% | 21.128 |
| `measurement_change` | 21.988 | 18.733 | 14.8% | 19.368 |
| `outputs_in_flight` | 26.205 | 22.898 | 12.6% | 23.446 |

Idle retains exactly the same allocation traffic. Its small p50 change and wider
p95 in this shared-desktop run are not evidence of a separate idle optimization.
The intended reduction is in changed-update compilation.

At 128 panes the changed-workload p50 values fall from 2.007–2.466 ms to
1.701–2.175 ms. At 16 panes they fall from 0.193–0.270 ms to 0.171–0.229 ms,
but idle p50 rises from 0.048 to 0.055 ms and in-flight p95 rises from 0.337 to
0.442 ms. These short samples do not establish a general tail-latency improvement.

## Allocation traffic and retained heap

| Workload | Baseline allocations | Final allocations | Baseline requested MiB | Final requested MiB |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 4,840 | 4,840 | 17.451 | 17.451 |
| `select` | 133,762 | 133,013 | 94.492 | 85.614 |
| `drag` | 80,980 | 80,231 | 88.941 | 80.063 |
| `measurement_change` | 73,058 | 72,309 | 88.239 | 79.362 |
| `outputs_in_flight` | 133,782 | 133,033 | 94.495 | 85.617 |

Changed updates request 9.4–10.1% fewer cumulative bytes. The reduced traffic
comes from temporary compilation inventories; final retained checkpoints match
the baseline exactly at all measured pane counts.

| Panes | Baseline four outputs, MiB | Final four outputs, MiB | Baseline settled, MiB | Final settled, MiB |
| --- | ---: | ---: | ---: | ---: |
| 16 | 1.499 | 1.499 | 0.234 | 0.234 |
| 128 | 11.738 | 11.738 | 1.704 | 1.704 |
| 1,024 | 93.891 | 93.891 | 13.528 | 13.528 |

Requested traffic is cumulative allocation volume, not retained memory or RSS.
The CSV also preserves case-prefix peaks, which include setup, warmup and prior
samples rather than a single update. The temporary index permutation adds one
small allocation for unsorted input, while avoiding large-record sort movement
and the old full-ID touch-fragment intermediate inventory.

## Attribution and changes

Fresh CPU-only sampling of the unchanged workload identified repeated full-ID
comparisons while joining touch fragments back to hit rows, large-record sorting,
and guide-catalog lookups. A separate measurement-change-only sample confirmed
that repeated guide lookups occur outside drag setup as well. Sampling is a
selection aid; the combined benchmark does not attribute a speedup to any one
change.

Touch compilation now emits private input-region rows. A single stable pass
compacts hit records and remaps fragments together, preserving multiple fragments
and touch-only records whose mouse area is zero. Invalid row order or range
prevents manifest construction. Full identities, output tickets and immutable
manifest ownership remain the public lookup authority. There is no published
intermediate inventory or second mutable cache.

Canonical hit sorting orders a temporary `usize` permutation using the same full
identity comparator, then moves the sole large-record arena in place. Each cycle
of length K needs K-1 record swaps. The temporary permutation marks its own visited
rows and is dropped after sorting; no second record arena is created. Already
sorted input needs no permutation. Ordered touch adjacency is consumed by its
allocation prefix or protected suffix, preserving every effective pair and cut
order and the existing dense fallback.

Guide overlap validation reads each guide's target after every slot has proved
exact catalog Arc ownership, slot, layer and geometry. The nested pair order and
earlier slot-error priority remain unchanged. Canonicalization rebinds guides
against the existing sorted target arena after availability COW, eliminating a
second temporary map while keeping older plans' owners unchanged.

## Verification

The focused run passes 25 cases. New regression tests compare full sorted records
and geometry endpoint bits, exercise permutation direction and independent cycles,
and cover empty-row filtering, touch-only rows and malformed private indices.
Guide tests preserve slot rejection before overlap, first-collision order,
stale-before-policy availability and exact new/old Arc ownership after COW.
Existing complete touch fixtures retain their ordered endpoint-bit differential
checks against the frozen subtraction implementation.

Fresh serial gates pass on the measured source:

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,081 passed |
| Independent host, default | 292 passed, including all three 10,000-depth cases |
| Independent host, persistence | 332 passed; already-covered depth binary filtered |
| Official egui | 24 passed |
| Product egui, default / persistence | 114 / 119 passed |
| Fork-backed native, all features and targets | 556 passed; one pre-existing ignored case |
| Workspace/native correctness and suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed |
| Workspace documentation examples | Ten passed |
| Format, diff and renderer-neutral dependency checks | Passed |

Builds use one job and tests use one nextest worker, reusing existing targets.
An initial formatting check exposed Rust 2021 formatting in the touched test
file; applying repository Rust 2024 formatting fixed it. Both final formatting
checks pass. There were no failing behavioral tests in this round.

Three independent read-only reviews found no evidenced P1/P2 in the implemented
changes, including a final review of adjacency category skipping. No physical GUI
qualification is claimed for this internal compilation change.

## Remaining limits

Changed-update p50 remains 18.733–22.898 ms at 1,024 panes, above a 16.7 ms budget
before application rendering, text shaping, GPU submission and physical windows.
Idle and retained scene payloads remain essentially unchanged. Any further
optimization needs new attribution of this reduced implementation; the previous
hotspot distribution is no longer sufficient evidence for the next change.
