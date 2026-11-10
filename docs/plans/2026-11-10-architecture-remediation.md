# Architecture audit remediation

Baseline: `2289c889b30343bb705d9b6786f09dc09d633619`  
Input: `docs/audits/2026-11-10-architecture-design-audit.md`  
Authorization: implement all findings, including necessary breaking changes; preserve the canonical workspace and exact lifecycle guarantees.

## Ownership and integration

Execution uses the current feature checkout. No main worktree or external UI-framework PR is needed. All Cargo verification is serial with `CARGO_BUILD_JOBS=1`, shared `target`, and nextest where supported. Workers do not run Cargo, commit, or delegate further. The primary agent integrates and verifies.

| Lane | Initial write ownership | Work |
| --- | --- | --- |
| Core worker | Operation/workspace, engine mutation and presentation authority, reducer, associated tests | A02, A04, A05 |
| Renderer worker | Product pointer and tests, native style consistency | A06, A07, A15 |
| Native worker | Focus/coordinator, mailbox/capacity and tests | A01, A08; design and then implement A11 |
| Primary | Document/runtime publication, runtime focus/paint/measurement, adapter entry points, CI/scripts/docs | A03, A09, A10, A12, A13, A14; hidden-minimum contract and prose |

The primary waits for the core worker to release reducer and engine files before changing configuration phases. Native surface-driver and adapter run entry points remain primary-owned. Any additional shared file requires a handoff. The excluded reviews directory and reference checkouts with unrelated changes remain untouched.

## Completion ledger

- [x] A01: exact focus dispatch callback releases adapter ownership; successors and shutdown progress.
- [x] A02: equal configuration does not cancel pending transfer.
- [x] A03: ordinary document publication prepares only placement data, without identity-map copies.
- [x] A04: proven single-command no-op preserves snapshot provenance and ABA protection.
- [x] A05: one root derivation shared within the same exact workspace source/version.
- [x] A06: immutable receiver/focus tables share storage across drafts.
- [x] A07: packet-level blocker join preserves rectangle multiplicity and declaration ownership.
- [x] A08: finite native admission avoids queue-dependent repeated accounting.
- [x] A09: published focus requests and observations are independent of output admission.
- [x] A10: measurement retention is centralized and scoped to affected surfaces.
- [x] A11: native preparation driving has a clear host-owned seam; first-live, revocation and retirement remain exact.
- [x] A12: accepted input is retained while configuration applies before measurement/encoding.
- [x] A13: existing native serde regressions enter CI's complete selection.
- [x] A14: product benchmarks reject changed input/executable hashes.
- [x] A15: complete mapped configuration determines style agreement after full style validation.
- [x] Hidden minimum: make metadata requirements explicit and verify unloaded content behavior without changing accepted-selection semantics.
- [x] Correct conformance README release semantics.
- [x] Independent final review, targeted and complete verification, and fresh cost comparison.

## Verification boundaries

Preserve prepared/suspended document invalidation, generation/frontier monotonicity, failed-frame rollback, multiset blocker rejection, stale/foreign output and focus rejection, same-batch input causality, repeated encoding without replaying input, callback reservation draining, first-live atomic transfer and final quiescence. Encoding rollback cannot undo arbitrary application callback side effects.

Structural counters and allocation slopes are the primary performance evidence. Timing samples supplement them without platform thresholds or an unsupported ImGui ratio. Keep the original audit and its receipts immutable; record remediation results separately.

## Integrated checkpoints

The following checkpoints record implementation-stage results. Complete passing gates in the final verification section supersede their partial test selections.

