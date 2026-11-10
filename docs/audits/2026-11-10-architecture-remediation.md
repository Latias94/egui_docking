# Architecture remediation

Date: 2026-11-10 (Asia/Shanghai)

Audit baseline: `2289c889b30343bb705d9b6786f09dc09d633619`

Scope: all findings in the [architecture audit](2026-11-10-architecture-design-audit.md), the inactive-pane minimum contract, and conformance documentation.

All fifteen audit findings, the inactive-pane minimum contract and conformance documentation are repaired. Core migration culminates in `98d33c5`; native preparation, candidate-style encoding and final regressions are committed in `68a2245`. Complete local gates and Linux X11 window qualification pass. The [execution ledger](../plans/2026-11-10-architecture-remediation.md) records the implementation commits and verification. The original audit and measurement artifacts remain unchanged.

## Ownership and repeated work

Ordinary persistent publication now prepares only placement preferences. The append-only external-key maps, generation and item scope remain in their existing binding. Restore uses its already-owned incoming binding. No new persistent-map abstraction, sidecar revision or cache registry was added.

A proven single-command no-op selection keeps its workspace allocation and derived authority. Net-zero transactions still advance issuance where needed to prevent ABA. Semantic snapshots and the workspace index derive root data once for the same exact workspace source and version; prospective native work retains its independent provenance.

The egui pointer adapter shares immutable receiver and focus slices across private drafts. One packet-scoped ownership join subtracts distinct declared widgets by rectangle multiplicity; all edges and stationary probes reuse that frozen result. Unknown widgets at the same rectangle remain blockers. Signed zero and NaN preserve the original floating-point equality behavior. This removes repeated joins, not the remaining per-point scan of unknown blockers.

Finite native admission keeps pointer count, payload bytes and unprepaid-roster count in the existing ingress journal. The locked callback performs one complete growth preflight before allocation. Explicit diagnostics independently recount retained owners. Owner/lifetime traversals remain; this is not a claim that the entire callback is constant time.

## Public host and configuration behavior

Committed pane-focus requests can be queried independently of renderer output admission. A request can prepare its actual observation; submitting it still checks domain, epoch, intent, target and native activation. An uncommitted native observation cannot expose a new request through the independent query.

Measurement policy is a per-surface core decision. The gesture source and exact-preview surfaces retain their geometry; an unrelated surface can refresh title/minimum metadata. Adapters keep their framework-resource and output-backpressure constraints. Every open pane, including inactive or unloaded content, supplies minimum-size metadata independently of constructing its UI. Missing metadata defers rendering and does not block accepted semantic selection.

Configuration closes the accepted semantic prefix and updates the same private candidate before measurement and encoding. Earlier input retains its original policy and visible facts. Inline and deferred compilation validate the new candidate policy. Restart discards only derived encoding work and retains input/configuration without replay. Native measurement and paint use the matching candidate style; public style and configuration publish after final commit.

Native focus dispatch ownership ends at its exact dispatch callback. `Requested` does not invent actual focus. Successor requests no longer wait for the OS to grant their predecessor focus. Quarantine distinguishes undispatched failure from indeterminate dispatched work and drains the original exact callback reservation.

Style admission still validates all adapter fields, then compares the complete mapped configuration built from the accepted core settings. A hand-maintained field list only names diagnostics. The full native CI runner enables all features so existing persistence regressions run. Product comparison verifies source, reference and executable hashes again before marking a run successful; a temporary executable-substitution experiment confirms rejection.

## Native preparation boundary

The docking core no longer owns the egui hidden/pre-show/show/visible/post-show script. `NativeCreateStrategy`, the core ShowWindow effect/acknowledgement, staging paint requests and staging output/report transport are removed. Enrollment takes the initial pointer roster; `with_presentation` takes only the observed state.

The native adapter runs its preparation procedure inside the existing exact window resource. Preparation paints attach to the existing output owner; show uses the existing exact callback reservation. Prepared requires a pre-show Presented result, matching show callback, later real Visible observation and post-show Presented result. Geometry, renderer or visibility loss invalidates the generation. A Created revocation must commit before the same geometry can become Prepared again.

Core retains create-acknowledgement causality, binding/provider identity, positive coordinate and visibility authority, continuous Prepared provenance, fresh semantic first-live output floors, source reservation and resource conservation. Only accepted exact semantic first-live output publishes source transfer and topology atomically. Cancellation, compensation, replacement and final quiescence remain with their original owners. Retained source resources are not preparation placeholders and remain in the core lifecycle ledger.

See the [breaking migration guide](../ENGINE_FIRST_MIGRATION.md) for the resulting host contract.

## Measured persistence cost

The unchanged [audit probe](2026-11-10-architecture-design-audit/persistence_cost.rs) holds one live pane and varies registered/history keys. It measures nine batches of 64 release-mode idle commits after eight warmup frames. Timing runs without active heap profiling; one separate dhat frame counts allocations. Setup and key construction are outside both windows.

