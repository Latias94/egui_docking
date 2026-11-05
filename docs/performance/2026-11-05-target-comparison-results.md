# Target comparison and guide rebinding measurements

Date: 2026-11-05. Base: `133bf9c`. Implementation: `a3451df`.

## Method

The comparable complete runs use 12 samples and three warmups, 16/128/1,024
panes, two tabs per pane and three older retained outputs. Complete ordered
OwnedRecord payloads, content callbacks, authority checks and reverse settlement
are unchanged. Timing uses the inactive-dhat release allocator; active heap
samples are separate. No Cargo build/test or CPU sampler started by this session
overlaps the complete measurements. The Apple M4 shared desktop is not isolated
and power settings are uncontrolled. With 12 samples p95 equals the observed
maximum; it is not a tail bound or GPU frame-rate measurement.

- [Complete baseline CSV](2026-11-05-target-comparison-baseline.csv)
- [Complete final CSV](2026-11-05-target-comparison-results.csv)
- [All interleaved confirmation rows](2026-11-05-target-comparison-confirmation.csv)
- [Environment, commands and executable digests](2026-11-05-target-comparison-environment.json)
- [Exact source inventory](2026-11-05-target-comparison-source.sha256)
- [Unchanged workload definitions](2026-11-03-engine-first-methodology.md)

## Complete update time at 1,024 panes

| Workload | Baseline p50, ms | Final p50, ms | Reduction | Final p95/max, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 3.062 | 2.964 | 3.2% | 3.461 |
| `select` | 20.547 | 20.106 | 2.1% | 27.123 |
| `drag` | 17.490 | 17.076 | 2.4% | 18.171 |
| `measurement_change` | 16.783 | 16.366 | 2.5% | 16.823 |
| `outputs_in_flight` | 21.237 | 20.864 | 1.8% | 53.645 |

This is a small changed-update p50 improvement on a shared desktop. Idle has no
intended computation change, so its 3.2% difference is a reminder of timing
variability. Selection p95 rises from 21.192 to 27.123 ms and in-flight p95 from
22.061 to 53.645 ms; no general tail-latency improvement is claimed.

To check the few-percent difference, four additional timing-only processes run
in baseline/final/final/baseline order at 1,024 panes, with 48 samples and four
warmups each. Neither Cargo nor CPU sampling overlaps these runs. All 40 timing
rows, including settlement, remain in the confirmation CSV.

| Workload | Baseline 1 p50, ms | Final 1 p50, ms | Final 2 p50, ms | Baseline 2 p50, ms |
| --- | ---: | ---: | ---: | ---: |
| `idle` | 2.979 | 2.922 | 2.934 | 3.026 |
| `select` | 20.559 | 19.945 | 20.107 | 20.845 |
| `drag` | 17.260 | 16.785 | 16.981 | 17.573 |
| `measurement_change` | 16.613 | 16.065 | 16.239 | 17.620 |
| `outputs_in_flight` | 20.691 | 20.366 | 20.476 | 21.028 |

Both final p50 values are lower than both baselines for every changed workload
in this check. Baseline drift remains visible, especially for measurement
changes; this confirms the observed direction without establishing a universal
percentage or future latency bound. In the complete smaller-workload sample,
16-pane selection p50 rises from 0.206 to 0.219 ms, and some 128-pane p95 values
also regress. Those observations are retained in the complete CSVs.

## Heap and retained outputs

Every active-heap metric, including all p50/p95/max allocation-event, requested
byte, live-checkpoint and case-prefix peak values, matches the baseline exactly
across all cases. This change reduces computation, not allocation volume or
retained history. At 1,024 panes the ordinary changed-update requested traffic
remains 74.103–80.355 MiB. Four-output tracked heap remains 93.891 MiB and the
settled checkpoint 13.528 MiB.

Requested bytes describe cumulative allocation volume rather than retained heap
or RSS. Case-prefix peaks include setup, warmup and preceding samples, not one
update's peak. Complete ordered recording is unchanged.

## Attribution and changes

Fresh CPU sampling of the current workload command and a separate process that
repeats only measurement changes identify target comparison and guide rebinding
costs. The separate case avoids attributing drag press setup to the timed changed
update. Sampling selects this work; combined benchmark results do not attribute
the gain of each individual edit.

The target comparator matches borrowed same-variant fields and compares them
lexicographically, stopping after the first difference. It compares variant ranks
only for different variants. The explicit variant order, surface/root/node/gap
field priority and four-edge order remain unchanged. Node comparison uses the
existing complete generational NodeId order, without repacking opaque IDs or
changing public types, equality, hashing or serialization.

Every guide still proves catalog membership and exact Arc ownership before
canonicalization. Sorting moves only Arc handles. If availability canonicalization
changes no target, the existing owners and guide ID/hit/preview snapshots remain
valid and need no rebind. A local flag records any actual availability change;
when set, the entire previous binary-search rebind loop still runs after COW.
Old plans keep their owners and availability. No persistent flag, cache, mutable
registry, alternative graph or authority is added.

## Verification

Six focused tests pass. The independent frozen pre-change comparator checks all
84,681 pairs of 291 mixed identities for comparison, PartialOrd, equality and
antisymmetry. It includes every variant, full field priority, null nodes,
same-slot node generations, maximal stable IDs, all edges and maximal gap index.
Mixed reversed/rotated/duplicate sorting and deduplication match the frozen order.
The reference uses its own explicit variant and edge ranking helpers.

The no-change guide regression reverses the catalog and validates twice while
retaining the old plan, checking original Arc owners, exact guide bindings and
unchanged hit/preview snapshots. Existing regressions retain stale-before-policy
behavior, COW rebinding, old-output independence and slot-error priority.

Fresh serial gates pass on the measured source with one build job and one
nextest worker, reusing existing targets:

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,088 passed |
| Independent host, default | 292 passed, including all three 10,000-depth cases |
| Independent host, persistence | 332 passed; already-covered depth binary filtered |
| Official egui | 24 passed |
| Product egui, default / persistence | 114 / 119 passed |
| Fork-backed native, all features and targets | 556 passed; one pre-existing ignored case |
| Workspace/native correctness and suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed |
| Workspace documentation examples | Ten passed |
| Format, diff and renderer-neutral dependency checks | Passed |

Two crossed independent read-only reviews find no evidenced P1/P2 in the final
comparator and guide-owner changes. No behavioral test failure occurred in this
round. The measured source remained unchanged after the release build.

## Remaining limits

Application rendering, text shaping, GPU work and physical windows remain outside
this headless update benchmark. Physical GUI qualification is not rerun. Further
optimization requires new attribution of the reduced implementation; complete
validation and output recording remain part of the workload.

The complete 1,024-pane measurement-change p50 reaches 16.366 ms, while its
observed maximum is 16.823 ms. Other changed-update p50 values remain
17.076–20.864 ms. This does not establish a 16.7 ms complete-frame budget before
application content rendering, text shaping and GPU work.
