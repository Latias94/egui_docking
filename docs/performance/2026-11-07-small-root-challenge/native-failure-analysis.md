**Finding.** Run `42330748876` establishes a failing Child input-state barrier, not a three-second inspection-service deadlock. There were **147 successful Child tree reads**, and the final returned step was **343**, after press step **46**. The final tree contains the Child tab and content, but no pointer diagnostic. The final socket EAGAIN occurs after those unsuccessful semantic comparisons have consumed the stage budget.

There is a concrete race in the smoke contract: it treats a delivered threshold Move as observable only while that coordinate remains egui's current `pointer.latest_pos()`. The pinned native path can start drag routing before extracting Child UI input, and the ensuing pointer pass-through can produce `CursorLeft`/`PointerGone`. Egui deliberately clears latest_pos without releasing the held button. Consequently, a real Move can start the product drag yet become permanently unobservable to this state-based barrier. Fresh inspection output does not recover an expired state.

The exact failed-run Move/Gone/platform-ack ordering is **not recorded**, so this report does not declare a proven product event-delivery defect or claim that one particular batching sequence has been observed. The useful correction is an occurrence-based Child semantic receipt with strict freshness and ordering, preceded or accompanied by the small distinguishing trace below. No timeout, retry, skipped Child barrier or production routing delay is justified.

Only this report was written. No Cargo, compiler, tests, runner, timing, profiling, network request, delegation, repository/fork/cache or candidate modification occurred. The diagnosis skill was restricted to the supplied CI evidence and read-only source analysis; this lane did not independently reproduce or repair the failure.

**Evidence and comparisons.** All timestamps below are November 7, 2026, UTC.

| Evidence | Established result |
| --- | --- |
| `B/retained-head-ci-failure/run.json` | Head `9ddb63f425ca8316bdf483d6297e923c35a16ac3`, attempt 1, native coordinator job failed; the other five jobs passed |
| Same directory, `native.log:5136–5143` | Stable Child tab step 38; hover 44 and press 46 at local `(40,14)`; Root checkpoint 47 |
| `native.log:5144–5146` | Nonquiescent product, then roster epoch 7; threshold barrier fails before cleanup release at `03:16:46.582` |
| Diagnostic at line 5146 | Expected desktop `(572,241)`, Child local `(12,15)`, primary down; 147 reads; last step 343; last lookup Missing; final tree 18 visible nodes |
| `native.log:5147–5148` | Roster epoch 8, then Retired/Edge(North), **after** the explicit pre-cleanup error marker |
| `B/unified-ci-failure.log:87–94` | Original `20fe10d` attempt 1 fails at the same threshold stage, then reports the same downstream north redock; it lacks intermediate-read diagnostics |
| `B/unified-ci-rerun-attempt2-native.log:5140–5164` | Identical `20fe10d` head succeeds: press 45, Root checkpoint 46, threshold step 48 after two reads, Child local `(12,15)`; ingress, pass-through ACK, Center, release and quiescent retirement succeed |

