# Dockspace behavior-spine validation

The canonical plan was revised on 2026-10-27 to prioritize `dockspace` as a
custom-engine library and egui as its interactive validation adapter. The
planned external public host consumer does not exist yet. Its eventual
recording-renderer results will prove API and output-contract portability,
not real GPU or OS presentation. Open-GPUI integration remains deferred.

This document is the U1 validation contract. It records semantic journeys and
current evidence without turning the shared conformance crate into a GUI DSL.
Executors resolve their own current semantic bounds and platform facts. The
shared Rust catalog carries the finite L03 source, basin, normalized inset
samples, cadences, and normalized outcomes alongside the existing L08 semantic
steps. It does not carry screen coordinates, guide selectors, frame counts, or
another product authority.

## 2026-10-27 audit baseline

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
