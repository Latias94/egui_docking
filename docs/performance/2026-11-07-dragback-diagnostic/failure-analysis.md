# Native physical drag-back failure: source and trace analysis

**Finding.** The first established failure is the threshold-stage child semantic barrier. The available evidence does not identify a proven product root cause or justify a semantic/runtime fix. In particular, the later `Retired + Edge(North)` observation must not be treated as the initiating event: the driver unconditionally releases the physical primary button after a failed held-drag operation, before reporting its error. Cleanup release at the threshold location is a sufficient, source-supported explanation for that later edge commit.

The smallest useful next change is failure observability in `input_barrier`, preserving the exact current deadline, input sequence, and assertions. Capture the failed stage and its existing attempts/last snapshot before cleanup release. If the unchanged-head rerun fails again, a bounded trace at existing native callbacks can then distinguish a missing child pass/admission wake from received-but-mismatching pointer facts. No timeout increase, blind retry, altered target, skipped child barrier, or weakened Center/retirement assertion is justified by this trace.

This report uses the existing CI failure and historical green artifacts plus immutable source. No Cargo, compilation, test, smoke runner, timing, profiler, network request, or agent was run. Only this report was written; repository, fork, cache, and candidates were not modified. The diagnosing-bugs workflow was restricted to evidence analysis as explicitly requested, so this is not a fresh reproduced or repaired result.

**Evidence binding.**

- Failed run `42323715317`: head `20fe10df0a97340858db3523da1b17deb5075d18`. The failed step is `Run physical X11/XTest docking smoke`; the preceding physical unit-test step and native lifecycle/host gates passed according to the existing job receipt.
- Historical green run `42319679465`: head `59a498086caee70ec9e3d2403711f16bfff6dc09`. Its immutable receipt and complete log are preserved under `docs/performance/2026-11-07-binary-inputs/`.
- Failed receipt SHA256: `9f384a064f2396710edc36e8a47bb4b1a5aa32f51e518de95cff939d30555321`.
- Failed log SHA256: `280f16315a153189cc4bd66f7de723b46f7c5c9f71de214dec836202dbdc0340`.
- Historical green receipt SHA256: `aababe8ea37b13020f4c496efab4f6cee3d4f5c1f3e143119136bd8cb075c2c2`.

The smoke driver/scenario, inspection client, X11 injector, assertions, app, native adapter, CI workflow, native workspace manifest/lock, and fork pins are byte-identical between the two heads. Runtime changes in this interval are confined to `layout.rs`, `layout/projection.rs`, and `layout/topology.rs`; the fourth changed core source is layout tests. A scheduling-sensitive failure can be exposed by unrelated timing changes, but this comparison does not prove a layout semantic regression or establish timing as the cause.

Relevant immutable source hashes:

| Source at failed head | SHA256 |
| --- | --- |
| `physical_x11/driver/scenario.rs` | `85454244bd86a09978066b3a9af4f34f5912c28a95b1d9bb46b19ccd2acc34c7` |
| `physical_x11/driver/inspection.rs` | `9888b05fe588886b21c549ee8632f13492403b4a5ffdff042ad5a3fd071fdea7` |
| `physical_x11/driver/x11.rs` | `535a3cd3db366ea0e0dc7ec29b5f7cab1e86f1268c74b6bfb0907d5b5079f5ce` |
| `physical_x11/app.rs` | `9670e6b4edacee973be5abd7b4e8eeacf18f705f01433c8160c95b4dbf6eb2e0` |
| `physical_x11/assertions.rs` | `0682fabeb17f9ea3acfc69dae69dfb4254ffa20cacf7b852a4d09f7600cac6f7` |
| Native `mailbox.rs` | `b0d3b193cc4adfa1d96d92b5d4d27f7ed144c1875596e4c32a78d0b4c8bec3f6` |

Smoke paths above are under `integration/egui-native-smoke/src/bin/`; native paths are under `crates/egui_dockspace_native/src/`. All function/line references below refer to that failed immutable head unless explicitly identified as fork source.

The actual egui pin is `f94ee58b3914de7ad71dab36919b329ef36b4035`. Its local Cargo Git checkout was verified against that commit; `repo-ref/egui` is not the pin and was not used as native protocol authority. Pinned inspection plugin SHA256 is `902fae4ba28ace39c3cc30dad3afa8cd7124ab733b8a9b6fc3a8fe97ab74a447`; pinned Glow integration SHA256 is `5e93f3218bfbcdb721f17c4ccf010f584b5ea028ee3fe53678ed8c2debe84325`.

