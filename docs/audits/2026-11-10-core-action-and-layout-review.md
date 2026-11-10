# Core action authority and split normalization review

Date: 2026-11-10 (Asia/Shanghai). Baseline: `828e6d1`.
Implementation commits: `ce42e4e`, `ba127f0`, `8f75298`, `38433c7`.
Qualification fixture revision: `4cb90d2`.

This review focuses on renderer-neutral `dockspace`. Three independent lanes
inspect transaction/publication, topology/layout and input/presentation. The
primary lane checks identity, persistence and retained resources, reproduces
findings through the public host interface and owns serial Cargo verification.
Review and reference exclusions remain untouched. Implementation and final serial
and actual X11 qualification are complete at `4cb90d2`.

## R01: independent sessions alias presentation transition identities

`DockspacePresentationTransitionId` contained only `ReductionCause`. A
programmatic native root transfer uses an Input cause whose tick, ordinal and
producer sequences are local to its engine. Two sessions with the same layout
and input history therefore returned equal request identities. A host sharing an
asynchronous completion queue could match a terminal to the other session's
request. The public type allowed equality comparison without limiting it to one
session.

The independent host regression registers native Root and Child declarations in
two sessions, supplies exact window facts, executes complete output and reports
Presented through the normal protocol. Each session then requests the same
complete-child redock. The request-inequality assertion fails in
`76585ebb-e1d2-4d6a-abf9-f8da9d8d67d4`
([before](2026-11-10-core-action-and-layout-review/transition-identity-before.log)).

Commit `ce42e4e` includes the existing engine authority domain alongside the
unchanged cause. Ordinary, semantic-interaction and pointer request projections
and terminal rehome events all use the transition's authority domain. No new
allocator or event protocol is introduced. The regression passes in
`84f79110-6233-4d05-bec7-3f04a12d1e83`
([after](2026-11-10-core-action-and-layout-review/transition-identity-after.log));
it also proves that each actual Presented terminal matches its own request and
cannot match the other session. Independent review confirms the request and
terminal construction paths.

## R02: direct prepared Escape cancels a successor gesture

`prepare_escape_cancel` captured the exact interaction status, including gesture
session identity. Direct submission discarded that status when lowering the
action to the raw cancel-current-interaction input. Cancelling the old pointer
stream and pressing again can start a successor without changing the workspace
version, so the old Escape cancelled the new gesture. The deferred path already
checked the captured status.

The public regression presents a close receiver, presses it, captures Escape,
cancels that stream, starts another press at the same workspace version and
submits the old action. The successor-survival assertion fails in
`a9e1573a-8b77-4a21-be8c-6420910a19d2`
([before](2026-11-10-core-action-and-layout-review/action-authority-before.log)).
The same regression includes a fresh direct Escape positive control. Commit
`ba127f0` retains the witness, full captured interaction status and delivery in a
dedicated prepared Escape input. It requires the exact workspace version before
reusing the existing frozen-action validation, which already compares gesture
identities. The raw internal input's intentional cancel-current semantics remain
unchanged. Independent review confirms that only the validated path executes
cancellation and that stale rejection leaves the successor intact.

## R03: deferred contained resize uses superseded measurements

Deferred contained resize validated the root and durable floating rectangle,
but not the surface bounds and target minimum used by its captured geometry.
A measurement-only publication could change those constraints without changing
the workspace version or durable rectangle. Resizing then used and revalidated
the same obsolete witness, allowing a shrink below the new minimum or outside
the newly measured host bounds.

Two public regressions independently change the target minimum and surface
bounds, require the old action to remain inert, then require a fresh action to
use the new geometry. Both stale-action assertions fail in the same
`a9e1573a-8b77-4a21-be8c-6420910a19d2` run. Two controls already pass: a discarded
first pass may provide the first legal witness, and an unrelated background
pane's minimum must not expire the floating resize.
Commit `8f75298` separates contained close and resize. Its private helper shares
the existing current-requirement and scene-state checks, then compares only the
Ready plan's bounds and target floating minimum for contained resize. Splitter
and junction checks retain their existing measurement comparison. No new state,
public protocol or broad scene fingerprint is added. Independent review confirms
both the geometry dependencies and the non-expiration controls.

## R04: f32 prefix compensation invalidates wide split weights

`SplitWeight::normalize` divided positive inputs by an f64 total and validated
each f32 result, then replaced the last share using an f32 prefix sum. That
extra compensation accumulated rounding error and could produce a sum outside
the core's existing f64 tolerance. It also ran a second scalar-validation loop
over results already validated during construction.

The public 712-way equal split fails layout construction with InvalidTopology.
A valid 713-way split fails `resolve_close(Allow)` when removing its last item
requires normalization of 712 survivors. Both regressions fail in
`84081671-8ceb-47c0-9ce7-29507b57c157`
([before](2026-11-10-core-action-and-layout-review/split-weights-before.log)).
An existing internal 2,000-child move test required normalization failure and
rollback, so it established equivalence between two failing paths rather than
successful movement of valid input. Commit `38433c7` removes the erroneous
prefix compensation and duplicate trailing scalar check and migrates that
expectation to successful equivalent moves.
Positive/finite input, representability and underflow checks and
the global normalization tolerance remain unchanged. Independent review confirms
that per-component f32 rounding, rather than accumulated f32 prefix error, is
well below the existing tolerance for these fixtures, and that the migrated
move regression preserves the complete report, placement and item-conservation
checks.

