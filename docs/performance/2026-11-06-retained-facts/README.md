# Retained-facts qualification evidence

This archive supports the [result and migration report](../2026-11-06-retained-facts.md) and [executed plan](../../plans/2026-11-06-001-dockspace-retained-facts-refactor.md).

Baseline: `34ab23adf8062960298abbc5fbcba370104f3b86`, runtime `54840717a2454c957b13a667a939ddf8229e52e9`. Final precommit source snapshot: `4d874443ffdae2220c70401fe6f46335d004823983887dd09b6e0bc13dcfbc20`. There are 565 frozen Rust/build/workload inputs and 41 changed inputs. Git build labels are precommit labels; the recorded content hashes identify the actual validated tree.

| Artifact | Meaning |
| --- | --- |
| `baseline-provenance.json.gz` | Frozen baseline source/workload/reference hashes, binary digests and prior qualification receipts |
| `state.json.gz` | Final source hashes and exact changed-path scope; the snapshot digest hashes `json.dumps(state["source_sha256"], indent=2) + "\n"` as UTF-8 |
| `checks.json`, `checks/*.log.gz` | Eleven successful serial checks, all bound to the final snapshot, with complete output |
| `qualification-commands.json`, `builds/*.log.gz` | Actual final build/reference commands, times and build output |
| `review-outcomes.json` | Peer review report hashes, closed source findings and exact final snapshot relationship |
| `implementation-review.md`, `simplification-review.md` | Actual diff review and subsequent fix reviews; historical findings are followed by explicit closure |
| `design-reviews/` | Three independent design alternatives and the original plan review |
| `frozen-product/raw.csv.gz` | Every formal baseline/candidate product sample, labeled by group/run/condition |
| `frozen-product/summary.csv`, `environment.json` | Recomputed product p50/p95/max, binary hashes, order, commands and timing receipts |
| `candidate-product/` | Separate full-product candidate/ImGui comparison, raw samples, summaries, pinned sources and build provenance |
| `headless/process-summaries.csv.gz` | All formal renderer-free process p50/p95/max values, all scales/cases and update/settlement phases |
| `headless/allocations-*.csv.gz` | Separate active-DHAT allocation/boundary-live/prefix-peak summaries; inactive timing rows here do not qualify latency |
| `headless/environment.json` | Headless build flags, binary hashes, all process commands and their serial start/end receipts |
| `resize-followup/` | Separate 128-pane resize ABBA check with 96 samples/12 warmups per process; it does not replace the formal groups |
| `profiles/` | Separate eight-second CPU captures, their receipts, recorded workload rows and inclusive-category summaries |
| `in-flight-followup.md` | Read-only branch/dependency trace for the mixed in-flight/measurement-change workload |
| `evidence-review.md` | Independent final arithmetic/provenance review, added after audit completion |

Product formal groups are ABBA then BAAB, with 24 samples/six warmups per process for 16/128/1,024 panes. Each group/condition/case has 48 raw samples. Recompute each product percentile by sorting its actual `elapsed_ns` values and taking the nearest-rank index `ceil(n * percent / 100) - 1`. The existing [product comparison script](../../../scripts/compare_products.py) supplies the same percentile function and row validation.

All seven work fields are equal across formal baseline/candidate runs at each case/sample: frames, actual passes, content calls, visible contents, vertices, indices and draw commands. Resize-follow-up work fields also match. Complete product timings cover CPU draw-data generation, actual press/release frames, content and presentation reporting; they exclude GPU/native-window/vsync. Cross-product ratios include framework/rendering differences.

Headless formal groups also use ABBA/BAAB, with 48 samples/six warmups and explicitly **one** older in-flight output. Its process summaries are not raw timing samples: do not average or pool their percentiles into a purported overall p50/p95. Allocation runs use six samples/two warmups, separately from timing. Requested bytes are cumulative allocation requests; boundary live values and case-prefix peaks are tracked heap values including the recorder/output owners, not RSS. Prefix peaks include setup, warmup and earlier samples.

Checks finish before the product build/reference and formal product groups. The headless build then finishes before formal headless timing; active allocation runs, the resize follow-up and CPU captures follow, sequentially. No build, test or active allocation profiler runs alongside a formal timer in these receipts. Desktop scheduling/power state are not isolated.

The CPU profiles include setup, warmup, untimed hover/geometry preparation, validation and disposal. Their recorded CSV times are instrumented observations, not qualification data. Category roots are disjoint only within one category; categories overlap and must not be added or converted into exact frame-stage budgets. Output-audit stacks include real pane/application rendering. The category patterns/parser are the same as the existing [architecture-review parser](../2026-11-06-architecture-design-review/summarize_samples.py).

To repeat current complete workloads, use the recorded commands and build environment. The main product entry is:

```text
python3 scripts/compare_products.py --backend fork --profile production --samples 24 --warmup 6 --panes 16,128,1024 --runs 4 --out <new-directory>
```

The headless build uses `CARGO_BUILD_JOBS=1`, `CARGO_PROFILE_RELEASE_LTO=thin`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`, then:

```text
cargo build --manifest-path integration/dockspace-host-harness/Cargo.toml --release --locked --offline --example frame_cost --message-format=json-render-diagnostics
<recorded-executable> --timing-only --samples 48 --warmup 6 --in-flight 1 --panes 16,128,1024
<recorded-executable> --samples 6 --warmup 2 --in-flight 1 --panes 16,128,1024
```

The baseline executables were reused only after checking the prior qualified source/binary content, not rebuilt from the modified tree. For a new baseline comparison, rebuild the recorded revisions/inputs in independent checkouts, preserve complete workloads, and run conditions serially. Runtime binaries and provisional/failed implementation logs remain at `/tmp/dockspace-fearless-20261106-34ab23a/`; binaries and new maintained qualification scripts are not committed. The source/check/archive receipts here are the final successful freeze.
