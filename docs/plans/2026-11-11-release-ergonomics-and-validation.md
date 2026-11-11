# Release ergonomics, module locality and native validation

Baseline: `8a98548` on `refactor/egui-pane-ergonomics`.

## Outcome

Make existing docking interfaces easier to use and customize, organize the
remaining mixed-responsibility files and qualify the actual egui/native host.
Keep one core mutation authority, one native application command slot and the
existing exact-output, retry and atomic-publication contracts.

## Work

1. Preserve the existing `DockspaceSubmittedAction` identity in host-frame
   `*_current` convenience commands. Prove mixed successful/rejected batch
   correlation through the independent host consumer. Clarify egui action
   result documentation: accepted publication is distinct from an applied
   product action.
2. Allow native applications to replace complete runtime policy through the
   existing configuration submission and bounded command slot. Keep queued
   policy unpublished until commit and preserve drop/retry behavior. Cover
   per-pane rules and slot admission using existing native tests.
3. Apply existing floating-menu visual overrides to the presentation command
   popup. Preserve the local egui frame geometry, shadow and defaults. Prove
   the actual rendered menu both with overrides and with only local theme.
   Preserve application-supplied rich title colors across tabs, overflow rows
   and drag feedback unless an explicit docking or egui text override applies.
4. Split cohesive responsibilities out of remaining long implementation or
   product-test files. Keep state in the existing owners, preserve method/test
   bodies and registration, and use only the private visibility needed by
   each moved implementation. Select seams from actual callers and shared
   state rather than a target line count.
5. Run the actual eight-pane editor, shared-Root/native-Child journeys and
   renderer/backpressure regressions in the existing isolated Linux X11
   container. Exercise the native style and policy configuration paths.
   Fix confirmed qualification-driver errors without weakening the host
   contract. Record actual commands, source identity, platform profile and limits.

## Verification

Use existing nextest suites on macOS and the existing native Cargo target in
the Linux container. The container lacks nextest, so its native suites use
`cargo test`; real event-loop tests run in separate processes on an isolated
display. Independent workspaces may build concurrently with two build jobs;
one display has only one input driver at a time.

Required gates are affected core/egui/native tests, independent host default and
serde tests, egui product default and serde tests, correctness/suspicious Clippy,
strict documentation and doctests, formatting and the core dependency check.
Reuse existing behavior tests for mechanical module moves. Add tests only for
the confirmed interface and style changes.

Finish with independent review, precise local Conventional Commits and a dated
qualification record linked from current validation documentation. No push,
PR, merge, upstream framework change or physical-platform claim beyond the
observed Linux X11 profile is included. Do not read or modify `docs/reviews/**`
or `repo-ref/**`, and preserve any unowned working-tree change.