**Where the failed sequence diverges.** All times below are November 7, 2026, UTC.

| Time | Established observation |
| --- | --- |
| 01:49:35.922 | ChildFirstLive, quiescent true |
| 01:49:35.948 | Child Tab stable at step 34, roster epoch 6, local bounds x=28…77.28125, y=0…28 |
| 01:49:35.981 | Root target stable at step 38; source/threshold/ingress coordinates logged |
| 01:49:35.991 | Child hover step 39, local `(40,14)`, primary up |
| 01:49:36.010 | Child press step 41, local `(40,14)`, primary down |
| 01:49:36.018 | Root pre-route checkpoint step 42 completed |
| 01:49:36.029–.048 | ChildFirstLive remains, now nonquiescent; roster changes to epoch 7 |
| 01:49:39.037–.075 | Roster epoch 8; product reports Retired, nonquiescent, `Edge(North)` |
| 01:49:39.078 | Driver returns child `GetViewportTree` socket read EAGAIN and exits failed |

`reach_target_surface_while_pressed` (`scenario.rs:2271–2298`) performs the successful press barrier, records root checkpoint 42, then moves the pointer to `(572,241)`, asserts the X11 position/button state, and waits for the child's threshold observation. There is no successful threshold log. The next planned motion to root ingress `(381,244)` at line 2299 has therefore not been reached on a successful return from that barrier; nor have route confirmation, stable Center guide selection, or intended Center release.

`input_barrier` derives a three-second stage deadline at lines 3553–3555. `InspectionClient::request` sets the socket's remaining absolute read budget before `read_message` (`inspection.rs:67–75,144–148`). The reported EAGAIN is a TCP response read failure at that budget, not an X11 injection EAGAIN, a typed Removed/Unknown viewport response, or a core `InvalidPointerFacts` error.

The trace currently cannot prove that no child trees were received throughout the entire interval. At `scenario.rs:3565`, `inspection.tree(...)?` exits immediately on a request error. Any earlier successfully received snapshots, attempts, last step, and mismatching pointer observations are then lost. The useful detailed error at lines 3603–3607 is bypassed. This is a confirmed diagnostic gap.

**Why retirement is downstream evidence.** `inject_redock_drag` evaluates its held-drag closure into `pressed`, then always calls `desktop.set_primary_button(false)` before matching the result (`scenario.rs:2221–2229`). It returns a stored `pressed` error only after that physical release. The X11 injector flushes and queries the real button state (`x11.rs:357–380`). Thus an inspection failure can still cause product mutation during cleanup.

At the threshold point, desktop `(572,241)` also lies within Root's overlapping client bounds `(280,147)…(1000,667)`, corresponding to Root local `(292,94)`. A held native drag can acquire an edge preview there; cleanup release can commit it. This matches the late actual model observation. `publish_observation` derives Retired only after the Child item is in the Root surface and the former child surface is no longer presented (`app.rs:1508–1565`); it does not merely echo a request to close a window.

The exact callback/release timestamp and selected preview before cleanup are not logged, so cleanup causality is a strong explanation rather than an independently timestamped proof. A marker immediately before cleanup release plus the existing native button callback would settle the ordering. There is no recorded evidence of retirement before the threshold barrier started.

**Coordinate, control, and lifecycle hypotheses.**

| Hypothesis | Evidence and falsifiable boundary |
| --- | --- |
| Child semantic-output/admission/wake progress is lost or delayed after threshold/capture/passthrough | Highest-value next investigation. The pin completes child tree requests only on that exact viewport's output, not any Root output. A trace with zero child begin-pass/output after the threshold event, alongside native admission/terminal states, would support this branch. |
| Child geometry/input coordinates changed, so received trees failed the frozen-position comparison | Still possible, because the failure discards intermediate observations. Requires advancing child snapshots with pointer position inconsistent with the frozen client geometry, or a changed native/X11 client rectangle. Current roster epoch changes alone do not prove a move: the epoch covers all native roster facts, not only geometry. |
| Initial press landed on Close or an unstable wrong tab | Weakly supported. `perform_drag_back:827–841` selects Tab bounds and excludes the semantic Close control through `tab_drag_point`; hover/press both observe `(40,14)`. Both green and failed runs have identical discovered geometry and pointer coordinates. Close deletion also does not explain a surviving Child item redocked as a north split. Retaining/rechecking the same press snapshot's selector bounds would make this negative explicit. |
| Child disappeared before threshold, causing the read failure | Not established. No early retirement or typed Removed/Unknown reply is recorded. The pin would report unavailable inventory when it observes removal. Exact roster/window facts before cleanup are required to establish this branch. |

