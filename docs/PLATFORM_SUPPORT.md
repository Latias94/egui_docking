---
title: "Dockspace Platform Support"
status: "Scoped evidence; native qualification incomplete"
reviewed: "2026-10-31"
source_revision: "13dd12747b600d7c92921effa8cf521bfd120359"
---

# Dockspace Platform Support

`dockspace` is the renderer-neutral product. An engine supplies its UI, renderer,
input, scheduling and platform resources; `egui_dockspace` and
`egui_dockspace_native` are reference consumers. Support below separates the
public host contract, adapter output ownership and actual platform qualification.
Implementing an API or publishing a capability bit does not certify physical
input on that backend.

This snapshot follows the [canonical engine-host plan](plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md)
and the dated [validation record](VALIDATION.md). The production/physical
baseline is `13dd127`; the separately identified W16 model result below is
committed as `9140b18` on 2026-10-31. Overall U9 and the complete plan remain open.
Earlier dated qualifications
retain their historical scope; the 2026-10-31 entries supersede earlier
physical-smoke coverage limits.

## Core and public engine-host contract

| Boundary | Current contract and evidence | Host responsibility |
| --- | --- | --- |
| Core model | One item/root/surface graph, validated atomic mutations, policy, selection, layout, target acquisition, close and persistence identity. | Application pane data, fonts, drawing resources and preset storage. |
| Public runtime | `DockspaceSession` and rollbackable `DockspaceHostFrame` expose measurements, semantic commands, ordered input, surface programs, output disposition and focus/repaint requests. | Register logical surfaces, supply complete facts and execute the requested work. Local/contained use does not require desktop coordinates or native enrollment. |
| Immediate or deferred rendering | A host may execute the ordered program immediately or compile it into owned display-list/retained-node records that outlive the frame borrow. | Preserve composition, clipping, content slots and the snapshot's hit/semantic identities. |
| Current-output authority | Complete sink acceptance yields an execution receipt; confirming it in a committed frame emits an exact affine output. A later committed acceptance of that output's terminal `Presented` result can promote current receiver authority. | Report the exact output as `Presented` or `Dropped` at the renderer boundary the host owns. Submission and traversal alone do not prove presentation or GPU scanout. |
| Optional native lifecycle | The same runtime accepts incarnation-bound window facts, effects, readiness, recovery and retirement. `DirectPrepared` and `Staged` share exact first-live publication. | Own OS windows/renderers, report real preparation and lifecycle facts, and retain terminal obligations. Fixed egui staging passes are a reference-host strategy. |

The implementation boundary is documented in
[`runtime.rs`](../crates/dockspace/src/runtime.rs),
[`runtime/session.rs`](../crates/dockspace/src/runtime/session.rs),
[`runtime/host_frame.rs`](../crates/dockspace/src/runtime/host_frame.rs),
[`runtime/paint.rs`](../crates/dockspace/src/runtime/paint.rs),
[`runtime/presentation.rs`](../crates/dockspace/src/runtime/presentation.rs) and
[`runtime/native.rs`](../crates/dockspace/src/runtime/native.rs).
The independent [public host harness](../integration/dockspace-host-harness/README.md)
qualifies immediate/deferred recording, local/contained interaction and explicit
native host-contract traces without egui or OS windows. Its recording
`Presented` observations establish that host contract, not a platform
certification. Open-GPUI and the user's actual engine are outside the current
completion dependencies.

