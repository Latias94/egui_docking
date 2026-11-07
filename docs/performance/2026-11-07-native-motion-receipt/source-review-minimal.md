# Independent source review

No substantive source blocker found. The candidate replaces the held-motion observation window with evidence produced by the actual UI input path. It does not bypass the physical motion, tree, window-lifetime, button or product assertions. This conclusion is source-only; the candidate has not been compiled or executed by this reviewer.

## Exact artifact reviewed

- Parent: `9ddb63f425ca8316bdf483d6297e923c35a16ac3`.
- Patch SHA-256: `70656ac0269be13f06be277593ea6a1551703bd6066ac7aaa6786e87b2268eff`.
- Manifest SHA-256: `a575052b45b7e2587f7e3f8275877b2bcd530a478fa8bfb21ccf3c6dc787e659`.
- READY SHA-256: `2572f40e6d1dba187136b58e4dfc5c3e0ae3593804143b3a47e58d935decc689`.
- Base set: `c33a7a3e906544cd77f12b38593fc8ae1e456bcb00511bb2ba30d4b24ea18a9e`.
- Candidate set: `5d26e7e472a53981a071dce76cd6f5cd4816bc3ed602fa9a735199e5f2dbec7c`.

The three existing base files and five context files match the immutable parent byte-for-byte; the new module is absent from that parent. Independent in-memory application of all patch hunks reconstructs the four complete tree files. Counts match: **+561/-58**, including the 225-line test section; runtime **+336/-58**. Source-set digests use the declared UTF-8 path-string ordering. The author's scope checks were not treated as executable qualification.

## Correctness trace

**The producer is UI input.** The existing `PhysicalSemanticProbe::on_begin_pass` calls `MotionProbe::publish` (`app.rs:893–898`). The latter reads `InputState.raw.events` and processed pointer state (`motion_receipt.rs:104–150`). Native callback records never produce or advance these receipts. Sharing the latest UI-produced record with the driver is therefore distinct from accepting a host acknowledgement as UI delivery.

**Baseline precedes injection.** `inject_held_motion` copies the exact viewport's held native-scoped record under the shared lock, releases that lock, then performs the original XTest move and desktop button assertion (`scenario.rs:3532–3545`). The fourteen migrated sites retain their coordinates, viewport, previous-step requirements and stage deadlines. The fifteen original state-barrier calls remain state barriers. No additional inspection request, timeout extension or extra physical retry was inserted.

**Acceptance requires both output and input progress.** The common loop still requires an inspection step strictly greater than the supplied previous step. Its motion branch additionally pairs the AccessKit sequence/position with the current typed UI record, requires a new Move after the baseline's entire event sequence, compares lifetime and viewport, and verifies both the held state at Move and the current held state (`motion_receipt.rs:33–64`; `scenario.rs:3623–3649`). Empty repaints cannot provide input progress. A newer shared record cannot simply be paired with an older tree sequence.

**Gone preserves evidence; release/cancellation invalidates it.** Ordered event processing records primary state at the Move itself. `PointerGone` advances the event sequence and records diagnostics without deleting the Move. Primary release, focus loss or Escape advances the cancellation marker, so release/repress cannot borrow an earlier held Move. The final conjunction with `input.pointer.primary_down()` also prevents a retained true bit from overriding a currently released UI state. Saturation fails freshness rather than creating false progress.

**Native scope is checked without inventing token authority.** The publisher uses the actual current `NativeOutputToken`, filtered to its viewport. A context, native window or create-attempt change resets the receipt. Frame/pass rollback resets it as well, and inventory pruning removes absent viewport records (`motion_receipt.rs:77–106`). The retained first token distinguishes receipt lifetimes; the injection baseline requires a native-scoped record. A replacement lifetime consequently cannot satisfy an old baseline. Unmanaged test records are allowed by the publisher but cannot pass the production injection helper's native-baseline filter.

**Repeated passes do not fabricate motion.** The pinned fork's `Context::run_dyn` calls `begin_pass(new_input.take())` and increments the cumulative frame only after the outer run completes. The candidate processes events once per cumulative frame and only updates current diagnostics on its repeated passes. This is appropriate for this physical integration's `run_ui` path. It does not claim to implement a general input-injection protocol for arbitrary manual pass scheduling.

The existing pass-through acknowledgements, route/root checks, guide and product assertions, release cleanup and retirement checks remain outside the changed acceptance mechanism. The sole arithmetic moved around a few injection sites is an infallible step/max calculation. The existing socket/deadline implementation and physical injector are unchanged.

## Test quality and remaining qualification

The three registered regressions install a real egui Plugin, run actual Context/UI passes and parse actual AccessKit output from the production publisher. They are not tests of a separately implemented state machine. They cover same-batch Move/Gone, later Gone, empty and genuinely discarded/repeated passes, stale/foreign observations, Move-before-press, release/repress and focus cancellation.

These unmanaged Context tests do not establish real native-window lifetime behavior or Linux delivery. That limitation is stated in the package. Root still owns ordinary compilation/tests and the actual unchanged-strength physical X11 sequence. Neither this review nor the candidate establishes the failed CI run's unique Move/Gone ordering or a successful local Linux run.

## Scope and direct simplification

The line count alone is not evidence of an excessive framework here. The implementation has one concrete record, one viewport-indexed producer and one semantic sample/parser; the larger total also includes fourteen caller migrations and the existing barrier wrapper. There is no new product authority, native token type, dependency, generic provider or replay executor. Further separation would add interface surface without helping this fix.

There is one directly removable duplication: `MotionReceipt.token` (`motion_receipt.rs:19,102`) and the corresponding baseline check (`scenario.rs:3540`). After `publish`, `lifetime.is_some()` and `token.is_some()` are equivalent: a missing current token resets the receipt, and a present token initializes its lifetime. The producer already filters the token by the record's viewport. Removing the latest-token field/check retains the lifetime/window comparison and freshness semantics. If retained for its diagnostic output nonce, describe it as diagnostic data rather than another acceptance authority. This is a small nonblocking trim, not a reason to expand the design.

The inserted injection helper also inherits the old `too_many_arguments` allow, leaving the state wrapper and common loop without their own local annotation. Restoring the established annotation style is a minor cleanup; it is not a claim that the configured CI Clippy gate fails.

Only this review file was written. No Cargo, compiler, formatter, tests, runner, timing, network or delegation was used. No independent MRU analysis report was read.