`B` is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`. A read-only immutable diff from `20fe10d` to `9ddb63f` across smoke/native/product sources, fork workspace pins/lock and CI workflow shows only the previously reviewed `scenario.rs` diagnostic delta, +14/-1. The physical/native implementation and pins did not change across these failures. A same-head successful rerun demonstrates scheduling sensitivity; it does not prove which event was lost or identify a numerical-layout cause.

The diagnostic's `last_observation=(40,14,true)` is retained across later Missing results (`scenario.rs:3586–3614`). It is **not** a claim that step 343 still had that observation. Nor does the log count how many of the 147 reads were mismatches versus Missing. It does prove repeated completed Child output and a final missing label. Do not reinterpret Missing as a missing response.

**The source-supported causal chain.** Repository references use immutable `9ddb63f`; fork references use egui `f94ee58b3914de7ad71dab36919b329ef36b4035` and winit `2664589f0bd6fd03b2a5490f23bcd692853e63dd`. Both local checkout HEADs were verified at those pins.

1. The driver proves Child press, obtains a Root output, then physically moves to threshold and waits for a Child tree with exactly that position and held button (`scenario.rs:2277–2304`). Root ingress is not injected until that barrier succeeds. The final north commit therefore cannot be blamed on a completed intended Center sequence.
2. Native `on_window_event` captures pointer callbacks into the journal (`mailbox.rs:6198–6221`). The native pre-input hook services pending prefixes **before** `take_egui_input` (`surface_driver.rs:245–290,315–375`; fork Glow `846–920`). Core threshold crossing calls `begin_drag` and `begin_drag_routing` (`engine/pointer_transaction.rs:1085–1132`), which can request SetPointerPassthrough (`frame.rs:3664–3689,2510–2530`). There is no contract that Child's diagnostic UI must first publish the threshold state.
3. Post-commit dispatch submits the queued route command (`surface_driver.rs:1341–1359`). The fork applies it with `window.set_cursor_hittest(!enabled)` (`host_seam.rs:2983–3000,3104–3113`). Pinned winit implements this by retiring explicit capture and replacing the X11 input shape with an empty rectangle list (`x11/window.rs:1719–1754`). Its XI leave handler emits CursorLeft (`x11/event_processor.rs:1741–1763`). These are actual code paths, not proof that this run's epoch 7 was that ACK: the epoch summarizes roster changes and is not a typed pass-through receipt.
4. Egui-winit converts CursorLeft to PointerGone (`lib.rs:415–421`). Egui processes raw events in order: PointerMoved sets latest_pos, while PointerGone clears it and explicitly **does not release** the pointer (`input_state/mod.rs:1142–1156,1219–1227`). CursorLeft is Other rather than core pointer ingress (`native event.rs:42–59`, `mailbox.rs:1972–1980`), so it does not by itself cancel the journal-owned product drag.
5. `PhysicalSemanticProbe::on_begin_pass` calls `publish_pointer_diagnostic`, which emits nothing when latest_pos is None (`app.rs:877–890,1832–1856`). If Move and Gone arrive in one UI batch, the final state is already None. Even if a short-lived threshold state was emitted in an earlier pass, a later exact-viewport request need not observe that pass. Subsequent empty-input passes preserve the absence.

This explains why existing product progress and absent Child pointer state can coexist. The successful rerun's threshold-before-ACK ordering is compatible with winning this race. The failure's native callback and raw-event sequence are still needed to distinguish this mechanism from a Move delivered only to another viewport, a viewport-state reset, or a different admission ordering.

**Inspection and settlement boundary.** Pinned InspectionPlugin rearms pending Child paint requests (`plugin.rs:161–177`) and replies to GetViewportTree only from the selected viewport's output (`415–440`). The client rejects a response without an AccessKit tree (`driver/inspection.rs:90–120`). Thus the 147 successful reads are not cached Root substitutes or typed no-tree replies.

They prove egui output progress, **not successful native presentation/publication**. The inspection output hook runs inside `end_pass_before_finalization`; the native finalizer follows it (`context.rs:1188–1203,3289–3297`). Native Defer can prevent some passes before extraction, while a later retained/skipped result can suppress accessibility at settlement (`surface_driver.rs:87–98,1204–1225`). Do not turn an inspection receipt into new product authority or bypass those barriers. Equally, sustained admission starvation cannot explain all 147 completed Child passes. There is no evidence supporting a wake-protocol fix for this failure.

**Minimum distinguishing observation.** Keep input coordinates, stage deadline, assertions and cleanup unchanged. At failure, expose:

- Existing pass-through callback sequence/status/window and the current Child window identity; the wrapper already retains route facts (`app.rs:1068–1089`).
- Last real Child CursorMoved ordinal/position, CursorLeft ordinal, primary press/release and capture-loss ordinal in the existing native wrapper. A few fixed last-fact slots suffice; no unbounded event log is needed.
- At the semantic probe, the last raw PointerMoved and PointerGone order within the Child pass, its native output token/pass identity, resulting latest_pos and primary-down state. Retain the step associated with the last matching diagnostic, rather than only its value. Include that token's admission and finalization disposition only for the relevant boundary.

A trace showing threshold Move, route Applied, then Child Gone with no later release confirms state expiration. No native threshold Move points toward X11 routing/injection; native Move but no corresponding raw Child Move points toward the adapter/extraction boundary. Raw Child Move with later Gone and a fresh missing label directly confirms the smoke-observation race. This small trace avoids guessing from roster epochs.

**Concrete bounded fix.** Make movement barriers require a Child semantic receipt of an actual consumed raw PointerMoved, rather than persistence of current hover. Keep the existing state diagnostic for hover/press/release where appropriate. A smoke-only per-viewport latest receipt is enough; do not build a generic journal or change native gesture authority.

The receipt must advance only on real ordered input events, carry the actual local coordinate and button condition, and remain publishable in a later fresh Child tree after Gone. Take the receipt sequence checkpoint from the successful press barrier; threshold must have a strictly newer sequence, the expected position, and a fresh returned Child step. Track primary button transitions in event order; reject a later release, focus/capture cancellation or viewport/window replacement. Gone alone may remove hover but must not forge release or erase the historical receipt. Repeated/empty passes must not increment the receipt sequence. Bind storage to the actual Child/window lifetime and never populate it from the driver's expected coordinate, another viewport, a cached prior drag or an inspection-injected event.

This preserves the Child barrier's purpose: proving the requested physical motion entered Child's egui input path. It does not claim the pointer must remain hovering there after the product legitimately makes Child click-through. Root route confirmation, exact typed enable/restore ACKs, stable Center preview, physical release, Center result and retirement/quiescence checks remain mandatory. An `interact_pos()` fallback alone is insufficient: it is cleared on a following pass and leaves the same observation-window race.

Meaningful regression coverage should demonstrate Move→Gone in one consumed batch and in separate passes followed by a fresh inspection request; the retained movement receipt must pass, while no Move, old sequence, wrong Child/window, wrong coordinates/button, replayed empty pass and intervening release/cancel must fail. Keep the existing inspection wake and native admission/retirement tests, then rerun the unchanged real X11 scenario at Root's serialized gate. No such validation was executed here, and a product fix remains conditional on an actual trace showing product event loss.

**Hash binding.**

| Artifact/source | SHA-256 |
| --- | --- |
| New `run.json` | `3728330040f7bc0c1933b96ab4da1c0b7e047e471f866e6115cf2fbbb89ae98b` |
| New `native.log` | `e6a439409a4d0b80569db53c7831876668468a27f0c8d9c32a34bfc53e990edb` |
| Old failed log | `280f16315a153189cc4bd66f7de723b46f7c5c9f71de214dec836202dbdc0340` |
| Same-head successful rerun receipt | `cc74e4e0e2f7564c4c4f4b90c347aea875db61f6f5e94e266e0950578fa883a7` |
| Same-head successful native log | `2f6b019bd61a8c46194f4c3e772e9c95d5a935874957a308689e0580235d6303` |
| `9ddb63f` smoke `app.rs` | `9670e6b4edacee973be5abd7b4e8eeacf18f705f01433c8160c95b4dbf6eb2e0` |
| `9ddb63f` smoke `driver/scenario.rs` | `aab11b5e61ffb5280f032a4b72764203a07735d3776b9df26b73c629859d6916` |
| `9ddb63f` native `surface_driver.rs` | `7ab22b651f05d33b1cb88cd2c358e108561b8466dad14973e56deb7e1bea0246` |
| `f94ee58` inspection `plugin.rs` | `902fae4ba28ace39c3cc30dad3afa8cd7124ab733b8a9b6fc3a8fe97ab74a447` |
| `f94ee58` egui input state | `c0ef7a0553b20e677578e17c848ff89c64a015e89a3fc1eb7459d49c34008394` |
| `f94ee58` egui Context | `29458bff79abaa920ffa8dd94ed1ac7e492221e199b0d16e15621e2500d385d0` |
| `2664589` X11 window | `75afcd696201b15ab1186853c0b733467d6ceaaf51265a6b85ec5646f2977862` |
