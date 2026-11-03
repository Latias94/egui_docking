# Complete host-update samples — 2026-11-04

These are measurements of the frozen engine-first redesign, not an ImGui
comparison or a renderer frame-rate claim. The functional, public and actual
editor gates passed before this run. Source provenance includes newly added
files, not only a Git diff against the baseline.

- [Methodology and metric scope](2026-11-03-engine-first-methodology.md)
- [Complete 180-row CSV](2026-11-04-engine-first-results.csv)
- [Environment, command, compiler and source digest](2026-11-04-engine-first-environment.json)
- [Frozen 540-input SHA-256 inventory](2026-11-04-engine-first-source.sha256)

The release build ran on an Apple M4 with 16 GiB RAM, macOS 27, Rust 1.95.0,
on AC power with low-power mode disabled. This was a shared desktop, without
CPU isolation. All cases use two tabs per pane, four warmups and 24 samples in
each separate timing and heap pass. Three older outputs remain outstanding in
`outputs_in_flight`; its measured update ends with four outputs outstanding.
The command and exact start/end UTC timestamps are in the environment record.
The complete command, including compilation and active-heap profiling, took
488.115 seconds; that duration is not a frame cost.

## Complete update times

Units are milliseconds. Each cell is p50 / p95 / largest observed value from
the inactive-dhat pass. The installed allocator still has inactive bookkeeping
overhead. Times include input, measurement, full ordered-program recording,
publication, report consumption and retained recording checks, but exclude
fixture setup, application rendering/text shaping, GPU work and native windows.

| Panes | Workload | p50 ms | p95 ms | max ms |
| --- | --- | ---: | ---: | ---: |
| 16 | `idle` | 0.183 | 0.206 | 0.317 |
| 16 | `select` | 0.275 | 0.297 | 0.328 |
| 16 | `drag` | 0.255 | 0.347 | 0.413 |
| 16 | `measurement_change` | 0.224 | 0.231 | 0.233 |
| 16 | `outputs_in_flight` | 0.279 | 0.288 | 0.289 |
| 128 | `idle` | 1.900 | 2.070 | 2.529 |
| 128 | `select` | 4.307 | 4.509 | 4.563 |
| 128 | `drag` | 4.541 | 4.985 | 4.996 |
| 128 | `measurement_change` | 3.919 | 7.241 | 11.199 |
| 128 | `outputs_in_flight` | 4.303 | 4.519 | 4.727 |
| 1,024 | `idle` | 30.561 | 31.750 | 32.736 |
| 1,024 | `select` | 134.948 | 136.760 | 137.129 |
| 1,024 | `drag` | 143.704 | 145.541 | 147.516 |
| 1,024 | `measurement_change` | 129.317 | 130.562 | 130.674 |
| 1,024 | `outputs_in_flight` | 134.408 | 138.931 | 142.722 |

## Allocation traffic and live ownership

These are separate active-dhat observations. Requested bytes count successful
allocation and reallocation requests, including the entire new size of a
reallocation. They are not retained bytes or RSS. Live values include the
session, scenes, queues, host recordings and spare capacity tracked by this
profiler. MiB means 1,048,576 bytes. All columns below are p50 checkpoints;
the complete CSV also contains p95/max and cumulative case-prefix peaks.

| Panes | Workload | Update allocation events | Update requested MiB | Live with output(s) MiB | Live after settlement MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 1,380 | 0.920 | 0.415 | 0.293 |
| 16 | `select` | 3,265 | 1.446 | 0.670 | 0.293 |
| 16 | `drag` | 2,947 | 1.410 | 0.678 | 0.304 |
| 16 | `measurement_change` | 2,091 | 1.325 | 0.667 | 0.293 |
| 16 | `outputs_in_flight` | 3,285 | 1.449 | 1.802 | 0.294 |
| 128 | `idle` | 8,848 | 6.688 | 3.286 | 2.317 |
| 128 | `select` | 26,453 | 13.600 | 5.441 | 2.317 |
| 128 | `drag` | 36,633 | 14.420 | 5.461 | 2.361 |
| 128 | `measurement_change` | 18,209 | 12.826 | 5.417 | 2.317 |
| 128 | `outputs_in_flight` | 26,473 | 13.604 | 14.813 | 2.318 |
| 1,024 | `idle` | 68,409 | 53.359 | 26.024 | 18.273 |
| 1,024 | `select` | 204,615 | 105.277 | 43.112 | 18.273 |
| 1,024 | `drag` | 418,482 | 124.106 | 43.159 | 18.515 |
| 1,024 | `measurement_change` | 140,089 | 99.218 | 42.917 | 18.273 |
| 1,024 | `outputs_in_flight` | 204,635 | 105.280 | 117.629 | 18.274 |

## Interpretation and next work

The 16-pane reference costs remain below 0.42 ms in this sample. At 128 panes,
ordinary mutation/measurement p50 is 3.92–4.54 ms; the measurement-change maximum
is 11.20 ms on this shared host. At 1,024 panes, painted idle alone takes
30.56 ms p50, and changed updates take 129.32–143.70 ms p50. This profile does
not meet a 60 Hz editor frame budget at that scale. No latency acceptance
threshold was part of the redesign, and passing correctness tests does not
make these costs acceptable for a target engine.

The growth is more than proportional to pane count for changed updates.
The measurements do not attribute that growth to a specific function. The next
performance task should profile the complete seam, then reduce repeated
layout/receiver/scene compilation and allocation work for unchanged subtrees.
It should retain the single document, output ledger and lifecycle owner rather
than adding another cache with independent validity rules. Measure that change
against this exact workload and a real engine workload before making a speedup
claim.

For 1,024 panes, four in-flight outputs retain 117.63 MiB of tracked heap,
falling to 18.27 MiB after exact reverse-order settlement. The case-prefix peak
is 119.10 MiB, includes setup/warmup and prior samples, and is not a per-update
peak. Output admission is bounded, but large scenes and host recordings still
have material memory cost. A host should choose its capacity and recording
strategy from this evidence; the default capacity is a count limit, not a byte
budget.

Settlement timing and all five heap metrics remain in the CSV. This sample
establishes a reproducible baseline for complete host updates and output
ownership; it does not certify another CPU, platform, renderer, content workload,
or a performance equivalence with ImGui.
