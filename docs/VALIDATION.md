# Dockspace behavior-spine validation

The canonical plan was revised on 2026-10-27 to prioritize `dockspace` as a
custom-engine library and egui as its interactive validation adapter. The
external public host consumer now lives in `integration/dockspace-host-harness/`.
Its recording-renderer results prove API and output-contract portability,
not real GPU or OS presentation. Open-GPUI integration remains deferred.

This document is the U1 validation contract. It records semantic journeys and
current evidence without turning the shared conformance crate into a GUI DSL.
Executors resolve their own current semantic bounds and platform facts. The
shared Rust catalog carries the finite L03 source, basin, normalized inset
samples, cadences, and normalized outcomes alongside the existing L08 semantic
steps. It does not carry screen coordinates, guide selectors, frame counts, or
another product authority.

## 2026-10-27 U3a host input domain and terminal convergence

The public host now owns one affine pointer producer across logical surfaces.
`enable_host_pointer` requires no native enrollment or presented surface;
`submit_host_pointer_batch` assigns one global sequence to all ordered edges.
Every edge independently qualifies manipulation/source and hover coordinates
with exact presented capabilities. Capture belongs to the host domain. Unknown
logical routes do not grant desktop outside-all authority. Existing ledger,
receiver, rollback, producer-drain and retirement mechanisms remain the owners
of transport integrity.

The downstream public consumer submits a complete press/move/release batch from
A to B, where both surfaces have identical bounds. Manipulation stays in A while
hover selects a different point in B. Actual owned recording outputs carry the
pending preview, and Presented or Dropped settlement respectively commits or
cancels the frozen release. No follow-up pointer events are synthesized. A
selection-changing press uses a separate measurement/paint cycle after the
complete input batch; pending release survives this real rendering lifecycle.

The seven new public journeys cover actual target-output settlement,
selection-changing activation, source/hover coordinate isolation, unknown
hover, dropped-frame retry and repeated release, stale route/current receiver
mixing, and foreign-session capabilities. Additional core tests cover local
owner cancellation without panic, rear-contained Raise/resize/release,
HostLocal producer lifecycle and ledger boundaries, and locked scroll tails
whose event-time route or receiver becomes unknown.

Tab and contained release now share their actual terminal implementations
between journal and local-response frontends. Exact-owner cancellation also
uses one implementation; local session/source/target checks remain at the
frontends. A terminal splitter release with unavailable coordinates cancels
its owner instead of leaving a gesture waiting for an already-consumed release.
Legacy internal preview ACK reducers and the response-based input frontends
remain explicitly scheduled for U3b retirement.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,585 passed | Core, conformance and official adapter; includes source-continuation, locked-scroll and exact hover-emission regressions |
| Public engine host, default and serde | 17 passed each | Seven new public pointer journeys; normal dependencies contain no egui, Winit or WGPU |
| Product harness, all features | 90 passed, 1 ignored | Original single-call Burst exclusion remains open |
| Official harness, all features | 12 passed | Includes caller-owned WGPU demo compilation |
| Native workspace, all targets | 198 passed | Existing egui 0.37 fork pin; physical scenarios not executed |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Workspace API documentation and compile-fail boundaries |
| Correctness/suspicious Clippy and formatting | Passed | Shared test support is compiled once; builds use one Cargo worker and Nextest thread |

Independent review and the public tests closed four input boundaries: target
hover must use its own coordinate route; logical cross-surface movement does
not require native-window drag capability; Select/Raise may retain only the
exact active source's existing continuation; and that continuation expires
when another host takes the surface's presentation stream. Hover and new
delivery remain strict current-output claims. Another pointer cannot borrow
the source continuation. Stream checks use current active ownership rather
than historical host lookup.

Two mutation checks were executed. Restoring source coordinates in the hover
comparison fails the complete cross-surface release test. Removing source
continuation fails the unselected-tab release test. Production code was restored
before the final gates. These recording and unit tests do not establish OS
input delivery or physical GPU presentation.

