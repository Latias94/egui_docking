# Semantic-state refactor completion audit

Date: 2026-11-03. Status: complete within the documented host and finite reference profiles.

This audit applies the complete
[semantic-state plan](../plans/2026-11-02-dockspace-semantic-state-refactor.md).
It follows the [ImGui comparison](2026-11-02-dockspace-imgui-review.md) against
`d88f238d1d363ef387382b2e29ad12cf96f826ac`. The library remains renderer-neutral;
egui validates its public contracts. Open-GPUI and unenabled physical-platform
claims remain outside this plan.

The implementation commits are `72a75d9`, `ddd77a5`, `3a294c6` and `68bb666`.
The final tranche replaces the incident-only resize solver and closes additional
scroll, stream-reincarnation and numerical interleavings found by independent review.
Its frozen v2 inventory contains 512 source, test, manifest, lockfile, README
and tool paths, excluding the independent CI change and `docs/reviews/`.
These external changes are preserved and excluded from commits.

## Requirements and evidence

| Plan item | Current implementation and maintained oracle |
| --- | --- |
| P0: retain the nine counterexamples | F1: `layout_reset::review_root_policy_survives_closing_and_restoring_defaults`; F2: `presentation_prefix::later_dropped_output_preserves_the_last_actually_presented_output`; F3: the independent popup-owner cases in `public_contract`; F4/F5: the distinct-route and oversized-source cases in `contained_projection`; F6: `native_surface_creation::review_identical_hidden_snapshot_preserves_inflight_pre_show`; F7: `direct_prepared_native::review_external_root_token_cannot_exhaust_core_child_identity_space`; F8: `splitter_intent::review_zero_displacement_splitter_click_preserves_durable_intent`; F9: `edge_scroll_manual_owner::review_unrelated_blocked_scroll_preserves_the_drag_owner_continuation`. All are maintained public consumers, with normal and stale/foreign/rollback controls. |
| P1: coherent presentation settlement | Runtime settlement preserves the last actual Presented while advancing the retirement frontier. `presentation_prefix` covers grouped, separately committed, abandoned and retried results. Popup semantic routing is independent of owner-only visual proof. One `PendingMutation` and shared presentation tracker serve drag release, contained release and root rehome; terminal commands remain variant-specific. |
| P2: semantic identity and lifetime evidence | Defaults and document restore preserve declared Root/Surface/Floating identities and merge allocation history; epoch/revision/binding checks reject old capabilities. Host and managed window tokens occupy distinct namespaces in the existing registry. Stable policy, host-token maximum, duplicate/reused resource and native document/root replacement tests cover the boundary. |
| P3: one manipulation geometry | Complete contained content and terminal preview consume the same manipulation-derived proposal. All eight resize directions, distinct hover/manipulation coordinates and oversized movement have public coverage. The touching solver uses exact retained projections, propagates shared edge displacement and limits it with all affected minima/maxima. `nested_splitter` covers pointer, splitter and junction actions, central choices, N-ary geometry, immediate/deferred handling, exact weight equality, zero intent, atomic rollback and retry. `deep_layout` measures, paints and presents a 10,000-level declaration on the ordinary stack; declaration compile/clone/debug/drop and minimum traversal are iterative. |
| P4: continuous native preparation | Repeated durable facts preserve PreShow/PostShow requests in existing observation streams. Unknown, changed geometry/scale, readiness/resource failure and provider/recovery changes revoke old proofs. Create/Show acknowledgements settle independently of readiness. DirectPrepared/Staged share exact first-live publication. Public `native_surface_creation`, `direct_prepared_native`, work-area placement and private native bringup tests cover continuity, revocation and deterministic intersection/nearest-area selection. |
| P5: self-drawn host seam and copying | Public `logical_surfaces` executes S1 present, empty S2 attach, docking, migration back and empty retirement without native enrollment or epoch replacement. S1 authority survives; reused S2 requires fresh proof. Nonempty/native/recovery/reservation conflicts reject. Frame candidates share one immutable `Arc<Workspace>` while actual commands use the existing staging copy. `frame_cost` records idle and selected-item samples at 16/128/1024 panes; owned-output and core counters supply separate structural bounds. |

The ten invariants map directly to these paths: semantic identity and token domains
to P2; Presented settlement and independent popup owner to P1; shared contained
geometry, zero intent and scoped timed work to P3; readiness continuity to P4;
incremental membership to P5. Failed and abandoned frames in the public host,
native and transaction suites retain topology, producer watermarks, presentation,
focus and lifecycle state until atomic publication.

The five-batch release samples recorded before and after workspace sharing have
the following median microseconds per frame. They are local workload evidence,
not an ImGui benchmark or a platform latency guarantee.

| Panes | Idle before / after | Selected-item before / after |
| --- | --- | --- |
| 16 | 19.25 / 19.21 | 70.68 / 70.82 |
| 128 | 81.17 / 74.29 | 505.21 / 498.87 |
| 1,024 | 611.21 / 506.99 | 4,820.95 / 4,709.79 |

## Independent review and obsolete-path deletion

