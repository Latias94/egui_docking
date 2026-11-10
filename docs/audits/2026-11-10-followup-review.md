# Architecture follow-up review

Date: 2026-11-10 (Asia/Shanghai)

Baseline: `306af2d421e650da591c5ca7bb957f33a1b6c84b`

Scope: core transactions, persistence and geometry; egui input, measurement and output ownership; native preparation, cancellation, replacement and terminal settlement. Three independent Codex review lanes inspect the implementation, with the primary agent reproducing and integrating repairs. Cargo verification is serial with one build job and the existing targets. Excluded review and reference directories remain untouched.

This review confirms five P2 findings and no new P1. Each confirmed defect has a failing reproduction and an implemented repair. Targeted verification, all complete serial gates and actual Linux X11 qualification pass against frozen implementation `116917b`.

## F01 — P2: finite corners can introduce an infinite extent into a saved document

The two constructors in [geometry.rs](../../crates/dockspace/src/geometry.rs), `LogicalRect::from_min_max` and `PhysicalRect::from_min_max`, previously checked finite, ordered corners without checking their difference. Corners at `-f64::MAX` and `f64::MAX` therefore produced an infinite width. The infallible `size()` methods then constructed sizes that their own validated constructors reject.

The public persistence reproduction builds a contained pane from that logical rectangle, creates a persistent session and successfully saves it. Restoring those exact bytes fails with `invalid type: null, expected f64`: JSON serialization encoded the infinite extent as `null`. The baseline failure was reproduced in nextest run `b7b9192f-7239-44ae-ab1b-81f8dcbb344e`; the primitive constructor regression independently failed in `2fd6f9bc-b0fd-4e0d-a5a5-6f205aa74641`.

The repair rejects non-finite spans at the two rectangle construction interfaces. This keeps `size()` infallible and makes the invariant available to every caller. Four repeated span-finiteness checks are removed from contained placement. The original infinite-span placement counterexample moves to the constructor regression, with both axes and logical/physical coordinates covered. Arithmetic overflow checks for later computations remain.

Compatibility: `from_min_max` now returns `GeometryError::NonFinite` for ordered finite corners whose extent cannot fit `f64`. Empty rectangles, negative origins and the largest finite extents remain valid. The persistence regression checks an actual save/restore at the largest finite extent, so rejecting every extreme input cannot make it pass vacuously.

## F02 — P2: an obsolete output can mutate successor preparation

The deferred viewport paint path in [mailbox.rs](../../crates/egui_dockspace_native/src/mailbox.rs) previously obtained preparation work by viewport, started its paint, and only then checked whether the output reservation belongs to that binding. A rejected predecessor output could consequently finish the successor paint as failed, revoking its preparation generation and invalidating its pending revocation receipt.

The real X11 output-token regression reproduces the fault: rejecting the predecessor changes the successor's Created observation from generation 4 to 5. The test directly exercises the production deferred-paint classifier. Eframe supplies actual output identities, pass lifetimes and terminals; the preparation renderer basis and route replacement are deterministic owner fixtures, rather than a claim of physical OS-window replacement in this test.

The exact-binding guard now runs before any preparation state mutation. The regression passes after the repair, including the successor paint and original Created receipt. The predecessor's abandoned owner remains until its own real terminal arrives; the lower-level binding check remains in place. The existing Xvfb CI selection now includes this ignored real-window regression. No permanent hang or incorrect first-live admission has been established from this sequence.

## F03 — P2: ensuring an existing key copies the frozen identity history

`begin_update` leaves a suspended candidate holding the document's immutable item-identity scope. The unnecessary mutation is in [document.rs](../../crates/dockspace/src/document.rs); batch allocation lives in [external_item_key.rs](../../crates/dockspace/src/external_item_key.rs). Calling public `ensure_external_item` for an already assigned key previously unconditionally invoked `Arc::make_mut`, copying that complete scope before discovering that its insertion is a no-op. The map, frontier and item identity have not changed. Starting a fresh suspended update each frame can repeat the cost; repeated queries after a single separation do not necessarily copy again.

The independent dhat consumer reproduces the allocation slope through those public calls. At one historical key the operation allocates three blocks and 168 bytes; at 128 keys it allocates fourteen blocks and 1,408 bytes. The exact-equality regression fails in nextest run `f197ea77-2768-4a5b-9101-1d65f084ae17`. These counts measure cumulative allocation traffic, not peak memory or latency.

The repair mutates the scope only for an actual new key. Batch registration returns an iterator over the already committed batch's new item identities internally and extends only those, instead of re-inserting the complete history. The public method preserves atomic failure behavior and caller-order identity assignment. A genuinely new key still copies a shared scope so it cannot authorize input in the suspended candidate.

## F04 — P2: overflow-menu and framework focus navigation can both run

The overflow menu in [tab_chrome.rs](../../crates/egui_dockspace/src/product_render/tab_chrome.rs) previously consumed ArrowUp/ArrowDown and queued core navigation, but egui had already recorded its own directional focus movement at pass start. End-pass geometry navigation can consequently move focus away from the core-selected row, especially when core clamps navigation at the first or last row. The ordinary tab keyboard path already clears this pending framework movement.

