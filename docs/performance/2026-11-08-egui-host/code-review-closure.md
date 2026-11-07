# Official host exercise: final code review closure

**All three P2 findings are closed in the source read below. No source-review blocker remains.** Read this closure together with `/tmp/egui-host-final-code-review.md` (SHA-256 `c14ece8fda9667bce03036c3a6e6c18741ecaf2c428b2f6183a63c90f4971f7c`). That original report deliberately binds the pre-fix source and is unchanged.

1. `prepare` now rejects `self.frames >= 640`, preventing attempt 641 while counting setup drops in the same total.
2. The Idle/Hover/Edit/Scroll version comparison now runs in `finish_frame` after the presentation outcome check and before sample recording or phase advancement. It covers Scroll's last committed output. The unused `dock` parameter was removed from `prepare`, and both the real-host and headless call sites were migrated.
3. Final ReleaseTab validation now requires the inserted `~` count to remain `PHASE_FRAMES`, alongside the exact original-text, tab-order/selection and eight-item checks. The success log uses the actual sample count and calls them UI frame samples.

The final README keeps the accepted `ui_to_present_ns` boundary and CPU-side timing limitations. This closure does not enlarge the scope of the original review or claim tests, a real-window run, hardware-GPU timing, or publication qualification.

## Final source binding

| File | SHA-256 |
| --- | --- |
| `.github/workflows/ci.yml` | `2503d3cb5eaff5bb09a6ea40b5f5b6cd06110e1d19a66b096ec068d8ce936392` |
| `integration/egui-official-harness/Cargo.toml` | `e3cb03cab5cf6b234738d20f1907442b452a037eddfc8055a1654ae9b3df98ec` |
| `integration/egui-official-harness/README.md` | `d50916d90411950815df6b7841c1bf5c4cedc88378744956c4ed3558653db33c` |
| `integration/egui-official-harness/src/bin/official_caller_owned_demo.rs` | `58033b120d976d04f7753eceb5a94e05a31d5fc09b85287b9f8dd807327ccfb3` |
| `integration/egui-official-harness/src/bin/official_caller_owned_demo/exercise.rs` | `0c84a6c7fa9f4a62ce360a86255c35cfcee844d6e43e2d225fb84ef37f2107f2` |

Only this closure was written. No original report, repository source or test was edited; no Cargo, test, GUI, timing, profiling or network command was run. I am stationary for Root's actual-window validation.
