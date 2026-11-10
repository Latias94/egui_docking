# Shared native window implementation

Baseline: `205195d`. Status: **complete for the recorded finite Linux X11
profile**. The first ownership slice at `1718683` passed sixteen serial and
thirteen real-X11 gates. The complete G01 phase based on `78c1eaa` now passes
sixteen serial and eighteen real-X11 gates, including ordinary and late-discard
shared-window journeys. Core and public-host changes are committed as `270d81b`;
the native adapter and actual-window consumer are committed as `3136d5e`.
See the [final qualification record](../audits/2026-11-11-shared-native-window.md)
for source provenance, migration and physical support boundaries.

## Lane map

Mode: `execute_direct`. Repository: current checkout and branch.
Verification owner: primary, serial Cargo, one job, no incremental builds and
existing target directory. Local commits only. Never read or write
`docs/reviews/**` or `repo-ref/**`; never revert unrelated changes.
No worker may run Cargo, commit or spawn more agents.

| Lane | Initial role | Writable files | Expected output |
| --- | --- | --- | --- |
| primary | integration and core implementation | this plan; core ownership after coordinated design | exact window/placement model, coherent public journey and serial validation |
| arch_core | read-only core migration analysis | none | registry, lifecycle, first-live and close changes with concrete minimal migration |
| arch_native | read-only Joined input analysis | none | placement proofs, source/hover routing and event handoff migration |
| arch_render | read-only native rendering analysis | none | one-window multi-contribution ownership and egui integration migration |

The approved contract is the [shared native window design](2026-11-10-shared-native-window-design.md).
The lane map records the initial analysis assignment; the completion phase below
records implementation ownership. No public partial composition entry,
synthetic OS-window identity, second input producer or adapter-owned docking
transaction was introduced.

## Acceptance

One external Root hosts independent A and B with distinct origins, scales and
clips. Paint both under one output token. Drag A to B, create a managed Child
without early source transfer, drag it back, change placements and window facts,
replace the workspace, and close the whole window with atomic Allow/Defer/Veto.
Failures, stale evidence and bounded-capacity rejection retain existing ownership.

Protocol evidence and physical platform qualification are recorded separately.

## Historical ownership slice

The first slice removed coupled internal ownership while retaining the existing
one-surface native entry points. At that checkpoint, it was an internal refactor
and did not complete the public G01 journey:

- `WindowBinding` has no surface identity. `ViewportBinding` joins that exact
  window with one logical surface. Existing complete comparison/hash order is
  preserved explicitly rather than changing with struct field order.
- `WindowRecord` owns OS lifecycle and observation streams once. Surface rows own
  admission and projection. A Copy borrowed `ViewportRecord` joins them without
  cloning another physical owner. Restore moves all predecessors before placing
  successors; detach removes the exact window and token index together.
- One internal client mapping supplies origin, uniform scale and clip to native
  local/desktop conversions. Default whole-client behavior and signed-zero
  arithmetic are retained. Hover uses the physical clip independently of source
  capture. That checkpoint exposed no configurable shared-window placement entry.
- One native output token owns a collection through final-pass validation, one
  core publication, receiver staging and renderer terminal. Batch core reporting
  validates every affine output before changing any terminal slot.

The subsequent G01 phase completed window-scoped physical ingress,
configurable exact placement generations, cross-placement receiver selection,
first-live roster admission, whole-window close/recovery and last-placement
vacancy/retirement before qualifying public shared-window composition.

The [qualification record](../audits/2026-11-10-shared-window-ownership.md)
retains the unchanged default-stack bulk-input regression, its iterator-borrow
fix, complete source provenance, independent reviews and fresh physical results.

## Goal-backed completion phase

Completed objective: the entire G01 acceptance journey with its exact negative
gates and native adapter migration. Baseline for this phase: `78c1eaa`.

Mode: `execute_direct`. Verification and commits remain primary-only; workers
never run Cargo or spawn other agents. The existing forbidden paths and no-revert
rules apply. All four lanes implemented against the common contract in this
checkout, then exchanged independent read-only reviews.

| Lane | Next responsibility | Initial writable scope |
| --- | --- | --- |
| primary | window registry, placement generation, candidate publication and integration | this plan; core registry and coordination after shared type agreement |
| arch_core | atomic window close, recovery and first-live roster | close, admission, recovery modules; physical focus and replacement focus |
| arch_native | window-scoped Joined ingress and cross-placement source/hover | pointer journals/receipts, runtime input, native receiver/coordinator/routes |
| arch_render | actual egui multi-surface window composition | multi-surface native driver, transforms/output, new X11 consumer |

Completion requires the complete public journey and negative gates, migrated
native adapter, fresh serial verification, a real X11 shared-window journey,
independent P1/P2 disposition, accurate support/migration documents and local
commits. Intermediate commits do not complete this objective.


### Implemented shared contract

`NativeSurfacePlacement` contains `surface`, independent logical `size`, physical
client `origin`, positive pixels-per-logical-unit `scale_factor`, and physical
client `clip`. Clipping never implicitly resizes layout. Core alone mints each
`PlacementGeneration`. Changing B retains A's exact placement and window lifetime.
`DockspaceHostFrame::set_native_surface_placements(window, &[...])` replaces the
complete external-Root roster within its rollbackable candidate. Session and
candidate queries return exact bindings and placement geometry.

Physical facts and provider sequences are stored once per real window; one
snapshot entry covers all siblings. Surface measurements, presentation outputs,
receivers and recovery anchors remain independent. Window-hit questions select a
presented artifact from the framework's completed hit graph before the existing
surface-qualified receiver protocol. Captured source movement and hover use
separate projections from the same joined edge.

Initial shared composition is for externally owned Root windows. Runtime-owned
Child windows retain their standard whole-client placement and the existing
create/recovery obligations. Public-host tests qualify the full protocol,
including DPI invalidation, atomic Allow/Defer/Veto and exact quiescence. The
real X11 profile qualifies physical A-to-B, A-to-Child-to-B, placement origin and
scale changes, Root reset and WM close, both normally and with discarded passes.
Its native DPI remains one; other platform profiles and a new ImGui performance
comparison are outside this qualification.
