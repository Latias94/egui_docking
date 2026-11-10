# Focus-provider storage and menu review

Date: 2026-11-10 (Asia/Shanghai)

Baseline: `a1af041`. Implementation commits: `7c36060` and `e159073`.
Three independent review lanes rotate across persistence
and layout replacement, egui keyboard/focus and native lifecycle failure paths.
The primary agent audits retained resources, reproduces findings and owns
serial Cargo verification. Excluded review and reference directories remain
untouched. Final serial and actual Linux X11 qualification pass at implementation
revision `e159073`.

## R01: removed surfaces accumulate private focus-generation history

`RuntimePaneFocusState` kept a `BTreeMap<SurfaceId, PaneFocusObservationGeneration>`
for every surface that had reported UI focus. The core removes retired surfaces'
actual focus history, but the runtime's provider map had no reclamation path.
Repeated layout replacement or native surface churn could therefore retain
arbitrarily many retired identities for the lifetime of a session.

The independent public host regression executes layout reset, measurement,
complete paint, exact Presented settlement and focus reporting. A paired control
executes the same workload without focus reporting. The difference in live
tracked allocations includes current core focus state and the runtime provider;
it isolates the additional focus cost from unrelated identity/stream retention.
Before repair, that difference grows from 1,760 bytes after sixteen retired
surfaces to 36,320 bytes after 1,024. The regression fails in nextest run
`aabd4bb6-cd06-4cc6-86a9-1dc4ae5c06ff`.

[focus.rs](../../crates/dockspace/src/runtime/focus.rs) now owns one session-wide
provider watermark. Both independent host-focus facts and exact pane-request
observations allocate `max(watermark, baseline) + 1` with checked arithmetic.
Core requires each surface's generations to increase, not to be consecutive.
The stronger global order therefore needs no retired-surface map or cleanup
protocol. Failed candidates still consume their private observation identities;
retry cannot reuse them. Existing epoch, intent, surface/item and exact
host/stream/native-binding checks continue to decide observation authority.

The public allocation regression and 34 layout-reset, focus-restore and
owned-update regressions pass in `02c7c072-2b49-44bc-9dfd-921d4140f465`.
The complete independent persistence host passes 344 tests in
`76f30426-1af0-4ee0-af1c-0a10f42e13b9`. Its successful allocation transcript
reports 1,088 additional live bytes for both history sizes. The implementation
and regression are committed in `7c36060`.
Independent reviews confirm the cross-surface and failed-candidate semantics.
This is a retained-memory repair, not a frame-time or ImGui-ratio result.

## Review scope

Persistence review checks source-document validation on suspended restore,
identity-scope expansion at replacement, retained closed keys and monotonic
allocation frontiers, native root continuation and atomic publication. Native
review traces failed-create ownership, compensating close, preparation
revocation, late callbacks and shutdown drainage. These lanes establish no
additional P1/P2 from the inspected sequences.

Keyboard review checks disabled and AccessKit actions, focus request settlement,
owned/callback passes, overflow and presentation menus.

## R02: overflow Enter activates a different row from the framework focus

A real keyboard regression opens a two-row overflow menu, establishes A as the
core and framework focus with Home, presses Tab and immediately presses Enter
in the next run. The terminal framework focus after Tab belongs to B, but Enter
selects A. Nextest run `14e82bd9-b1b9-4cb8-83a9-076ff50f138a` reproduces the
incorrect selection; the other five directional, reverse-Tab and discarded-pass
controls pass. Repeated passes can hide the discrepancy by reasserting A.

The adapter requested the core-focused row's framework focus during every paint,
while allowing egui's sequential Tab traversal. Keyboard activation then used
the old core-focused row instead of the actual framework-focused widget.

[tab_chrome.rs](../../crates/egui_dockspace/src/product_render/tab_chrome.rs) now
reads actual egui focus after registering the menu rows. Enter and Space activate
that row; keyboard activation outside the menu remains with the focused widget.
Arrow, Home and End navigation starts from that same actual row and cancels
duplicate framework cardinal movement. Framework traversal updates core menu
highlight and reveal through the existing prepared focus action. An instance
keeps one temporary semantic-edge marker containing the menu and last core row
widget identity. A new core edge is recorded before considering a request.
Positioning requires no newly acquired framework focus and either a new menu,
no actual focus or actual focus still on the previous core row. This preserves
initial positioning and removed-row fallback while preventing a confirmation
from reclaiming focus that the host has already moved. Ordinary repaint and
discarded passes do not reassert the previous row. The marker retains no per-menu
or per-surface history. Tab and Shift+Tab continue to use egui's traversal,
including its deferred reverse traversal, and Escape retains the existing
return-to-trigger behavior.

The public adapter regressions use real raw key events and fixed semantic
expectations: A to Tab to B to Enter selects B; B to Shift+Tab to Enter selects A.
Both run with zero and two late discarded passes. A separate callback-host
journey traverses A, B and two real external buttons, then checks that Enter
activates only the second button. It also overlaps the core acknowledgement of B
with the next Tab and keeps B selected. The two existing clamped ArrowUp controls
remain. All seven pass in `295a037d-f905-44e2-a034-efaffde8e811`, and all 178
adapter all-feature/all-target tests pass in
`19c8c4e1-8cfb-425e-92fb-07d77b5c604e`.

