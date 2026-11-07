# Native style/configuration source review

Verdict: **no blocking finding**. The change closes a real configuration split:
the native renderer receives a DockStyle while its existing session owns the
geometry and interaction configuration. Rejecting disagreement before native
enrollment is preferable to silently changing either authority.

This is an independent static review of the current worktree against
`2697b04ca9733af173863419d657bdac5d67de5d`. No Cargo, tests, GUI, network or timing
was run, and no repository source was edited.

## Field completeness and independent settings

`style.rs:266–301` has 25 builder setters. The 25 ordered comparisons in
`native_support.rs:42–150` cover each of them once, with the same conversion:

| Style fields | Core representation |
| --- | --- |
| `hover_selection_delay`, `touch_long_press_delay`, `tab_strip_edge_scroll_interval` | Identically named optional/non-optional Duration values |
| `touch_press_slop_distance`, `touch_resize_hit_extent`, `tab_bar_height`, `tab_strip_edge_scroll_band`, `tab_group_grip_extent`, `tab_horizontal_padding`, `tab_min_width`, `tab_max_width`, `splitter_thickness`, `splitter_hit_extent`, `splitter_keyboard_step`, `dock_fraction`, `floating_title_height`, `floating_border_width`, `floating_resize_extent` | Identically named f64 getters after the existing f32-to-f64 conversion |
| `tab_close_size` | `tab_close_extent` |
| `drop_guide_extent`, `drop_guide_gap`, `drop_guide_hit_padding`, `drop_guide_outer_inset` | `guide_extent`, `guide_gap`, `guide_hit_padding`, `guide_outer_inset` |
| `minimum_pane_size`, `minimum_floating_size` | Both components of the corresponding LogicalSize |

No core-only getter is accidentally compared. In particular,
`tab_strip_scroll_line_extent`, `pointer_drag_start_distance`,
`magnetic_center_fraction`, `magnetic_outer_band_fraction`,
`magnetic_outer_band_minimum`, `magnetic_outer_band_maximum` and
`target_release_hysteresis` remain independently configurable. Visual overrides
and `ghost_offset` are not compared; the latter is still validated for its
adapter-local sign/finite contract.

The successful construction test changes all seven core-only fields, changes
two represented style settings in agreement, and changes adapter-local visuals
and ghost offset. It then checks the retained complete configuration, version
and style. This is useful behavior coverage, not a mirror of the 25 getters.

## Rejection order and side effects

`surface_driver.rs:475–499` retains the existing order of the root-surface
existence check and root resource/close-slot check. It then validates style and
checks agreement before constructing the coordinator or queueing root
registration. The helper borrows both inputs and does not normalize, overwrite,
enroll or publish.

Detailed combined-storage validation still belongs to the coordinator
constructor (`coordinator.rs:470–504`), after this new gate. This review therefore
does not claim that every possible storage/configuration error pair retains
its old priority; the explicit existing root/owner-slot prefix is preserved.
Valid configurations retain the existing native enrollment behavior.

`NativeHostProtocolError::StylePresentationMismatch { field }` maps to
`InvalidConfiguration` (`error.rs:135–139`). Its concrete source remains
available through the standard error chain. Style failures travel through the
existing Adapter(DockspaceError) variant; that chain retains the original
DockStyleError instead of converting it to a string. The new invalid-style test
checks this using a non-finite adapter-only ghost offset.

## Floating-point and public API contract

The comparison is exact **numeric** equality, with no epsilon. It is consistent
with DockPresentationConfig and LogicalSize's existing PartialEq contracts;
it is not a to_bits identity check. Non-finite style values are rejected before
comparison. Positive and negative zero compare equal for fields whose
non-negative contract admits zero.

A core f64 literal such as `0.3_f64` need not equal
`f64::from(0.3_f32)` from the style. The new public conversion gives callers a
reliable way to create matching represented values. Do not add a tolerance or
replace the session configuration to conceal this mismatch.

The documentation correctly limits `DockStyle::presentation_config` to the
represented fields and core defaults, and explicitly keeps adapter-only
validation separate. `NativeDockspaceApp::new` documents the new rejection and
the independent core settings; constructor overloads delegate to the same
gate. The public API does not expose a later mutable session/configuration
escape hatch in the inspected app.

One optional documentation clarification would be to state “compared after
the style's f32-to-f64 conversion, without a tolerance” beside “match exactly.”
This is not a correctness blocker or a request for another test matrix.

## Scope and simplification judgment

The field list is long but concrete, runs only at construction, and has no
heap-backed inventory or persistent state. It reuses the existing conversion
and validator rather than creating a second numeric validator. Comparing the
whole mapped configuration would incorrectly reject core-only customizations.
Deleting validation would permit the observed style/session disagreement;
overwriting the session would discard independent configuration.

There is no smaller deletion that preserves both current owners and this
contract. A future single-configuration API could remove the overlap, but
would broaden this bounded correctness fix substantially. Keep this change;
do not introduce a generic property table, cached validity bit, provider or
additional synchronization layer.

The two mismatch cases test meaningful geometry and timing failures and typed
diagnostics. The original invalid-style source test and successful preservation
test cover separate responsibilities. Static field auditing establishes
completeness here; this report does not claim runtime qualification of every
one of the 25 mismatch entries.

## Source SHA-256 bindings

```text
08b7b337720115ff7dd8b6e0b787c82f14df36eccc73933532470f084d0e0f95  crates/egui_dockspace/src/style.rs
f88669b6922a21ea40f7214f8a0bb79508046803486eee9d70cefa1131b5f396  crates/egui_dockspace/src/native_support.rs
6402656e91ad030482d319feef6c672964b701bd321d9d3347bedc621ee2903b  crates/egui_dockspace_native/src/surface_driver.rs
531ed081b3c58647ea0f4bb27f3bf7fbe554c31328020ceb922a6be5a2623831  crates/egui_dockspace_native/src/error.rs
c3d46eef8eca464aeb5fcbb9e21784b653fad95a0f1847e52c616a57aee00c22  crates/egui_dockspace_native/src/app.rs
31d4e0309e2cc2efeed6fe919b99180f62d14cae1e6f85669e2eb59861362d92  crates/dockspace/src/presentation_config.rs
```