- `95fd10d`: native full runner enables all features; product comparison verifies final source/reference/executable hashes. A temporary executable-substitution experiment rejects changed bytes before marking the run finished.
- `95d8c58`: ordinary document publication retains its identity-map owner. An independent dhat consumer verifies identical idle allocation counts and requested bytes at 1, 128, 1,024 and 16,384 registered keys.
- `ef2c422`: actual-change configuration invalidation, proven no-op snapshot preservation and shared root derivation. Nineteen focused core regressions pass, including the ABA counterexample.
- `6f9c518`: immutable output tables share slices; one frozen ownership join per pointer packet; complete mapped-style comparison. Nine focused adapter regressions pass at 1/16/128 content registrations, with unknown blockers and duplicate declarations.
- `5deca92`: focus dispatch ownership ends at the exact dispatch callback; pointer payload/translation accounting stays in the original ingress owner. Twenty-nine focused native regressions pass; an independent core reviewer found no blocking P1/P2 issue. Full diagnostic recount remains intentionally available.
- `2df4f19`: configuration applies to the private input candidate before measurement and encoding. Fifty-five focused core regressions pass, including old-policy pointer causality, new-policy inline/deferred compilation, encoding restart, rollback, equal configuration and native focus publication. Independent renderer review found no blocking P1/P2 issue. Native style integration and complete gates subsequently pass at `68a2245`.
- `798b5da`: committed focus queries do not reserve output capacity; per-surface measurement policy replaces global gesture freezes in both egui entries. Three fresh independent-consumer regressions pass, and the native-activation commit fence passes in the core run. The hidden-minimum contract is explicit and preserves semantic selection. Native measurement integration subsequently passes with the candidate-style patch.
- A11 consumer migration: all 339 independent persistence-consumer tests pass, including the three depth-10,000 cases, source/first-live/replacement/revocation cases and the new public contracts. All 37 conformance tests pass. Two initial quarantine snapshot-debt failures and core shared-fixture migration failures were repaired; the complete workspace and native reruns pass.
- A03 release probe: one-pane persistent idle publication uses 70 allocations and 42,792 requested bytes at each of 1/128/1,024/16,384 history keys. Raw timings and binary/source metadata are recorded with the remediation artifacts. The exact temporary probe copy was removed after content verification.

## A11 migration boundary

The approved redesign removes the strategy selector and the staged renderer protocol from core. The native adapter's exact window resource owns hidden creation, pre-show presentation, exact show acknowledgement, a later visible observation and post-show presentation. Original output and callback owners carry the work; no parallel resource registry is introduced. The adapter publishes Created during preparation and Prepared only after completion. Readiness loss must reach a committed snapshot before preparation can become valid again.

Core retains creation acknowledgement causality, exact binding/provider/coordinate and continuous Prepared provenance, prospective semantic content, first-live output floors, source conservation, atomic topology publication, compensation and retirement. Retained source descriptors are not renderer placeholders and must not be removed with the staged paint protocol. Public consumers migrate to host-owned preparation facts; tests of preparation procedure move to native while readiness and first-live counterexamples remain core/public tests.

## Final verification

- `98d33c5`: core and independent-consumer migration committed after 2,293 workspace tests and 339 persistence-host tests passed; source/first-live/cancellation/replacement and preparation-script test migration independently reviewed.
- `68a2245`: native private preparation and same-update style encoding committed after the final 592-test pinned fork gate, 560-test no-debug-assertions gate, cross-layer revoked-readiness regression and independent reviews passed.
- Default host: 295 passed, including three depth-10,000 cases. Official consumer: 24 plus three demo tests. Product consumer: 114 default / 119 persistence. Correctness/suspicious Clippy, strict docs, eleven doctests, core dependency boundary, formatting and package checks passed.
- Actual X11/Glow: six separate renderer test processes passed; unlimited and 4 MiB bounded create/first-live/redock/retirement passed; the complete actual-editor docking/lifecycle/Root-rebinding runner passed with all children destroyed and final quiescence.
- Raw commands, logs, run IDs and hashes are linked from the remediation report. The final frozen-source cost probe verifies unchanged inputs; results establish the historical-key slope repair, not a new product/ImGui ratio.