A follow-up proof review added exact route-authority validation before dispatching
any known HostLocal hover disposition. `NoReceiver` and `Blocked` now receive the
same exact emission binding as a `Dock` result, even though neither goes through
Dock-region geometry validation. The regression produces two actual presentations
with identical semantic output and different emissions: generic semantic receipt
validation accepts their unchanged interaction geometry, while the HostLocal
route check rejects the mixed emission. Matching route/receipt pairs remain valid.
Removing this check makes the regression fail. The workspace, both public-host
feature gates, correctness/suspicious Clippy, strict rustdoc and formatting were
rerun after restoration. Preview position remains derived from the validated hover
position; delivery and threshold continue using the independent manipulation route.

U3 remains open. The egui adapter still reconstructs pointer phases from its
frame-level `Response`, and its original single-call Burst matrix is still
ignored. U3b must consume ordered raw edges through the same host owner with
event-bound receiver facts, remove duplicate pointer actions/local owners and
internal ACK paths, then qualify the unchanged Burst matrix in U3c. The plan
records this split; neither the full U3 nor the full U13/AE12-AE14 matrix is
declared complete by this slice.

## 2026-10-27 U2 owned-output settlement

The official adapter now completes the same exact output contract used by engine
and native hosts. `run_ui` returns owned output plus an affine host receipt;
returning from egui does not acknowledge presentation. The actual WGPU demo owns
acquisition, submission and `queue.present`, and reports the result after present.
Receipt drop retires its exact outputs. Discarded predecessors remain owned until
the terminal receipt is disposed, so an intermediate pass cannot revoke its own
replacement while egui is still rendering it.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,571 passed | Complete core, conformance and official-adapter regression suite |
| Product harness, all features | 90 passed, 1 ignored | Existing single-call Burst exclusion remains a U3 blocker |
| Official harness, all features | 12 passed | Includes WGPU demo compilation and delayed pane-focus observation |
| Native workspace, all targets | 198 passed | Existing exact egui 0.37 fork pin; smoke binaries compile, physical scenarios are not run |
| Public engine host, default and serde | 10 passed each | Downstream public API only; no egui dependency |
| Strict workspace rustdoc and doc tests | Passed; 10 doc tests | Includes compile-fail protection for both removed public preview ACK methods |
| Correctness/suspicious Clippy and formatting | Passed | Cargo builds and Nextest execution remain serialized |

Complete surface spans include ordinary geometry, panes, previews, clip data,
post-tessellation transforms and exact pixel scale. Pane callback checkpoints
reject overwriting shapes already emitted by the current program. Other-layer
snapshots retain layer identity, transform and content. Nonempty unenumerated
layers are explicitly unavailable. The marker scrubber removes only private
markers and preserves user compounds and callbacks. This is a conservative
synchronous single-layer admission contract, not inspection of application paint
callback internals or physical GPU presentation.

Independent review found and closed four boundary gaps: pane callbacks deleting
already emitted chrome, identical shapes migrating between layers, delayed focus
observations being lost before core accepts their artifact, and child receipts
waking the root viewport. Waiting focus observations remain paired with their
artifact until a committed `presents` or `retires` result; context/viewport lane
changes retire that adapter cache. A failed observation frame leaves the waiting
observations intact. Affine focus observations are consumed only after the output
has been accepted; failure of that separate submission requires a fresh pane
observation rather than duplicating its capability.

Two core corrections are also covered directly. Successful exact preview
promotion preserves its still-current target and source gap, while Dropped
successors still revoke it. Identical complete measurement and coordinate facts
retain the immutable Ready candidate; changed facts still replace it, and every
concrete paint keeps a distinct emission key. Replacement fixtures now change
geometry explicitly instead of relying on identical remeasurement to force a
new scene stamp.

Three mutation checks were executed: removing the positive-promotion guard,
removing the pane append check, and dropping waiting focus observations each
made its corresponding behavioral regression fail. Restoring the production
code passes the final gates above. The redundant renderer-pass bridge and both
public preview-only ACK methods are deleted. U3's manipulation-owner and ordered
logical-surface input work remains separate; no native multiview or real-window
interaction completion is inferred from these headless gates.

