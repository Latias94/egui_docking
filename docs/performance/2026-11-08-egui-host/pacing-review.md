# Linux exercise pacing: static diagnosis and completed run

**No fixed low-frequency redraw throttle or missing-successful-redraw loop was found in the demo or the local winit X11 path.** The completed single-worker run establishes that this unchanged host journey can finish through the real software-rendered surface within the existing bound. It does not uniquely establish why the default-worker runs were slow.

No renderer rewrite, new scheduling framework, weakened phase assertion or enlarged bound is justified by the evidence reviewed here.

## Application and winit control flow

The successful-frame branch at `official_caller_owned_demo.rs:391–397` always calls `window.request_redraw()` until the finite journey completes. It does not wait for a repaint callback or a delayed timer before requesting the next successful frame.

The only new explicit pacing delay is `request_repaint_after(16ms)` on a **Dropped setup output**. Any workload-phase drop fails before this branch can continue. The successful Linux log reports zero startup drops, so that recovery delay does not explain the completed run's steady-frame cost.

The ordinary repaint handler retains `WaitUntil` for positive egui repaint delays. That does not suppress an already queued redraw on the reviewed winit X11 implementation:

- `winit-0.30.13/src/platform_impl/linux/x11/window.rs:1844` sends the window ID to the redraw channel.
- `x11/mod.rs:435–464` includes incoming redraw requests in `has_pending`; pending work forces a zero poll timeout instead of the `Wait`/`WaitUntil` deadline.
- `x11/mod.rs:553–570` drains/coalesces redraw requests and dispatches `RedrawRequested`. A new redraw requested inside that callback remains pending for the next iteration.

There is no application sleep, per-frame GPU completion wait, 400ms timer, or deliberate approximately 2.5fps cap in these paths. The source therefore does not support blaming the exercise's repaint scheduler for the observed failure progress. This is a control-flow finding, not a measurement of event-loop overhead.

## The actual renderer can block

The demo retains AutoVsync and includes acquire, texture/buffer work, render encoding, submission and backend present in `paint_present_ns`. The local `wgpu-hal-30.0.0/src/gles/egl.rs` implementation:

- Returns the configured renderbuffer from `acquire_texture` at line 1440, without a source-level sleep.
- Performs context changes, a full framebuffer blit and `eglSwapBuffers` in `Surface::present`, lines 1112–1188.

Those driver/OS calls, plus software rendering and queue work, can take time not explained by the application's redraw logic. AutoVsync is an environment/backend choice, not evidence of a particular 400ms wait. The current aggregate timer does not identify which internal operation dominates. Do not label all of it GPU execution or swap waiting.

## Observed attempts and successful environment

Preserved receipts report:

| Run | Result |
| --- | --- |
| Linux debug, default worker environment | Failed the unchanged exercise bound at `Scroll/1` |
| Linux production, default worker environment | Failed the unchanged exercise bound at `Scroll/13`; process return code 1, not an external timeout |
| Linux production with `LP_NUM_THREADS=1` | Return code 0; 384 UI sample rows; all six phases completed; zero startup dropped outputs |

The successful command uses the same production executable path, window/layout and recorded source hashes as the failed production attempt, with the thread-limit variable and a distinct new output path. Root reports the same frozen executable was used. The successful environment receipt binds binary SHA-256 `6a9bda6032f4fbf4652120fce3f5b7d4581aea5f9200d355e0e089c62fda5490`.

The environment is one CPU quota, 4GiB memory, Xvfb, scale 1, 1280x820 framebuffer, GL backend, llvmpipe/Mesa CPU adapter and AutoVsync. This is real surface/render/present **functionality with software rendering**, not Apple M4 GPU performance or physical-input evidence.

The successful receipt spans approximately 47.25 seconds. Its saved summary reports UI-update p50 0.304–0.383ms and paint/present p50 97.071–107.459ms, with UI-to-present p50 97.411–107.887ms across the six phases. I read those recorded values; this pacing review does not independently re-audit the summary calculations or assign an internal driver time share.

The evidence is consistent with environment-sensitive software-renderer scheduling and makes a missing-successful-redraw explanation less plausible. It is **not** a controlled proof that worker oversubscription is the sole cause. There is no complete default-worker CSV for an equivalent phase comparison, no repeated environment pairing, and no isolation of shader/cache history or other host scheduling variation. Do not turn bound failure versus completion into a numerical speedup claim.

## Disposition

The immediate functional pacing problem is resolved by the completed, source-bound single-worker environment without changing the journey, phase drop rules, renderer or 640-frame/90-second admission bounds. Preserve both earlier failures and the successful environment setting. No additional run or code change is requested by this review.

If a later task requires deeper attribution, use the already present UI/tessellation/paint windows and retain bounded partial results on failure before considering a new measurement mechanism. The present task needs only honest software-GPU/environment labeling and final ordinary qualification.

## Bindings

Evidence directory: `/tmp/dockspace-egui-host-20261107`.

| Artifact | SHA-256 |
| --- | --- |
| `linux-native.json` | `d605aa5bd08bb9ebc1ddf95d72c5c2618dce028e62349b0bff619db80fce3a62` |
| `linux-production.json` | `31f37ab477bf16a4ef65094f81aad5d87d69a94bed0e9bca1e6b4c7c509bc420` |
| `linux-single-worker.json` | `be6ef9ce4e9937a18a89bb351752ee69400436e34468ababf58702aaadfabff6` |
| `linux-single-worker.stderr` | `d4fed744d982eb31452926deec50c6207cfac876b4aab10901b8ae447ec32766` |
| `environment.json` | `7ea4ff7bc642639fb7a62ab218fafc921ba43d2302a0b3d935fc4b3a527dbcec` |
| `summary.json` | `f2ccaaf7813ebd74c41ea25be0e9e0b7d8264e65ef119831a63c0bfc99ff69f0` |
| Demo source | `58033b120d976d04f7753eceb5a94e05a31d5fc09b85287b9f8dd807327ccfb3` |

Only this report was written. No Cargo, compile, test, GUI, runner, timing, profiling or network action was performed; repository, cache, fork and candidate contents were not modified.
