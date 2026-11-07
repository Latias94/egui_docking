# Final official host exercise code review

**No new renderer, docking, normal-mode or CI blocker was found. Three small assertion/boundary corrections remain in the source read for this report; all were identified with Root during review.** Resolve these before treating the exercise's documented bounds and content preservation as exact. No additional framework, benchmark or platform implementation is warranted.

Scope: `.github/workflows/ci.yml` and the official harness manifest, README, demo and directly read untracked `exercise.rs`. This is static review only. Root's headless and actual-window executions are independent evidence, not tests executed by this reviewer.

## Remaining actionable findings

### P2: frame-admission check allows attempt 641

`exercise.rs:140` checks `self.frames > 640`, while `finish_frame:238` increments the counter after every attempted output, including setup drops. When 640 attempts have finished, one more call is admitted because `640 > 640` is false.

Change the admission comparison to `>= 640`. This preserves the intended ability to complete on attempt 640 while rejecting a new attempt afterward. It does not require a new fixture or a test of counter representation.

The elapsed bound is checked at frame preparation, not a hard preemption of a blocked renderer call. Do not interpret the declared 90 seconds as a mechanism that can interrupt WGPU or the OS.

### P2: final Scroll output lacks the content-only version check

The current version invariant is checked in `prepare:149–158`. Adjacent Idle/Hover/Edit content phases catch preceding outputs on the next preparation, but Scroll transitions directly to PressSplitter. A mutation on Scroll's final output is therefore not checked by that invariant.

Move the existing Idle/Hover/Edit/Scroll version comparison to `finish_frame`, after successful presentation and before recording a sample or advancing the phase. It then checks the actual committed result for every content-only frame, including Scroll's final frame, without duplicating the condition. The check uses existing `dock.version()` and baseline state; no new validator or test framework is needed.

No additional independent correctness finding requires expanding this patch.

### P2: retain the inserted-text count through the final drag

The Edit-stage check requires exactly `PHASE_FRAMES` marker characters. Final ReleaseTab validation, however, only strips all markers and compares the original text. That permits loss of some or all inserted markers during subsequent interaction while still declaring retained content.

Repeat the existing `text.matches('~').count() == PHASE_FRAMES` condition in the final conjunction, alongside the existing original-text check. This directly verifies that the actual added content survives the drag; no new fixture or general content digest is needed.

## Setup drops and diagnostics

The new retry is limited to `Phase::Setup` (`exercise.rs:238–247`). Each attempted output increments the same total counter. A setup drop increments `setup_drops`, returns before mesh sampling and step advancement, and cannot become a successful warmup or CSV row. Setup advances after eight successful outputs; drops do not need to be consecutive to remain honestly counted. Any drop in Idle, Hover, Edit, Scroll or either held gesture still returns an error.

The host reports the real presentation receipt **before** the exercise sees the outcome (`demo.rs:364–379`). The retry does not turn a Dropped output into Presented, bypass settlement, or execute a new hidden gesture. Pending texture deltas remain owned until a successful acquisition. Startup failures are counted and logged, and the same total frame/elapsed budget applies.

The repaint delay requested after a setup drop avoids the exercise's own immediate redraw branch. Existing renderer recovery requests for Lost/Outdated/Timeout remain unchanged. Persistent occlusion or validation cannot yield a successful workload: it eventually fails the bound. A closed/failed window also cannot yield a successful CSV, because `require_complete` checks the terminal phase.

`drop_reason` is private diagnostic state reset for each paint attempt, populated at each failure branch, and does not change the original WGPU result. Adapter/window logs, first-drop diagnostics, and explicit `set_visible`/`focus_window` calls are opt-in. These are appropriate for distinguishing the locked macOS/Occluded run from a successful real-window run, without modifying screen-lock state or platform authority.

## Timing and presentation

The accepted naming remains `ui_to_present_ns`: start before the full public `run_single_surface`, end after receipt reporting. It includes platform output handling, tessellation, texture/buffer work, render encoding, submission, actual present call and settlement. It excludes native input extraction, driver preparation, coordinate inspection, semantic assertions and CSV flushing. The README states those exclusions and does not call the metric full event-loop CPU time.

`update_ns`, `tessellate_ns` and `paint_present_ns` are separate CPU-side elapsed windows, not exhaustive partitions whose medians can be added. Surface acquire/present may block. The README correctly excludes GPU execution duration, scanout and OS-input-to-photon latency.

