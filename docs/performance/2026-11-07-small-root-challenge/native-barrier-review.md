# Independent review of the native drag-back input barrier

The barrier currently demands a surviving current pointer position, rather than proving that the new physical motion reached the UI. The pinned input implementation explicitly permits `PointerGone` to clear that position while the primary button remains down. This is a real mismatch between the barrier's implementation and its stated OS-event/UI purpose.

The failed run is consistent with motion followed by a legal leave, either in the same pass or before the next inspected pass. Its existing diagnostics cannot distinguish that case from a motion that never reached egui. Do not claim that this run uniquely proves either input loss or a same-pass move/gone sequence.

## Evidence binding

The inspected failure is run `42330748876`, head `9ddb63f425ca8316bdf483d6297e923c35a16ac3`, recorded in `retained-head-ci-failure/run.json`. The log SHA-256 is `e6a439409a4d0b80569db53c7831876668468a27f0c8d9c32a34bfc53e990edb`.

The four smoke source files read from the live tree match that CI head byte-for-byte. In particular:

| Source | SHA-256 |
| --- | --- |
| `physical_x11/app.rs` | `9670e6b4edacee973be5abd7b4e8eeacf18f705f01433c8160c95b4dbf6eb2e0` |
| `physical_x11/driver/scenario.rs` | `aab11b5e61ffb5280f032a4b72764203a07735d3776b9df26b73c629859d6916` |
| `physical_x11/driver/inspection.rs` | `9888b05fe588886b21c549ee8632f13492403b4a5ffdff042ad5a3fd071fdea7` |
| `physical_x11/assertions.rs` | `0682fabeb17f9ea3acfc69dae69dfb4254ffa20cacf7b852a4d09f7600cac6f7` |

The examined fork checkout is exactly egui `f94ee58b3914de7ad71dab36919b329ef36b4035`, paired with winit `2664589f0bd6fd03b2a5490f23bcd692853e63dd`. The input-state, egui-winit, native run/Glow/host-seam, inspection-plugin and X11 event/window files were checked against their respective committed blobs. No newer fork behavior was substituted.

## What the failure establishes

The relevant log lines are `native.log:5138–5149`:

| Observation | Consequence |
| --- | --- |
| Source hover: child `2CEC`, step 44, `(40,14)`, primary up | The child UI diagnostic and coordinate conversion initially worked. |
| Source press: step 46, `(40,14)`, primary down | The real press crossed the existing UI barrier. |
| Root checkpoint: step 47 | The pre-route root inspection completed. |
| XTest threshold: desktop `(572,241)`, expected child-local `(12,15)` | The driver injected a distinct motion and checked the desktop pointer/button state. |
| Product becomes nonquiescent; roster advances to 7 | Product/native activity occurred; this alone does not prove the exact UI motion. |
| 147 tree replies; last step 343; final lookup `Missing` | Inspection repeatedly produced newer output, without the required pointer label. |

`last_observation=(40,14,true)` is only the most recent **present diagnostic** that the loop found. `input_barrier` leaves that field unchanged on `Missing` (`scenario.rs:3586–3614`). It is not a record of the last raw motion, and does not establish that the UI pointer remained at the press location.

The final tree lists all 18 nodes and lacks the pointer diagnostic. It still contains the child workspace/tab, so this sample is not evidence of child removal. The final `os error 11` follows approximately three seconds of unsuccessful polling. `inspection.rs:67–75,144–164` bounds each socket read by the remaining absolute deadline; that terminal error does not explain the preceding missing labels. This is not evidence of a three-second inspection stall throughout the barrier.

## Exact input semantics

`PhysicalSemanticProbe::on_begin_pass` calls `publish_pointer_diagnostic` (`app.rs:877–890`). That function returns without creating its AccessKit node whenever `pointer.latest_pos()` is `None` (`app.rs:1832–1840`).

The pinned path permits exactly that state during a held gesture:

1. X11 `xinput2_mouse_left` forwards `CursorLeft` for an existing window without excluding grab/ungrab crossing modes (`winit/event_processor.rs:1741–1763`).
2. egui-winit translates `CursorLeft` to `PointerGone`, clears its local pointer position and requests repaint (`egui-winit/src/lib.rs:415–421`). Cursor motion separately appends `PointerMoved` (`927–958`).
3. `PointerState::begin_pass` processes these events in order. A move sets `latest_pos` and `interact_pos`; a later gone clears only `latest_pos`. The code explicitly preserves the held-button state and does not synthesize release (`egui/src/input_state/mod.rs:1142–1156,1217–1227`).
4. `interact_pos` survives gone within that pass, but the next pass initializes it from the now-empty `latest_pos` (`1135–1136`). A subsequent empty inspection pass can therefore have both positions absent while primary remains down.

