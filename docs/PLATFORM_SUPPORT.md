---
title: "Dockspace Platform Support"
status: "Qualified finite profiles; clean-checkout delivery reproduced"
reviewed: "2026-11-10"
baseline_source_revision: "444d618"
---

# Dockspace Platform Support

`dockspace` is the renderer-neutral product. An engine supplies its UI, renderer,
input, scheduling and platform resources; `egui_dockspace` and
`egui_dockspace_native` are reference consumers. Support below separates the
public host contract, adapter output ownership and actual platform qualification.
Implementing an API or publishing a capability bit does not certify physical
input on that backend.

This snapshot follows the [canonical engine-host plan](plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md)
and the dated [validation record](VALIDATION.md). The source-input capability
split extends baseline `8d43aee`; each physical or model record below identifies
its own source and scope. U9's remote-derived delivery qualification is complete;
the remaining unenabled platform profiles are capability limits.
Earlier dated qualifications
retain their historical scope; the 2026-10-31 entries supersede earlier
physical-smoke coverage limits.

The [2026-11-04 engine-first qualification](VALIDATION.md#2026-11-04-engine-first-breaking-redesign-qualification)
refreshes the public/egui/native update contracts and actual eight-pane editor
journeys against the frozen implementation inventory. It does not extend the
set of certified physical backends.

The [2026-11-10 architecture remediation](VALIDATION.md#2026-11-10-architecture-remediation-and-host-preparation)
qualifies the host-owned preparation interface, configuration before encoding
and independent focus/measurement contracts. Fresh Linux X11 renderer, bounded
redock and complete editor journeys pass; the platform limits below remain.

The [follow-up review](VALIDATION.md#2026-11-10-follow-up-ownership-and-interaction-review)
qualifies obsolete-output isolation, identity-scope sharing, finite geometry and
overflow-menu keyboard focus at `116917b`. Seven actual eframe test processes,
the bounded redock profile and complete editor journeys pass on Linux X11.

The [deep-layout and handoff review](VALIDATION.md#2026-11-10-deep-layout-and-endpoint-handoff-review)
qualifies stack-safe captions and borrowed-view diagnostics, fixed-capacity
endpoint handoff and touch-menu cancellation across discarded passes at
`b6fc6e5`. All fifteen serial gates, seven actual eframe processes, the 4 MiB
redock profile and complete editor journeys pass. These repairs retain the
platform scope below.

The [focus-provider and menu review](VALIDATION.md#2026-11-10-focus-provider-storage-and-menu-review)
qualifies bounded focus-provider storage and framework-owned overflow keyboard
focus at `e159073`. Final serial gates, seven actual eframe processes, the 4 MiB
redock profile and complete editor journeys pass. Twelve real focused edits, six
destroyed child lifetimes and Root rebinding preserve the original Root and end
quiescent. These results refresh the finite X11 profiles below.

The [core action and layout review](VALIDATION.md#2026-11-10-core-action-authority-and-split-normalization-review)
qualifies session-scoped transition identities, exact prepared Escape, measured
contained-resize authority and wide-split normalization at `4cb90d2`. All sixteen
serial gates, seven real eframe processes, the 4 MiB redock profile and complete
editor drivers pass. The original Root survives rebinding; all six Child windows
are destroyed and final quiescence holds. The finite platform scope below remains.

## Core and public engine-host contract

The [core lifecycle and persistence review](VALIDATION.md#2026-11-10-core-lifecycle-and-persistence-review)
qualifies exact JSON geometry, representable docking configuration, native
content-close scope and monotonic approval revocation, and current touch/scroll
targets behind retained outputs at `444d618`. All sixteen serial gates, seven
real eframe processes, the 4 MiB redock profile and complete editor drivers pass.
Twelve actual focused edits and six destroyed Child lifetimes finish quiescent;
the original Root survives rebinding. These repairs refresh the existing finite
X11 profiles.

| Boundary | Current contract and evidence | Host responsibility |
| --- | --- | --- |
| Core model | One item/root/surface graph, validated atomic mutations, policy, selection, layout, target acquisition, close and persistence identity. | Application pane data, fonts, drawing resources and preset storage. |
| Public runtime | `DockspaceSession`, rollbackable `DockspaceHostFrame` and owned `DockspaceUpdate` expose ordered input followed by measurement/encoding and one publication. `SurfaceWork::Deferred` expresses unavailable measurement or bounded output admission. | Register logical surfaces, supply complete facts and execute the requested work. Local/contained use does not require desktop coordinates or native enrollment. |
| Immediate or deferred rendering | A host may execute the ordered program immediately or compile it into owned display-list/retained-node records that outlive the frame borrow. | Preserve composition, clipping, content slots and the snapshot's hit/semantic identities. |
| Current-output authority | Complete sink acceptance yields an execution receipt; confirming it in a committed frame emits an exact affine output. A later committed acceptance of that output's terminal `Presented` result can promote current receiver authority. | Report the exact output as `Presented` or `Dropped` at the renderer boundary the host owns. Submission and traversal alone do not prove presentation or GPU scanout. |
| Ordinary logical release | Accepted drag/move/resize release commits in the successful update and emits delivery once. Its exact visible source and target are revalidated against the current relevant semantic scope. | Preserve event-time presented routes and receiver facts. A dropped output requests later rendering rather than undoing accepted ordinary input. |
| Optional native lifecycle | The same runtime accepts incarnation-bound window facts, effects, revocable Prepared readiness, recovery and retirement. Exact semantic first-live presentation gates source transfer. | Own OS windows/renderers and their preparation procedure, report current preparation and lifecycle facts, and retain terminal obligations. The egui hidden/show/render procedure stays inside its adapter. |

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
Prepared, host show dispatch and preparation output cannot transfer ownership by
themselves. See [`engine/native_projection.rs`](../crates/dockspace/src/engine/native_projection.rs),
[`engine/native_admission.rs`](../crates/dockspace/src/engine/native_admission.rs)
and the [R15 qualification](VALIDATION.md#2026-10-30-r15-first-live-ownership-replacement).

## Official egui reference adapter

The official adapter baseline is egui/eframe `0.37.0`. Its output capability
depends on who owns the run:

| Integration | Supported boundary | Limitation |
| --- | --- | --- |
| Owned `Dockspace::run_single_surface` | Holds the sole input candidate across actual repeat passes; validates terminal output after hooks before one publication. Returns complete `FullOutput` plus an affine `DockspaceOutputReceipt` and publication response. | Drawing must be synchronous and append-only on the supplied Ui layer. Exact shape/clip/transform spans are checked after output hooks. The renderer reports the complete output's actual result. Application callback side effects cannot be rolled back or automatically replayed. |
| Composed `egui_dockspace::run_ui` | Returns complete `FullOutput` plus an affine settlement receipt for callback outputs. | Each accepted callback publishes independently; a later run/hook failure cannot roll back that publication. Single-layer append-only proof and exact receiver clearance still apply. |
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
results do not qualify native physical Touch or successful native stationary
input. The limited automatic native scheduler is qualified separately below.

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
| X11 | Thirteen finite XTest target journeys; complete eframe/Glow stationary hover-selection and real InputOnly negative profiles, each with two fresh passes; a center-only high-rate profile; programmatic/output-pressure and deterministic lifecycle evidence. | Qualified only under the exact authority and environment below. Additional DPI, ancestry, backend or load profiles require separate evidence. |
| macOS | Real-window default/bounded programmatic tear-off, first-live, redock and retirement; six output-pressure cases. Same-window event-time facts have Winit test coverage, and local mouse geometry has deterministic producer/adapter coverage. | Programmatic and same-window fact evidence does not certify a physical cross-window release. `AuthoritativeButtonState` remains absent; cross-surface physical promotion stays fail-closed. Retina/mixed-DPI and the complete physical matrix remain unqualified. |
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
adds two and reruns all ten. The subsequent
[nested qualification](VALIDATION.md#2026-10-31-synchronous-native-host-service-and-nested-physical-targets)
adds the fixed center/west leaf cases and reruns all twelve. The subsequent
[overflow qualification](VALIDATION.md#2026-10-31-physical-overflow-insertion-and-retained-focus)
adds one clipped target and refreshes the full 15-entry matrix, including the
separate stationary-negative and load profiles:

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
| `nested-pane-center` | Non-guide Main content in the fixed two-axis layout | Center merge in the target leaf; empty central, ancestor weights and Sibling remain exact. |
| `nested-pane-edge` | Non-guide west basin of that Main leaf | Inner `[Child,Main]` split; ancestor weights, empty central and Sibling remain exact. |
| `overflow-tab-before` | Checked visible Main body after real backward scrolling in a fixed three-tab overflow strip | Exact before-Main insertion, `[Anchor, Child, Main]`, Child selected, destruction and quiescence. |

Pane points are normalized to current content. Physical rounding must stay in
the named inner-pane basin, outside the outer-root band and every actual guide
hit; the runner rechecks those facts before release. It cannot substitute
another point or a guide. Every successful journey checks held source input,
a painted Dock program before physical release, the requested model layout,
exact child X11 `DestroyNotify`, semantic viewport retirement and quiescence.
A viewport step is a progress barrier and a painted marker proves emission;
neither is a native release receipt or GPU presentation proof.

The 2026-11-01 source-input correction refreshes these fifteen journeys.
External Root content drags retain the window's native hit regions, including
complete-root payloads: application UI outside dockspace remains host-owned.
Partial Child payloads and roots with other main or contained presentations
also preserve input. Only a runtime-owned Child's complete sole main root,
without contained siblings, enables whole-window pass-through and retains its
exact enable/restore acknowledgement requirements. A preserving route still
requires current receiving-input facts and independent edge delivery/capture;
it cannot bypass an unsettled pass-through restoration. The physical runner
checks that no Root enable occurs throughout drag-out, redock and retirement,
while the complete Child still enables and restores input.

The internal `physical_cross_surface_drag(source_input)` and
`physical_native_drag(source_input)` queries distinguish these requirements
with `NativeDragSourceInput`. Hosts declare support through the public
`runtime::NativeHostCapabilities`; the internal platform module and placement
proof types are implementation details. Preserve does not depend on
`PointerHitTestControl`; PassThrough does. The mode comes from core's exact
ownership and payload decision, survives pending release and is retained in
the tear-off placement proof. Losing control cannot cancel a preserving route
by itself; it cancels a PassThrough route or pending release with the typed
capability reason while retaining its restore obligation. Independent input,
hit-observation and presentation requirements remain unchanged. This capability
split does not supply missing positive stationary OS facts.

Native desktop continuations now expose the frozen source requirement through
the read-only `source_input()` projection. Public hosts may report a newly
acquired capture boundary with `capture_acquired_event()`, actual sample time
and independent source delivery/capture facts. Core restarts stationary timing
without treating the acquisition as motion or authorizing its previous
deadline. This public projection does not expose the internal composed
capability queries or placement-proof constructors. No X11 capture operation
or positive stationary capability is enabled by that core contract alone.

Tab-half selection uses an opt-in passive diagnostic copied from the painted
tab's complete visible bounds; the Tab accessibility receiver still excludes
its close button. These diagnostics confer no input or presentation authority.
The original single fully visible target has no overflow chrome. The later
overflow fixture qualifies one partially clipped Main target after real manual
scrolling. The later finite stationary profile qualifies two automatic steps
and stopping after exit in that fixed strip; arbitrary overflow geometry remains
unqualified. The nested fixture preserves
Horizontal→Vertical ancestry and requires the additional Horizontal split only
inside the target leaf for west docking. Painted content/tab diagnostics confer
no target authority. The fractional source width (179.75 pixels) also exercises
platform request quantization without replacing observed geometry.

The recorded environment is Linux/arm64, Rust 1.95, Xvfb/Openbox, one X screen,
scale 1, a 2-CPU/4-GiB container and software GL with one renderer thread.
[`physical_x11/driver/scenario.rs`](../integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs),
[`driver/x11.rs`](../integration/egui-native-smoke/src/bin/physical_x11/driver/x11.rs)
and [`assertions.rs`](../integration/egui-native-smoke/src/bin/physical_x11/assertions.rs)
own the finite scenario, OS evidence and semantic checks. The roster/geometry
slice passed 50 Linux runner tests. Its macOS native all-targets gates each
passed 507 tests normally and with adapter assertions disabled, including two
separate 10,000-lifetime runs. Linux additionally passes the focused request regression
in both assertion configurations and the three request-conversion tests.
Core, product and official-egui results retain the `7003b13` qualification;
this adapter-only slice does not claim they were rerun.

The [continuous-input qualification](VALIDATION.md#2026-10-31-continuous-x11-input-and-pending-release-wake)
adds `EGUI_DOCKSPACE_PHYSICAL_LOAD=1khz` for explicit `pane-center-a/b/c` only.
It injects 1,000 distinct checked motions and releases immediately without an
inspection/idle drain. Guide and edge targets are rejected before the gesture.
The release-to-model measurement is a conservative upper bound beginning at
the native release callback. This finite profile does not certify hardware
1 kHz or the 2 ms reducer service boundary, maximum-roster/O256 cross-saturation,
arbitrary nested targets, mixed DPI or high-rate edge docking. The two fixed
nested targets reject the high-rate combination before starting a gesture.

### Configured-capacity model evidence

The [terminal-guard correction](VALIDATION.md#2026-10-31-terminal-guards-for-never-created-native-bindings)
extends the existing cleanup owner to proven never-created bindings. Binding
publication and Create delivery remain separate facts, so a published Pending
binding still retains its producer obligation when report backpressure delays
the effect. Cancellation, reset and replacement discard preserve exact-provider
quiescence without manufacturing Destroyed observations. The finite W4/A1,
E1, M4/C0/L2 close/create combination at that historical checkpoint passes on macOS and Linux normally and
without adapter debug assertions, under 4 MiB. It qualifies actual cancellation
and Failed candidate retry, exact per-owner successor promotion and full
reclamation in the coordinator model. It does not claim real OS close/create
or simultaneously fill every owner class.

The [single-Focus cleanup](VALIDATION.md#2026-11-01-removal-of-unreachable-root-close-and-duplicate-focus-queue)
removes the unreachable RuntimeOwned Root close effect and ordinary command
Vec. Focus dispatch now reads its existing sole owner directly; its exact
callback, failure and retirement budgets remain independent. The storage
constructor selects resources and close slots, with no command-slot argument.
Fresh native normal/noassert all-target gates each pass 570 including their
10,000-lifetime soaks; the mixed model measures one queued Focus under 4 MiB.
Older C0/C2 capacities remain dated evidence rather than current configuration.

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

The [synchronous output-host measurement](VALIDATION.md#2026-10-31-synchronous-native-host-service-and-nested-physical-targets)
additionally spans admission entry through return from the last real pass
finalizer, including commit, effect scheduling, repeated passes, lock release
and fixture bookkeeping. Fixed sample storage is separate from native input
capacity. Both optimized platform runs cover all six pressure cases and one
real repeated output in Combined. Observed Combined root maxima are 8.920083 ms
on macOS and 2.097750 ms on Linux, with over-2-ms counts kept in the results.
Renderer/GPU/terminal waits and actual OS-loop yield are excluded; no hard
real-time or sustained active-input claim follows.

### Native stationary observations

The public [`NativePointerContinuation`](../crates/dockspace/src/runtime/native/continuation.rs)
is a read-only request from the existing gesture owner. Explicit host-contract
tests cover timed work with fresh source binding, capture/delivery, coordinates,
time and exact target-output facts. A request or its expected source identity
cannot supply those observations.

The [native pointer observation prerequisite](VALIDATION.md#2026-10-30-native-pointer-observation-prerequisite)
can query a limited X11 desktop point/root-child hover with exact device/window
correlation. Ordinary and implicit-only queries retain Unknown capture and
delivery: `XIQueryPointer` does not identify a current grab owner, and retained
implicit capture cannot repair that gap. The later
[finite fresh sample](VALIDATION.md#2026-11-01-finite-fresh-x11-capture-sampling)
originally qualified viewable direct-root sources. The current
[observed-parent component](VALIDATION.md#2026-11-01-observed-parent-x11-capture-component)
also admits one root-observed WM parent. Known source capture/delivery require
an exact active explicit capture, a fresh successful G2 grab refresh, the original
press and finite parent/native-interval/FIFO checks; queued or intervening native
events reject the sample. The seam reports observations without
synthesizing motion. The adapter uses that result seam for finite Desktop Mouse
scheduling: deadlines request wakes, only final passes dispatch, and one query
keeps its existing command receipt until full ordered commit. Queries can
freeze the original Primary-press identity; admission rechecks it and sample
conversion requires two present equal identities. None/None cannot authorize
a sample. Identity-free observations still settle their original receipts,
and result/retirement keys retain the existing command identity. Unknown or
Unavailable results cannot continuously reschedule the same core token. The
[finite scheduling qualification](VALIDATION.md#2026-10-31-finite-native-stationary-scheduling)
passed native526/526 and the full bounded X11 negative journey at that checkpoint;
it did not qualify positive OS stationary execution. Physical Touch and
native-local OS handoff remain unqualified.

The [explicit-capture effect prerequisite](VALIDATION.md#2026-11-01-serialized-x11-capture-effects-and-exact-cleanup)
originally added a press-correlated X11 acquire/release operation for viewable
direct-root sources and exact enabled master pointers. It serializes ordinary application
grabs and keeps exact cleanup after revocation. Release invalidates the old
implicit owner and requires a new server-sync/FIFO boundary. `Acquired` reports
operation settlement only. The native adapter now connects capture acquisition,
its exact callback receipt and release to the existing gesture and resource row.
The public read-only capture requirement is available before finite timed work;
pending input temporarily hides it and does not prove cleanup. Pending acquire
defers old timed queries. A first fresh capture sample restarts the interval,
including a query prepared before acquisition settled. Failed unchanged samples
remain bounded. The integration passes its
[scoped qualification](VALIDATION.md#2026-11-01-native-capture-acquisition-and-ordered-release-integration);
the real backend test uses recording core receivers and simulated eframe receipt transport.
The subsequent [observed-parent component](VALIDATION.md#2026-11-01-observed-parent-x11-capture-component)
qualifies actual Openbox source/peer windows, foreign InputOnly interception and
ancestor-only loss/remap. Source history admits direct-root or one root-observed
parent; deeper source ancestry stays unavailable. Its 16 component gates pass.
Bounded hover uses only its terminal reply's point and child.

The complete [X11 stationary hover-selection journey](VALIDATION.md#2026-11-01-complete-x11-stationary-hover-selection-journey)
passes both positive and real InputOnly negative twice normally and once without
native assertions through
the actual eframe/Glow host, XTest input and exact first-live presentation. The
positive case observes Root hover independently from original Child
capture/delivery. Its first fresh sample leaves Main inactive; a distinct later
sample selects Main after the restarted interval while Primary stays held without
intervening motion/button input. The actually recompiled 100 ms early-dwell
control fails the fresh-interval assertion as intended.

Incomplete stationary observations revoke only their pending timed action;
they do not infer delivery or capture loss. Explicit None/Foreign/mismatched
delivery or capture owners still use the ordinary cancellation gates. The current bounded X11
negative journey uses an actual foreign InputOnly blocker and observes a genuine
query with the exact desktop point, `Unknown` or `Foreign` hover and independently
known Child capture/delivery. Main remains inactive through matching post-result
model observation and Repeat/slow-suffix pressure. After blocker removal, the
same held gesture resumes; both stationary profiles require exact redock,
Child destruction, viewport retirement and quiescence. These fixed three-tab
profiles retain their finite scope. The callback-following UI observation is not
independently an exact receipt-commit proof; core/adapter models cover ordered full commit.
All 36 final gates pass in
`/tmp/dockspace-wm-stationary-final-qualification.json`, including macOS native
workspace575, assertion-disabled libraries529 on macOS and Linux with the
10,000-lifetime soak, real windows/pressure and both-platform lint checks.

The [stationary overflow qualification](VALIDATION.md#2026-11-01-real-x11-stationary-overflow-steps)
adds one fixed 400-point strip with two real automatic steps, offset0/40/80.
Three bounded diagnostics bind stats to distinct exact Root Presented outputs;
each subsequent sample starts after the preceding terminal and a fresh configured
interval. The exit control stays at40 for two quiet intervals after the held
source UI observes real exit. Sticky query/capture checks remain through the
post-terminal source barrier. Both modes retain the original press/capture and
complete exact redock, destruction and quiescence. All 19 scoped final gates pass,
including twice with native assertions enabled and once disabled. Deeper source
ancestry, native-local/Touch OS input and macOS/Windows positive stationary
behavior remain unqualified.

The earlier finite-scheduling checkpoint also refreshed all twelve ordinary
physical targets, the separate 1 kHz entry, six pressure cases per platform and
both programmatic storage profiles. Linux library484/484 excluded the soak; the
macOS native normal/noassert runs each retained their separate 10,000-lifetime soak. Public
default/serde132/139, workspace1,897, product114 and official15 were rerun
for the core missing-fact semantic correction.

### Native content input and editor workflow

The eight-pane native `basic` example uses actual editable text, scrolling and
drawn content, with refreshed widget declarations and stable focus IDs. The
bounded X11 OS-input workflow checks four exact focused edits through native
first-live, Dock Back, a second child lifetime and system-close recovery,
including stable item identities, exact application text, destruction and
quiescence. Its initial content click follows 400 ms without input. These
results are separate from physical docking and do not certify macOS, Windows
or Wayland content input.

The [dated content-input record](VALIDATION.md#2026-10-31-native-content-input-and-bounded-editor-showcase)
records 61 final checks for a pre-input host admission hook and a coalesced
standalone native service event. This service settles original terminal
resources without running application logic/UI, consuming buffered input or
creating another semantic output. Captured callback age and ordering remain
unchanged. First-live and required lifecycle paints retain explicit wakes.
Physical targets use registered TextEdit content; completed-pass tests cover
Button/canvas mapping and overlay blocking separately. Final refreshed gate
results and source hashes are recorded in that entry.

The bounded application facade supplies close decisions, reopen and arrangement
locking. Its qualification is recorded separately in `VALIDATION.md`. The fixed
host surface cannot be removed by content close:
the facade reports `HostSurfaceRequired`, while individual-item closes can leave
an empty central dock area for reopening. Subsequent public Root continuation
and example qualifications cover replacing that host and restoring Default or
named presets after rebinding, preserving actual text edits and stable item
identities. Their [dated evidence](VALIDATION.md#2026-11-01-application-default-and-named-presets-after-root-continuation)
does not certify arbitrary native layout migration or the entire editor.
The subsequent stationary overflow/exit qualification completes the remaining
finite U10 journey. Additional stationary, geometry, owner/service and platform
combinations require their own evidence before those positive claims are enabled.

## Dependency reproducibility and open scope

Current native Git pins are aligned with the qualified sources and published
fork branches:

| Dependency | Qualified and published revision | Tracked Git revision |
| --- | --- | --- |
| egui/eframe fork | `f94ee58b3914de7ad71dab36919b329ef36b4035` (inspection component) | `f94ee58b3914de7ad71dab36919b329ef36b4035` |
| Winit fork | `2664589f0bd6fd03b2a5490f23bcd692853e63dd` | `2664589f0bd6fd03b2a5490f23bcd692853e63dd` |

The latest egui revision adds pending-child inspection paint scheduling over
`60c40304`. Its seven plugin tests and formatting pass locally. Current-head
physical X11 smoke is pending; the historical integration counts below retain
their original source scopes.

The tracked revisions are visible in the
[native workspace manifest](../integration/egui-fork-workspace/Cargo.toml) and
[smoke manifest](../integration/egui-native-smoke/Cargo.toml).
The [2026-10-30 Touch-fact record](VALIDATION.md#2026-10-30-native-touch-event-time-coordinate-pairs)
identifies its historical local revisions; the 2026-10-31 edge record retains
its older overrides and failing tracked-pin check. The preceding aligned pair
passed complete integration gates. The current pair passes component qualification
and all 36 final integration checks, including positive and real InputOnly negative
profiles twice normally and once without native assertions. Qualification uses
locked/offline Git sources and locally seeded objects, without path overrides.
That historical qualification is now followed by successful publication and
exact-SHA GitHub fetches in new empty repositories. Commits `785e51b`,
`1611316`, `996fd4c` and `542294b` align the three CI checkout refs, generate the
Winit source-test lockfile, install Wayland headers and select the actual monitor
test module. A detached clean checkout with separate initially empty Cargo Git
caches passes native and fork-command qualification;
all12 Git-family packages resolve the published pair without local object
seeding or path overrides. The separately owned workflow edits remain unstaged.

Validation references such as `/tmp/dockspace-physical-edge-qualification.json`
and `/tmp/dockspace-physical-load-final-qualification.json` identify local
artifacts recorded in `VALIDATION.md`; they are not portable checkout assets.
Current dated snapshots record native gates, programmatic windows, release
pressure, finite physical targets and measured active service; earlier results
retain their separate source scope. The extracted core/official-adapter archive
gate passes independently of these optional-native fork dependencies.

The qualified owner/service, editor and physical profiles remain finite.
Broader close/create/command combinations, arbitrary overflow/nested geometry,
mixed DPI, native-local/Touch OS input and macOS/Windows positive stationary
behavior require separate evidence before those positive claims are enabled.
They are capability limits, rather than an unlimited completion matrix.
The former `not our ref` publication blocker is resolved. Clean native builds,
default-feature normal/noassert libraries, framework/fact tests, corrected CI
module/list commands and five real X11 window journeys pass. This is local
CI-equivalent reproduction, not a GitHub Actions run; see the
[remote delivery record](VALIDATION.md#remote-fork-publication-and-clean-checkout-delivery).
The finite target, positive/negative stationary, active-service and center-load
results retain their recorded source scope.
