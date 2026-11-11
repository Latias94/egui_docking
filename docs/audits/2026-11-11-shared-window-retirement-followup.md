# Shared-window publication and retirement follow-up

Baseline: `b174192`. This follow-up reviews the shared external Root composition
implemented by G01. It preserves the previous
[qualification record](2026-11-11-shared-native-window.md) and its finite platform
scope. No renderer-free ImGui benchmark was rerun in this follow-up.
Status: implementation, independent review and final qualification complete.
Implementation commits: `4272736`, `474bb9d`, `26084e4`, `ad3c4ce`.

## Corrected ownership boundaries

The principal defect was the adapter preparing native resource changes before
the core's final vacancy reduction. It could see Root continuations but could
not distinguish a removed placement from physical window retirement.

`DockspaceHostFrame::prepare_commit` now finishes reduction and reserves core
publication storage while retaining the exclusive session borrow. Its exact
continuations, placement detachments and complete proposed rosters let the
adapter prepay under its existing mailbox lock before publication. The ordinary
`commit` method uses the same preparation path. Document sidecars remain atomic
with the owned core candidate; the duplicate publication branches were removed.

For a Root continuation, the complete published predecessor roster equals the
disjoint union of continuation predecessors and proven placement detachments.
The complete final roster equals continuation successors. The physical receipt
moves once per window, while each old exact placement keeps its own drain lane.
Ordinary docking vacancies use the same final-reduction proof.

The Root driver now treats an application placement declaration as a pending
request. Input freezes one roster per token; the ordinary entry and idle redraw
obtain their complete roster from committed core state. Caches share prepaid
backing and are checked against the core. Reset, restore and ordinary vacancy
invalidate requests which cannot apply to the resulting Root scope.

## Repaired failure cases

- Adding B beside an implicit whole-client A used to replace A's exact identity
  even when A's effective geometry was unchanged. Equivalent declarations now
  preserve identity and presentation while saving explicit geometry for resizes.
- A shared Root could claim a Child reserved by a restored native establishment.
  Placement admission now preserves that existing creation owner.
- A full producer refused `[A, B] -> [A, C]`. Immediately obsolete placements
  return their candidate budget; exact cleanup and terminal guards retain slots.
- A placement changed and vacated within one candidate could leave an
  unquiesceable guard. Unpublished placement-only terminal guards are compacted.
  A genuinely required physical cleanup receives a prepaid producer lane and
  window row, including when it never appeared in the published registry.
  This repair uses the published exact retirement state: a producer lane already
  released by host quiescence, and its physical window row, must not be recreated
  from retained core cleanup. The original one-slot public-host regression
  exposed that intermediate error; the corrected path keeps its fixed capacity
  and additionally asserts that neither old owner returns.
- Changed placements followed by reset or restore could retire an unpublished
  predecessor. They now publish in separate frames. An equivalent explicit
  declaration retains its exact identity and may precede replacement.
- Shared reset or restore which removed B incorrectly required a continuation
  for B. Proven detachment now covers B without synthesizing a successor.
- Ordinary B vacancy could remain blocked by A's active physical resource.
  Placement drains still protect B's exact callbacks, effects, receivers and old
  output tokens, while subsequent A redraws do not keep B alive indefinitely.
- Physical retirement and sibling detachment now work in either order. An exact
  physical owner is reused without losing its receipt or destruction threshold;
  only genuinely new exact rows consume more retirement slots.
- Physical close dispatch incorrectly required its original transport tag to
  remain a placement. It now validates the real window lifetime plus the exact
  route, receipt and close correlation.
- Physical retirement could drain only that original transport tag, leaking A/B
  producer lanes. The owner now checks and drains all retained exact lanes in its
  window before completing. Partial input-capacity refusal keeps the owner and
  retries only unrecorded lanes. Normal service commits the accepted prefix
  instead of entering fatal shutdown.
