# Product CPU comparison and paint snapshot extraction

Date: 2026-11-05. Base: `0a2c94e`. Benchmark: `d71e3b1`.
Implementation: `a0b8d5a`.

## Result and scope

Complete egui product frames cost substantially more than the Dear ImGui
reference on this workload. At 1,024 panes, the first egui idle p50 is 95.203 ms
against ImGui's 1.095 ms. The earlier internal recording-renderer idle result of
about 3 ms excluded product UI and meshes and cannot stand in for this comparison.

Moving already-cloned paint lists instead of cloning their shapes again reduces
egui's 1,024-pane p50 by 16.93–18.92%. All output audits remain enabled. Final
egui/ImGui p50 ratios are still 68.7–81.3 at that size. These observations do not
establish a GPU frame rate or isolate the docking algorithms from their product
adapters, text, styling and output protocols.

## Method and evidence

The [workload contract and reproduction command](../../integration/product-comparison/README.md)
define one context/surface, all 16/128/1,024 panes visible, two tabs per pane,
balanced alternating splits, nominal 14-point text, close disabled and actual
selected text plus an 8 by 8 colored square. Both products generate complete
CPU draw data. Rust uses the recommended owned single-surface run, tessellates
all shapes and reports the exact Presented receipt. ImGui submits all 2N
windows, renders and acknowledges CPU texture requests. Reference sources at
`d88f238d1d363ef387382b2e29ad12cf96f826ac` are unchanged.

An idle or resize sample is one complete frame. Resize alternates logical width
16,384/16,448 without rebuilding the tree. A click sums real press and release
frame times. Both products first complete two untimed frames to observe actual
tab geometry and establish hover; ImGui's ordinary overlap rules require the
preceding hover. Timing ends with draw data still live. Counting, validation and
disposal occur outside each timer, before the next frame. Ordinary egui repeat
passes and ImGui hidden-window pre-layout content remain enabled and counted.

Each condition builds serially, then runs egui/ImGui/ImGui/egui, with 24 samples
and six warmups per case per process. Each has 864 raw rows, 18 groups and 48
pooled samples per product/case. Summaries use nearest-rank p50/p95 and observed
maximum. Old and new egui conditions are separate blocks; only the products
within a block are interleaved. No Cargo, tests or CPU sampler started by this
session overlaps their measurement processes. The shared Apple M4 desktop and
power settings are uncontrolled, so these are descriptive samples rather than
tail guarantees. Rust is 1.95.0, Apple clang is 21.0.0, release builds use normal
System allocation, and no heap/RSS ratio is measured.

| Condition | Raw samples | Summary | Environment and exact input/binary/result SHA-256 |
| --- | --- | --- | --- |
| Before production change | [baseline/raw.csv](2026-11-05-product-comparison/baseline/raw.csv) | [baseline/summary.csv](2026-11-05-product-comparison/baseline/summary.csv) | [baseline/environment.json](2026-11-05-product-comparison/baseline/environment.json) |
| Owned extraction, before empty-list branch | [owned-extraction/raw.csv](2026-11-05-product-comparison/owned-extraction/raw.csv) | [owned-extraction/summary.csv](2026-11-05-product-comparison/owned-extraction/summary.csv) | [owned-extraction/environment.json](2026-11-05-product-comparison/owned-extraction/environment.json) |
| Final extraction with empty-list branch | [final/raw.csv](2026-11-05-product-comparison/final/raw.csv) | [final/summary.csv](2026-11-05-product-comparison/final/summary.csv) | [final/environment.json](2026-11-05-product-comparison/final/environment.json) |

Every directory also retains build logs and each process's original stdout and
stderr. The environments record the working-source hashes, since all runs were
made before local commits at the same base HEAD. The independent crossed review
recomputes every statistic and compares diagnostics by product, pane count,
scenario, run and sample. Frames, passes, content calls, visible contents,
vertices, indices and draw commands match exactly across all three conditions.

