# Product CPU comparison with Dear ImGui

Date: 2026-11-05. Status: complete. Base: `0a2c94e`.

## Outcome

Establish an executable product-level reference before choosing further
optimizations. Existing internal `frame_cost` samples remain useful for attribution
and unchanged; they are not a Dear ImGui comparison. The user authorized pushing
the completed work first. `origin/refactor/dockspace` now points at `0a2c94e`.
Preserve the pre-existing CI and review changes. No PR is requested.

## Shared workload contract

- Products: public `egui_dockspace` facade with egui 0.37.0, and the existing
  read-only Dear ImGui docking reference at
  `d88f238d1d363ef387382b2e29ad12cf96f826ac` (1.93.1 WIP).
- One logical surface/context, no native window, GPU, vsync or accessibility
  tree. Logical size 16,384 by 16,384, scale 1, deterministic 1/60-second input
  steps. All 16/128/1,024 panes remain visible. Build a balanced equal binary
  split tree alternating horizontal/vertical axes, with two tabs per leaf.
- Stable preallocated tab titles `Pane 0001 A` / `Pane 0001 B`, etc. Every
  selected content callback actually draws `Content` plus an 8 by 8 color square.
  Disable pane close affordances on both sides. Keep each product's ordinary
  styling, font and docking implementation; report draw counts rather than
  asserting identical triangle counts or pixel output.
- `idle`: one complete logical frame. `surface_resize`: alternate width
  16,384/16,448, keeping height 16,384, and produce one complete frame without
  rebuilding the layout. `tab_click`: choose the inactive tab in the first pane
  from actual current title/tab geometry, then establish a real mouse hover
  before timing press and release. Both products use two unmeasured preparation
  frames (geometry observation, then mouse move/hover). ImGui's normal tab
  overlap handling needs that preceding hover frame. Verify the selected tab
  changed through public state/read-only checks.
- Time input submission, complete UI update and mesh generation. Rust executes
  the public owned run, tessellates all output shapes and reports the exact
  Presented receipt. ImGui executes NewFrame, DockSpace, all 2N Begin/End calls,
  Render and CPU acknowledgement of texture requests. No GPU upload/render is
  performed. Each frame stops timing with its complete draw data still live;
  mesh statistics, validation and output disposal then run outside its timer.
  A click sample sums its two frame timers, consuming the press output before
  starting release. Setup, geometry queries, correctness checks, aggregation and
  CSV output stay outside timing. Do not replace rendering with counters.
- Warm readiness from observable content/docking facts with a bounded diagnostic
  limit, then run the declared performance warmups. Validate N visible selected
  contents per UI pass, all panes visible, positive complete mesh output, actual
  tab switching and actual resized bounds. Record every real content callback,
  including native pre-layout callbacks. ImGui may call one additional hidden
  window body when a tab is pressed; retain this normal work and report it
  separately from visible selected contents. Do not gate ordinary `Begin=true`
  content on a private visibility flag or disable egui multi-pass behavior.
- Both executables accept `--samples`, `--warmup`, `--panes`. They emit one raw
  measured-sample CSV row using this exact header:
  `product,panes,tabs_per_pane,scenario,sample,frames,elapsed_ns,passes,content_calls,visible_contents,vertices,indices,draw_commands`.
  Product labels are `egui_dockspace` and `dear_imgui`. Geometry counts and passes
  are totals over the sample's one or two logical frames. Retain all raw samples.
- Use normal release/System allocation on both sides. This first comparison
  reports CPU timing and complete draw-data diagnostics; it makes no heap/RSS
  ratio claim. ImGui draw data has synchronous lifetime; the existing asynchronous
  retained-output stress case has no fabricated counterpart here.

## Work

- [x] Push and verify the already-tested branch on the remote.
- [x] Inspect local public product interfaces and pinned upstream reference;
  confirm input, output and workload comparability before implementation.
- [x] Add the Rust product executable and standalone C++ executable without
  modifying production code or the reference checkout.
- [x] Add a small cross-platform Python build/run/aggregation entry point;
  serialize compilation and benchmark processes, reuse existing targets.
- [x] Verify workload checks, independent crossed review, relevant tests and
  complete interleaved release measurements. Record exact sources, compiler
  versions, commands, raw rows, summary and limitations.
- [x] Use the measured baseline to select and verify a production optimization;
  prepare the validated comparison, production change and evidence for the
  authorized branch push.

## First product baseline and selected optimization

The first complete interleaved release run is preserved at
`/tmp/dockspace-product-comparison-01a1a5fb/formal-baseline/`, with frozen
executables alongside it. The 1,024-pane product idle sample is about 95 ms for
the current egui facade and 1 ms for Dear ImGui, despite similar vertex counts.
These timings include actual UI and mesh generation and supersede using internal
recording-only times as an external performance comparison.

Separate CPU sampling identifies pane append-only audit snapshots and shape
clone/drop as major costs; tessellation is a small part of that profile. The
selected first optimization retains every audit and the cloned complete graphics
needed to reject unknown layers. It transfers each already-cloned nonempty
PaintList into a temporary single-layer GraphicLayers and drains its shapes,
instead of cloning them a second time. Empty lists retain their layer snapshot
without inserting into the extractor. Exact layer identities, transforms,
prefixes, markers and unknown-layer rejection remain unchanged. This removes
duplicate deep copying without claiming linear complexity; the cumulative
per-pane audits remain quadratic.

The [complete results](../performance/2026-11-05-product-comparison-results.md)
retain all three conditions, raw samples, build/process logs, compiler and source
provenance, and qualification. The first extraction candidate regressed small
layouts; the final empty-list branch returns their p50 to the original range.
At 1,024 panes final egui p50 is 77.186 ms idle, 176.821 ms per two-frame click,
and 90.417 ms per resize, a 16.93–18.92% reduction. Dear ImGui remains
1.123/2.275/1.112 ms on the same cases, so the large external gap is explicit.
Twenty focused tests, all 18 serial gates and crossed independent reviews pass.
Do not infer a GPU/physical-input qualification, a universal small-layout gain,
uniform tail improvement or removal of the remaining quadratic audits.
