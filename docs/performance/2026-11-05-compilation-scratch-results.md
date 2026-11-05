# Compilation scratch work measurements

Date: 2026-11-05. Base: `4484e40`. Implementation: `5130dba`.

## Method

Both complete runs use 12 samples and three warmups at 16/128/1,024 panes, two
tabs per pane and three older outputs in the retained-output workload. Timing
uses the inactive-dhat release allocator; active heap samples run separately.
Complete ordered records, all content callbacks, output authority checks and
reverse settlement remain unchanged. No Cargo build/test or CPU sampling started
by this session overlaps the complete measurements. The shared Apple M4 desktop
is not CPU-isolated; power settings are not controlled. With 12 samples p95 is
the observed maximum, not a tail-latency bound.

- [Complete fresh baseline CSV](2026-11-05-compilation-scratch-baseline.csv)
- [Complete final CSV](2026-11-05-compilation-scratch-results.csv)
- [Earlier complete baseline CSV](2026-11-05-compilation-scratch-baseline-initial.csv)
- [All interleaved small-workload confirmation rows](2026-11-05-compilation-scratch-small-confirmation.csv)
- [Environment, commands and digests](2026-11-05-compilation-scratch-environment.json)
- [Exact source inventory](2026-11-05-compilation-scratch-source.sha256)
- [Unchanged workload definitions](2026-11-03-engine-first-methodology.md)

The initial baseline and final run ended up more than seven hours apart. A repeat
of the frozen baseline immediately after the final run checks shared-desktop
drift. The primary comparison below uses that adjacent repeat; the initial CSV
and all timestamps remain available. At 1,024 panes changed-update p50 differs
by at most 1.2% between the two baselines, with identical allocation counters.
The candidate ran before the primary baseline, rather than randomizing run order.

## Complete update time at 1,024 panes

| Workload | Baseline p50, ms | Final p50, ms | Reduction | Final p95/max, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 3.008 | 3.036 | -0.9% | 3.290 |
| `select` | 22.522 | 20.554 | 8.7% | 23.739 |
| `drag` | 19.516 | 17.593 | 9.9% | 18.582 |
| `measurement_change` | 18.928 | 16.990 | 10.2% | 17.447 |
| `outputs_in_flight` | 22.995 | 21.147 | 8.0% | 21.770 |

Idle has identical allocation traffic and no intended computation change; its
small timing difference is within this shared-desktop observation. These samples
do not establish a general tail-latency improvement.

The complete CSV also retains smaller-workload regressions. At 128 panes,
measurement-change p50 rises from 1.710 to 1.768 ms (3.4%) and p95 rises from
1.935 to 3.106 ms; the other changed-workload p50 values fall 3.4–7.8%. At
16 panes all changed p50 values fall, but measurement-change p95 rises from
0.193 to 0.303 ms. No universal per-case or tail improvement is claimed.

After Cargo validation completed, four additional timing-only processes ran in
baseline/final/final/baseline order at 16/128 panes, with 64 samples and four
warmups each. At 128 panes measurement-change p50 is 1.651/1.626 ms in the two
baselines and 1.488/1.500 ms in the two final runs; the earlier p50 regression does
not reproduce. All 80 timing rows, including other workloads and settlement,
remain in the linked confirmation CSV. These checks do not replace the original
complete-run observations or prove a tail bound.

## Allocation traffic and retained heap

| Workload | Baseline allocations | Final allocations | Baseline requested MiB | Final requested MiB |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 4,840 | 4,840 | 17.451 | 17.451 |
| `select` | 133,013 | 131,189 | 85.614 | 80.355 |
| `drag` | 80,231 | 78,407 | 80.063 | 74.804 |
| `measurement_change` | 72,309 | 70,485 | 79.362 | 74.103 |
| `outputs_in_flight` | 133,033 | 131,209 | 85.617 | 80.358 |

Changed-update requested allocation traffic falls 6.1–6.6%. Requested bytes are
cumulative allocation volume, not retained heap or RSS. All live-before and
live-after p50 checkpoints match the baseline exactly across every workload and
pane count. Four-output tracked heap remains 1.499/11.738/93.891 MiB at
16/128/1,024 panes; after settlement it remains 0.234/1.704/13.528 MiB. The CSV
also preserves case-prefix peaks, which include setup, warmup and previous
samples rather than one update's peak.

## Attribution and changes

Fresh CPU-only sampling covered all five workloads. A separate process repeated
only measurement changes, retaining the same update and complete recording code,
to distinguish compiler costs from drag press setup. Both profiles identify hit
compilation, magnetic catalog construction and surface-program uniqueness scratch.
These profiles select work; the combined result does not attribute gains to
individual edits. Global `memmove` samples are not treated as hit-record-copy
cost: they also include rectangle subtraction and unrelated paths.

Touch compilation borrows each immutable hit record rather than copying it into
the temporary allocation roster. Handles reuse their first splitter lookup for
the anchor. A call-local lookup checks the complete splitter ID order once and
uses binary search only when it is strictly increasing. Unsorted or duplicate
inventories retain the previous linear first match; missing records retain their
existing handle-skip or junction-base fallback behavior. Surviving base fragments
flow directly into the final row inventory after the same ordered cuts.

Surface-program uniqueness sets reserve from their existing input rosters and
store immutable references. Hash and equality still use complete values, with
the same category order and first-observed duplicate. The sets are released after
their last check. Stable paint ordering, block construction, linked instructions
and publication authority are unchanged; references never enter the final tape.

Magnetic validation collects expected root, pane and guide target IDs in one
temporary vector, sorts and deduplicates it, then compares every ID and both
iterator ends with the existing ordered target index. This replaces three
transient ordered sets. Every preceding identity, ownership, geometry, fraction,
depth and target check remains in place, including its rejection order.

## Verification

The focused run passes 29 cases. New tests compare splitter lookup owners and
ordered touch endpoint bits for canonical, reversed, rotated, duplicate and
missing inventories, including complete root/node/gap identities. Existing full
touch fixtures additionally compare with forced linear lookup and frozen ordered
rectangle subtraction. A surface-program regression proves duplicate-category
priority, the first duplicate instead of the smallest ID, unchanged rejected
drafts and repair/retry. A structurally valid extra target must still fail the
final magnetic catalog comparison with guides enabled or disabled.

Fresh serial gates pass on the measured source, with one build job, one nextest
worker and existing targets reused:

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,085 passed |
| Independent host, default | 292 passed, including all three 10,000-depth cases |
| Independent host, persistence | 332 passed; already-covered depth binary filtered |
| Official egui | 24 passed |
| Product egui, default / persistence | 114 / 119 passed |
| Fork-backed native, all features and targets | 556 passed; one pre-existing ignored case |
| Workspace/native correctness and suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed |
| Workspace documentation examples | Ten passed |
| Format, diff and renderer-neutral dependency checks | Passed |

Two independent read-only reviewers cover validator/program and touch changes
separately from their implementers and find no evidenced P1/P2. No behavioral
test failure occurred in this round. The measured production source did not
change after its release build.

## Remaining limits

These are headless logical-surface measurements. Application content rendering,
text shaping, GPU work and real window presentation are outside the measured
scope. Physical GUI qualification is not rerun for this internal compilation
change. Synthetic noncanonical splitter inventories intentionally retain linear
lookup, and the public standalone splitter getter is unchanged.

Final changed-update p50 remains 16.990–21.147 ms at 1,024 panes, still above a
16.7 ms budget before application rendering and GPU work. Further optimization
needs fresh attribution of the reduced implementation; no new persistent cache,
alternate graph or shortened recording workload was introduced.