Final independent review exposed a remaining confirmation-versus-command error
in that first repair. A newly acknowledged B could override an AccessKit Focus
request or an application's `before_docking` focus request. Checking only newly
acquired focus would still miss an application that moves focus after `show`
in the previous run. All three public regressions fail at their intended focus
assertions in `2a3e27a8-ecf3-4d25-8f5e-3fe289483947`
([clean failing transcript](2026-11-10-focus-and-menu-review/overflow-focus-confirmation-reproduced.log)).
The previous-core-row
identity condition above fixes all three without caching actual framework focus
or scanning raw accessibility events. Each regression uses zero and two late
discarded passes; the application callback requests focus only once. The
external-button cases also require Enter to activate the intended real button
exactly once. The final 21-test chrome and owned touch-menu group passes in
`4813aeae-c6ee-418d-99cb-20ed61153233`, and independent review reports no remaining
blocking P1/P2 in this diff. Final full adapter/host qualification follows below.
The implementation and eight new public adapter regressions are committed in
`e159073`.

The initial repair probe incorrectly required Shift+Tab's destination before
egui's next run. That fixture expectation was corrected to preserve egui's
deferred traversal and still require immediate Enter to select A; it was not a
new production defect. The failing original forward-Tab reproduction and both
repair transcripts remain in the artifacts.

## Verification

Verification is serial with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` and the
existing target directories. Commands, run IDs and before/after receipts are
retained in [the qualification artifacts](2026-11-10-focus-and-menu-review/).
The full adapter and consuming host gates were repeated after the final
confirmation repair; core and independent-host source had no intervening changes.

| Gate | Result | Receipt |
| --- | --- | --- |
| Core and shared conformance, all features and targets | 2,133 pass; 3 existing skips | [core-tests.log](2026-11-10-focus-and-menu-review/core-tests.log), [command](2026-11-10-focus-and-menu-review/core-test-gate.json) |
| Adapter, all features and targets | 181 pass | [final-adapter-tests.log](2026-11-10-focus-and-menu-review/final-adapter-tests.log) |
| Complete independent persistence host | 344 pass | [host-serde.log](2026-11-10-focus-and-menu-review/host-serde.log) |
| Official unmodified-egui consumer | 24 pass | [final-official-consumer.log](2026-11-10-focus-and-menu-review/final-official-consumer.log) |
| Product default / persistence configurations | 114 / 119 pass | [default](2026-11-10-focus-and-menu-review/final-product-default.log), [persistence](2026-11-10-focus-and-menu-review/final-product-serde.log) |
| Pinned native fork, all features and targets | 593 pass; 1 existing skip | [final-native-tests.log](2026-11-10-focus-and-menu-review/final-native-tests.log) |
| Final core/adapter/native Clippy and strict documentation | Pass | [final-gates.json](2026-11-10-focus-and-menu-review/final-gates.json) |
| Workspace doctests | 11 pass | [workspace-doctests.log](2026-11-10-focus-and-menu-review/workspace-doctests.log) |
| Workspace/independent-host/native formatting and core dependency boundary | Pass | [serial-gates.json](2026-11-10-focus-and-menu-review/serial-gates.json), [final-gates.json](2026-11-10-focus-and-menu-review/final-gates.json) |

The core and adapter total is 2,314 passing tests with three existing skips.
Clippy denies `correctness` and `suspicious`; unrelated existing style warnings
are not part of that gate. Rustdoc uses `RUSTDOCFLAGS=-D warnings`.

The independent-host regression is reproducible without egui or native windows:

```console
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --all-targets --locked --target-dir target --test-threads=1
```

## Actual Linux X11 qualification

All thirteen recorded X11 build, window-manager and execution gates pass in
[real-x11-gates.json](2026-11-10-focus-and-menu-review/real-x11-gates.json).
Seven exact eframe tests execute in separate real-window processes, including
successor-preparation isolation, terminal output hooks, style publication,
capacity backpressure, ingress deduplication, text retention and native resize.
The 4 MiB bounded native smoke completes create, first-live, redock and retirement
in [native-redock-bounded.log](2026-11-10-focus-and-menu-review/native-redock-bounded.log).

The [complete editor driver](2026-11-10-focus-and-menu-review/actual-editor-lifecycle.log)
passes both docking and lifecycle profiles, completing at
`2026-11-10T07:21:57.655539+00:00`. It verifies twelve real focused edits and six
destroyed child lifetimes, first-live ownership, Dock Back, OS-close recovery,
programmatic close/Allow/reopen, arrangement lock, Default and JSON restoration,
live Child replacements, Root rebinding and fresh editing after both Root
replacements. The original Root X11 window survives, all children are destroyed
and final quiescence holds.

The existing container uses Linux X11, Xvfb/Openbox at 1920 by 1080 and Glow
software rendering. It is limited to one CPU and 4 GiB RAM. Container builds
reuse `/native-target`, with one Cargo build job, no incremental compilation,
development/test debug info disabled and one LLVMpipe thread. These checks
refresh the recorded finite X11 profiles; they do not extend physical backend,
mixed-DPI or ImGui frame-time claims. No implementation source changes follow
the qualification revision.