## 2026-10-27 U14 and dependency migration baseline

The implementation preserves the audit below as historical failure evidence.
The following results were freshly executed after the U14 fixes, public host
contract work, and official egui 0.37.0 migration. Cargo builds were serialized
with `CARGO_BUILD_JOBS=1`, and Nextest used `--test-threads=1`.

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace, all features and targets | 1,566 passed | Official egui 0.37.0; includes three new failed-output/focus regressions |
| Independent public host, default features | 10 passed | Owned geometry/content/identity records, exact present/drop and commit authority, out-of-order results, two logical surfaces and layout commands |
| Independent public host, `serde` | 10 passed | Includes dual-stream receiver isolation, retired and stale output rejection, automatic output drop, popup barrier retention, and failed core commit/retry |
| Product harness, all features | 90 passed, 1 ignored | Official egui/kittest 0.37.0; existing Burst exclusion retained |
| Official consumer, all features | 9 passed | Includes compilation of the caller-owned WGPU demo |
| Native workspace, egui 0.37 fork | 180 passed | Remote exact pin `0716beee5b7284389f2e80ffb6b24b5af6ae2910`, three test binaries |
| Complete native workspace, including smoke unit targets | 198 passed | Five test binaries; this compiles the smoke executables but does not run a real-window scenario |
| Native library, debug assertions disabled | 173 passed | Same remote 0.37 pin |
| Fork egui and egui-wgpu libraries | 112 passed | Includes five new real elastic-scroll consumption/snapshot regressions and retained-overlay test |
| Fork eframe library, Glow + WGPU + native seam | 53 passed | Host finalizer, output identity, create, retention and renderer seams |

The native fork branch `dockspace-native-seam-0.37` is committed and pushed.
Manifests, lockfile, CI checkout, README, and seam admission agree on the exact
revision. Winit remains at `f378b50f29cafafec31cd8675c17646b4ef57d70`.
The 0.37 scroll query preserves source-dependent elastic consumption as
`Unknown` rather than publishing known-empty. Offscreen screenshots preserve
their callback path without falsely reporting surface presentation.

The public core acknowledgement now cross-checks the actual committed scene's
accepted exact emission instead of echoing a host `Presented` result. Retired
surfaces and invalidated scenes settle without promotion. Valid proofs waiting
behind a multi-surface popup barrier remain retainable until the sibling surface
presents; their receiver bindings stay unavailable until the barrier completes.

Workspace correctness/suspicious Clippy gates, strict core/adapter rustdoc,
eight documentation compile-fail tests, and formatting across the root and
independent workspaces pass. Real-window and physical X11 scenarios were not
rerun on this macOS host; platform claims require their separate CI evidence.

U14 removes required mutations from six `debug_assert!` expressions and promotes
native receivers immediately after successful core commit. The old shared
acknowledgement queue is deleted; idle hosts no longer depend on follow-up input
to consume a committed presentation. A regression test failed before this fix
and passes afterward:
`receiver::tests::committed_host_frame_promotes_receivers_without_follow_up_input`.
Before upgrading the native fork, the repaired 0.36 baseline passed all 180
native workspace tests and all 173 native library tests with debug assertions
disabled. CI now carries the assertion-disabled gate.

The final review also required an exact-emission regression inside
`receiver::tests::exact_hit_without_hover_lane_is_unclaimed_while_uncertain_hits_fail_closed`.
The test now emits the same semantic scene twice in one session with identical
binding and coordinates, presents the newer output, and verifies that its native
query accepts only the newer artifact. Temporarily removing the emission-key
comparison made that test fail at the old-artifact rejection; restoring the
comparison passes. This distinguishes exact output identity from semantic scene
equality.

One non-blocking coverage recommendation remains: exercise managed-host renderer
termination through `prepare_output_prefix`, abandon the subsequent host frame,
then retry and verify exact promotion and one boundary release. Current core
failed-commit/retry, idle ACK, and atomic receiver tests cover its component
contracts, but are not that complete coordinator path. No production defect was
confirmed in this path by the review.

