# Window identity and surface output ownership refactor

Date: 2026-11-10 (Asia/Shanghai). Baseline: `205195d`.
Implementation: `1718683`.
Status: ownership-slice implementation, independent review and qualification
complete. Shared-window composition remains unavailable.

## Scope

This is the first ownership implementation slice of the
[shared native window design](../plans/2026-11-10-shared-native-window-design.md).
It removes coupled internal ownership before opening configurable placements.
It does not complete the G01 public acceptance journey or qualify another
physical platform profile.

`WindowBinding` identifies domain, epoch, token and incarnation without a
SurfaceId. `ViewportBinding` joins a window with a surface. Its complete ordering
and hash field order remain domain, epoch, surface, token and incarnation, so
representation changes do not reorder existing protocol work. The opaque public
`NativeWindowBinding` includes provider authority; lifetime correlation ignores
a provider handoff while ordinary equality remains exact.

The registry stores window-owned role, ownership, OS lifecycle and observation
streams once. Surface rows retain admission and coordinate projection. A Copy
borrowed `ViewportRecord` joins these owners. Detachment, token indexes, provider
revocation, generation exhaustion and Root cycles retain their existing exact
candidate and cleanup semantics. Duplicate window tokens remain rejected.

One internal origin/scale/clip mapping now supplies native client and desktop
conversions. Existing registration initializes the whole-client mapping. Capture
retains the original physical client point, exact binding and coordinate
generation, and validates event/callback geometry. Captured source motion may
leave the clip; hover independently rejects physical points outside it. Default
whole-client conversion retains signed-zero endpoint arithmetic. Native placement
continues to use its existing native-scale and destination work-area semantics.

One renderer terminal can report an affine output collection through
`report_surface_presentations`. Under one pending-store mutex, the complete
collection must retain ownership, exact unretired slots and unique artifacts
before any terminal changes. Failure returns the original Vec and every output;
success writes the common terminal and disarms every handle before releasing
ownership. The single-output entry reuses this slice path without allocating
a Vec. Core presentation admission still follows each exact stream prefix.

The native token, final-pass paint, receiver roster and terminal paths now use
owned collections under their existing publication and budget owners. The former
single-output binder and its boxed error wrapper were removed; its sole fixture
uses the same collection path as production.
No second input producer, synthetic window token or adapter docking transaction
is introduced.

## Evidence and independent review

[Fresh logs and receipts](2026-11-10-shared-window-ownership/) retain actual
commands, timestamps, source digests and intermediate failures. Cargo uses one
build job, no incremental builds, one nextest worker and the existing targets.

The core focused run passes 80 cases. Both full core/conformance runs pass
2,152 cases with three existing skips. Independent reviews cover registry
migration, client mapping/source-hover separation, batch terminal reporting and
the complete frozen native output chain. The final reviewer verifies all 29
source/consumer hashes against the manifest and finds no blocking P1/P2 in this
internal slice. All sixteen final serial gates pass:

| Gate | Fresh result |
| --- | --- |
| Core/conformance, all features and targets | 2,152 passed; three existing skips |
| Independent public persistence host, all targets | 388 passed |
| egui adapter, all features and targets | 186 passed |
| Official egui consumer | 24 passed |
| egui product, default / persistence | 114 / 119 passed |
| Native workspace, all features and targets | 607 passed; one existing skip |
| Workspace/native correctness and suspicious Clippy | Passed |
| Strict workspace/native documentation | Passed |
| Workspace documentation examples | Eleven passed |
| Workspace/host/native format and core dependency boundary | Passed |

The deprecated ID constructor in a new proof-test fixture was migrated to the
repository's existing `Id::unique` convention after its full adapter gate. This
changes only the test and receives a final narrow rerun. Two existing native
menu fixtures were also migrated to shared Arc hit authority before the final
native gate; no production change was needed for those fixture corrections.
The final five proof tests pass after the test-only ID migration, without the
deprecation warning. All thirteen real Linux X11 gates also pass:

- Seven isolated eframe processes cover preparation isolation, final-pass
  publication, style/geometry, bounded output retention, native ingress and
  focused text before and after resize. The native-ingress process also checks
  token roster discard/retry and duplicate/out-of-order refusal.
- The 4 MiB bounded redock scenario completes create, first live, redock and
  retirement. Peak resident storage is 2,086,216 bytes; peak reserved storage is
  3,206,432 bytes, below its 4,194,304-byte selected limit. This is a capacity
  qualification, not a measured performance comparison.
- Both actual-editor drivers pass: twelve real focused edits, six Child
  lifetimes, close/Allow/reopen, lock rejection, default reset, document restore,
  live Child replacement and cross-surface Root continuation preserve state.
  The original Root window survives, all Children are destroyed and the final
  runtime is quiescent.

`x11-environment.json` records the reused one-CPU/4-GiB Xvfb/Openbox/Glow
container. Its one 1920x1080 X screen has one active identity-transform CRTC;
native scale remains one. These finite physical checks exercise the existing
one-surface-per-window native entry points. They do not qualify configurable
shared-window placement, physical multi-Root swaps, mixed-DPI interaction or
additional platforms.

`serial-gates.json`, `real-x11-gates.json` and the final proof receipt identify
successful results; files labelled `before`, `initial` or `reproduction` retain
diagnosis rather than superseding those results. Log trailing whitespace and
redundant final blank lines are normalized for repository formatting. All thirty
final source/consumer hashes match implementation commit `1718683`.

An initial registry test omitted visible presentation and a new public clip test
assumed a fixed tab width. Their fixtures now supply explicit visibility and
calculate the client edge from actual presented tab positions. Existing production
checks were not weakened to accommodate those fixture failures.

### Default-stack array ingestion regression

The first independent-host full run passes 387 of 388 tests; the existing
`current_close_decisions_wait_for_the_complete_native_prefix` aborts from stack
overflow. Its unchanged input is a 256-element array passed through the public
native sequence interface. A narrow default-stack rerun deterministically
reproduces the abort. Cached pre-change/current artifacts measure
`NativePointerInput` at 728/776 bytes and `NativeLocalCoordinates` at 232/280 bytes.
The added mapping exposes repeated by-value array/iterator storage across the
preparation call chain.

Preparation now borrows the caller iterator at its internal seam. The owning
iterator stays in a small scope and finishes destruction before any maintenance,
reclaim or append. This removes the extra large argument/iterator stack frame
without allocating another buffer, using dynamic dispatch, changing the public
input, or enlarging the test stack. All twelve unchanged close controls pass
under the default stack. Independent review confirms the existing constructor,
Next/Drop panic, refusal boundary, peak allowance and append rollback contracts;
the second full serial run covers these controls again.

## Migration and remaining G01 work

The low-level `ViewportRecord` query is a borrowed view. Clone its registry when
an owned historical snapshot is required; cloning the view retains references.
Stable session entry points continue to support the existing singleton native
profile. `NativeSurfaceBinding::window()` supplies opaque window correlation,
not presentation, coordinate, input or close authority for another surface.

Before opening shared-window composition, implement exact configurable placement
generations, window-scoped native ingress, cross-placement source/hover receiver
selection, required first-live rosters, whole-window close/recovery and final-
placement vacancy/retirement. These use the existing lifecycle owners and must
ship together. No new ImGui timing or mixed-DPI/platform claim follows from this
internal ownership refactor.
