# Cross-lane ergonomics source review

**Finding: one P1 blocker in the official demo's close-all/restore journey. No additional blocking defect found in the PaneResponse migration or the owned composition path.** The UI implementation was authored by this reviewer; this review independently examines the other lanes and only checks the parent's subsequent test corrections for that UI slice.

Reviewed read-only on 2026-11-08 against working sources over immutable `2697b04ca9733af173863419d657bdac5d67de5d`. No Cargo, tests, GUI, network, profiling, timing, source edits, or git mutations were performed. The parent's report of passing tests is not independently executed or certified here.

**P1: closing the last group removes the only route to Restore layout.**

At `integration/egui-official-harness/src/bin/official_caller_owned_demo.rs:320`, the host always calls `run_single_surface_with_ui(...SURFACE...)` to render both its toolbar and docking. At `:431–459`, `apply_editor_commands` allows every Pending item in every new close plan, including root/group plans. Closing main and contained groups can therefore remove all roots and the last logical surface. This is valid existing product behavior: `crates/egui_dockspace/examples/workspace_persistence.rs:540–567` explicitly requests a complete-root close, allows all items, and verifies both the root and the entire surface roster disappear.

The next demo redraw fails `product_owned_run.rs:270` because `surfaces.len() != 1`, before `before_docking(ui)` at `:302`. The host error branch merely sets the native window title, requests another redraw, and returns. Its Restore layout button is never drawn again, and raw clicks cannot reach it. All-close can be reached by closing the main root via Close Group and the remaining Inspector root, in either order; no forged source or private hook is needed.

The new `editor_close_and_restore_keep_application_document_edits` test at `exercise.rs:484` closes only Editor and directly invokes the reset helper. It is useful evidence for that supported journey, but cannot catch the empty-roster recovery gap.

The smallest fix can stay in the demo: when no Dockspace surface exists, render the application toolbar/empty-workspace message through the existing `egui_dockspace::run_ui` owner, with no Dockspace paint callback, and accept Restore after the completed run. Keep the ordinary populated path on `run_single_surface_with_ui`. An empty application-only output grants no docking authority and needs no synthetic surface, automatic reopen, weakened output proof, or second session. Avoid silently resetting immediately on close: that would undo the user's requested closure. A meaningful regression should close the two actual groups through normal request/decision commands, verify zero surfaces, render the live toolbar, activate its real Restore button, then verify the default's eight stable items and unchanged application text return. The direct close commands establish the legal state; the restore must use real encoded UI input rather than call the helper as its only witness.

PaneResponse and migration:

- `pane.rs:20–64` is a small callback result with an inline first ID and a Vec for additional IDs. Empty and single-widget responses need no content-ID allocation. Sequential `with_content`/`with_content_ids` calls preserve the declared order, including duplicates; iteration yields copied Id values without lending application storage across calls. It does not promise lifetime proof or automatically infer widget ownership.
- `PaneView::ui` remains object-safe and now returns the declarations from that same callback. The renderer reads them while the child Ui is still alive, before dropping it. `product_render/tabs.rs:103–149` preserves the original current-pass widget lookup, enabled/layer/finite/positive/containment checks and receiver descriptors. It does not promote an unknown widget by equal geometry or remove another widget's blocker.
- I inventoried every baseline `.rs` file containing the removed getter: all 11 files are migrated, with no `docking_content_widgets` implementation/call left in current Rust sources. Remaining mention in the migration guide is intentional. The native basic example preserves heading, canvas controls, scroll-area children and actual registered scroll controls. The physical smoke and official editor preserve their exact TextEdit registrations. The official hover-blocker fixture still declares only its target canvas. Undeclared fixtures return an empty response. Product/native test copies of declared IDs remain observations of the returned result, not a second runtime declaration channel.
- Focus registration stays separate, as required by pre-render focus sampling. The public documentation correctly treats declarations as host-application ownership facts, not unforgeable registration proof. The source-and-return migration is an intentional source break for external PaneView implementors; the old getter and stored per-item ID catalogs should not be retained as a compatibility layer.

Owned composition:

