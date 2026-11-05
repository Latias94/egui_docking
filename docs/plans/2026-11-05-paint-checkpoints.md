# Incremental paint auditing and remaining product costs

Date: 2026-11-05–06. Base: `36f3fb0`. Status: complete.

## Completion criteria

Keep the complete existing product workload, input/output integrity obligations,
and post-hook surface proofs. Target 1,024-pane p50 of at most 12 ms idle,
35 ms per two-frame click, and 20 ms resize. Confirm with two separate ABBA
product comparisons, preserving every sample, workload count, p95 and maximum.
Run the relevant regression and existing consumer gates, obtain independent
review, and commit and push the qualified implementation and evidence.

The official-egui path and the fork-backed path are distinct conditions. Report
both explicitly. A fork improvement must also be compared with the previous
fork revision under the same driver and build settings. Do not attribute that
improvement to an unchanged official dependency.

## Selected architecture

The official 0.37 interface exposes arbitrary mutable graphics access but no
mutation evidence. It retains the complete exact snapshot implementation.
The owned fork can protect lazy checkpoints before mutable access escapes,
allowing ordinary appends to verify only newly encountered prefix descriptors.
Opaque writes materialize the original prefix and retain exact value comparison.
Successful self-equality progress and immutable-galley owners are paint-local.

The ordinary renderer owns a private paint-audit module. A private fallback
extension trait returns no backend on official egui; the fork's same-named
inherent getter supplies its fixed implementation. No application-supplied
provider, public installation registry, source-dependent Cargo feature or custom
compiler flag can forge validity or change the default caller interface.

The synchronous backend invokes the application callback exactly once without
holding a Context lock. It updates cumulative validity before releasing frozen
payloads outside that lock. Normal child-Ui finalization stays inside the check;
unwinding retains the original snapshot-before-child destruction order.

## Verification obligations

- Compare the new module with the frozen snapshot predicate through real
  Context/Painter operations, including gap A-to-B then callback B-to-A.
- Cover old-index and suffix edits, whole-container replacement, clone/swap,
  range transforms, unknown and duplicate layers, and current/other-layer order.
- Retain length and field short-circuit ordering, NaN/panic behavior, opaque
  Drop reentry, Weak lifetimes, and cleanup after an unwinding callback.
- Audit every internal graphics-write path, not only public graphics_mut.
  Registrations must survive viewport replacement without stale certificates.
- Keep the existing product workload source and ImGui implementation unchanged.
  The fork harness uses the original Rust example with an explicit fork lockfile.
- Serialize Cargo jobs and tests. Builds, tests and sampling must not overlap
  formal timing. Reuse existing target directories.
- Preserve the user's pre-existing CI/review files and dirty older egui checkout.

Runtime evidence and frozen executables are staged outside the checkout at
`/tmp/dockspace-paint-checkpoints-20261105-36f3fb0/` until qualification.

## Measured follow-up

The first complete fork comparison confirms that the append-check backend
preserves all 864 sample workload counters. It removes the dominant growing
prefix scans on the ordinary append path. Official consumers retain their exact
fallback. The backend is committed and published as egui
`60c40304dab75305d2ac0f54a1cabfc354669006`.

| Condition | Idle p50 | Two-frame click p50 | Resize p50 |
| --- | ---: | ---: | ---: |
| Previous fork, fresh frozen baseline | 30.966 ms | 83.320 ms | 43.530 ms |
| Incremental callback audit | 11.921 ms | 45.438 ms | 24.315 ms |
| Borrowed hit IDs, sealed program validation, shared Item resources | 10.150 ms | 43.306 ms | 23.090 ms |
| Borrowed fat-plan queries | 9.689 ms | 41.533 ms | 22.120 ms |

Those early conditions had not yet met the target. Separate profiles selected further work on repeated
DropTarget catalog sorting and repeated source-root fingerprint construction.
One sorted row inventory can serve duplicate checks, guide joins and final
canonicalization without reordering input validation. An existing workspace
index can serve drag-source preparation only when both the exact immutable
workspace allocation and version still match; all other cases retain live
validation. A weak source binding avoids keeping an old full workspace alive.

The measured SurfacePaintPlan is 1,392 bytes on this build. Hot query methods
now borrow it. Ordinary method calls keep their syntax; function-item users of
changed public query methods must use a closure to adapt owned arguments.

Independent reviews also found and repaired a resource-presence corner case:
native diagnostics still require the exact tab visual identity, so a ghost's
Item resource alone cannot introduce a previously absent diagnostic shape.

## Final selected result

The complete final fork implementation uses the delivered `production` profile
(release + ThinLTO + one codegen unit), with matching C++ ThinLTO. Two final
ABBA groups (`qualified-twenty-one` and `qualified-twenty-two`) independently satisfy all
three 1,024-pane p50 thresholds: idle 8.315/8.296 ms, click 30.346/30.136 ms,
resize 18.923/18.865 ms. All 1,728 sample workload counters match the previous
fork exactly. The matched old fork also uses ThinLTO; profile effects are not
attributed only to source changes.

Follow-up work includes exact immutable hit-geometry sharing, separately proven
canonical-order reuse, a spatial touch-neighbor hierarchy, resource-only
traversal, smaller program payloads, and exclusive final-output ownership.
The last removes duplicate full validation only while a private wrapper prevents
any output-shape mutation; exact capture identity, complete initial checks and
all final settlement checks remain required. Generic caller-owned runs retain
their original check boundary.

Full 22-group qualification passed on final source `31b1759`. The final report records every
scale's p95/max, rejected experiments, source and build provenance, public API
migration and independent review receipts:
[results](../performance/2026-11-05-paint-checkpoints-results.md).

A separate final-source official-egui production ABBA control also preserves
the full workload. Its 1,024-pane p50 is 28.086/70.552/39.146 ms, retaining the
exact snapshot fallback. The final qualification applies to the pinned fork
condition above. All official and fork observations remain in the evidence
archive, with a complete SHA-256 manifest.
