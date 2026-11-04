# Complete host-update sampling methodology

This document specifies the opt-in `frame_cost` example and metric definitions.
The [2026-11-04 frozen-source results](2026-11-04-engine-first-results.md) record
actual measurements and their limits separately from this methodology.

## Run and sample controls

Run from the repository root, using the independent host-harness manifest:

```text
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --release --locked --example frame_cost -- --samples 24 --warmup 4 --in-flight 3 --panes 16,128,1024 > frame-cost.csv
```

For a small functional run, use `--samples 2 --warmup 0 --in-flight 1 --panes 16`.
The CLI accepts positive sample counts, a nonnegative warmup count, a positive
number of older in-flight outputs, and a subset of the three supported pane
counts. Defaults are 24 measured samples, four warmup samples, three older
in-flight outputs, and all three pane counts. Each timing and heap pass gets its
own session and the same warmup/sample counts. `--help` writes help to stderr and
does not start a profiler. Stdout contains CSV only; the example writes no profile
files. There are no wall-clock pass/fail thresholds.

Use `--timing-only` to omit the active-dhat phase when collecting CPU samples.
This preserves every workload and ordered recording operation, and emits only
the timing rows. The default still runs both timing and tracked-heap phases.

Use a release build. Record the exact commit and uncommitted diff or source hashes,
Rust version, build flags, target, OS, CPU, power settings, other workloads, command,
and complete CSV when collecting actual results. A maximum is the largest observed
sample, not a bound on future latency. Small defaults support local exploration;
increase sample counts and repeat independent processes when studying tails.

## Fixture and workloads

Each case uses one headless logical surface with 16, 128, or 1,024 leaf panes in a
balanced binary tree whose split axes alternate. Every pane has two tabs, so the
item counts are 32, 256, and 2,048. Initial surface bounds are 16,384 by 16,384
logical units, pane minimums are 64 by 48, and uniform tab intrinsic width is 64.
These bounds keep the panes visible instead of measuring a mostly clipped layout.

The example owns its `RecordingSink`, using the harness's existing public
`OwnedRecord` format without changing shared fixtures. It consumes the complete
ordered public paint program, owns geometry/content/receiver/feedback records,
reaches `SurfaceEnd`, and obtains the execution receipt. Content callbacks record
pane content bounds and selected item identity; there is no application rendering,
text shaping, GPU submission, native window, or OS presentation work.

| Scenario | Input and measured work |
| --- | --- |
| `idle` | No semantic input; resupply unchanged measurements and encode a complete output. This is a painted idle update, not an unpainted early exit. |
| `select` | Alternate the selected tab in the first pane through `select_item_current`, then measure and encode the selected candidate. |
| `drag` | Move an active root horizontal splitter by 64 logical units, alternating direction between samples. A press update and its presentation settlement establish the gesture outside the measured windows. The measured update resolves a current public `host_pointer_hit`, submits the move with mouse metadata, measures and encodes the active preview, and commits. A release update and settlement finish the gesture outside the windows. |
| `measurement_change` | Alternate surface width between 16,384 and 16,448 and tab intrinsic width between 64 and 72. Encode the changed measurement in the same update without a preceding measurement-only commit. |
| `outputs_in_flight` | Before the measured update, retain K older outputs, each produced by changing selection and measurements and retaining its complete owned recording. Measure another changed update with these exact outputs still outstanding. K defaults to three. |

Drag starts from a receiver bound to the last presented output, away from splitter
junctions. Every pointer edge obtains current delivery facts and route authority
from the public API. The timed drag sample is one motion update; it does not divide
an entire gesture's time by its number of events. It measures splitter dragging,
not tab docking, native drag transfer, or contained-window dragging.

The session output capacity and host pending vector are reserved for K + 1 outputs
in every case. The in-flight case uses all these slots without testing admission
failure or backpressure. Prefill is outside the per-update time and allocation
delta windows; its retained allocations remain live and observable.

## Measurement windows and output ownership

An `update` window includes `begin_host_frame`, input resolution where applicable,
`measure_surface`, `surface_work`, `execute_surface_program`, receipt confirmation,
the single publishing `commit`, report consumption and destruction, correctness
checks, and retention of the matching permit and owned records. Construction of
the fixture and reservation of sample storage are outside these windows.

The update ends with one output in flight in ordinary cases, or K + 1 in the
in-flight case. These snapshots have `end_state=retained_in_flight`; the CSV records
both `outputs_before` and `outputs_after`.

A separate `settlement` window reports all pending permits as `Presented` in
reverse emission order, drops their recording payloads, and commits an explicitly
unpainted host frame to settle the exact output prefix. Checks verify that the
newest artifact is the presented artifact. It emits no replacement output. These
snapshots have `end_state=settled` and zero in-flight outputs, while the session
still owns the last presented scene, persistent state and reusable capacity.
For `drag`, "settled" refers to output ownership; the gesture is still active at
that snapshot and is released afterward. Headless `Presented` reports establish
public API authority; they do not claim real display presentation.