The Rust and C++ harness sources are byte-identical throughout. The only measured
production-source difference is `output_proof.rs`. All reference inputs and the
C++ executable SHA-256 remain identical. During baseline review, the runner's
fixed Rust executable path was found to be unsafe when Cargo redirects its
target directory. Subsequent builds identify the unique executable from that
build's Cargo JSON artifact stream and never fall back to the old path. The
baseline used the default path without a target-directory override, and its
actual executable/source hashes are preserved. Its
[exact runner snapshot](2026-11-05-product-comparison/baseline-runner.py.txt)
and the first candidate's
[exact production snapshot](2026-11-05-product-comparison/owned-extraction-output-proof.rs.txt)
match their recorded hashes. The runner change is outside timed execution.

## Complete product p50

All timings below are milliseconds per sample. `tab_click` includes two full
frames. Reduction compares final egui to baseline egui; the ImGui column is the
final block's separately measured reference.

| Panes | Case | egui before | egui final | Reduction | ImGui final | Final egui / ImGui |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.184 | 0.182 | 1.33% | 0.017 | 10.9× |
| 16 | `tab_click` | 0.616 | 0.601 | 2.48% | 0.034 | 17.6× |
| 16 | `surface_resize` | 0.296 | 0.293 | 1.21% | 0.019 | 15.8× |
| 128 | `idle` | 2.534 | 2.293 | 9.49% | 0.133 | 17.3× |
| 128 | `tab_click` | 7.618 | 7.033 | 7.67% | 0.275 | 25.6× |
| 128 | `surface_resize` | 3.809 | 3.532 | 7.28% | 0.139 | 25.3× |
| 1024 | `idle` | 95.203 | 77.186 | 18.92% | 1.123 | 68.7× |
| 1024 | `tab_click` | 213.510 | 176.821 | 17.18% | 2.275 | 77.7× |
| 1024 | `surface_resize` | 108.849 | 90.417 | 16.93% | 1.112 | 81.3× |

At 1,024 panes, the unchanged ImGui control's p50 moves by +2.60%, +1.17% and
−0.11% across the two blocks. The large egui improvement appears in both egui
processes and both candidate blocks. The 16-pane final/baseline differences are
small, and per-process directions mix; no dependable 1–2% small-layout gain is
claimed.

## Tails and the empty-list branch

| Panes | Case | Before p95 | Final p95 | Before max | Final max |
| --- | --- | ---: | ---: | ---: | ---: |
| 16 | `idle` | 0.247 | 0.258 | 0.344 | 0.288 |
| 16 | `tab_click` | 0.779 | 0.790 | 1.018 | 1.105 |
| 16 | `surface_resize` | 0.399 | 0.362 | 0.481 | 0.594 |
| 128 | `idle` | 2.824 | 2.789 | 2.909 | 3.146 |
| 128 | `tab_click` | 8.185 | 7.767 | 8.414 | 7.977 |
| 128 | `surface_resize` | 4.209 | 4.000 | 4.779 | 4.135 |
| 1024 | `idle` | 96.718 | 79.208 | 97.569 | 80.080 |
| 1024 | `tab_click` | 223.745 | 183.581 | 247.578 | 188.406 |
| 1024 | `surface_resize` | 110.238 | 95.768 | 115.741 | 97.496 |

The first owned-extraction candidate retains the large-layout benefit but
regresses 16-pane p50 to 0.201/0.651/0.299 ms. Its idle and click regressions
appear in both processes. The final branch returns empty shapes directly for
an actually empty PaintList, retaining the layer identity and transform while
avoiding extractor insertion and drain. Idle/click p50 improves against that
candidate in both small-layout processes. At 1,024 panes the extra branch is
essentially neutral. All three raw datasets remain available; the final small
click/resize maxima and 128-pane idle maximum still exceed baseline. There is
no uniform tail-latency improvement claim.

