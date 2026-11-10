# Multiview composition refactor

Baseline: `6a63fb6`. G02/G03/G04 are implemented and independently reviewed at
`1d1abe8`; all sixteen serial gates and thirteen real-X11 gates pass. G01 is
also complete from baseline `78c1eaa`, with core/public-host implementation at
`270d81b`. Its sixteen serial and eighteen real-X11 gates pass, including the
new shared-window journey and late-discard variant. The
[G01 qualification](../audits/2026-11-11-shared-native-window.md) states the
finite physical profile and separates it from protocol-only negative gates.

## Contract

Separate the lifetime and authority of a real native window from the placement
of its logical docking surfaces. Keep a single joined input order and reuse the
existing creation, first-live, close, recovery and retirement owners. Do not
duplicate window tokens or introduce a second docking state machine in an
adapter. Keep layout size independent of the physical origin, scale and clip.

The public acceptance journey is one external Root hosting two independent
surfaces, cross-area dragging, managed Child creation without early source
transfer, dragging back, placement changes and exact Root continuation, then
atomic window-close approval. Stale mappings, blocked hover, discarded
candidates and unavailable recovery must preserve the original ownership.

Programmatic item/group/root tear-off and default placement should reuse the
same payload admission path as pointer interaction. Explicit multi-Root restore
is an independently complete first step toward removing the single-Root assumption.

## Execution lanes

Mode: `execute_direct`; repository: current checkout; base: `6a63fb6`.
Verification and commits: primary agent only, serial Cargo with one build job
and the existing target directory. No push or upstream changes. Never read or
write `docs/reviews/**` or `repo-ref/**`. No lane may spawn more agents.

| Lane | Initial role | Initial writable files | Expected output |
| --- | --- | --- | --- |
| primary | integration and implementation | this plan; implementation assigned after design | coherent model, public acceptance, serial verification and commits |
| arch_core | read-only design | none | minimal window/surface ownership model and close/recovery implications |
| arch_native | read-only design | none | public item/group/default native command design and exact source capture |
| arch_render | read-only design | none | joined input, placement and rendering contract with adapter migration points |

The three design passes agreed that G01 requires window identity, placement,
aggregated close, vacancy and first-live changes together. Its public entry
must wait for that complete replacement; see the
[shared native window design](2026-11-10-shared-native-window-design.md).

Implementation assignments: primary owns batch continuation, persistence and
shared command wiring; arch_core owns the new public multi-Root acceptance file;
arch_native owns source selection, native command preparation and its public
acceptance file; arch_render owns batch native route validation/publication and
its tests. Native producer and rendering files are not shared between writers.
After implementation, lanes exchange read-only reviews.

## Implementation sequence

1. Complete batch external-Root continuation and restore, including complete
   cycles, duplicate/stale rejection, rollback and distinct Child recovery hosts.
2. Add programmatic payload selection and default native placement by reusing
   existing payload capture, source reservation and first-live admission.
3. Migrate the reference adapter and public consumers. Independently review
   identity, rollback, close scope, coordinates and resource cleanup.
4. Run focused regressions, relevant serial full gates and real-window checks
   appropriate to the changed behavior. Update support claims only from fresh
   evidence and commit reviewable slices.
5. Implement G01 as the complete window/placement ownership replacement specified
   by its design, with joined input and unified close/recovery acceptance.

Stop and revise the design if it needs fake window identities, competing input
producers, adapter-owned docking transactions, or weakened first-live and close
authorization. These are design failures, not acceptable shortcuts.

## Completed implementation slice

G02 and G03 share Item, captured Tabs and Root preparation with the existing
native create, source reservation, first-live and compensation owners. Explicit
placement and core-derived default placement are available from session, current
frame and native application commands. Partial payloads use their actual pane
geometry and measured minimum. Captured groups retain exact source authority;
foreign groups are rejected locally by the native application's existing slot.

G04 publishes explicit external-Root continuation batches atomically, including
complete cycles and per-Child recovery hosts. Both document restoration and
non-persistent layout reset use that batch contract. The native route consumer
prepays resources and installs the complete committed batch under its existing
publication lock. The reference application's automatic restore product entry
continues one external Root window's complete placement roster. Several
independent external windows use explicit core batch mappings.

The [implementation and qualification record](../audits/2026-11-10-native-command-root-refactor.md)
tracks the earlier evidence and Clone migration. The separate completed
[G01 phase](2026-11-10-shared-native-window-implementation.md) replaces window
identity, placement, Joined routing, output ownership, window-wide close and
final-placement retirement together. No pending implementation item from this
approved refactor remains; additional physical platforms and performance
profiles retain their documented qualification limits.