`run_single_surface` delegates to the new `run_single_surface_with_ui` with a no-op callback. The body remains the one existing numeric/measurement/render/transaction driver. The added callback runs once per actual pass before docking measurement, and the supplied Ui's remaining rectangle is used for focus bounds, measurement and painting. It does not expose the unpublished candidate or allow a safe same-owner mutable reborrow. Recursive egui execution is still governed by the existing run owner.

Raw pointer input and queued prior-run actions are reduced only on the first pass; later passes resume the same update. Terminal output checks, surface execution receipt confirmation, commit, UI-action publication, output proof, and renderer receipt staging remain in the same order. No final-output hook is bypassed. Application callback mutations remain outside rollback, explicitly documented in `product_owned_run.rs:249–255`; callers must gather/deduplicate external command intents and apply them after success.

The new actual-output test at `product_owned_run_tests.rs:234` compares all four published workspace AccessKit bounds to the rectangle left by top/bottom panels and checks pane bounds are contained. The repeated-pass test at `:439` now verifies the composition callback follows all three actual passes while selection/output publish once. The damaged-pane-output test at `:787` checks a real mutation stays unpublished on proof rejection while the external callback count remains observable. These are meaningful assertions rather than reproductions of private geometry. I did not execute them. The new before-docking-only callback is deliberately limited; arbitrary after-docking overlays are not smuggled into this interface.

Other construction and demo behavior:

`DockspaceBuilderSource` was a single-variant private wrapper; storing DockspaceLayout directly preserves style validation and persistent/nonpersistent constructor order. The private `from_session` initializer consolidates the same field initialization for ordinary, persistent and decoded-document construction without exposing raw session replacement.

In the ordinary nonempty demo, retaining DockspaceResponse now correctly consumes the previously ignored close requests. Approval remains application-owned and uses exact plan/token pairs. Reused requests are skipped, Pending state is checked, and application pane data is retained. Reset takes priority over old close events when both were requested. Renderer disposition is still reported from the actual paint path before those application commands; the commands do not manufacture presentation. The toolbar's OR accumulation avoids duplicating a click over repeat passes, and errors do not automatically replay input. The new close/edit-preservation test supports this nonempty journey; it does not remove the P1 empty-roster problem above.

The parent-only corrections to this reviewer's UI tests are valid source changes: `global_style_mut` selects the supported egui style setter for the margin fixture, and the composition test compares actual public AccessKit bounds instead of calling a private conversion helper. I have not re-certified my own rendering work as an independent review. None of these changes grants terminal-presentation proof to an unmodified eframe callback.

SHA256 bindings at review time:

| File | SHA256 |
| --- | --- |
| `crates/egui_dockspace/src/pane.rs` | `cf57366e307f79ab3770343f075864de84595ab372463b9e757ad2a961a8349d` |
| `crates/egui_dockspace/src/builder.rs` | `c4ec18d9e577288b12943e373248ad44d45d28a95729355373aec01cdf03252e` |
| `crates/egui_dockspace/src/product_dockspace.rs` | `3fea64e46d5b68bd78c722df5ac3880b739824d7d57f6ce01c5fbbee903cab32` |
| `crates/egui_dockspace/src/product_owned_run.rs` | `39f207e30100173d1438d2bb0d417f811b7c36ae462778316ca0502ade0e950b` |
| `crates/egui_dockspace/src/product_render/tabs.rs` | `797a1f4329c11e9ae005fa9a223223ddee6f47ced24b0bbb49ad5a312894009b` |
| `crates/egui_dockspace/src/product_owned_run_tests.rs` | `6620532336fc6a64cedbed91d4ea539a8f589bb4b3c74237b2382849363ecb76` |
| Official demo `official_caller_owned_demo.rs` | `f011c20d12cdeb431ff83fd7f3b4b51f067ff5c3180623af2749a48e90f3ea84` |
| Demo `exercise.rs` | `b5c736ac83a00f456f6b34236d8e49ba23cede5b396d6180a7135d173524cda7` |

## Empty-roster fix follow-up, 2026-11-08

**The production source fix closes the identified P1 control-flow defect. The first new all-tab-close test does not yet establish the intended empty-roster regression. No execution result is asserted.**

Both the real host (`official_caller_owned_demo.rs:320`) and the two existing exercise tests now call the same private `run_editor`. At `:438–445`, an empty roster uses the existing `egui_dockspace::run_ui` owner to draw the live toolbar and an explicit all-closed message, returning `(output, None)`. It does not call the single-surface driver or manufacture DockspaceResponse. The populated path retains the original owned composition driver and returns Some(response).

