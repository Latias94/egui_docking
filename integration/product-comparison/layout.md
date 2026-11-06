# Renderer-free layout attribution

```text
python3 scripts/compare_layouts.py --out /tmp/dockspace-layout-comparison
```

This probe complements the [complete product comparison](README.md). It calls
the existing layout implementations without fonts, widget submission, text,
draw lists, tessellation or a window system. The pinned ImGui checkout is read
without modification. Its private `DockNodeTreeUpdatePosSize` implementation is
made callable by including the original `imgui.cpp` in the probe translation
unit, rather than copying or rewriting the algorithm.

| Case | Timed work | Output |
| --- | --- | --- |
| `dockspace/project_full` | `project_root`: topology traversal, constraint measurement and projection | New immutable `LayoutProjection`, including node rectangles, composed constraints and split outcomes |
| `dockspace/project_prepared` | `RootMeasurement::project`: projection of freshly measured inputs | The same complete projection; preparation is recorded separately in `preparation_ns` |
| `dear_imgui/project_in_place` | Upstream `DockNodeTreeUpdatePosSize` | Position and size updated in the existing docking tree |

The Rust probe is an ignored in-crate test, so it can access private production
functions without exporting an API or introducing a benchmark feature. Cargo
builds it with `--profile production`: ThinLTO, one codegen unit and release
debug-assertion settings. No layout work-count instrumentation is enabled in
these functions by `cfg(test)`. The executable remains a test build, which is
recorded explicitly rather than presented as the earlier product executable.

The ImGui probe uses `-O3 -DNDEBUG` and matching link-time optimization. The
runner also supports GCC and MSVC using their existing LTO options. Cargo builds
serially and reuses the root target directory. Build logs and compiler commands
are kept with the results; no dependency is added.

## Common geometry

Both sides receive a balanced binary tree of 16/128/1,024 visible leaves,
alternating horizontal and vertical axes, equal split weights, no central leaf,
and four-unit separators. All minima/maxima remain inactive at the measured
sizes. Width alternates between 16,380 and 16,444; height is 16,380.

These widths deliberately make every recursively halved extent integral through
the deepest measured axis. This removes rounding differences between ImGui's
pixel-rounded `f32` bounds and dockspace's continuous `f64` bounds for this
subset. It does not establish equivalence of their general constraint solvers.

Setup constructs and validates the tree before timing. Each timed call updates
every reachable node. After every call, each node rectangle is checked against
the complete balanced-tree oracle; node, leaf and split counts must match.
A deterministic geometry digest must also agree across all three cases and
both widths in the recorded samples. Output observation and disposal occur
after the timer. ImGui performs no allocation of a new projection to dispose.

The default run uses two serial ABBA groups, 96 samples and 16 warmups per
process, at all three scales. `raw.csv` preserves every sample, preparation
duration, output count and geometry digest. `summary.csv` reports each group's
nearest-rank p50/p95 and observed maximum. Environment metadata records source
and executable hashes, profile settings and build/run boundaries.
Single-call timings for the smallest ImGui tree are close to timer overhead;
use the larger cases when judging magnitude rather than interpreting every
reported nanosecond as independent precision.

## Interpretation limits

This is a comparison of layout subsystems over a shared geometry workload.
Rust's richer output, validation and allocations remain part of its cost;
ImGui mutates a persistent structure with a simpler size policy. Treat the
results as evidence about data flow and projection construction, not as an
equal-feature algorithm speed ratio or a Rust-versus-C++ language benchmark.

`project_prepared` explicitly moves measurement outside its projection timer.
Its `preparation_ns` must remain visible, and `project_full` remains the complete
reference. Do not attribute their difference to an optimization shipped to the
product. Neither case includes semantic scene compilation, hit/touch geometry,
receiver preparation, output proofs or presentation settlement.

The full docking update cannot simply be replaced with a call to
`DockContextNewFrameUpdateDocking`: that routine updates floating roots, while
dockspace roots are updated through `DockSpace`/`DockNodeUpdate`. Tab bars,
splitter interaction and docking previews are interleaved with widget and draw
submission. Additional operations need their own explicit input/output boundary;
assigning a selected-tab field is not a measured click or docking transaction.
