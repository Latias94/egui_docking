# Shared native window implementation

Baseline: `205195d`. Status: the first ownership slice is implemented at
`1718683`, independently reviewed and qualified by all sixteen serial and
thirteen real-X11 gates. Shared-window support remains unavailable until the
complete ownership and acceptance journey passes.

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
Worker ownership will be assigned after the shared types and migration sequence
are settled. No public partial composition entry, synthetic OS-window identity,
second input producer or adapter-owned docking transaction is acceptable.

## Acceptance

One external Root hosts independent A and B with distinct origins, scales and
clips. Paint both under one output token. Drag A to B, create a managed Child
without early source transfer, drag it back, change placements and window facts,
replace the workspace, and close the whole window with atomic Allow/Defer/Veto.
Failures, stale evidence and bounded-capacity rejection retain existing ownership.

Protocol evidence and physical platform qualification are recorded separately.

## Current ownership slice

Implementation first removes coupled internal ownership while retaining the
existing one-surface native entry points. This is a complete internal refactor,
not completion of the public G01 journey:

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
  capture. No configurable shared-window placement entry is exposed yet.
- One native output token owns a collection through final-pass validation, one
  core publication, receiver staging and renderer terminal. Batch core reporting
  validates every affine output before changing any terminal slot.

Remaining G01 implementation still includes window-scoped physical ingress,
configurable exact placement generations, cross-placement receiver selection,
first-live roster admission, whole-window close/recovery and last-placement
vacancy/retirement. These must precede public shared-window composition.

The [qualification record](../audits/2026-11-10-shared-window-ownership.md)
retains the unchanged default-stack bulk-input regression, its iterator-borrow
fix, complete source provenance, independent reviews and fresh physical results.
