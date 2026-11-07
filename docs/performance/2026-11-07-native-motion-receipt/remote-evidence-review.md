**No evidence blocker found.** Immutable fix head `c1ed518992d0cd88026960940ea061aca9fb0433` passed all six jobs in CI run `42337299402`, attempt 1. The actual Linux/X11 drag-back succeeds while its threshold observation has **Move followed by Gone, primary held, and latest_pos absent**. This is direct evidence that the new occurrence receipt handles a real observation window which the old current-state check could not accept.

This review only read existing local artifacts and immutable source. No Cargo, compilation, tests, runner, timing, profiling, network, delegation or source/candidate changes occurred. Only this report was written.

**Binding.** `B` is `/tmp/dockspace-imgui-twofold-20261107-ea4161e`. I read `B/native-motion-remote/{run.json,native.log,native.log.xz}`. The complete compressed log decompresses byte-for-byte to native.log. The receipt identifies the exact fix head above, completed successfully. All six jobs report success: Workspace, independent engine host consumer, official egui consumer, native Windows compile, native macOS compile and fork-backed native coordinator.

The native job's formatting, pinned inspection/output-settlement/framework/pointer tests, native workspace/lifecycle tests, actual editor/output ownership qualification, real-window build, programmatic Glow smoke and physical X11/XTest docking smoke all report success. Its physical-binary unit-test log records **63 passed, 0 failed** at line 4923; all three new real Context/UI/AccessKit regressions explicitly pass at lines 4882, 4887 and 4919.

I independently matched all **eight** source hashes in immutable `docs/performance/2026-11-07-native-motion-receipt/checks.json` to the fix commit, including all four smoke files, manifests/lock and recorded layout context. The local receipt records successful formatting and 32 macOS physical-binary unit tests; its retained unit log confirms 32 passed, 0 skipped. Those local results are separate from the new Linux/native success.

The committed source differs from the lean candidate only by import ordering/formatting and the already documented test-helper texture disposal. The sole semantic addition after the lean package is `output.textures_delta.clear()` in the renderer-free test helper; production behavior and test assertions are unchanged. The earlier 29-pass/three-texture-disposal-panic attempt is retained in the local evidence and is not concealed by the successful final result.

**Observed remote sequence, November 7, 2026, UTC.** References are native.log line numbers.

| Lines | Actual observation |
| --- | --- |
| 5143–5146 | Child hover step 42 and press step 44 at `(40,14)`; Root pre-route checkpoint step 46 |
| 5149, `04:43:25.214` | Child threshold step 50, first attempt: sequence 7, Move sequence 6 at `(12,15)`, Gone sequence 7, moved_held=true, primary_down=true, latest=None |
| 5150–5154 | Distinct first/routed ingress receipts; pass-through enable sequence 1 acknowledged; Root route confirmation and preservation succeed |
| 5155–5158 | Stable Center guide, exact Center input receipt and fresh stable preview step 65 |
| 5159–5162 | Product Retired with verified Center result; physical-release step 66; pass-through restore sequence 2; preview precedes release |
| 5167–5175 | Retired and quiescent; exact DestroyNotify for window 4194326; Child viewport unavailable/Unknown; Child item remains in live Root; target verification and stage=complete |

The threshold receipt's native lifetime names viewport `2CEC`, window `4194326` and its actual create attempt. The same window later receives the observed DestroyNotify. The scope is an actual physical integration, not a synthetic native-token test.

**What this proves about the correction.** At the committed producer, only processed `InputState.raw.events` advances Move/Gone sequences (`motion_receipt.rs:102–147`). Thus sequence 6 followed by Gone 7 establishes their consumed UI order in this new run. It does not establish that they occurred within the same raw batch: the receipt retains occurrences across passes.

The original state publisher still returns without a label when `latest_pos()` is None (`app.rs:1840–1847`). Therefore it cannot satisfy the old state check at this threshold snapshot, despite a real held Move having been consumed. The new barrier instead requires a baseline captured before physical injection (`scenario.rs:3532–3541`), a fresh exact-viewport step, matching tree/UI sequence, the same lifetime, a strictly newer Move, matching actual coordinate and held state without later cancellation (`motion_receipt.rs:32–63`; `scenario.rs:3618–3641`). The remote success exercises that intended distinction without skipping Child observation.

**Causal limits.** This is new-head evidence of the proposed Move→Gone observation race actually occurring and being handled, plus one complete native success. It cannot retroactively supply the missing raw callbacks, ACK ordinals or UI batch ordering for old failed runs `42323715317` attempt 1 or `42330748876`. The old failure's 147 completed Child trees and absent final label remain consistent with this mechanism, not a complete historical event trace.

Likewise, the emitted ACK log proves the existing ACK barrier succeeded; its print position is not the original callback timestamp. No native event-delivery defect, universal flake elimination, exact scheduler cause or performance conclusion follows. The evidence supports retaining this smoke-observation correction. No additional diagnostic framework or assertion weakening is needed on the basis of this run.

| Artifact/source | SHA-256 |
| --- | --- |
| Remote `run.json` | `c7a86d7bfe020bf2721edb340622e9baba4cbf51cc15c6648952c1562acbd6f7` |
| Remote `native.log` | `e63cb52c2a28e834748b526023e44741d778a3d3aca2e3d04f45d50595de7e2b` |
| Remote `native.log.xz` | `f7c36634d9794ea46a571017554ee626cef365503c46906226e9c65453bec98c` |
| Immutable final `checks.json` | `dccbdc5987402d35997ebf691dfaefb1e66bfbd2a700b72456614cceb640dbe4` |
| Final `motion_receipt.rs` | `c53eed7947d8ced187d923983b7f69cdb68919ff7a05d451da79e82f20db60b8` |
| Final `driver/scenario.rs` | `11006f9f9bc5eaa0057cec76238c32b6c66a67ed725d077ee611050a94f62314` |
| Local final physical-unit log | `f5c565a6740dad57cc0df74d2b4387d356e8929a10908029badb7dcdabb2cec4` |