The first full-core run exposes an old nested-minimum rounding fixture whose
positive-residual prerequisite depended on the removed compensation:
`da428f1a-1927-4eed-b769-191d60ffaff1` passes 2,132 tests and fails that prerequisite
([transcript](2026-11-10-core-action-and-layout-review/core-initial-rounding-fixture.log)).
Its outer shares now produce zero overflow before reverse expansion is tested.
Commit `4cb90d2` pins the fixture's original valid complementary f32 shares,
`[0.9, 1.0_f32 - 0.9_f32]`, which sum to exactly one in f64 and restore the
approximately `5.7e-14` nested minimum residual. All original assertions remain:
positive residual, both axes, reverse expansion, a fixed far edge and unchanged
weights in the constrained direction. The focused test passes in
`c63370c8-adf0-492c-adfa-eaed61610128`
([command and result](2026-11-10-core-action-and-layout-review/rounded-minimum-fixture.json)).

## Focused verification

All 28 public regression and adjacent deferred-action tests pass in
`372b8332-2a3e-4cce-902d-6581389d8f2d`.
The ten core local-move and raw Escape controls pass in
`4cd06389-1a0c-469b-ac7d-94234652faff`. Eight graph validation tests, including
non-finite, non-positive and f32-underflow rejection, pass in
`32e1c321-5ee4-47c9-8a44-45176fce5fa0`. Exact commands and results are retained in
[targeted-gates.json](2026-11-10-core-action-and-layout-review/targeted-gates.json).
The failing original public commands and their expected results are recorded in
[reproductions.json](2026-11-10-core-action-and-layout-review/reproductions.json).

## Review limits and qualification

The transaction lane confirms existing candidate poisoning, owned-update
exclusivity, encoding-pass receipt retirement, configuration ordering and
deferred authority checks in the inspected sequences. Those checks have actual
protocol responsibilities and are retained. No additional persistence or
retention defect has been confirmed in this pass. These conclusions are bounded
reviews of the inspected paths, not a proof that all interleavings are correct.

Independent cross-review reports no remaining blocking P1/P2 in these four
repairs. Existing ImGui timing and physical platform scopes remain unchanged.

## Final verification

Serial gates use `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` and the existing
target directories. Full-core, independent persistence-host and adapter suites
pass at `4cb90d2` after the rounding fixture correction. The public host suite
now contains 352 tests across 63 binaries, including eight new public
regressions from this round. Core and adapter together pass 2,314 tests, with
three existing core skips.

| Gate | Result | Evidence |
| --- | --- | --- |
| Core and shared conformance, all features/targets | 2,133 pass; 3 existing skips | [core-tests.log](2026-11-10-core-action-and-layout-review/core-tests.log) |
| Independent persistence host, all targets | 352 pass | [host-serde.log](2026-11-10-core-action-and-layout-review/host-serde.log) |
| egui adapter, all features/targets | 181 pass | [adapter-tests.log](2026-11-10-core-action-and-layout-review/adapter-tests.log) |
| Official unmodified-egui consumer | 24 pass | [official-consumer.log](2026-11-10-core-action-and-layout-review/official-consumer.log) |
| Product default / persistence | 114 / 119 pass | [default](2026-11-10-core-action-and-layout-review/product-default.log), [persistence](2026-11-10-core-action-and-layout-review/product-serde.log) |
| Pinned native fork, all features/targets | 593 pass; 1 existing skip | [native-tests.log](2026-11-10-core-action-and-layout-review/native-tests.log) |
| Workspace and native correctness/suspicious Clippy | Pass | [serial-gates.json](2026-11-10-core-action-and-layout-review/serial-gates.json) |
| Strict workspace and native Rustdoc | Pass with `RUSTDOCFLAGS=-D warnings` | [serial-gates.json](2026-11-10-core-action-and-layout-review/serial-gates.json) |
| Workspace doctests | 11 pass | [workspace-doctests.log](2026-11-10-core-action-and-layout-review/workspace-doctests.log) |
| Workspace/host/native format and core dependency boundary | Pass | [serial-gates.json](2026-11-10-core-action-and-layout-review/serial-gates.json) |

All sixteen serial gates pass. The recording host's Presented facts establish
public protocol behavior; they do not prove physical renderer presentation.

## Actual Linux X11 qualification

All thirteen build, window-manager and execution gates pass in
[real-x11-gates.json](2026-11-10-core-action-and-layout-review/real-x11-gates.json).
Seven exact eframe cases execute in separate real-window processes, covering
predecessor-output isolation, terminal output hooks, style publication, output
backpressure, native ingress, retained editing and native resize. The bounded
create/first-live/redock/retirement profile passes under a 4,194,304-byte limit,
with recorded resident and reserved peaks of 2,220,400 and 3,340,616 bytes
([native log](2026-11-10-core-action-and-layout-review/native-redock-bounded.log)).
These are that profile's owned storage measurements, not a total process-memory
or performance comparison.

The [complete editor driver](2026-11-10-core-action-and-layout-review/actual-editor-lifecycle.log)
passes docking, lifecycle and Root rebinding at
`2026-11-10T09:02:20.281032+00:00`. It completes twelve real focused edits, six
destroyed child lifetimes, first-live ownership, Dock Back, OS-close recovery,
programmatic close/Allow/reopen, arrangement lock, Default and JSON restore,
live Child replacement and fresh editing after Root rebinding. The original
Root window survives; every child is destroyed and final quiescence holds.

Qualification reuses the existing one-CPU, 4 GiB Linux container with Xvfb/Openbox
at 1920 by 1080 and software Glow. Its existing `/native-target` is reused with
one Cargo build job, incremental compilation and development/test debug info
disabled, and one LLVMpipe thread. This refreshes the finite X11 profiles and
does not extend the mixed-DPI or other physical-platform certification. No source
changes follow the qualified `4cb90d2` revision.