`apply_editor_commands` now accepts Option<&DockspaceResponse>, handles Restore before attempting close decisions, and treats None as no close events. Reset still uses the existing facade transaction and the application-owned DemoPanes catalog. The real renderer still presents and reports the actual output before applying application commands, including the empty-toolbar output. This is the requested narrow demo repair, with no weakened output/presentation authority and no automatic reopening on close.

The new `closing_every_pane_keeps_the_empty_editor_and_restore_button_usable` encodes actual AccessKit Close Tab actions and would activate the actual Restore button after observing None, checking both eight reopened items and retained edited text. Its critical setup assumption is incorrect for this demo: `demo_layout` contains `central_tabs([EDITOR, PREVIEW])` (`official_caller_owned_demo.rs:612`). Closing each item individually preserves the empty central root/dock area, so all eight tab closes need not remove the final surface. The test then cannot enter its None branch or exercise Restore recovery before its finite bound. It should not be reported as covering this P1 merely because it targets every tab.

Use actual Close Group menu activation for the main root and close the Inspector root, or establish zero surfaces through ordinary `request_close_root_current` plus exact Allow decisions, then activate Restore through its real UI output. The latter is still a semantic transaction witness for legal empty-state entry and a genuine UI witness for recovery; do not label its setup as physical/menu input. Verify both roots and the surface roster are absent before drawing the empty toolbar. A separate all-tabs test may correctly assert the empty central dock area remains available, but it cannot substitute for the root-close recovery case.

This follow-up binds demo SHA256 `3ab0001b254392d89eb312dbe73a2d66f4854fcd0b61fa6453e2ae021e8597d7` and exercise SHA256 `81d848e8da8d8ac12ffb10c29f6fd243f0faf28ac97725e44b7b7d4981a6cc48`. The earlier findings and hashes remain the historical pre-fix review. No source was changed and no test/build/GUI was executed by this reviewer.

## Final P1 closure, 2026-11-08

**The original P1 and its initial regression-test setup gap are now closed. No remaining blocker was found in this review's scope.** The preceding sections preserve the initial finding and intermediate source bindings, rather than describing the final test.

I read the revised `closing_all_groups_keeps_the_empty_editor_and_restore_button_usable` at `exercise.rs:561–668`. Its inputs now target the actual enabled “Close floating Inspector” control, then the main root's “Presentation commands” menu and “Close Group” item. Every activation is an AccessKit Click against an ID obtained from the actual preceding egui output, with the normal response/approval consumer settling the close plans. It does not rely on closing individual central tabs or private state mutation.

The witness explicitly reaches None response and an empty surface roster, checks actual painted all-closed text, obtains the real enabled Restore layout button from that empty output, and submits its Click in a later input batch. The shared `run_editor` path processes it; `apply_editor_commands` then restores through `reset_layout`. Final assertions verify all eight known items are open, the eight-entry application catalog remains, and the edited Editor text is unchanged. The real host continues to call the same helper, whose empty branch still creates only application UI and no fabricated DockspaceResponse.

The parent supplied `/tmp/dockspace-ergonomics-20261108/official-tests-3.log`. I independently read that saved log: it reports 27 tests run, 27 passed, 0 skipped, and contains the explicit PASS for this all-groups witness as test 25/27. The finite original input journey and one-pane close/edit-preservation witness also pass in that run. These are previously executed results supplied by the parent; this reviewer did not run or repeat any test, build, GUI, or timing. AccessKit/CPU test input is not OS/GPU presentation evidence.

Final source and saved-log SHA256:

| Artifact | SHA256 |
| --- | --- |
| `official_caller_owned_demo.rs` | `0d4e5e03ad3ac430905996b6026b3fcfc0363f5e1580f5b3c155573852503a3f` |
| `official_caller_owned_demo/exercise.rs` | `2fac38b8b1dcc161fb03acb009ca521abac8addc1d6fa221975f79c78093967b` |
| `official-tests-3.log` | `5273fc90c8b6fe2ae9d548f844d8491b20601a9bc3a2df30ab7fd4f03a0ef80f` |
