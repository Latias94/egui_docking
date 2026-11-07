# Continuous editor exercise in the official egui host

The existing official egui/WGPU demo now completes a continuous editor journey
without an engine. One actual-window run passed all six phases and produced
384 presented frame samples. The retained run uses Linux X11 and a software
GPU; the local Apple M4/Metal attempt did not produce accepted timing samples.

This change extends the application harness on base
`b7df71e0dd9530f7480addc56bcd349ef8230a89`. It does not change the docking core,
egui adapter library, dependency pins, or the earlier ImGui kernel results.

## Run the host

From the repository root, open the normal interactive editor:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --profile production --target-dir target --locked -j1 --bin official-caller-owned-demo
```

Supply a new output path to exercise the same application and renderer:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --profile production --target-dir target --locked -j1 --bin official-caller-owned-demo -- --exercise /tmp/egui-host-frames.csv
```

The host owns eight stable application items: Editor, Preview, Outline,
Inspector, Console, History, Profiler and Assets. Inspector uses a contained
floating window. Editor text, the scrolling Console and illustrative profiler
bars are actual egui content. The exercise supplies ordinary egui input events;
normal mode continues to accept native input.

Each recorded phase has eight warmup frames and 64 retained frames. Setup,
focus, press/release and final validation also execute but are not CSV samples.
The exercise admits at most 640 frames and checks a 90-second elapsed budget
between frames; it cannot interrupt a blocked WGPU or OS call. The retained
execution also had an external 105-second timeout.

Completion requires:

- Every presented exercise frame contains actual mesh geometry. No dropped
  workload output is accepted; startup drops can only be retried during setup.
- Painted idle, hover, text editing and scrolling preserve the docking version,
  including each phase's final output.
- Text editing inserts 72 marker characters while retaining every original
  character; both the original and inserted content survive the later drags.
- Wheel events change the actual Console scroll offset.
- After splitter release and settlement, the workspace version, durable split
  weights and current Outline width change. A temporary preview alone cannot pass.
- After tab release, Outline contains exactly `[Outline, Preview]`, Preview is
  selected, Editor contains exactly `[Editor]`, and all eight items survive.

CSV creation uses a new file and occurs only after final semantic validation.
The source and assertion review is recorded in the
[review](2026-11-08-egui-host/code-review.md) and its
[closure](2026-11-08-egui-host/code-review-closure.md).

## Actual-window result

The successful run took 47.25 seconds, including unrecorded frames and host
startup, and reported **zero startup drops**. Its environment was:

| Setting | Observed value |
| --- | --- |
| Host | Linux aarch64, isolated Xvfb display |
| Container quota | 1 CPU, 4 GiB memory |
| Adapter | llvmpipe, LLVM 15.0.6, 128-bit; CPU device |
| Backend | WGPU GL, Mesa 22.3.6 |
| Window | 1280 × 820 physical pixels, scale 1 |
| Surface | `AutoVsync`, `Rgba8Unorm` |
| Build | Rust 1.95.0; production profile inherits release, thin LTO, one codegen unit |
| Software raster threads | `LP_NUM_THREADS=1` |

All values below are milliseconds. Each row retains all 64 samples. Update,
tessellation and paint columns are independent medians; they are not added to
construct the total. Total p50 is the median, p95 uses nearest rank, and maximum
includes every row.

| Phase | Update p50 | Tessellation p50 | Paint/present p50 | UI-to-receipt p50 | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Painted idle | 0.304 | 0.023 | 97.071 | 97.411 | 100.553 | 105.012 |
| Hover | 0.330 | 0.024 | 98.078 | 98.474 | 102.818 | 122.190 |
| Text editing | 0.346 | 0.025 | 99.030 | 99.420 | 103.470 | 107.259 |
| Scrolling | 0.321 | 0.025 | 99.731 | 100.137 | 103.047 | 104.185 |
| Splitter drag | 0.383 | 0.024 | 100.118 | 100.562 | 102.988 | 114.692 |
| Tab drag | 0.376 | 0.032 | 107.459 | 107.887 | 113.581 | 121.490 |

`update_ns` measures the complete public `run_single_surface`, including
application content, measurement, encoding and publication. `tessellate_ns`
converts the complete output shapes to CPU meshes. `paint_present_ns` includes
surface acquisition, uploads, render encoding, submission and the actual
`queue.present` CPU call, including any blocking inside those calls.

`ui_to_present_ns` starts immediately before the public update and ends after
the presentation receipt is reported. It includes platform output and host work
between the component windows. Native event delivery, input extraction, driver
preparation, coordinate inspection, semantic assertions and CSV writing are
outside this interval. It is not GPU execution time, scanout or input-to-photon
latency. Painted idle forces repaint and does not test scheduler sleep.

Across the retained rows, the paint/present call interval accounts for 99.62%
of the summed UI-to-receipt interval. This locates the dominant observed cost
in this software-rendering/presentation path. It does not isolate raster work
from blocking, establish a hardware bottleneck, or attribute the complete
update interval solely to docking. One host run is a functional result and
an environment-specific observation, not a stable hardware performance baseline.

## Failed attempts and scope

The local Apple M4/Metal host returned `surface occluded` in an independently
observed locked macOS session. The archived renderer logs establish the drop;
the lock state was a separate local session observation. No successful workload
CSV was accepted. The attempt and actual drop diagnostics remain in
[macos-occluded.json](2026-11-08-egui-host/macos-occluded.json) and
[macos-occluded.stderr](2026-11-08-egui-host/macos-occluded.stderr).

In the one-CPU X11 container, the debug build exceeded the elapsed budget at
`Scroll/1`; the production build with default llvmpipe threading exceeded it at
`Scroll/13`. Both failures are retained with their commands and logs. Re-running
the production binary with `LP_NUM_THREADS=1` completed the full journey under
the same frame and elapsed limits. This supports matching software-renderer
threading to the container quota; it does not uniquely prove the source of the
earlier stalls or provide a matched before/after performance comparison.

Input injection here operates at the egui event boundary. The existing native
eight-pane editor, physical X11 input and multiview lifecycle qualifications
remain separate. The headless journey uses a test-owned Presented receipt and
zero timing placeholders; it verifies UI semantics, not WGPU or OS presentation.

The next useful performance capture is this same production exercise on an
unlocked, visible hardware-GPU host, with fixed window size and DPI. Compare
the complete host intervals first, then use the existing renderer-free kernel
benchmarks to investigate docking cost within the update. The software-GPU
total above does not justify another docking-core rewrite.

## Evidence and checks

- [Raw 384-row CSV](2026-11-08-egui-host/frames.csv) and
  [per-phase statistics](2026-11-08-egui-host/summary.json).
- [Successful command and source hashes](2026-11-08-egui-host/run.json),
  [actual renderer log](2026-11-08-egui-host/run.stderr), and
  [environment and binary/CSV hashes](2026-11-08-egui-host/environment.json).
- [Final production build](2026-11-08-egui-host/build.json),
  [debug timeout](2026-11-08-egui-host/linux-debug-timeout.json), and
  [production timeout](2026-11-08-egui-host/linux-production-timeout.json).
- [Verification commands and outcomes](2026-11-08-egui-host/verification.json).
  Formatting, all 25 official harness tests, correctness/suspicious Clippy
  checks and the Linux headless journey passed. The CI demo gate now runs that
  journey instead of only checking compilation.
  This application harness has no library target, so doctests do not apply.
- [Artifact checksums](2026-11-08-egui-host/checksums.json).
