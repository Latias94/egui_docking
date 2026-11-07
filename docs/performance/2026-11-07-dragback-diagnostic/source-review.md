# Independent source review: drag-back failure diagnostics

**Findings first.** No source blocker was found. The exact two-hunk change is useful failure-only instrumentation and preserves the existing semantic verdict, deadlines, physical inputs, and cleanup-release control flow. It can distinguish no successfully returned child snapshot from prior returned snapshots that did not satisfy the barrier. It must not be described as fixing the product root cause, or as proving zero UI passes merely from `attempts=0`.

Only this report was written. No repository/candidate/fork/cache change, Cargo, compilation, formatting, test, smoke runner, timing, profiler, network, or delegation was performed. The existing execution/qualification queue remains Root's responsibility.

**Exact scope and identity.**

| Artifact | SHA256 |
| --- | --- |
| Original `scenario.rs` | `85454244bd86a09978066b3a9af4f34f5912c28a95b1d9bb46b19ccd2acc34c7` |
| Reviewed changed `scenario.rs` | `aab11b5e61ffb5280f032a4b72764203a07735d3776b9df26b73c629859d6916` |
| Read-only Git diff for that path | `04c93b541157c377d461d3a52129742961da6b03f88e098713d038e906525171` |
| `B/native-diagnostic-scope.json` | `1a06ea2e76b250d4049ab92c71c898ecae07e8a3c87225b43cd2895fd2063271` |

Here the source is `integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs`, and `B` is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`. The scope receipt records HEAD `eb94cb242722962c9e0d88074d81cf462d784357`, the original source hash, and unrelated preexisting worktree changes. Those unrelated changes are outside this review; no clean-worktree or whole-candidate identity is asserted.

Independent reconstruction from the original source using only the two described edits exactly equals the reviewed source. The diff has two hunks and +14/−1 lines. No semantic assertion, timeout, input injector call, barrier predicate, cleanup match arm, or other source body changed.

**Input-barrier error context.** At changed lines 3571–3578, the same `inspection.tree(viewport, deadline)` call remains in the same position. `map_err` runs only if it fails, wraps the original error string with existing local state, and returns through the same `?`. There is no extra request, retry, wait, deadline recalculation, or success path.

The added context retains stage, viewport, previous/last step, expected physical/logical coordinates, frozen client/scale, button expectation, last observation/lookup, successful-snapshot counter, and last-tree summary. The original detailed end-of-loop error remains unchanged. `semantic_tree_summary` bounds displayed nodes to 24 and text fields to 64 characters; it does not dump an unbounded textual tree. Formatting and allocations occur only on the failure path.

Interpretation remains precise:

- `attempts` increments after `InspectionClient::tree` returns a valid snapshot. Zero means no successful snapshot return within this barrier before the reported error; it does not count requests, backend callbacks, or UI passes.
- Positive attempts plus `last_lookup="pointer diagnostic did not match"` retain evidence that at least one returned observation failed the expectation. The recorded position, button state, step, and client geometry permit a concrete mismatch analysis.
- Stale-step or missing-diagnostic snapshots remain distinguishable through `last_lookup` and the last tree.
- `last_observation` is the latest successfully parsed mismatch, while `last_tree` is the latest returned tree; they can refer to different snapshots after a subsequent missing/stale result. The labels do not claim they are one atomic snapshot.
- The original error is retained, so TCP timeout/EAGAIN is still distinguishable from a typed missing viewport, absent AccessKit tree, or another protocol error.

This closes the previously identified diagnostic loss without converting any error or mismatch into success. Exact zero-pass attribution still requires native output/admission or inspection hook evidence.

**Before-cleanup marker.** At changed lines 2222–2227, the added branch borrows `pressed` only when it already contains an error and prints it before obtaining the original restore checkpoint and releasing primary. It neither consumes nor replaces that result. The checkpoint, unconditional `set_primary_button(false)`, original error/cleanup-error matching, and all success-path release/retirement assertions remain in their previous order.

The marker therefore lets a future log separate a held-drag failure from layout/lifecycle changes caused by the subsequent cleanup release. Normal stderr I/O adds a small failure-path delay before release; it does not establish when the underlying product first became faulty. It introduces no new physical event and no success-path logging.

The source is ready for Root's formatting/native checks when its serial Cargo queue permits. No additional framework, product-semantic patch, or mirrored test is needed for this narrow instrumentation. A later run must retain the original fresh-input, exact Center preview/release, retirement, and quiescence requirements. This review approves the diagnostic mechanism, not an unverified root-cause repair or a green result.