This permits both sequences:

- one UI pass consumes `Moved(threshold), Gone`, so its pointer label is absent immediately;
- a UI pass consumes the threshold motion, then a later leave/pass removes the label before the driver obtains its next tree.

Neither sequence entails missing physical motion. Merely polling more fresh trees cannot restore a position that the integration correctly cleared.

The native controls make these sequences plausible without proving their occurrence in this log. The complete owned child-root drag requests pass-through (`dockspace/engine.rs:4128–4158`); routing tracks that request (`frame/pointer_passthrough.rs:204–239`). eframe applies it through `set_cursor_hittest(!enabled)` (`host_seam.rs:2983–3001,3104–3113`). The pinned X11 implementation changes the window input shape and retires explicit capture bookkeeping for that window (`window.rs:1719–1754`). Explicit capture also uses a real XI device grab with `NO_OWNER`, rather than keeping hover state artificially alive (`pointer_capture.rs:120–145`). Capture eligibility/results are not promises that `latest_pos` stays present.

The host callback and egui input are separate observations. The native run loop calls the host handler before normal window-event dispatch; only held close handling bypasses that dispatch (`eframe/native/run.rs:500–540`). For a known live viewport the Glow path subsequently forwards the event into egui-winit (`glow_integration.rs:1422–1428`). A host callback or capture/pass-through acknowledgement alone consequently does not prove UI consumption.

## Smallest reliable correction

Keep the physical XTest injection, desktop assertions, source viewport requirement, deadlines and route/guide assertions. Add one bounded **UI-observed motion sample per viewport** in the existing smoke probe, and use its freshness for the motion barriers:

- In `on_begin_pass`, read the already processed egui input's original ordered pointer events. Update the sample only when that viewport actually contains a `PointerMoved`; record its actual position and advance a viewport-local motion sequence. Do not derive it from the requested target or native host callback.
- Retain the last sample across empty/leave-only passes, and publish it through that viewport's AccessKit diagnostic even when `latest_pos` is absent. Publish the current primary-down state independently of position.
- Capture the motion-sequence baseline from the source snapshot **before injection**. Require a later source tree, sequence greater than that baseline, matching actual motion position under the existing tolerance and the expected current button state. Repainting, `PointerGone`, a host ACK or an unchanged old sample cannot satisfy the motion barrier.

The existing press snapshot can carry the threshold baseline. Thread the sample sequence forward for subsequent held-motion barriers in `reach_target_surface_while_pressed`; changing only the first threshold check would leave the same semantic problem at the ingress checks. Keep existing hover/button-state checks where current state is the intended contract. This requires a small smoke diagnostic/parser/caller change, not a fork input-behavior change or a new inspection protocol.

For causal diagnostics, the same sample needs only the move's pass number plus the most recent gone pass/order and current `latest_pos`/`interact_pos`/primary state. Publish an explicit absent position rather than dropping the entire diagnostic. That distinguishes same-pass erasure from later-pass erasure. If the fresh motion sample never advances, retain the failure and compare a small correlated native `CursorMoved`/`CursorLeft` record with this UI record; native evidence is diagnostic, not a substitute success condition.

Switching solely to `interact_pos()` is insufficient: it helps the same pass but loses the observation on the next empty pass. Accepting a new tree step, nonquiescence, roster change or pass-through ACK alone would weaken the OS-to-UI barrier. A persisted position without an independently captured sequence baseline would also permit stale success.

## Minimal meaningful witnesses

Before treating the correction as qualified, exercise the real smoke plugin with a held press followed by `Moved(expected), Gone`, then an empty pass before inspection. The UI sample must prove the new motion while current position remains absent and primary remains down. Run the paired negative with only `Gone`/empty passes: its unchanged sample must not satisfy a new motion baseline. An actual release must fail a held-motion check.

Then run the unchanged physical drag-back sequence with the additional diagnostics and original timing/OS strength. A missing new UI sample remains a genuine failed handoff requiring diagnosis; this proposal does not assume every native event necessarily reaches the required source UI.

Only this report was written. No Cargo, compilation, test, timing, profiling, network or delegated work was performed, and no MRU analysis report was read.