The threshold is local `(12,15)`: inside Child's client `(560,226)…(1280,746)`, but left of the original Tab's x=28 boundary. This is intentional held-drag motion, not a new press on a different control. The historical green run uses exactly the same source `(600,240)`, threshold `(572,241)`, ingress `(381,244)`, routed ingress `(395,254)`, and both window rectangles; it observes local `(12,15)` at child threshold step 47 and then completes Center redock/retirement/quiescence. Changing the threshold solely because it is outside the original Tab would not address a demonstrated regression.

Semantic discovery already requires two stable selector observations with a stable roster epoch (`scenario.rs:303–355,3665–3695`). It is followed by a matching hover and press observation. This is not evidence of a missing initial stability wait. Nevertheless, the Tab/Close bounds used to choose the point come from the earlier discovery snapshot, so validating those bounds against the already obtained press snapshot is a useful fail-closed diagnostic if control geometry is suspected.

**Pinned protocol boundary.** In pinned `egui_inspection/src/plugin.rs:320–333,415–440`, GetViewportTree enters an exact-viewport waiting phase, replies Removed when its cached viewport is removed, and otherwise requires that viewport's fresh output. `maybe_repaint:161–177` explicitly rearms Root and pending child RequestPaintWhileHidden commands, including the existing consumed-wake regression. Returning cached old child data or Root data would violate this contract.

In pinned `eframe/native/glow_integration.rs:846–905`, native output admission Defer or pre-input admission rejection can return before egui runs any pass. Inspection's output hook cannot observe such an attempted pass. The project records a blocked semantic viewport when child output reservation is deferred (`native/mailbox.rs:5309–5318`) so a later complete Root boundary can release its continuation. These are specific liveness boundaries to observe, not proof that either failed here. The ordinary guide-control smoke currently records neither output admission decisions nor threshold native motion/capture facts.

No explicit native window move request was found in the inspected smoke/product drag path. Passthrough changes alone are not window destruction and must not be treated as authority retirement. The final EAGAIN also does not justify accepting stale child input or bypassing the existing native full-generation/ordinal/capture checks.

**Minimum diagnostic and meaningful validation.** First preserve the cheap information already owned by the driver: replace the bare tree-error propagation with an error that includes the exact stage, viewport, previous step, expected logical/physical point, frozen client bounds/scale, successful-read attempts, last step, last pointer observation/lookup, and bounded last-tree summary. Emit this failure context before cleanup release and mark that a real cleanup release follows. Do not lengthen the budget, add an input, or convert a mismatch into success.

If that still leaves the boundary ambiguous, temporarily record a bounded last-fact/counter set through existing `DiagnosticNativeHost` hooks: the threshold motion/button ordinal and window/viewport/frozen coordinate facts in `on_window_event`; capture/passthrough result and wake; and child output token/admission, `before_output_input` decision, pass finalization, and terminal wake. The current wrapper already forwards these hooks (`app.rs:939–960,989–1017,1068–1091,1198–1236`). Correlate only this stage/child; no unbounded event history or new production routing mechanism is needed. At failure, the existing copied RootRosterKey can disclose whether bounds, input routing, or inventory actually changed.

A true fix must follow the resulting discriminator: restore a lost exact child continuation if the admission/wake trace proves one; correct coordinate correlation if advancing trees prove geometry changed; or correct authoritative lifecycle handling if retirement actually precedes release. This report does not select a speculative runtime patch.

The unchanged-head rerun is useful evidence about intermittency, but success alone will not identify or repair the underlying cause. Keep all present semantic requirements: real source hover/press/threshold facts, actual passthrough ACK, Root preserved, exact fresh Center guide/preview before release, Child in the intended Center layout, quiescence, exact X11 DestroyNotify, and removed semantic viewport inventory. A diagnostic unit witness should confirm that a tree transport error retains prior mismatch observations and still fails; a liveness regression belongs at the real native admission/inspection seam once its failing ordering is known. Do not treat EdgeNorth as an acceptable alternative, nor retry a timed-out uncorrelated request/response stream as though its eventual reply were fresh.