| Historical keys | Before median ns/frame | After median ns/frame | Before allocations/frame | After allocations/frame | Before requested bytes/frame | After requested bytes/frame |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 11,293 | 11,452 | 74 | 70 | 53,032 | 42,792 |
| 128 | 19,228 | 11,667 | 368 | 70 | 74,424 | 42,792 |
| 1,024 | 68,487 | 11,613 | 2,460 | 70 | 232,056 | 42,792 |
| 16,384 | 919,411 | 11,338 | 38,294 | 70 | 2,920,984 | 42,792 |

The unbound control is 10,776 ns, 68 allocations and 42,496 requested bytes after remediation. The final probe runs against `68a2245` and verifies the frozen source manifest before and after execution. The important invariant is the flat allocation slope of persistent frames; the independent consumer asserts exact equality across history sizes. Requested bytes are cumulative traffic, not peak/live memory. Timings are local macOS arm64 samples and include the integrated core changes. These results do not establish an ImGui ratio or whole-product renderer throughput.

Pointer tests provide separate structural evidence: K=1/16/128 content registrations and 128 edges produce one join with `(joins, bindings, rectangles) = (1, 2K, K + unknown)`. Arc identity tests verify shared immutable tables through discarded drafts and reverse output settlement. These are structural assertions, not measured whole-frame allocation or latency results.

Raw results, source/binary metadata and integration checkpoints are in [the remediation artifacts](2026-11-10-architecture-remediation/).

## Verification

All final gates pass. Cargo commands ran serially with one build job and shared targets. The final [source manifest](2026-11-10-architecture-remediation/final-source-hashes.json) records 599 exact source, manifest, lockfile and script hashes at `68a2245`. The final probe confirms those inputs remain unchanged. Commands and captured logs are in [serial-gates.json](2026-11-10-architecture-remediation/serial-gates.json), [native-gates.json](2026-11-10-architecture-remediation/native-gates.json) and [checkpoints.json](2026-11-10-architecture-remediation/checkpoints.json). Captured log files normalize trailing whitespace and terminal blank lines only.

| Final local gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,293 passed; three existing skips |
| Independent engine host, default / persistence | 295 / 339 passed; both include three depth-10,000 cases |
| Conformance library | 37 passed, included in the workspace total |
| Official egui consumer / caller-owned demo | 24 / 3 passed |
| Official product consumer, default / persistence | 114 / 119 passed |
| Pinned native fork, all features and targets | 592 passed; one existing ignore |
| Native library without debug assertions | 560 passed; one existing ignore |
| Workspace and native correctness/suspicious Clippy | Passed |
| Strict core, adapter and native API documentation | Passed with warnings denied |
| Workspace doctests | 11 passed |
| Formatting and core dependency boundary | Passed |
| Core package verification / adapter package contents | Passed |

Actual Linux X11/Glow qualification ran under Xvfb/Openbox in a one-CPU, 4 GiB container using the same frozen sources. Six separate eframe processes pass owned-update/repeat publication, candidate style and core geometry in the same pass, output-capacity recovery, native UI ingress retry/deduplication, and text retention both before editor resumption and after native resize. These are executed window tests, rather than DISPLAY skips. Exact commands, timestamps and results are in [real-x11-gates.json](2026-11-10-architecture-remediation/real-x11-gates.json).

Both unlimited and bounded 4 MiB storage profiles pass native creation, exact first-live admission, redocking, retirement and final completion. The actual editor runner passes five physical docking journeys and the complete lifecycle sequence, including twelve focused text edits, six child lifetimes, Dock Back, OS-close recovery, programmatic close/allow/reopen, lock/reset, JSON missing-placeholder reload and child replacement. Root rebinding accepts fresh editing; every child is destroyed, the original Root survives and the final coordinator is quiescent. See [the editor log](2026-11-10-architecture-remediation/actual-editor-lifecycle.log).

This qualification covers the tested Linux X11 profile. It does not establish macOS/Windows physical parity, arbitrary mixed-DPI behavior or OS focus-refusal coverage. These are local results; no new remote CI result is claimed.

Independent review found no blocking P1/P2 in the core Prepared seam, the native preparation/callback/snapshot seam, configuration encoding, focus dispatch, bounded input accounting, or shared pointer tables. A separate migration review checked that source conservation, revoked readiness, cancellation, replacement and cleanup counterexamples survived removal of the core-specific preparation script.

The added coordinator regression runs the real preparation state and production snapshot-settlement gate through frame drop, successful core commit, adapter settlement and renewed first-live output. Its roster sidecar is a tokenless model fixture. It proves the settlement barrier and output floor; it does not substitute for real fork output/callback and window-manager execution.
