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

- [ ] A01: exact focus dispatch callback releases adapter ownership; successors and shutdown progress.
- [ ] A02: equal configuration does not cancel pending transfer.
- [ ] A03: ordinary document publication prepares only placement data, without identity-map copies.
- [ ] A04: proven single-command no-op preserves snapshot provenance and ABA protection.
- [ ] A05: one root derivation shared within the same exact workspace source/version.
- [ ] A06: immutable receiver/focus tables share storage across drafts.
- [ ] A07: packet-level blocker join preserves rectangle multiplicity and declaration ownership.
- [ ] A08: finite native admission avoids queue-dependent repeated accounting.
- [ ] A09: published focus requests and observations are independent of output admission.
- [ ] A10: measurement retention is centralized and scoped to affected surfaces.
- [ ] A11: native preparation driving has a clear host-owned seam; first-live, revocation and retirement remain exact.
- [ ] A12: accepted input is retained while configuration applies before measurement/encoding.
- [ ] A13: existing native serde regressions enter CI's complete selection.
- [ ] A14: product benchmarks reject changed input/executable hashes.
- [ ] A15: complete mapped configuration determines style agreement after full style validation.
- [ ] Hidden minimum: make metadata requirements explicit and verify unloaded content behavior without changing accepted-selection semantics.
- [ ] Correct conformance README release semantics.
- [ ] Independent final review, targeted and complete verification, and fresh cost comparison.

## Verification boundaries

Preserve prepared/suspended document invalidation, generation/frontier monotonicity, failed-frame rollback, multiset blocker rejection, stale/foreign output and focus rejection, same-batch input causality, repeated encoding without replaying input, callback reservation draining, first-live atomic transfer and final quiescence. Encoding rollback cannot undo arbitrary application callback side effects.

Structural counters and allocation slopes are the primary performance evidence. Timing samples supplement them without platform thresholds or an unsupported ImGui ratio. Keep the original audit and its receipts immutable; record remediation results separately.
