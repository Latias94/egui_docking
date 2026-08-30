# Dockspace behavior-spine validation

This document is the U1 validation contract. It records semantic journeys and
current evidence without turning the shared conformance crate into a GUI DSL.
Executors resolve their own current semantic bounds and platform facts. The
shared Rust catalog carries only `JourneyId`, cadence profiles, and admission
capabilities; the source/basin/preview/outcome/ownership/focus/quiescence
checkpoints stay here so they can be reviewed without adding another product
authority.

## Journey directory

| Journey | Semantic source / basin | Preview | Outcome / ownership | Focus / quiescence | Cadences | Executors and current status |
| --- | --- | --- | --- | --- | --- | --- |
| L01 | Tab body; below-threshold select or threshold-crossing drag | None before selection | Select without drag; source remains owner | Selected tab; local quiescence | Burst, Stepwise | `core-host` and `egui-product-headless` characterize selection; exact human threshold remains product evidence |
| L02 | Tab body; semantic tab gap before/after a sibling | Visible insertion candidate | Reorder once; local target owns the tab | Moved tab; local quiescence | Burst, Stepwise, ReleaseWithFinalMove, DiscardBeforeRelease | Both existing shared product/core paths cover the local command spine; product drag evidence is normative, pass-count details are not |
| L03 | Tab body; pane center plus north/south/east/west basins | Magnetic candidate, not a guide-center requirement | Center merge or edge split; target owns payload | Moved tab; local quiescence | Stepwise, Hesitant, ReleaseWithFinalMove | Ordinary non-guide center merge is normative; four-edge cadence and hysteresis coverage remain U4 gaps |
| L04 | Contained title/edge; surface free space and eligible pane basin | Full contained-window projection | Move/resize/re-dock commits once; contained or target owner | Moved item; local quiescence | Stepwise, Hesitant, ReleaseWithFinalMove | Durable placement and ownership are covered; full-window projection remains an intentional known-invalid baseline |
| L05 | Tab body; current basin can become stale or disappear | Candidate is cleared while source stays recoverable | Cancel/reject and restore source without partial mutation | Preserve source focus; local quiescence | Stepwise, TargetDisappears | `core-host` stale-target and target-destruction fixtures provide the current normative evidence |
| L06 | Tab body; center/edge basin with release barrier | Exact candidate must be presented before release commits | Commit presented candidate or restore source | Moved item or preserved source; local quiescence | Burst, Hesitant, ReleaseWithFinalMove, DiscardBeforeRelease | Discarded-preview and release-waits tests are normative; ordinary fixed hover loops are known-invalid test technique |
| L07 | Group handle, contained title, menu, and tab close; no overlapping owner | Group/command preview only where applicable | One semantic owner executes each chrome action | Close policy chooses successor; local quiescence | Burst, Stepwise | Close-region and group-drag tests characterize the current ownership boundary |
| L08 | Splitter, tab, close, keyboard, AccessKit, and touch sources; semantic command target | Splitter proposal where applicable | Shared prepared command commits layout action | No focus guessed from dwell; local quiescence | Stepwise, TouchLongPress | Existing shared local scenario runs through `core-host` and `egui-product-headless`; touch long-press has a typed capability gap |
| N01 | Contained/native title; outside-all release | Native pending while source remains recoverable | Create → first-live → redock → retirement | No inferred pane focus; native lifecycle quiescence | Stepwise | Core-host managed lifecycle passes; official/product hosts remain capability-negative without native facts |
| P01 | Tab body in one window; center/edge basin in another window | Completed-pass hover candidate | Physical redock under one qualified handoff | Moved item; native lifecycle quiescence | Burst, Hesitant, ReleaseWithFinalMove | X11 physical smoke is characterization evidence; other hosts fail closed until their physical gates pass |

The current shared representative journey is L08. It is intentionally the
existing `LocalTabCloseSplitterAndContained` scenario rather than a new input
language. Its two executors emit the same ordered `SemanticObservation` values:

- `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics`