The renderer still acquires before draining texture updates, submits actual commands before freeing textures, calls `pre_present_notify` and `queue.present`, then returns Presented. Failed acquire paths return Dropped. Neither the driver nor the headless test changes this real renderer sequence.

A Linux Xvfb run with a software adapter may establish that the actual surface, renderer and present path work. It does not establish Apple M4 rendering performance or hardware-GPU duration. Keep the adapter/device/backend and build/environment logs with any timing CSV. The locked macOS run is a failed environmental attempt, not an accepted timing sample.

## Semantic assertions and normal behavior

The exercise injects ordinary egui RawInput events and deliberately discards external native event payloads. It is deterministic UI-input replay in a real host, not physical OS injection. Native eight-pane/X11 gates remain separate.

Text validation requires the exact marker count and retention of the original document. Scroll now targets the actual `ScrollAreaOutput.inner_rect`, retains a filled viewport through `auto_shrink([false, false])`, and checks its clamped offset. These changes exercise the actual widget rather than a larger pane rectangle that may not be scrollable.

The splitter pre-press snapshot reads the intended top binary split's weight bits. After eight release frames, the result requires changed durable weights, a changed workspace version, and changed current Outline geometry, as well as the earlier held movement. This verifies committed state after the preview, rather than only a historical preview rectangle.

Final tab validation requires `[OUTLINE, PREVIEW]` with Preview selected, Editor's original group reduced to `[EDITOR]`, eight live stable items and original text preserved. The marker-count correction above extends that final check to the newly inserted content. The journey uses actual pointer press/move/release across distinct frames; no direct docking or selection command is substituted. AccessKit bounds remain egui logical coordinates, matching the injected event positions.

With no exercise argument, native input is not replaced, no finite phase driver runs, AccessKit is not forced on by this path, and completion does not auto-exit. Normal rendering, texture retention/recovery and repaint handlers keep their previous control flow. The expanded eight-item content/layout is an intentional interactive-demo change. The new private diagnostic assignments and clocks do not introduce another renderer or platform seam.

The 384 stored rows describe held-stage frame costs: press/release/settlement stages execute but are unrecorded. Keep that distinction when reporting drag performance. Success creates a new file only after final semantic validation; no existing CSV is overwritten.

## Headless test and CI

The single registered test runs the real owned UI path and tessellation through all six phases, checks the same semantic observations, requires completion and 384 samples, and cleans up its temporary CSV. It explicitly drops texture deltas as a headless owner, reports a test-owned Presented receipt and supplies zero timing fields. Its comments and README correctly deny GPU/OS evidence.

The test is not coverage of WGPU acquisition failure/recovery or native window events. The actual-window run must supply that separate evidence. Adding a mirror state-machine suite or pretending the headless test certifies startup drops is unnecessary.

The official CI job already installs nextest. Its changed command enables the existing demo feature and runs the registered journey test by the matching filter. This requires no DISPLAY, Window/EventLoop creation or GPU adapter because the test only constructs Context and Dockspace. Enabling the feature also compiles the real demo code. It replaces the previous compile-only gate with a stronger semantic gate, without changing other jobs or dependency pins. No source-level CI issue was found.

## Review bindings

The following hashes bind the source actually reviewed before the three outstanding local fixes. Root may format or apply those corrections afterward; that is a new source binding, not an unreported test result here.

| File | SHA-256 |
| --- | --- |
| `.github/workflows/ci.yml` | `2503d3cb5eaff5bb09a6ea40b5f5b6cd06110e1d19a66b096ec068d8ce936392` |
| `integration/egui-official-harness/Cargo.toml` | `e3cb03cab5cf6b234738d20f1907442b452a037eddfc8055a1654ae9b3df98ec` |
| `integration/egui-official-harness/README.md` | `9b7de51d16b7e0d336d38e1f0090351d7f1156bc6745b4adec19c1d7d63f409a` |
| `integration/egui-official-harness/src/bin/official_caller_owned_demo.rs` | `7d5b1f72dff1b3fa86300df36300e99b82358fd9bee159fdc5fb37f407636688` |
| `integration/egui-official-harness/src/bin/official_caller_owned_demo/exercise.rs` | `d31eb9bd8f864c2515e0077e7dd5bc59e19ce1861810de0c1bbba2d5eb5830f1` |

Only this report was written. No Cargo, build, test, GUI, timing, profile or network command was executed, and no repository file was modified.
