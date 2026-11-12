# Platform support

`dockspace` is independent of a UI framework, renderer and window system.
Applications supply measurement, ordered input, rendering, scheduling and any
native resources. The [engine host guide](../crates/dockspace/docs/guide.md)
defines that contract; the [adapter testing guide](ADAPTER_TESTING.md) describes
how to exercise it from another framework.

`egui_dockspace` uses official egui for local surfaces and contained windows.
`egui_dockspace_native` is an experimental, unpublished integration with pinned
egui/eframe and winit forks. Its dependency revisions are defined in the
[native workspace](../integration/egui-fork-workspace/Cargo.toml) and lockfile.
Keep those patches out of the official workspace.

## Logical surfaces and native windows

A logical surface is not an OS window. One external Root window may host several
independently placed surfaces; a managed Child window hosts one whole-client
surface. A surface may itself contain split panes and contained floating roots.

- Publish the complete Root placement roster atomically, before measurement and
  encoding. Every placement has its own logical size, physical client origin,
  clip and presentation scale. Apply native DPI and placement transforms once.
- Report each physical window once. Source capture, hover, pane focus and exact
  presented output remain independent facts, including between sibling surfaces.
- Native close captures the complete window's content roster. Deferral or veto
  cannot close only a subset of its surfaces.
- Child creation preserves source ownership until every required placement has
  fresh, exact first-live presentation. Created, Prepared and show dispatch alone
  do not transfer content.
- Restore and layout reset may continue external Root windows with explicit
  surface mappings. Publish the replacement roster atomically, retain Child
  recovery anchors, and require fresh presentation and focus afterward.
- Removing one placement leaves a window with surviving placements intact.
  Window destruction affects the entire roster; each retired output/callback
  lane must quiesce before its lifetime is reclaimed.

The [native overview](../crates/egui_dockspace_native/README.md) and
[egui guide](../crates/egui_dockspace/docs/guide.md) explain the public entry points.
The [native composition ADR](adr/0003-native-window-composition.md) records why
these identities and publication boundaries are separate.

## Native backend capabilities

The adapter currently advertises the following capabilities in
[`capabilities.rs`](../crates/egui_dockspace_native/src/capabilities.rs).
An advertised capability means the backend can supply that class of fact;
individual observations can still be unavailable or `Unknown`.

| Capability | Linux X11 | macOS | Windows | Wayland | Other |
| --- | --- | --- | --- | --- | --- |
| Authoritative inventory | Yes | Yes | Yes | Yes | Yes |
| Pointer hit-test observation | Yes | Yes | Yes | Yes | Yes |
| Global focus observation | Yes | Yes | Yes | Yes | Yes |
| Close cancellation | Yes | Yes | Yes | Yes | Yes |
| Managed native lifecycle | Yes | Yes | Yes | No | No |
| Hovered window | Yes | Yes | No | No | No |
| Desktop pointer position | Yes | Yes | No | No | No |
| Authoritative button state | Yes | No | No | No | No |
| Global window placement | Yes | Yes | Yes | No | No |
| Work area | Conditional | Yes | Yes | No | No |
| Pointer hit-test control | Yes | Yes | Yes | Yes | No |
| Window activation control | Yes | Yes | Yes | No | No |

Missing facts remain `Unknown`. A client-local point cannot establish desktop
position, outside-all authority or another window's hover route. Local and
contained docking can operate without global coordinates or native enrollment.
Physical cross-window promotion requires the complete current source, capture,
hover, routing and presentation authority; a capability bit cannot substitute
for a missing observation.

### Linux X11

The automated reference profile uses Xvfb, Openbox, software Glow and XTest on
one X screen at native DPI 1. Shared Root tests also exercise independent
placement scales 1, 1.25 and 1.4. These are controlled placement transforms,
not physical mixed-monitor or mixed-DPI qualification.

Authoritative work-area publication is deliberately narrower than general X11
monitor support: it requires the first X screen, one active identity-transformed
CRTC covering the root, and complete verified WM/EWMH, RandR and Xsettings facts.
Cloned, overlapping or transformed configurations and missing authority remain
`Unknown`; a global EWMH work area is not treated as independent monitor areas.

### macOS and Windows

Both backends have compile coverage for the native adapter. macOS also supports
programmatic lifecycle and same-window observations, but has no authoritative
button-state capability. Windows lacks hovered-window, desktop-pointer and
button-state authority. Neither backend currently has complete physical
cross-window drag qualification. Keep those paths unavailable when their required
facts are missing.

The [solar-system example](../crates/egui_dockspace_native/examples/solar_system.rs)
provides a finite programmatic check: three real children present and move,
pause, return one child through Dock Back, then create and move a fresh child.
This exercises live geometry and lifecycle; it does not simulate physical
cross-window dragging or an OS close-button event.

### Wayland and other backends

Wayland does not advertise managed lifecycle, global placement or work-area
support in this adapter. Local surface contracts do not require those facts.
Other backends retain only the common observations shown above. New backend
support needs actual input and presentation tests as well as compile coverage.

## Verification and limits

The default [CI workflow](../.github/workflows/ci.yml) checks core behavior,
independent consumers, feature combinations, native compilation/tests and a
small real-renderer publication/backpressure smoke.
The separate [native qualification workflow](../.github/workflows/native-qualification.yml)
runs the real-window and X11 input journeys on demand. Run it before a release
or when changing native input, output ownership, lifecycle or dependency pins.
The [native qualification guide](../integration/egui-native-smoke/README.md)
contains the local commands and environment requirements.

Recording-host `Presented` reports test the public contract. Headless egui tests
exercise widgets and mapping. Compile checks establish build compatibility.
Real-window tests exercise their declared renderer/backend and journeys. None
of these results extends to an untested platform or proves GPU scanout latency.
Keep complete run logs, source revisions and environment details in CI artifacts
or release attachments rather than maintaining a dated source-tree ledger.
