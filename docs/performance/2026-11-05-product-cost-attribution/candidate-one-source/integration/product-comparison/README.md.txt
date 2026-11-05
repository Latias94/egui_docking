# Product CPU comparison

This benchmark compares complete CPU draw-data generation in `egui_dockspace`
and Dear ImGui docking. Both products actually build the docking UI, tab labels,
selected content text and a small colored square. It complements the internal
`frame_cost` attribution benchmark; the two workloads are not interchangeable.

The Dear ImGui reference is pinned to
[`d88f238d1d363ef387382b2e29ad12cf96f826ac`](https://github.com/ocornut/imgui/tree/d88f238d1d363ef387382b2e29ad12cf96f826ac).
The runner reads that checkout without modifying it. Provide its path explicitly
if it is not already at `repo-ref/imgui`.

```text
python3 scripts/compare_products.py --imgui-root repo-ref/imgui --samples 24 --warmup 6 --panes 16,128,1024 --runs 4
```

Rust 1.95, Python 3 and a C++17 compiler are required. Rust dependencies must be
available locally because builds use `--locked --offline`. The runner supports
clang++, g++ and MSVC cl (from a configured developer prompt); `--cxx` selects a
compiler. It builds serially with one Cargo job and reuses the existing product
harness target. The C++ executable uses normal release flags and default system
allocation. No extra Rust dependency, window system, graphics context or GPU is
required.

For a functional smoke, use `--samples 2 --warmup 2 --runs 2`. `--out` selects a
new results directory; by default results go under the product harness's existing
target. Existing output directories are not overwritten.

| Case | Work per measured sample |
| --- | --- |
| `idle` | One complete UI frame and mesh generation |
| `tab_click` | Real pointer press and release, two complete frames; assert actual tab selection changes |
| `surface_resize` | Alternate logical surface width 16,384/16,448 and generate one complete frame |

For CPU attribution, the standalone Rust example also accepts
`--scenario idle`, `--scenario tab_click`, or `--scenario surface_resize`.
This selects cases outside timing; the comparison runner always executes all
three cases. Keep CPU sampling separate from formal measurement runs.

Every case uses a fully visible balanced tree of 16/128/1,024 panes, two tabs per
pane, alternating split axes, a height of 16,384, scale 1 and fixed 1/60-second
input steps. Close buttons are disabled on both sides. UI styling, fonts and
native layout behavior remain specific to each product. The output diagnostics
report the resulting geometry rather than pretending the products emit identical
triangles.

The measured egui path uses the public owned single-surface run, tessellates all
shapes and reports its exact presentation receipt. The ImGui path submits every
window through Begin/End, renders complete draw data and acknowledges texture
requests on the CPU. ImGui's DockBuilder is used only for initial layout, outside
timing. Subsequent selection uses observed tab bounds and real mouse events;
resize never rebuilds the tree.

A click has the same two unmeasured preparation frames on both sides: observe
the current tab geometry, then move the pointer onto the inactive tab and render
its hover frame. This retains ImGui's normal overlap/hover rules before the timed
press and release, rather than forcing private selection or hover state.

Each frame stops its timer with complete draw data still live. Mesh counts,
correctness checks and output disposal run afterward. A click sample sums the
press and release frame timers, consuming the first output before advancing.
This avoids timing mesh destruction on only one side or forcing either product
to retain an artificial history of render outputs.

Neither side disables ordinary pre-layout work. ImGui can execute a hidden
window's body to prepare a selected tab; egui can run multiple UI passes. Raw
rows record all `content_calls`, all `passes` and the distinct
`visible_contents`. Each pass must retain every visible pane. The benchmark
rejects missing samples, clipped-away panes, absent geometry and failed input
effects instead of returning a timing for a reduced workload.

Each executable writes raw sample rows after measuring. The runner alternates
the products in egui/ImGui/ImGui/egui order, preserves stdout/stderr per process,
validates coverage and writes `raw.csv`, `summary.csv` and `environment.json`.
Summaries pool the raw samples using nearest-rank p50/p95 and observed maximum;
the environment records sources, exact commands, compiler versions, run order
and executable/result SHA-256 digests.

The measurements end at CPU draw data. They do not include GPU upload, native
window presentation, vsync or real input latency. Default fonts, style and
output-buffer lifetime differ between the products; these are product workload
observations, not an isolated docking-algorithm contest. No heap/RSS ratio is
reported. The core's asynchronous historical-output stress case remains separate
because Dear ImGui's draw data is valid only until the next frame.

The upstream [docking documentation](https://github.com/ocornut/imgui/wiki/Docking)
describes DockBuilder as an internal setup interface. The upstream
[headless example](https://github.com/ocornut/imgui/tree/d88f238d1d363ef387382b2e29ad12cf96f826ac/examples/example_null)
provides the no-window model; this benchmark supplies real input and a CPU
texture acknowledgement path.