The public recording consumer does not register OS windows or a pointer
provider. Its same-coordinate surface tests use prepared tab actions; they
do not prove ordered multi-surface pointer routing or the complete 60-case
gesture matrix. The actual public UI event pipeline remains U3 work. Complete
program acceptance is explicitly documented as separate from GPU submission
and final presentation; core accepts the exact reported prefix in a later
successful host-frame commit.

### Remaining U3 replacement boundary

The official adapter now preserves complete execution receipts and uses the same
exact `PaintedSurfaceOutput` present/drop path as public engine and native hosts.
Both public preview-only ACK methods and the redundant renderer-pass bridge are
deleted. Caller-owned egui runs return `DockspaceOutput`; the actual renderer
owner takes the complete output and reports its terminal result exactly once.
No-report, receipt drop, output mismatch, discarded predecessor, foreign lane,
unwind, delayed report and pane-focus eligibility are covered by real emissions.

The official integrity check brackets the complete surface span and compares all
ordered shapes, clip rectangles, post-tessellation transforms and pixel scale
after output hooks. Its explicit admission limit is synchronous, append-only pane
painting on the supplied layer. Cross-layer writes are unavailable rather than
silently omitted. Application callbacks must honor source-gap opacity and clip;
the adapter does not inspect their renderer internals. Once output ownership is
split, the host must preserve the complete output it reports. Shape membership
never proves GPU submission or physical presentation.

U3 still replaces the independent local-response and journal terminal reducers,
supports ordered logical-surface routing, and removes the known-invalid Burst
exclusion. U2 output integration does not claim those later gates.

## 2026-10-27 audit baseline (before repairs)

The audit checked `3216855` plus the existing 26 uncommitted U2b files. These
results describe that worktree, not a clean release or the planned egui upgrade.
All Cargo builds used `CARGO_BUILD_JOBS=1`; Nextest used `--test-threads=1`.

| Gate | Fresh result | Limit |
| --- | --- | --- |
| Workspace, all features and targets | 1,563 passed | Does not include the excluded native workspace |
| Product harness, all features | 90 passed, 1 ignored | Full Burst matrix remains excluded |
| Ignored L03 complete matrix, explicitly selected | Failed | First Center/Burst sample `(280, 280)` has no magnetic preview after eight causal observations |
| Official consumer harness | 9 passed | No real-window run was performed in this audit |
| Native workspace, default test profile | 179 passed | Debug assertions enabled |
| Native library, debug assertions disabled | 164 passed, 8 failed | 172 library tests; does not include the other native test binaries |
| Workspace and native formatting checks | Failed | Existing U2b formatting differences remain |
| `git diff --check` | Passed | Patch whitespace only |

The native assertion-disabled run used:

```text
CARGO_BUILD_JOBS=1 cargo nextest run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --lib --locked --test-threads=1 --no-fail-fast --config 'profile.test.package.egui_dockspace_native.debug-assertions=false'
```

Required mutations in `NativeCoordinator::prepare_output_prefix`,
`HostRecords::reserve_output`, and `HostRecords::settle_viewport_roster` occur
inside `debug_assert!` arguments and disappear when assertions are disabled.
The failures cover six ingress/create/input lifecycle cases and two terminal
roster settlement/rearm cases. U14 owns this correction and its configuration gate.

The current U2b receiver path already waits for a successful core presentation
commit before promoting an artifact. That is stronger than the older immediate
physical-callback promotion. Remaining work includes idle ACK draining,
consistent commit exits, and removal of local preview-only ACK authority;
passing default tests do not establish those deletion gates.

No physical OS-input, multi-monitor, GPU submission, or current-release package
validation was rerun in this audit. The selected egui 0.37.0 migration and the
new public engine-host harness remain implementation work under U15 and U13.

## Journey directory

