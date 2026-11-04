# Boundary-repair complete-update samples

Date: 2026-11-04. Implementation: `48b00d8`, `b50d0e6`.

The touch compiler now filters disjoint allocation/chrome pairs before exact
partitioning. Dense overlaps retain exhaustive behavior with bounded neighbor
storage. Normalization also preserves unchanged validated split weights.
These results characterize the complete repaired implementation; they do not
isolate a single function or compare against ImGui.

- [Methodology and metric definitions](2026-11-03-engine-first-methodology.md)
- [Complete 180-row CSV](2026-11-04-boundary-repair-results.csv)
- [Compiler, host, command and source digest](2026-11-04-boundary-repair-environment.json)
- [Exact measured source inventory](2026-11-04-boundary-repair-source.sha256)

The release example ran with 12 samples and three warmups per case and per
instrumentation phase. Three older outputs are retained in the in-flight case;
its measured update ends with four. It uses a shared Apple M4 desktop, no CPU
isolation and no concurrent Cargo started by this session during sampling.
With 12 samples, the reported p95 is the largest observation; this is a coarse
tail sample, not a latency bound. Compilation is outside the measured windows.

## Complete update times

Times include the public update/measurement/ordered-recording/publication seam,
not application rendering, text shaping, GPU submission or native windows. The
inactive dhat allocator still has bookkeeping overhead. Units are milliseconds.

| Panes | Workload | p50 ms | p95 ms | max ms |
| --- | --- | ---: | ---: | ---: |
| 16 | `idle` | 0.174 | 0.207 | 0.207 |
| 16 | `select` | 0.273 | 0.294 | 0.294 |
| 16 | `drag` | 0.244 | 0.258 | 0.258 |
| 16 | `measurement_change` | 0.210 | 0.240 | 0.240 |
| 16 | `outputs_in_flight` | 0.263 | 0.283 | 0.283 |
| 128 | `idle` | 1.768 | 1.836 | 1.836 |
| 128 | `select` | 3.118 | 3.692 | 3.692 |
| 128 | `drag` | 3.373 | 3.762 | 3.762 |
| 128 | `measurement_change` | 2.680 | 3.099 | 3.099 |
| 128 | `outputs_in_flight` | 3.144 | 3.760 | 3.760 |
| 1,024 | `idle` | 29.411 | 30.394 | 30.394 |
| 1,024 | `select` | 51.824 | 52.997 | 52.997 |
| 1,024 | `drag` | 61.181 | 65.027 | 65.027 |
| 1,024 | `measurement_change` | 48.337 | 50.160 | 50.160 |
| 1,024 | `outputs_in_flight` | 53.904 | 55.887 | 55.887 |

The [previous frozen implementation](2026-11-04-engine-first-results.md) used
24 samples and four warmups: its 1,024-pane changed-update p50 was
129.317–143.704 ms. The repaired implementation records 48.337–61.181 ms on the
same workload definition. The implementations and sample counts differ, and the
shared desktop is not controlled, so this is not a causal microbenchmark claim.

The 1,024-pane result still exceeds a 16.7 ms frame budget. Painted idle alone is
29.411 ms p50. The remaining full-scene work and scene/record retention must be
profiled before claiming real-time qualification at that scale. Keep validity
and lifetime tied to the existing document and immutable output owners when
adding reuse; a second independently mutable scene cache is not justified by
these measurements.

## Tracked heap ownership

These p50 values come from the separate active-dhat phase, not the timing run.
They include session state, immutable scenes, host recordings and spare capacity.
They are tracked requested heap bytes, not RSS, GPU memory or a deep-size of core.

| Panes | Four outputs retained, MiB | After settlement, MiB |
| --- | ---: | ---: |
| 16 | 2.045 | 0.343 |
| 128 | 16.405 | 2.637 |
| 1,024 | 127.551 | 20.259 |

For 1,024 panes the four-output retained checkpoint is 127.551 MiB, falling to
20.259 MiB after reverse-order settlement. The previous frozen implementation
recorded 117.629 / 18.274 MiB. The repair improves update time while retaining
more memory; this remains an explicit scaling cost. Allocation traffic,
settlement times and cumulative case-prefix peaks remain in the CSV. The peak
includes setup/warmup/prior samples and is not a per-update peak.