- Native popup rows outside DockBounds were rejected despite belonging to an
  active, presented popup inside the physical clip. Exact popup coverage now
  participates in window selection. Unpresented, closed and clipped artifacts
  remain rejected. Locked smooth-scroll delivery retains its existing contract.
- Child receiver staging read ambient Root DPI after the Child pass completed.
  The completed hit graph now carries the actual finalized output scale through
  renderer and core presentation publication.

## Regression ownership

| Boundary | Regression owner |
| --- | --- |
| Final candidate drop, exact detachment, fixed producer capacity and reset ordering | Public-host `shared_window_layout`, `shared_window_retirement` and `native_input_capacity` suites |
| Unpublished placement vacancy, real physical cleanup, exact producer enrollment and atomic refusal | Core `runtime::native::tests::shared_placement_vacancy` |
| Detached callback drainage, physical close correlation, all-lane quiescence and accepted-prefix service | Native coordinator workspace-replacement tests, retirement capacity tests and `surface_driver::tests::retirement_backpressure` |
| Committed Root composition, pending requests, frozen token geometry and late replacement | Native application shared-window tests and isolated real-output backpressure tests |
| Completed Child hit authority and finalized scale through renderer/core publication | Receiver unit tests and isolated real Child-token process |
| Presented popup coverage and locked smooth-scroll continuity | Public-host `native_popup_plane` and existing host conformance tests |

## Verification boundary

Three independent review lanes cross-checked core authority, native lifecycle
and egui output behavior. Confirmed findings were repaired with deterministic
regressions before final qualification. Static review does not substitute for
runtime or physical evidence.

Real Child output tests control Root 1x and Child 2x geometry while obtaining an
actual eframe Child token. They verify receiver publication and resolution, not
physical mixed-DPI monitors. The platform backend scope remains Linux X11;
Wayland, Windows and macOS physical behavior are not extended by this record.

## Final execution results

| Gate | Result |
| --- | --- |
| Workspace all features and targets | 2,356 passed; three existing skips |
| Independent public host with persistence | 430 passed |
| Affected public-host suites with default features | 38 passed across five binaries |
| Native workspace, including fork product consumer | 654 passed; one existing skip |
| Workspace and native correctness/suspicious Clippy | Both passed |
| Workspace and native documentation with warnings denied | Both passed |
| Workspace documentation examples | Eleven passed |
| Three-workspace formatting and renderer-neutral dependency boundary | All passed |
| Isolated actual-eframe X11 processes | Nine passed |
| Bounded 4 MiB create/first-live/redock/retirement | Passed; root-boundary resident/reserved peaks 2,157,864/3,274,816 bytes |
| Shared-window physical journey, normal and late-discard | Both passed |

The [serial command receipts](2026-11-11-shared-window-retirement-followup/serial-gates.json)
record all thirteen final serial gates, UTC timestamps and portable log paths.
Cargo uses one build job with incremental compilation disabled and reuses
`target`. Stored terminal logs normalize trailing blank lines.
The [44 changed Rust source hashes](2026-11-11-shared-window-retirement-followup/rust-source-sha256.json)
were checked unchanged throughout final execution. The
[intermediate public-host failure](2026-11-11-shared-window-retirement-followup/intermediate-public-host-failure.json)
and its log remain separate from these successful receipts.

The [real X11 command receipts](2026-11-11-shared-window-retirement-followup/real-x11-gates.json)
record two successful Linux builds and twelve successful runtime processes.
Each ignored eframe test runs in its own process; winit event-loop recreation
is not supported in one test process. Both shared-window journeys complete
A-to-B docking, Child separation and return, changed B origin/scale, Root
replacement and WM close. The platform is the existing Xvfb/Openbox, Glow,
single-screen profile with native DPI one.
The storage peaks cover sampled host-owned input data; renderer allocations,
allocator metadata and fixture-owned data are outside that allowance.

Three review lanes finish without an unresolved confirmed P1/P2 in the inspected
follow-up scope. The full editor drivers retain the prior G01 qualification;
this follow-up does not claim that they or the ImGui benchmark were rerun.