| Journey | Semantic source / basin | Preview | Outcome / ownership | Focus / quiescence | Cadences | Executors and current status |
| --- | --- | --- | --- | --- | --- | --- |
| L01 | Tab body; below-threshold select or threshold-crossing drag | None before selection | Select without drag; source remains owner | Selected tab; local quiescence | Burst, Stepwise | `core-host` and `egui-product-headless` characterize selection; exact human threshold remains product evidence |
| L02 | Tab body; semantic tab gap before/after a sibling | Visible insertion candidate | Reorder once; local target owns the tab | Moved tab; local quiescence | Burst, Stepwise, ReleaseWithFinalMove, DiscardBeforeRelease | Both existing shared product/core paths cover the local command spine; product drag evidence is normative, pass-count details are not |
| L03 | Tab body; pane center plus north/south/east/west basins | Magnetic candidate, independent of guide targeting | Center merge or edge split; target owns payload | Moved tab; local quiescence | Burst, Stepwise, Hesitant, ReleaseWithFinalMove | `core-host` passes all 60 finite cases; `egui-product-headless` passes the 45 Stepwise/Hesitant/ReleaseWithFinalMove cases. A crate-private renderer receipt proves each product point remains outside every registered guide receiver. A single-call product Burst remains an explicit blocker because no Dock preview is published before the causal budget expires. |
| L04 | Contained title/edge; surface free space and eligible pane basin | Full contained-window projection | Move/resize/re-dock commits once; contained or target owner | Moved item; local quiescence | Stepwise, Hesitant, ReleaseWithFinalMove | Durable placement and ownership are covered; full-window projection remains an intentional known-invalid baseline |
| L05 | Tab body; current basin can become stale or disappear | Candidate is cleared while source stays recoverable | Cancel/reject and restore source without partial mutation | Preserve source focus; local quiescence | Stepwise, TargetDisappears | `core-host` stale-target and target-destruction fixtures provide the current normative evidence |
| L06 | Tab body; center/edge basin with release barrier | Exact candidate must be presented before release commits | Commit presented candidate or restore source | Moved item or preserved source; local quiescence | Burst, Hesitant, ReleaseWithFinalMove, DiscardBeforeRelease | Discarded-preview rejection is normative; release-wait and ordinary fixed-hover tests remain characterization or known-invalid until causal readiness replaces fixed frames |
| L07 | Group handle, contained title, menu, and tab close; no overlapping owner | Group/command preview only where applicable | One semantic owner executes each chrome action | Close policy chooses successor; local quiescence | Burst, Stepwise | Close-region and group-drag tests characterize the current ownership boundary |
| L08 | Splitter, tab, close, keyboard, AccessKit, and touch sources; semantic command target | Splitter proposal where applicable | Shared prepared command commits layout action | No focus guessed from dwell; local quiescence | Stepwise, TouchLongPress | Existing shared local scenario runs through `core-host` and `egui-product-headless`; touch long-press has a typed capability gap |
| N01 | Contained/native title; outside-all release | Native pending while source remains recoverable | Create → first-live → redock → retirement | No inferred pane focus; native lifecycle quiescence | Stepwise | Core-host managed lifecycle passes; official/product hosts remain capability-negative without native facts |
| P01 | Tab body in one window; center/edge basin in another window | Completed-pass hover candidate | Physical redock under one qualified handoff | Moved item; native lifecycle quiescence | Burst, Hesitant, ReleaseWithFinalMove | X11 physical smoke is characterization evidence; other hosts fail closed until their physical gates pass |

The current shared local journeys are the finite L03
`MagneticDockingBasins` matrix and the existing L08
`LocalTabCloseSplitterAndContained` scenario. L03 expands one source, five
basins, three normalized non-guide samples per basin, and four cadences without
becoming a general GUI language. Both executors emit normalized
`ItemDocked`/`Quiescent` observations; product readiness uses a causal predicate
with a diagnostic upper bound.

- `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`
- `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`
- `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics` (ignored known-invalid Burst gate)
- `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics`

N01 uses the existing lifecycle scenario and remains a separate capability
boundary. P01 is never inferred from a local or official-egui run.

## U2a ordered SurfaceProgram evidence