Both windows include the example's bookkeeping and correctness assertions. CSV
aggregation, sorting, formatting and writes occur after the relevant profiler has
been dropped and outside timing windows. The retained sample vector is reserved
before profiling and never grows during measured samples.

## Timing and heap instrumentation

The only global allocator declaration is the safe construction
`static ALLOCATOR: dhat::Alloc = dhat::Alloc` with `#[global_allocator]`.
`dhat = "=0.3.3"` is a development dependency; no local `GlobalAlloc` implementation,
unsafe support crate, or relaxation of `unsafe_code = "forbid"` is needed.

Each pane-count/scenario case has two separate passes:

1. `instrumentation=dhat_inactive`: use `Instant` around each window, with no active
   profiler and no `HeapStats` queries. The installed dhat allocator still wraps
   the system allocator and incurs its inactive bookkeeping/locking overhead;
   these times are not an uninstrumented `System` baseline.
2. `instrumentation=dhat_active`: start one
   `Profiler::builder().testing().trim_backtraces(Some(4)).build()` before constructing
   the session, retain it through setup, warmup and all samples, and query
   `HeapStats::get()` at window boundaries. `testing()` suppresses output files and
   summaries, but does not disable allocation tracking or stack collection. Four
   frames is dhat's minimum configured backtrace length. This pass emits heap
   metrics only, never timings distorted by active backtracing.

There is no profiler per frame, and no resetting of profiler state between samples.
Timing and heap rows are separate observations and are not paired samples whose
columns should be combined into a per-frame correlation.

| CSV metric | Meaning |
| --- | --- |
| `time_ns` | Elapsed wall-clock nanoseconds per complete update or settlement, from the inactive pass. |
| `allocation_events` | Difference in `HeapStats.total_blocks` across the window. In dhat 0.3.3, successful reallocations count as events as well as successful allocations. These are not counts of failed allocator requests. |
| `requested_bytes` | Difference in `HeapStats.total_bytes`. Each successful allocation contributes its requested layout size; each successful reallocation contributes its entire new requested size, not only growth. This is cumulative allocation traffic, not retained memory. |
| `live_before_tracked_bytes` | `HeapStats.curr_bytes` immediately before the window. |
| `live_after_tracked_bytes` | `HeapStats.curr_bytes` immediately after the window. Retained and settled rows describe different ownership checkpoints. |
| `case_prefix_peak_tracked_bytes` | `HeapStats.max_bytes` through that checkpoint, cumulatively since this case's profiler started. It includes session construction, initial output/settlement, warmup, in-flight prefill, drag setup/cleanup and earlier samples. It is not the peak of the current window. |

dhat exposes no safe API to reset the peak. Do not subtract two `max_bytes` values
and call the result a window peak, or interpret a settled prefix peak as memory
still live after settlement. A large setup peak can dominate both phases. The CSV
uses the explicit `case_prefix_peak_tracked_bytes` name to preserve this scope.

Live and peak values are totals of tracked requested heap sizes, not an exact
deep-size of the docking engine or any one output. Profiling starts before the
fixture is constructed, so its state, scene owners, queues, recording vectors and
their spare capacity are included. Other Rust allocations made while that profiler
is active can also contribute; the counters are process-wide, not type-tagged.
Pre-profiler allocations (including sample storage and already initialized runtime
or stdout buffers) are not tracked unless subsequently reallocated. dhat excludes
its own recursive bookkeeping allocations from its counters even though they still
consume real memory and affect runtime behavior.

System allocator metadata, size-class rounding, fragmentation, allocator-internal
transient overlap during `realloc`, stacks, static data, code, memory mappings,
foreign allocators, GPU resources and OS resident memory are outside these byte
totals. Freeing tracked allocations does not imply an equal RSS decrease. Runtime
initialization, compiler allocation elision and allocator behavior can change
observations. Compare like-for-like builds and workloads; these counters do not
establish a cross-library performance comparison or a precise object deep-size.

## CSV interpretation

Each row identifies pane count, tabs per pane, scenario, instrumentation, phase,
ownership checkpoint, sample/warmup counts, outstanding output counts and metric.
For each metric, `p50` and `p95` use the nearest-rank definition: sort N values and
select one-based rank `ceil(N * percentile / 100)`. `max` is the largest of those N
observations. Heap prefix-peak percentiles summarize cumulative high-water-mark
checkpoints, not independent per-window peak observations. Time values are integers
in nanoseconds; allocation events are counts; byte metrics are bytes.

The runnable example enforces only workload correctness: expected durable changes,
an active drag during motion, terminal splitter delivery on release, one complete
encoded permit matching `SurfaceEnd`, all pane content records, and the newest
artifact's exact settlement. It does not assert a performance budget. Actual
measurement, environmental qualification and storage of the resulting CSV belong
to the thread that owns Cargo execution.