Native creation retains durable source ownership while compiling prospective
child content. The accepted exact first valid interactive child output
atomically publishes transfer, workspace revision and admission. Created,
Prepared, Show dispatch and staging output cannot transfer ownership by
themselves. See [`engine/native_projection.rs`](../crates/dockspace/src/engine/native_projection.rs),
[`engine/native_admission.rs`](../crates/dockspace/src/engine/native_admission.rs)
and the [R15 qualification](VALIDATION.md#2026-10-30-r15-first-live-ownership-replacement).

## Official egui reference adapter

The official adapter baseline is egui/eframe `0.37.0`. Its output capability
depends on who owns the run:

| Integration | Supported boundary | Limitation |
| --- | --- | --- |
| Caller-owned `egui_dockspace::run_ui` | Returns complete `FullOutput` plus an affine `DockspaceOutputReceipt`. The renderer reports the terminal result; core accepts it at a later committed boundary. Local/contained preview-qualified gestures and pane-focus settlement use that path. | Drawing must be synchronous and append-only on the supplied Ui layer. Exact shape/clip/transform spans are checked after output hooks. Cross-layer writes or modified output make the corresponding authority unavailable; a filtered/replaced complete output must be reported `Dropped`. |
| Unmodified `eframe::App::ui` callback | Rendering and applicable semantic commands remain available, with typed capability/status results. | The callback does not own final output or renderer terminal disposition. Presentation settlement, preview-qualified dragging and pane-focus settlement remain unavailable there. Callback return and an end-pass hook cannot supply that missing fact. |

[`pass_settlement.rs`](../crates/egui_dockspace/src/pass_settlement.rs) owns the
caller-run receipt and discard/unwind handling.
[`output_proof.rs`](../crates/egui_dockspace/src/output_proof.rs) checks output
integrity; its private markers do not prove presentation. Receiver declarations
must match the exact accepted pass. Undeclared interactive content, popups,
modals, foreign overlays and ambiguous identities retain conservative blocker
semantics; see the [renderer contract](../crates/egui_dockspace/src/product_render/README.md).

The [2026-10-31 L03 qualification](VALIDATION.md#2026-10-31-l03-single-batch-burst-qualification)
passes the complete 60-case local product matrix, including a complete gesture
in one raw egui packet. Local timed hover, explicit Touch arming and edge
scrolling share the core continuation and exact-output barrier. These local
results do not qualify native physical Touch or automatic native stationary
input.

## Fork-backed native capabilities and qualification

The unpublished reference adapter uses graph-neutral Winit/eframe facts and
renderer terminal callbacks. The mapping in
[`capabilities.rs`](../crates/egui_dockspace_native/src/capabilities.rs) is a
backend contract: even a supported capability may yield an explicit `Unknown`
fact at an individual causal boundary, as specified by `NativeHostCapability`
in [`runtime/native.rs`](../crates/dockspace/src/runtime/native.rs).

All mapped backends advertise `AuthoritativeInventory`,
`PointerHitTestObservation`, `GlobalFocusObservation` and `CloseCancellation`.
The additional bits are:

| Capability | X11 | macOS | Windows | Wayland | Other/fallback |
| --- | --- | --- | --- | --- | --- |
| `NativeWindowLifecycle` | Yes | Yes | Yes | No | No |
| `HoveredWindow` | Yes | Yes | No | No | No |
| `DesktopPointerPosition` | Yes | Yes | No | No | No |
| `AuthoritativeButtonState` | Yes | No | No | No | No |
| `GlobalWindowPlacement` | Yes | Yes | Yes | No | No |
| `WorkArea` | Conditional exact facts | Yes | Yes | No | No |
| `PointerHitTestControl` | Yes | Yes | Yes | Yes | No |
| `WindowActivationControl` | Yes | Yes | Yes | No | No |

These bits must be read separately from the evidence:

| Platform | Recorded finite qualification | Remaining platform limit |
| --- | --- | --- |
| X11 | Ten finite XTest tear-off/first-live/redock/retirement journeys; three center-only high-rate cases; programmatic/output-pressure and deterministic lifecycle evidence. | Qualified only under the exact authority and environment below. Broader native readiness remains open. |
| macOS | Real-window default/bounded programmatic tear-off, first-live, redock and retirement; five output-pressure cases. Same-window event-time facts have Winit test coverage, and local mouse geometry has deterministic producer/adapter coverage. | Programmatic and same-window fact evidence does not certify a physical cross-window release. `AuthoritativeButtonState` remains absent; cross-surface physical promotion stays fail-closed. Retina/mixed-DPI and the complete physical matrix remain unqualified. |
| Windows | Conservative capability mapping and shared host/adapter contracts. | No current Windows real-input/platform certification is established by the cited records. Hovered-window, desktop-pointer and authoritative-button bits remain absent; placement/lifecycle bits alone do not qualify cross-window docking. |
| Wayland | Limited observation/control mapping; deterministic event-time local mouse geometry and Touch position/geometry-pair contracts. | No compositor/physical-input qualification. Managed native lifecycle, global placement, work area and desktop pointer/hover authority remain unavailable in this mapping. Proven local operations need no global desktop lane. |
| Other/fallback | The four common capability bits only. | No native lifecycle or physical multiview claim. |

The [fork seam admission record](knowledge/egui-native-fork-seam-admission.md)
describes these fact boundaries. Its dated dependency and migration checkpoints
are historical; the dependency snapshot below and latest validation entries
define current reproducibility.

### Exact X11 authority and finite physical coverage

Exact X11 work-area authority is limited to the **first X screen**, with **one
identity-transformed active CRTC covering the root**, and complete,
self-consistent supporting-WM, EWMH and XSettings authorities. Root, WM, RandR or
XSettings changes invalidate the capture. Multiple active CRTCs, non-identity
transforms or incomplete facts produce `Unknown`; monitor bounds cannot replace
usable-work-area authority. This restriction is recorded in the admission
document's Work-area roster entry and consumed by
[`work_area.rs`](../crates/egui_dockspace_native/src/work_area.rs).

The [2026-10-31 edge qualification](VALIDATION.md#2026-10-31-x11-non-guide-pane-edge-docking)
records eight physical journeys at `13dd127`. The subsequent
[tab-half qualification](VALIDATION.md#2026-10-31-x11-physical-tab-half-insertion)
adds two and reruns all ten:

| `EGUI_DOCKSPACE_PHYSICAL_TARGET` | Target | Required final layout |
| --- | --- | --- |
| Unset (`guide-control`) | Explicit Center guide control | Ordered `[Main, Child]` tabs in the original root. |
| `pane-center-a` | Content point `(0.35, 0.35)` | Same center merge. |
| `pane-center-b` | Content point `(0.65, 0.35)` | Same center merge. |
| `pane-center-c` | Content point `(0.65, 0.65)` | Same center merge. |
| `pane-edge-north` | Content point `(0.30, 0.16)` | Two singleton tabs leaves, expected vertical order and equal weights. |
| `pane-edge-south` | Content point `(0.30, 0.84)` | Two singleton tabs leaves, expected vertical order and equal weights. |
| `pane-edge-east` | Content point `(0.84, 0.25)` | Two singleton tabs leaves, expected horizontal order and equal weights. |
| `pane-edge-west` | Content point `(0.16, 0.25)` | Two singleton tabs leaves, expected horizontal order and equal weights. |
| `tab-half-before` | Physical point inside the full Main tab's left half, excluding close/guides | Exact insertion marker and ordered `[Child,Main]` tabs. |
| `tab-half-after` | Physical point inside its available right half before Close Main | Exact insertion marker and ordered `[Main,Child]` tabs. |

Pane points are normalized to current content. Physical rounding must stay in
the named inner-pane basin, outside the outer-root band and every actual guide
hit; the runner rechecks those facts before release. It cannot substitute
another point or a guide. Every successful journey checks held source input,
a painted Dock program before physical release, the requested model layout,
exact child X11 `DestroyNotify`, semantic viewport retirement and quiescence.
A viewport step is a progress barrier and a painted marker proves emission;
neither is a native release receipt or GPU presentation proof.

Tab-half selection uses an opt-in passive diagnostic copied from the painted
tab's complete visible bounds; the Tab accessibility receiver still excludes
its close button. These diagnostics confer no input or presentation authority.
The finite single fully visible target has no overflow chrome; clipped and
overflowed tab-half insertion remain unqualified.

The recorded environment is Linux/arm64, Rust 1.95, Xvfb/Openbox, one X screen,
scale 1, a 2-CPU/4-GiB container and software GL with one renderer thread.
[`physical_x11/driver/scenario.rs`](../integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs),
[`driver/x11.rs`](../integration/egui-native-smoke/src/bin/physical_x11/driver/x11.rs)
and [`assertions.rs`](../integration/egui-native-smoke/src/bin/physical_x11/assertions.rs)
own the finite scenario, OS evidence and semantic checks. The latest slice also
records 42 runner tests, 70 adapter units, 114 product and 15 official tests
passing. Core/fork production gates retain their previous scope.

The [continuous-input qualification](VALIDATION.md#2026-10-31-continuous-x11-input-and-pending-release-wake)
adds `EGUI_DOCKSPACE_PHYSICAL_LOAD=1khz` for explicit `pane-center-a/b/c` only.
It injects 1,000 distinct checked motions and releases immediately without an
inspection/idle drain. Guide and edge targets are rejected before the gesture.
The release-to-model measurement is a conservative upper bound beginning at
the native release callback. This finite profile does not certify hardware
1 kHz or the 2 ms reducer service boundary, maximum-roster/O256 cross-saturation,
nested targets, mixed DPI or high-rate edge docking.

### Configured-capacity model evidence

The [terminal-guard correction](VALIDATION.md#2026-10-31-terminal-guards-for-never-created-native-bindings)
extends the existing cleanup owner to proven never-created bindings. Binding
publication and Create delivery remain separate facts, so a published Pending
binding still retains its producer obligation when report backpressure delays
the effect. Cancellation, reset and replacement discard preserve exact-provider
quiescence without manufacturing Destroyed observations. The finite W4/A1,
E1, M4/C0/L2 close/create combination passes on macOS and Linux normally and
without adapter debug assertions, under 4 MiB. It qualifies actual cancellation
and Failed candidate retry, exact per-owner successor promotion and full
reclamation in the coordinator model. It does not claim real OS close/create
or simultaneously fill every owner class.

The [2026-10-31 maximum-roster record](VALIDATION.md#2026-10-31-configured-maximum-roster-and-terminal-fanout)
adds a focused W16 model qualification in the existing
[`combined_soak.rs`](../crates/egui_dockspace_native/src/coordinator/tests/combined_soak.rs)
test `configured_maximum_roster_and_terminal_fanout_survive_ordinary_freeze_and_retry`.
All 16 configured bindings and a complete roster coexist with 32 real fork-issued
focus/visibility command identities. After ordinary ingress freezes at 1,360
accepted raw cursor records, all 32 command `Retired` results and 16 original
`Destroyed` callbacks retain protected admission. Candidate and whole-batch
retirement retries preserve their owners, and all bindings/references retire
to quiescence.

The case and three adjacent controls pass 4/4 normally and 4/4 with native debug
assertions disabled. Sampled owned/reserved input data is
2,325,314 / 4,194,296 bytes under 4,194,304 bytes (4 MiB); allocator metadata,
workspace/renderer state and fixture allocations are excluded. This closes the
W16 profile's concurrent roster/property/Destroyed model boundary. It creates
no OS windows or eframe output tokens and uses a constant injected clock.
It does not combine W16 with the separate W2/R258 O256 real-output profile,
fill close/create owner classes, prove active-release semantics in this
cursor-only fixture, or qualify OS timing and the 2 ms reducer-service boundary.

The separate [real W16/O256 qualification](VALIDATION.md#2026-10-31-simultaneous-w16-roster-and-o256-real-outputs)
now combines 16 actual windows with O256/control2/protected-staging1 under an
explicit I64/W16/R258 profile. It keeps complete root-roster capture enabled
and verifies all exact native window identities in the callback that observes
full occupancy. All 258 original terminals drain, all 15 children redock and
receive Destroyed, a fresh root-only roster and exact `[1..16]` Main tabs return,
and final output owners reach zero. The new case and five legacy pressure cases
pass on both macOS and X11. Sampled owned/reserved bytes are
2,352,033/3,455,424 on macOS and 2,352,530/3,457,752 on Linux, under 4 MiB;
fixture storage is reported separately. This finite output/inventory combination
does not simultaneously fill the profile's remaining owner classes or prove
input throughput, release latency or the 2 ms service boundary.

The [opt-in release wall-time test](VALIDATION.md#2026-10-31-opt-in-release-reducer-wall-time-measurement)
adds actual claim/translation/core-poll segment measurements with one shared
system clock. Its first Linux and macOS runs preserve all 257 active moves across budget
expiry, an intentionally delayed receiver, a zero-progress expired poll and
exact release/output settlement. It reports the over-2-ms samples as observed.
Commit, painting and event-loop return are outside that segment, so this
evidence does not certify a strict 2 ms upper bound or the complete host-service
envelope. The ignored test is explicitly selected for release qualification;
ordinary CI keeps deterministic budget/continuation checks.

### Native stationary observations

The public [`NativePointerContinuation`](../crates/dockspace/src/runtime/native/continuation.rs)
is a read-only request from the existing gesture owner. Explicit host-contract
tests cover timed work with fresh source binding, capture/delivery, coordinates,
time and exact target-output facts. A request or its expected source identity
cannot supply those observations.

The [native pointer observation prerequisite](VALIDATION.md#2026-10-30-native-pointer-observation-prerequisite)
can query a limited X11 desktop point/root-child hover with exact device/window
correlation. **Fresh native stationary capture and delivery remain `Unknown`.**
`XIQueryPointer` does not identify a current grab owner, and retained implicit
capture cannot repair that gap. The seam emits no motion and enables no automatic
adapter timer/continuation. Automatic native stationary scheduling, physical
Touch and native-local OS handoff remain unqualified.

## Dependency reproducibility and open scope

Current native results depend on unpublished local overrides:

| Dependency | Locally qualified revision | Tracked remote revision |
| --- | --- | --- |
| egui/eframe fork | `70f7b51712284561d3408950273f128ff556a81b` | `0716beee5b7284389f2e80ffb6b24b5af6ae2910` |
| Winit fork | `6c10c87da2a06871371de7d30fedc656cf7c3681` | `f378b50f29cafafec31cd8675c17646b4ef57d70` |

The tracked revisions are visible in the
[native workspace manifest](../integration/egui-fork-workspace/Cargo.toml) and
[smoke manifest](../integration/egui-native-smoke/Cargo.toml).
The [2026-10-30 Touch-fact record](VALIDATION.md#2026-10-30-native-touch-event-time-coordinate-pairs)
identifies the local revisions; the 2026-10-31 edge record retains those
overrides and reports a failing direct tracked-pin check because required native
host seams are absent. A clean checkout using only the tracked remote pins
cannot reproduce this native qualification.

Validation references such as `/tmp/dockspace-physical-edge-qualification.json`
and `/tmp/dockspace-physical-load-final-qualification.json` identify local
artifacts recorded in `VALIDATION.md`; they are not portable checkout assets.
No runtime or platform qualification was rerun for this documentation snapshot.

Still open are remaining close/create owner
capacity and native service-time qualification; overflowed/clipped tab insertion, nested and
mixed-DPI physical coverage; fresh stationary capture, automatic scheduling and
native-local/Touch OS evidence; Windows/macOS platform certification; the native
showcase; and U9 dependency publication, clean-checkout reproduction, package
and remaining evidence/deletion closure. The ten-journey and
center-load results retain their finite scope.