The ordered traversal slice is verified independently from the later atomic
presented-artifact and host-terminal-settlement work. Core, product-headless,
and fork-backed native tests now consume `SurfacePaintPlan::visit_surface_program`
for root operations, popups, previews, guides, source-gap disposition, and
feedback. Surface-wide flat plan iterators are crate-private or test-only, and
the deleted synthetic walker cannot recreate a missing operation from parallel
records. This evidence proves traversal membership and renderer order; it does
not claim `RendererPassBridge` promotion, final host settlement, or unwind
abandonment.

Current gates:

- `cargo nextest run -p dockspace --lib --locked -j1 --no-fail-fast`:
  1430/1430 passed on 2026-08-31.
- `cargo nextest run --manifest-path crates/dockspace_host_conformance/Cargo.toml --locked -j1`:
  37/37 passed on 2026-08-31.
- `cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --locked -j1`:
  86/86 passed with one explicitly skipped test on 2026-08-31.
- `cargo nextest run --manifest-path crates/egui_dockspace_native/Cargo.toml --locked -j1`:
  178/178 passed on 2026-08-31.
- `cargo check -p egui_dockspace --all-targets --locked -j1`: passed.
- `cargo check --manifest-path integration/egui-fork-workspace/Cargo.toml --workspace --all-targets --locked -j1`:
  passed.

The remaining atomic artifact, durable discarded-pass action queue, official
host finalizer, and native unwind/retirement paths stay deferred to the next
settlement slice.

## Oracle matrix

Each row is classified as `normative`, `characterization-only`, or
`known-invalid`. Only normative rows may gate U1 completion. A source is listed
only when the named test or function exists in the repository; an intentional
gap is written as such instead of being given a synthetic test name.

| Row | Journey / concern | Classification | Existing evidence or explicit gap | Decision |
| --- | --- | --- | --- | --- |
| U1-01 | L01 action: click selects a tab | normative | `integration/egui-product-harness/tests/product.rs::default_features_click_selects_a_tab` | Preserve selection without drag |
| U1-02 | L01 retained frame: drag decoration after threshold | characterization-only | `integration/egui-product-harness/tests/product.rs::active_tab_drag_leaves_an_accessible_gap_and_paints_one_pointer_ghost` | Keep as visual baseline; fixed deltas do not gate behavior |
| U1-03 | L02 action: reorder commits once | normative | `integration/egui-product-harness/tests/product.rs::default_features_tab_drag_reorders_across_multipass_discard` | Preserve item order and ownership |
| U1-04 | L03 presentation: outer guide targeting | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Visual guide coverage only; exact guide targeting is not docking acceptance evidence |
| U1-05 | L03 presentation: center guide hit | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_center_guide_merges_tabs` | Do not promote guide aiming to the product contract |
| U1-06 | L04 action: durable contained move/resize/redock | normative | `integration/egui-product-harness/tests/product.rs::contained_move_resize_select_focus_and_dock_back_preserve_exact_ownership` | Durable placement changes only at terminal commit |
| U1-07 | L04 presentation: full-window contained drag preview shell | characterization-only | `integration/egui-product-harness/tests/product.rs::contained_title_drag_paints_a_full_window_preview_in_the_same_run` | The window-sized shell is present; live pane-content projection remains a later U4 gate |
| U1-08 | L05 action: stale target has no partial mutation | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_rejects_a_stale_target_without_partial_commit` | Keep fail-closed stale rejection |
| U1-09 | L05 presentation: target loss preserves source drag | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_target_destruction_preserves_the_source_drag` | Clear only the lost target route |
| U1-10 | L06 retained frame: discarded preview cannot release | normative | `integration/egui-product-harness/tests/product.rs::default_features_discarded_preview_cannot_release_a_drag` | Release needs current presented evidence |
| U1-11 | L06 presentation: release waits for exact preview | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_release_waits_for_the_new_preview_to_be_painted` | Retain the regression baseline, but its exact coordinates and fixed preparation/settlement frames are not causal preview-readiness proof |
| U1-12 | L06 retained frame: ordinary readiness via fixed hover loop | known-invalid | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Replace fixed ordinary waits with causal predicates |
| U1-13 | L07 action: close receiver stays disjoint from tab receiver | normative | `integration/egui-product-harness/tests/product.rs::tab_close_semantics_follow_the_exact_close_region_occlusion` | One close owner, no action stealing |
| U1-14 | L08 action: shared local observations are executor-parallel | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`; `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` | Keep the shared scenario narrow and normalized |
| U1-15 | L08 action: touch long-press | known-invalid | No existing executor test; `ScenarioCapability::TouchLongPress` remains unsupported | Preserve typed capability-negative evidence |
| U1-16 | N01 first-live ordering | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Record current causal ordering; do not infer an ownership verdict here |
| U1-17 | N01 retirement | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Preserve as lifecycle evidence until runtime replacement requalifies it |
| U1-18 | N01 quiescence | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Measure bounded cleanup, not callback count |
| U1-19 | P01 physical drag barriers | characterization-only | `integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs::perform_drag_out`; `::move_to_center_guide_while_pressed`; `::native_drag_preview_barrier` | X11 is evidence to requalify, not a portable authority |
| U1-20 | P01 missing native facts | known-invalid | `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` (admission assertion only) | No executor performs the physical gesture, reports the exact missing native facts, and proves source ownership unchanged; keep the gap explicit |
| U1-21 | L03 ordinary magnetic basin matrix | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`; `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`; `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics` | Core-host proves 5 basins × 3 normalized non-guide samples × 4 cadences; the crate-private product receipt proves the 45 supported product cases remain outside every renderer-registered guide receiver; the black-box product executor proves preview, true same-call ReleaseWithFinalMove, normalized docking, and stable quiescence |
| U1-22 | L03 single-call product Burst | known-invalid | `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics` (ignored; run explicitly) | The product executor reaches the first Burst case without a magnetic Dock preview after eight causal observations; retain the failing gate for the later manipulation-kernel replacement rather than substituting guide or fixed-frame success |

