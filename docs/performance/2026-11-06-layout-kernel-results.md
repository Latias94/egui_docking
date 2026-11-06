# Renderer-free docking layout attribution

Date: 2026-11-06. Base: `d6dc6d7f9f708dd857a216620bd9f1ad97ce67bc`.
No production implementation or public API was changed by this probe.

## Result

Removing both renderers exposes a substantial difference in the layout
subsystems. At 1,024 leaves, the final pooled p50 is **556.250 microseconds** for
dockspace's complete measurement/projection, **324.625 microseconds** for its
prepared-input projection, and **7.083 microseconds** for the pinned ImGui
in-place bounds update. The prepared case additionally records **217.292
microseconds** p50 preparation outside its projection timer; that work has not
been eliminated or shipped as an optimization. Separate medians are not an
additive phase decomposition.

These results support investigating layout data flow and projection allocation.
They do not establish an equal-feature algorithm or programming-language ratio:
dockspace returns a new immutable projection containing constraints and split
outcomes; ImGui updates positions/sizes in an existing tree with a simpler
allocation policy. Both produce exactly the same node rectangles in the tested
common subset.

| Final ABBA group, 1,024 leaves | Complete Rust projection p50 | Prepared Rust projection p50 | ImGui in-place update p50 |
| --- | ---: | ---: | ---: |
| 1 | 552.292 us | 322.875 us | 7.083 us |
| 2 | 556.958 us | 326.250 us | 7.083 us |

## All scales

Pooled final groups, 384 samples per row. All values below are microseconds per
projection, including every recorded tail sample.

| Implementation and boundary | Leaves | p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| dockspace, complete | 16 | 4.958 | 5.209 | 6.458 |
| dockspace, prepared inputs | 16 | 3.583 | 3.708 | 4.417 |
| ImGui, in place | 16 | 0.084 | 0.125 | 0.125 |
| dockspace, complete | 128 | 45.667 | 56.208 | 193.875 |
| dockspace, prepared inputs | 128 | 31.041 | 40.875 | 151.833 |
| ImGui, in place | 128 | 0.750 | 0.917 | 11.625 |
| dockspace, complete | 1,024 | 556.250 | 699.917 | 1,015.917 |
| dockspace, prepared inputs | 1,024 | 324.625 | 374.625 | 629.209 |
| ImGui, in place | 1,024 | 7.083 | 8.958 | 54.959 |

The smallest ImGui case is near single-call timer overhead. Use the larger cases
when assessing magnitude; the CSV's integer nanoseconds are not a claim of
independent nanosecond precision.

## Extraction and workload

The C++ probe includes the unmodified pinned `imgui.cpp` and calls its actual
private `DockNodeTreeUpdatePosSize`. It does not copy the implementation or
modify the reference checkout. The Rust probe calls the existing private
`project_root` and `RootMeasurement::project` through one ignored in-crate test.
This avoids creating a public benchmark API or feature. Rust uses the root
`production` profile: release optimization, ThinLTO and one codegen unit, with
debug assertions disabled. It is explicitly a test executable; the layout
functions have no `cfg(test)` work-count instrumentation. C++ uses Apple Clang
21 with `-O3 -DNDEBUG -flto=thin`. Rust is 1.95.0 on ARM64 macOS 27.0.

Both sides receive balanced binary trees with alternating axes, equal weights,
four-unit separators and no central leaf. All leaves are visible. Minima and
maxima never constrain these sizes. Width alternates 16,380/16,444 and height
is 16,380, so both f32 pixel-rounded and f64 continuous implementations produce
exactly integral, identical bounds at every depth. These dimensions deliberately
differ from the full product workload to make the geometry comparison exact.

There are no `NewFrame`, `Begin`/`End`, label, font, draw-list or tessellation
calls inside either timer. Tree setup is outside both. Each call updates all
2N-1 nodes. Every rectangle is checked after every call; node, leaf and split
counts and cross-product geometry digests must match. A separate Python pass
recomputed the expected integer geometry and every sample digest. Observation
and output destruction occur after timing. Prepared Rust measurements are
performed outside its projection timer and retained in `preparation_ns`.

The final run contains two serial ABBA groups, each with 96 samples and 16
warmups per process at 16/128/1,024 leaves. It retains 3,456 raw rows and 18
group summaries; all sample keys, process output, nearest-rank p50/p95, maxima,
313 source hashes, ten upstream input hashes and executable/result hashes were
verified. Builds and checks did not overlap measurements. The initial probe is
retained separately; it yielded approximately 570/332/7 us at 1,024 leaves.
Only source-provenance guards in the runner changed afterward; the Rust/C++
probe implementations and measured function boundaries were unchanged.

## What the result changes

The source shows a concrete structural difference. ImGui walks child pointers
and writes existing position/size fields. Dockspace measures through ordered
maps/sets, then each split constructs vectors for child measurements, axis
constraints, weights, extents and split geometry. The result owns ordered maps
and per-split arrays. This is evidence for investigating dense node storage,
reusable scratch capacity and a clearer boundary between trusted topology and
dynamic projection. Allocation counts and individual causal savings have not
been measured by this probe.

The earlier complete-product resize p50 was about 18.9 ms for the qualified fork
condition. The isolated complete projection here is about 0.56 ms. The workloads
and boundaries differ, so they cannot be subtracted as a precise frame budget.
Nevertheless, a layout-only rewrite does not explain the entire product gap.
Semantic scene compilation/validation, hit and touch geometry, receiver
preparation and output settlement remain separate attribution targets.

Full ImGui docking cannot be isolated by timing only
`DockContextNewFrameUpdateDocking`: it drives floating roots, while normal
dockspace roots are updated through `DockSpace`/`DockNodeUpdate`. Tab bars,
splitter interaction and previews interleave layout, state changes, widgets and
drawing. A future selection, split or move benchmark must define its actual
input/output contract rather than time a field assignment or reimplement a
simplified docking engine under the ImGui name.

## Reproduction and checks

See [methodology](../../integration/product-comparison/layout.md) and run:

```text
python3 scripts/compare_layouts.py --out /tmp/dockspace-layout-comparison
```

Focused production-profile nextest passed all five layout tests. The new
diagnostic remains ignored during ordinary test runs. Correctness/suspicious
Clippy over the core library/tests, formatting, Python compilation and diff
checks passed. The benchmark itself ran successfully in eight final processes
and checked complete geometry on every warmup and sample.

[Archived evidence](2026-11-06-layout-kernel/) contains raw data, per-process
output, build logs, environment metadata, verification and a checksum manifest.
Executables remain in the runtime directories under
`/tmp/dockspace-layout-kernel-20261106-d6dc6d7-{probe-one,final}/`.