N01 uses the existing lifecycle scenario and remains a separate capability
boundary. P01 is never inferred from a local or official-egui run.

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
| U1-04 | L03 presentation: outer guide targeting | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Record current guide evidence while basin targeting is built |
| U1-05 | L03 presentation: center guide hit | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_center_guide_merges_tabs` | Do not promote guide-center aiming to the product contract |
| U1-06 | L04 action: durable contained move/resize/redock | normative | `integration/egui-product-harness/tests/product.rs::contained_move_resize_select_focus_and_dock_back_preserve_exact_ownership` | Durable placement changes only at terminal commit |
| U1-07 | L04 presentation: full-window contained drag preview shell | characterization-only | `integration/egui-product-harness/tests/product.rs::contained_title_drag_paints_a_full_window_preview_in_the_same_run` | The window-sized shell is present; live pane-content projection remains a later U4 gate |
| U1-08 | L05 action: stale target has no partial mutation | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_rejects_a_stale_target_without_partial_commit` | Keep fail-closed stale rejection |
| U1-09 | L05 presentation: target loss preserves source drag | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_target_destruction_preserves_the_source_drag` | Clear only the lost target route |
| U1-10 | L06 retained frame: discarded preview cannot release | normative | `integration/egui-product-harness/tests/product.rs::default_features_discarded_preview_cannot_release_a_drag` | Release needs current presented evidence |
| U1-11 | L06 presentation: release waits for exact preview | normative | `integration/egui-product-harness/tests/product.rs::default_features_release_waits_for_the_new_preview_to_be_painted` | Commit exactly once after presentation |
| U1-12 | L06 retained frame: ordinary readiness via fixed hover loop | known-invalid | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Replace fixed ordinary waits with causal predicates |
| U1-13 | L07 action: close receiver stays disjoint from tab receiver | normative | `integration/egui-product-harness/tests/product.rs::tab_close_semantics_follow_the_exact_close_region_occlusion` | One close owner, no action stealing |
| U1-14 | L08 action: shared local observations are executor-parallel | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`; `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` | Keep the shared scenario narrow and normalized |
| U1-15 | L08 action: touch long-press | known-invalid | No existing executor test; `ScenarioCapability::TouchLongPress` remains unsupported | Preserve typed capability-negative evidence |
| U1-16 | N01 first-live ordering | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Record current causal ordering; do not infer an ownership verdict here |
| U1-17 | N01 retirement | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Preserve as lifecycle evidence until runtime replacement requalifies it |
| U1-18 | N01 quiescence | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Measure bounded cleanup, not callback count |
| U1-19 | P01 physical drag barriers | characterization-only | `integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs::perform_drag_out`; `::move_to_center_guide_while_pressed`; `::native_drag_preview_barrier` | X11 is evidence to requalify, not a portable authority |
| U1-20 | P01 missing native facts | normative | `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` (native admission assertion) | Unsupported hover/release/desktop facts remain typed and fail closed |
| U1-21 | L03 ordinary pane center basin | normative | `crates/dockspace/src/drop_resolver.rs::non_guide_center_basin_resolves_center_target`; `integration/egui-product-harness/tests/product.rs::ordinary_pane_center_release_uses_magnetic_merge_without_guide_aiming` | A non-guide point in the central 52% publishes the Center preview, highlights the explanatory guide, commits once, and preserves main-root ownership |

The N01 rows deliberately do **not** classify “native ownership before
first-live” as known-invalid. The existing fixture demonstrates a sequence,
but this U1 baseline does not promote that observation to an ownership verdict.
That comparison belongs to the later lifecycle replacement unit.

## Capability boundaries

| Executor | L01–L08 | N01 | P01 |
| --- | --- | --- | --- |
| `core-host` | L08 shared command scenario passes; gesture-preview rows remain characterization or gap | Programmatic lifecycle scenario passes | Unsupported without `NativePhysicalAuthority` |
| `egui-product-headless` | L08 shared command scenario and ordinary L03 center basin pass; four-edge/hysteresis rows remain gaps | Capability-negative | Capability-negative |
| `egui-official-harness` | Public local facade only | Capability-negative | Capability-negative |
| `native-coordinator-trace` / `egui-native-smoke` | Adapter-specific evidence | Programmatic lifecycle evidence | Physical X11 evidence only |

Capability-negative is a successful result when the exact missing fact and
unchanged source ownership are reported. No executor may substitute a rectangle
scan, final hover, fixed guide center, fixed ordinary frame count, receipt, or
internal scene/graph identity for a missing fact.

## Verification notes

- Shared scenarios contain semantic selectors and expected observations only;
  coordinates, graph/scene identifiers, receipts, and frame counts stay in
  private executor code or core geometry tests.
- Ordinary product readiness must use a causal predicate with a bounded
  diagnostic timeout. Fixed pass counts remain valid only when discard/terminal
  pass count is itself the contract.
- The remaining contained-content projection, four-edge hysteresis, and touch
  capability gaps are retained as explicit evidence. The window-sized
  preview shell is now covered separately and is not treated as full content
  projection until U4.
- The lightweight native example is a discoverability checkpoint. The complete
  game-editor showcase and layout/preset shell remain assigned to U10.
