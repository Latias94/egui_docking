---
name: dockspace-integration
description: Integrate or review Dockspace docking and native multiview in Rust applications using egui, winit, or a custom UI host. Use for choosing a host, connecting rendering and input, or diagnosing an integration's docking, presentation, and window lifecycle behavior.
---

# Dockspace integration

Connect Dockspace to the application's existing UI and resource owners. Use the
public contracts and the smallest host that supports the requested interaction.

## Find the matching sources

Identify the actual dependency revision, UI framework, renderer boundary and
target platforms from the consumer's manifests and source. Use that revision's
examples and signatures when implementing the integration. The project is
pre-release; names and availability can differ between checkouts.

Source paths below are relative to the matching Dockspace checkout, not to the
installed skill directory. If no checkout is available, locate the dependency
sources or retrieve the relevant files at its revision from
[Latias94/dockspace](https://github.com/Latias94/dockspace). Use upstream `main`
for navigation only when a dependency revision has not been selected.

## Choose the host

| Application need | Integration | Read next |
| --- | --- | --- |
| egui tabs, splits and contained floating groups in one OS window | `egui_dockspace`; use a complete-output host for drag docking | [egui](references/egui.md) |
| A different immediate or retained UI framework | `dockspace::runtime::DockspaceSession` | [Custom host](references/custom-host.md) |
| Shared winit event, coordinate and repaint helpers | `dockspace_winit` alongside the application's UI and renderer | [Custom host](references/custom-host.md) |
| Managed OS windows and cross-window interactions | Core native hosting, or the experimental `egui_dockspace_native` adapter | [Multiview](references/multiview.md), plus the chosen renderer's reference |

A contained floating group stays inside a logical surface. Moving content to a
separate OS window requires native hosting. Check `docs/PLATFORM_SUPPORT.md` for
the requested backend before selecting the multiview path.

## Keep the ownership boundaries

- One workspace has one session, shared across its logical surfaces and windows.
  Pane content stays in the application, keyed by stable `ItemId` values.
- Use `DockPolicy` for docking and close permissions, presentation configuration
  for geometry and timing, and adapter hooks for appearance. Let core resolve
  docking targets, candidate layouts and semantic outcomes.
- Treat accepted input, committed actions and actually presented output as
  separate stages. Follow the selected host's receipt and result APIs.
- Keep UI callbacks synchronous. Run saving and other I/O through
  application-owned workers, then deliver data and exact close tokens back to
  the UI owner.
- With `serde`, save the layout document using stable external pane keys; keep
  business data and file storage separate. Allocate pane identities through
  `DockspaceDocumentBootstrap::ensure_item` before building a persistent layout.
  Core sessions use `from_persistent_layout`; the egui builder accepts the
  bootstrap through `persistence`. See
  `integration/dockspace-host-harness/tests/document_float_roundtrip.rs`.
  Use committed `notification.document_changed()` for complete-document
  autosave, including native placement changes.

## Implement and verify

Start from the matching runnable example and adapt its host boundary to the
consumer. Preserve existing event-loop, renderer and pane ownership where the
public contract permits it. Load only the references needed for that host.

Build with the selected features and test the implemented workflow. For docking,
exercise a real press/move/release and preserve edited pane data through moves
and closes. For custom hosts and multiview, use the additional boundary cases in
their references. `docs/ADAPTER_TESTING.md` maps reusable conformance scenarios to
host responsibilities; select scenarios for the capabilities being implemented.

Report the chosen entry point, application-owned resources, verified interaction
paths and remaining platform limits. Separate headless contract coverage from
real renderer and OS-window verification.
