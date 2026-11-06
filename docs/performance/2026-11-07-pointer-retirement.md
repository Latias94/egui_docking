# Drain exact historical pointer facts after window admission ends

CI run [42297763668](https://github.com/Latias94/egui_docking/actions/runs/42297763668)
passes inspection, Center merge, physical release and passthrough restoration,
then fails during retirement quiescence with `Native(InvalidPointerFacts)`.
The earlier inspection-paint fix remains pinned at egui `f94ee58b`; this is a
separate failure. The log does not identify the exact rejected callback or owner.

The native adapter freezes a callback's delivery, hover and capture facts. A
preceding core commit can end a child's current admission while its exact
incarnation is still retained for input cleanup. The journal previously required
current admission when translating those immutable facts, preventing the
accepted native prefix from draining. Suppressing later route snapshots cannot
rewrite the already-owned record.

Ordinary pointer journals now require exact recognized provider/incarnation
ownership. Recovery pointer rosters separately retain their previous current
`contains_binding` check. The reducer still requires current geometry, exact
delivery/capture, routeable admission and valid presentation before any action.
Fresh local-coordinate capabilities stay current-only; foreign, forgotten and
wrong-backend capabilities, stale work areas and inconsistent facts stay rejected.
No event is discarded, relabeled, reordered or retried, and no new state layer
or reducer bypass is introduced.

## Executed causal witness and qualification

The existing registered retirement fixture captures ordinal 13 before release
suppression, retaining live Root delivery and exact child hover/capture. After
the child becomes noncurrent but remains recognized, replay fails at the added
expectation with the exact `InvalidPointerFacts` chain on the old runtime.
Compilation succeeds; the RED is one failed behavior test, not a build failure.
This reproduces the lifetime/admission mismatch, without claiming the exact X11
chronology of the original CI run.

The same test passes with the fix. It preserves original route facts and ordinal,
commits without changing Workspace version or starting a gesture, records the
exact Destroyed input-retirement outcome and completes translator/quiescence
cleanup. Additional cases retain an old delivery and a local sample captured
through the real initial-presentation API. Neither may query a current receiver
or start a gesture after retirement; fresh coordinates are unavailable. Current
roster capture still rejects the child, and forgotten/other-provider bindings
still fail before appending input. All previous fixture assertions remain.

Local serial qualification also passes:

- 340 core native/pointer/authority boundary cases, including registered stale
  move/release cancellation, reincarnation/ABA and replay/batch coverage.
- 545 native library tests, 2,245 workspace tests and 593 fork-workspace tests.
- 545 native tests with that crate's debug assertions disabled, matching the
  relevant CI profile override; this is not a whole-workspace release run.
- Formatting, correctness/suspicious Clippy, strict core/adapter documentation,
  ten doctests and dependency boundaries.

The one existing native opt-in case and three workspace probes remain skipped.
The combined saved-local-sample plus successor-geometry plus active-release
scenario is not claimed as a new end-to-end fixture; its authority components
have the separate registered coverage described in the evidence.

The [independent admission review](2026-11-07-pointer-retirement/admission-review.md)
checks every input/action boundary. Its
[final test supplement](2026-11-07-pointer-retirement/admission-test-supplement.md)
binds patch `7a0776a2c51201cbdadc768453e8a02bfccee463bcbc517c090c7567c4799c4a`.
A draft initially placed the local-sample block in the preceding test. Root
caught that scope error before application/execution; its exact draft and
fourteen-line relocation remain diagnostic evidence, never a passing candidate.

The [archive](2026-11-07-pointer-retirement/evidence.tar.xz) preserves 60 inputs
plus a manifest: the original remote failure, actual RED/GREEN logs, eleven
successful check receipts, exact source pairs against `c98569d`, reviewed final
patches and the superseded diagnostic. Every member round-trips byte for byte.
Local macOS qualification does not run physical X11. Fresh pushed-head physical
smoke and all remote CI jobs remain required before declaring CI repaired.

The fix author additionally performs a
[mechanical evidence check](2026-11-07-pointer-retirement/author-evidence-check.md)
against immutable `1c71696`, confirming all source pairs, archive members and
RED/GREEN receipts. This is labeled as an author check; the independent source
review remains the separate admission report and final test supplement.