## Output volume and attribution

For one final 1,024-pane idle frame, egui emits 167,948 vertices, 344,082 indices
and 5,121 mesh commands; ImGui emits 174,088 vertices, 353,304 indices and 1,029
commands. Similar vertex counts do not imply equal text shaping, pixels,
draw-command overhead or output protocols. Each has 1,024 visible selected
contents. A click totals 2,048 visible contents on both sides; ImGui's normal
hidden-window preparation executes one extra content body, which is retained
and reported as 2,049 content calls rather than suppressed.

Separate [baseline CPU sampling](2026-11-05-product-comparison/baseline-product-cpu.txt)
with [command and scope](2026-11-05-product-comparison/baseline-product-profile-meta.json)
covers the first eight seconds of a fresh 1,024-pane process, chiefly idle and
warmup. Shape clone/drop, prefix equality, `snapshot` and
`PaintAppendCheck::is_preserved` dominate; tessellation is a small part of that
profile. Inclusive call-tree counts overlap and must not be summed. This profile
selects the optimization; it does not explain every changed-input cost.

Previously each snapshot cloned complete GraphicLayers, then separately cloned
all known-layer shapes from the original graphics. The complete clone remains
necessary to reject residual nonempty layers not exposed by egui's public layer
enumeration. The change takes each owned PaintList from that clone into a local
single-layer GraphicLayers and drains it without applying transforms. The
per-layer vector is moved, so nested shapes and Arc handles avoid another deep
clone. Empty lists skip the temporary extractor. This adds no cache, public
interface, unsafe code or second authoritative graph.

Known layer identities, order, exact prefixes, global and deferred transforms,
clip bits, markers, per-content-callback append checks and unknown-layer
rejection remain intact. Repeated snapshots over growing paint lists still
have quadratic aggregate cost. The change reduces duplicate copying and its
destruction; it does not make auditing linear.

## Verification

Twenty focused output-proof tests pass, including two new differential fixtures
against the frozen pre-change snapshot. They cover empty/missing current lists,
multiple registered layers, nested shapes, Noop and empty compounds, marker
identity, raw global/deferred transforms, signed-zero/subnormal endpoints,
repeated non-destructive captures and unknown layers introduced before or after
capture. Existing tests still reject changed prefixes, foreign layers, forged
markers and broken terminal output. Crossed independent read-only reviews find
no concrete P1/P2 in the final benchmark, runner or production change.

Fresh serial gates pass with one Cargo build job and one nextest worker, reusing
existing targets. The [gate record](2026-11-05-product-comparison/validation/results.json)
retains exact commands, UTC start/end times and return codes; adjacent logs retain
their complete outputs (the doctest log is gzip-compressed without changing its
contents).

| Gate | Result |
| --- | --- |
| Workspace, all features/all targets | 2,090 passed |
| Independent host, default | 292 passed, including three 10,000-depth cases |
| Independent host, persistence | 332 passed; already-covered depth binary filtered |
| Official egui consumer | 24 passed |
| Product egui, default / persistence, all targets | 114 / 119 passed |
| Fork-backed native, all features/all targets | 556 passed; one pre-existing ignored test |
| Workspace/native/product correctness and suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed |
| Workspace doctests | Ten passed |
| Workspace/host/product formatting, diff and core dependency checks | Passed |

Source hashes still match the final measured build after qualification. Physical
GUI, GPU, package publication and OS input tests are not part of this CPU-only
comparison.

## Remaining work

The public egui append-audit interface cannot currently expose every layer with
an immutable snapshot or a mutation revision. Reducing the remaining quadratic
cost needs a design that can prove exact append-only output without rescanning
the complete accumulated shapes around each callback. Removing those checks or
replacing real output with counters would change the contract and is not a
valid performance gain. Future work should measure that design against this
same executable product reference, keeping output and input integrity explicit.