The N01 rows deliberately do **not** classify “native ownership before
first-live” as known-invalid. The existing fixture demonstrates a sequence,
but this U1 baseline does not promote that observation to an ownership verdict.
That comparison belongs to the later lifecycle replacement unit.

## Capability boundaries

| Executor | L01–L08 | N01 | P01 |
| --- | --- | --- | --- |
| `core-host` | L03 full 60-case matrix and L08 shared command scenario pass | Programmatic lifecycle scenario passes | Unsupported without `NativePhysicalAuthority` |
| `egui-product-headless` | L03 Stepwise/Hesitant/ReleaseWithFinalMove 45-case matrix and L08 shared command scenario pass; L03 Burst is known-invalid | Capability-negative | Capability-negative |
| `egui-official-harness` | Public local facade; the caller-owned demo supports exact terminal-output settlement, while unmodified eframe examples remain capability-negative | Capability-negative | Capability-negative |
| `native-coordinator-trace` / `egui-native-smoke` | Adapter-specific evidence | Programmatic lifecycle evidence | Physical X11 evidence only |

Capability-negative is a successful result when the exact missing fact and
unchanged source ownership are reported. No executor may substitute a rectangle
scan, final hover, fixed guide center, fixed ordinary frame count, receipt, or
internal scene/graph identity for a missing fact.

## Verification notes

- Shared scenarios contain finite semantic sources, basins, normalized inset
  samples, cadences, selectors, and expected observations only. Coordinates,
  graph/scene identifiers, receipts, and frame counts stay in private executor
  code or core geometry tests.
- Ordinary product readiness must use a causal predicate with a bounded
  diagnostic timeout. Fixed pass counts remain valid only when discard/terminal
  pass count is itself the contract.
- The remaining contained-content projection, overshoot/release-hysteresis,
  product Burst, and touch capability gaps are retained as explicit evidence. The window-sized
  preview shell is now covered separately and is not treated as full content
  projection until U4.
- The lightweight native example is a discoverability checkpoint. The complete
  game-editor showcase and layout/preset shell remain assigned to U10.