Real egui regressions reproduce incorrect focus and a failed row activation with consecutive ArrowUp and Enter, in both a single pass and two late-discard retries (`e79ea5f9-d46d-42ca-b093-f11faa3f4cf1`). Clearing pending framework focus movement after accepting core menu navigation fixes both. No new focus owner is introduced. The complete five-test renderer subset passes (`25a2aa38-121f-4030-a5d9-29348eb9b24f`).

## F05 — P2: narrowing rectangle components can overflow egui endpoints

The adapter in [product_render/geometry.rs](../../crates/egui_dockspace/src/product_render/geometry.rs) previously checked each `f64` rectangle component before narrowing to `f32`, then added the narrowed minimum and size. Two representable components can sum to an infinite endpoint. Conversely, finite endpoints can have a non-finite `f32` extent. Conversion now narrows the original minimum and maximum corners once, then validates both the target rectangle and its computed size. Unrepresentable rectangles return no drawable geometry. A separate rounding regression verifies that adjacent rectangles retain the same shared edge, rather than recomputing a maximum from separately rounded minimum and width. This is separate from F01: the source rectangle can be fully valid in the core's `f64` domain.

## Implementation commits

| Commit | Scope |
| --- | --- |
| `64feee4` | Reject non-finite core rectangle spans and migrate the counterexample to construction |
| `d69cf5e` | Retain existing-key scope sharing and insert only newly allocated batch identities |
| `6904396` | Reject obsolete native outputs before mutating successor preparation; wire the real-token regression into Xvfb CI |
| `116917b` | Preserve overflow-menu focus and narrow original rectangle corners consistently |

## Verification

All fifteen serial gates pass against `116917b`. Commands, timestamps and complete logs are in [serial-gates.json](2026-11-10-followup-review/serial-gates.json). The [source inventory](2026-11-10-followup-review/source-hashes.json) covers 603 source, manifest, lockfile, script and workflow files; verification confirms they remain unchanged after the gates.

| Complete gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,304 passed; three existing skips |
| Independent persistence host, all targets | 340 passed, including three depth-10,000 cases |
| Official egui consumer | 24 passed |
| Product consumer, default / persistence | 114 / 119 passed |
| Pinned native fork, all features and targets | 592 passed; one existing ignore |
| Workspace and native correctness/suspicious Clippy | Passed |
| Strict core, egui and native documentation | Passed with warnings denied |
| Workspace doctests | 11 passed |
| Workspace, host and native formatting | Passed |
| Core dependency boundary | Passed |

The geometry/placement subset also passes 24 tests, the identity-scope subset passes five, and both independent allocation-slope tests pass at histories 1/128/1,024/16,384. The real egui menu/geometry subset passes five tests. Each run ID is retained with the targeted receipts.

Actual Linux X11/Glow qualification passes seven separate eframe test processes: obsolete-output isolation, owned-update publication, same-pass style/core geometry, framebuffer/output-capacity recovery, ingress retry/deduplication, and both editor text-retention cases. They execute against a real Xvfb/Openbox display, rather than a DISPLAY skip. Commands, timestamps and process results are recorded separately in [real-x11-gates.json](2026-11-10-followup-review/real-x11-gates.json); the exact test binary hash is retained alongside them.

The bounded 4 MiB profile passes creation, semantic first-live admission, redocking and retirement. The complete actual-editor runner passes docking, close/recovery, lock/reset, JSON missing-placeholder reload, child replacement and Root rebinding, including twelve real focused edits and six child lifetimes. All child windows are destroyed, the original Root survives and the final coordinator is quiescent. See [the editor log](2026-11-10-followup-review/actual-editor-lifecycle.log). Source hashes remain unchanged after this qualification too.

These are local results for the tested Linux X11 profile and the recorded build configurations. The CI test selection is repaired; no new remote CI run is claimed.

Independent cross-reviews found no blocking P1/P2 in the final geometry, identity-scope, native-output or menu repairs. Core review also traced close decisions, focus successors, rejected product actions, candidate configuration and encoding restart without confirming another defect. Adapter review checked menu generations, owned-run rollback, frozen pointer blocker multiplicity and first-pass input consumption. Native review checked exact preparation callbacks, later visibility, readiness revocation settlement, cancellation, compensation, finite output backing and final resource release.

The native output regression uses a model preparation basis and direct production readiness settlement around real output tokens; it does not independently prove the coordinator's complete snapshot publication sequence. Actual editor journeys qualify that separate integration. Allocation assertions establish the identity-history slope; they do not measure wall-clock improvement or a new ImGui ratio. Unusual numeric examples exercise type and serialization invariants and are not claims about ordinary desktop coordinate magnitudes.

Reproduction run IDs and targeted results are recorded in [targeted-receipts.json](2026-11-10-followup-review/targeted-receipts.json). The source inventory, complete gate commands and logs are retained in [the follow-up artifacts](2026-11-10-followup-review/). Captured logs normalize trailing whitespace and terminal blank lines only.