The rebase module, all-surface popup rendezvous, duplicate popup proof map and
revision issuer, three independent pending mutation Options, incident-only resize
solver and recursive production minimum/declaration traversal are removed. The
remaining allocation frontier, layout projection and variant-specific terminal
commands have distinct responsibilities. No second docking graph, lifecycle
registry, readiness issuer or GUI journey language is added.

Independent reviews identified and then rechecked the following final defects:

- A new sibling scroll owner borrowed an obsolete visual popup revision after
  another surface's popup cycle. Current journal routing now starts the owner;
  a true routing change still terminates an existing owner.
- A removed surface's unsettled stream blocked its same-host headless successor.
  The successor has a new stream and fresh requirements. Old results settle only
  their predecessor; quiescence permits no active owner or a different same-host
  active successor and rechecks this when consumed. Two negative tests prevent
  foreign-host takeover from making an old host reclaim the surface.
- Fractional gap widths, nested min/max residuals and large-origin rectangle
  subtraction require coordinate-aware floating-point tolerance. Genuine overflow,
  unallocated space and compressed gaps remain rejected. Four maintained numerical
  counterexamples fail before this fix and pass afterward.
- Rounded noncentral f32 shares could erase a tiny positive central residual.
  The central weight retains its raw positive share when needed; the central
  allocation still consumes residual space and normalized-sum tolerance is unchanged.
- A complete resize frontier rebuilt one root per split. Validation now shares
  one checked root snapshot within the command call and checks every member and
  exact fingerprint. The 64-split test changes 64 captures to one while rejecting
  foreign and stale sources.

The six public nested-resize tests fail against an immutable `ddd77a5` snapshot
and pass against the new implementation. Corrected fixture attempts are separately
recorded and are not counted as functional counterexamples. Final independent
review of the central-residual and foreign-host fixes finds no remaining confirmed
P1/P2 in that scope. The full-plan coverage audit finds no unmapped acceptance item;
it does not replace the executed qualification below.

## Qualification

The final v2 Linux matrix passes:

| Gate | Result |
| --- | --- |
| Workspace, all targets/features | 2,005 passed |
| Public host, default / serde | 213 / 241 passed |
| Product, serde | 117 passed |
| Official egui consumer | 15 passed |
| Native workspace, all targets/features | 619 passed; 5 explicit platform/measurement cases ignored |
| Native library, assertion-disabled | 540 passed; the same 5 explicit cases ignored |
| Core/native correctness and suspicious Clippy | Passed |
| Core/native strict rustdoc | Passed |
| Workspace doctests | 10 passed |
| Renderer-neutral dependency boundary | Passed |

The ignored cases require their own actual platform/measurement runs; broad
model suites do not count them as passed. Recording-renderer tests prove the
public contract and affine output settlement, not GPU scanout or OS input.

The final matched extracted packages pass: both archives are Cargo-verified;
all 280 core and 36 adapter Rust files and both crate READMEs byte-match source.
Extracted public default/serde consumers pass 213/241, official egui passes 15,
and the engine-host example runs. Framework dependencies remain registry-sourced.

The final X11 ordinary/bounded redock, real recover/retain/cancel close policies,
six pressure modes, thirteen physical target cases, four stationary modes and
real Shift pause/reacquisition pass. Four separately selected native/renderer
tests pass, including predecessor output, full output capacity, actual original
press capture and close/create/age boundaries. The opt-in release wall-time gate
also passes. MacOS default-entrypoint and
assertion-disabled native runs each pass 540 tests, including the 10,000-lifetime
soak, with one explicitly skipped measurement. MacOS real default/bounded redock
and all six output-pressure modes pass. Five workspace formatting checks and
modified-document local links pass.

The 1 kHz load retains its unchanged input, 900 Hz floor, 100 ms release bound
and 4 MiB storage allowance. Its original 100 ms Docker CPU quota period produces
long injector stalls and fails the cadence floor despite receiving all callbacks.
An instrumented run records 14 throttle periods and 346 ms throttled time.
With the same image, binary, total 1 CPU quota and 4 GiB memory, a 10 ms period
reduces the maximum injection gap but still fails. A 1 ms quota period passes
twice: 988.4/988.2 Hz injection, 981.5/989.8 Hz callbacks, all 1,000 original
callbacks, 22.26/22.07 ms release-to-model upper bounds, and 3,534,000 reserved
bytes. Maximum injector gaps fall to 2.35/1.99 ms. No code, threshold, resource
budget or catch-up burst changes for these runs. The slower-quota failures remain
explicitly unqualified; the successful load profile records its 1 ms quota period.

The source and command receipts are local `/tmp` artifacts:
`dockspace-semantic-final-v2-source.json`,
`dockspace-semantic-final-v2-linux-gates.json`,
`dockspace-semantic-final-v2-package-gates.json`,
`dockspace-semantic-final-v2-x11-gates.json`,
`dockspace-semantic-final-v2-x11-isolated-gates.json`,
`dockspace-semantic-final-v2-load-1ms-period.json`,
`dockspace-semantic-final-v2-load-1ms-confirm.json`,
`dockspace-semantic-final-v2-macos-gates.json` and
`dockspace-semantic-final-review.json`.
Maintained test sources and explicit command arrays remain the reproducible
oracles. The final source, matched artifacts, platform receipts, independent
review and precise local implementation commits agree. No root branch is pushed
and no PR is opened.
