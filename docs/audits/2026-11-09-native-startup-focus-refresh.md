# Native startup focus refresh

The native startup focus gap from the [pane-focus audit](2026-11-09-egui-pane-focus.md) is fixed locally. Clicking the editor after a cold launch can now establish observed pane focus without changing OS focus to another application first. Tab selection remains unchanged.

## Cause and implementation

The original positive OS focus event could arrive before the exact root route and its committed visible roster existed. The callback correctly froze that observation as Unknown. Later route enrollment did not cause another OS focus event, so clicking inside the same window could not establish current global pane focus.

The eframe fork now exposes `request_native_global_focus`. Requests coalesce in the existing attachment and run through the shared native service event, after the host service callback has returned. The outer event dispatcher assigns a new event ordinal, samples the provider, and calls the existing `on_global_focus` callback. This avoids reentering the runtime lock and preserves buffered framework input.

Glow and WGPU query the exact live `winit::Window` objects with `has_focus()`. A unique focused window yields its viewport and physical window identity. No focused window, or conflicting focus claims, yields Unknown. The provider no longer uses eframe's initially selected root viewport as evidence for native global focus.

The native bridge requests this observation only when a committed snapshot makes an exact route newly focus-reportable. Unchanged visible snapshots remain idle. The bridge retains a single pending refresh bit until a fresh observation enters ordered ingress. Capacity rejection retains the request; after admission recovers, both report settlement and idle service completion can schedule a new provider query. No rejected focus payload is retained for later replay.

The same retry also covers an ordinary OS focus callback rejected by capacity when no refresh was previously pending. Previously, the last accepted Unknown or Dock observation could remain in effect after recovery with an unchanged roster, even though OS focus had moved to another window. A later successful fresh observation now settles that missing fact.

Callback-time binding qualification and reducer-time core qualification are preserved. Hidden, retired, and replaced bindings cannot acquire authority from an old observation. The change does not issue an activation command or change focus from the application's own widgets.

## Verification

Independent read-only behavior and standards reviews found no remaining P2 or higher issue in the final focus changes. Cargo used one build job, nextest used one test thread, and the existing target directory was reused.

| Check | Result |
| --- | --- |
| Eframe native library suite, Glow and WGPU enabled, macOS | 107 passed, 0 skipped. |
| Fork/native workspace, all features and targets except `egui-native-smoke`, exact final Git pin | 584 passed, 1 skipped. |
| Eframe and fork/native Clippy correctness/suspicious gates | Passed; existing advisory warnings remain. |
| Strict eframe documentation, `RUSTDOCFLAGS=-D warnings` | Passed with `glow,native-host-seam,x11,wayland,persistence,inspection`. |
| Main, native, eframe, and archived-fixture formatting | Passed. |
| X11 real-window capture assertions | Passed for dark, light, zoom 1.5, and background launch. |

The new regressions cover request coalescing, new event ordinals, provider changes between request and dispatch, Unknown and ambiguous native focus, exact physical-window replacement, service-time requests without callback reentry, unchanged visible snapshots remaining idle, and bounded-ingress recovery both with and without an existing refresh request. Existing committed-visible-roster, hidden-window, retired-binding, and rebound-window regressions continue to pass.


## Real-window evidence

The Linux X11 fixture uses Xvfb, Openbox, XTest clicks, the Glow renderer, and software GL. The cold-start editor assertion runs before any diagnostic focus round trip. A separate later blur/refocus check verifies removal and recovery of the cue. Captures also verify transfer to contained Inspector and clearing when ordinary egui content takes focus. A background launch case uses an Openbox application rule with `<focus>no</focus>`. The fixture also sets `ViewportBuilder::with_active(false)`, but winit documents that hint as unsupported on X11; the window-manager rule constructs the actual background state. The driver checks that the separate foreground window remains active after startup and output settlement.

The script recorded 23 node/text/shape snapshots. In dark, light, and zoom cases, cold-start editor assertions run before creating a blur-control window or activating the fixture window. The background case first verifies inactive startup and then explicitly activates the fixture for its remaining checks. Selected tabs are preserved throughout each case. Contained-pane handoff is checked in dark, light, and background cases; the zoom case checks the docked and external editors.

| Case | Capture |
| --- | --- |
| Original cold-start gap | [Before](2026-11-09-egui-pane-focus/evidence/dark-editor.png). |
| First editor click after cold start | [Dark](2026-11-09-native-startup-focus-refresh/evidence/dark-editor.png), [light](2026-11-09-native-startup-focus-refresh/evidence/light-editor.png), [zoom 1.5](2026-11-09-native-startup-focus-refresh/evidence/zoom-editor.png). |
| Real OS blur clears the cue | [Blurred editor](2026-11-09-native-startup-focus-refresh/evidence/dark-blurred.png). |
| Focus moves to contained Inspector | [Contained pane](2026-11-09-native-startup-focus-refresh/evidence/dark-floating.png). |
| Focus leaves dockspace for ordinary egui content | [External editor](2026-11-09-native-startup-focus-refresh/evidence/dark-external.png). |
| Background launch preserves the foreground window | [Inactive fixture and active control](2026-11-09-native-startup-focus-refresh/evidence/background-selected.png). |

Dark, light, zoom, background, blur, and contained-pane images were also inspected manually. The [receipt](2026-11-09-native-startup-focus-refresh/receipt.json) records exact commands, source hashes, scope, and artifact hashes. The [fixture](2026-11-09-native-startup-focus-refresh/fixture/Cargo.toml), [capture driver](2026-11-09-native-startup-focus-refresh/capture.py), compressed frame records, and validation logs are archived beside it.

## Scope and publication

Visual acceptance covers Linux X11 with Glow. The eframe library suite compiles both Glow and WGPU on macOS. Windows, Wayland, and native macOS visual behavior are not independently exercised in this run.

The egui fork revision is `0d9c48d765954dd3eff99c0d3cfa686e9d8591cf`, committed locally on `fix/native-focus-refresh-0.37`, based on `f94ee58b3914de7ad71dab36919b329ef36b4035`. The native workspace patches, smoke inspection dependency, lockfile, and CI checkout use the new revision. The winit pin is unchanged.

No push or merge has been performed. Locked/offline local validation seeded Cargo's Git caches from the local fork commit. Fresh clones and CI require publishing that fork commit before they can resolve the new pin.
