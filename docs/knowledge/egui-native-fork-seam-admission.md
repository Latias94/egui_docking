# egui 0.37 native fork seam admission

This document records why each remaining egui, eframe, or winit fork seam is
needed by the native docking vertical slice. It is an engineering admission
record, not a compatibility promise or an automated source-analysis gate.

The product contract remains simple:

- `dockspace` owns docking topology, lifecycle meaning, and authority.
- Forks expose graph-neutral facts or renderer terminal results only.
- Missing platform facts remain `Unknown` or `Unsupported`.
- A seam is removed when upstream exposes an equivalent typed fact, or when the
  native product no longer depends on that fact.

The publishable crates.io adapter does not depend on the native post-output
fork seam. A caller-owned raw-egui integration uses `egui_dockspace::run_ui`
and splits the returned complete output with `into_parts()`. Its affine receipt
reports the renderer's exact `Presented` or `Dropped` result through the same core
emission path used by engine and native hosts. Complete surface shape spans are
checked after output hooks, including ordinary geometry, clipping and transforms.
This conservative mode requires append-only synchronous pane painting on the
supplied Ui layer; cross-layer writes are unavailable. Private markers are only
output-integrity checks, not GPU or presentation proof. Once split, the host must
preserve the complete output it reports. Unwind and receipt drop retire outputs.
An unmodified eframe callback remains capability-negative because it does not own
the full renderer boundary. The native host still needs its fork seam to bind
renderer output tokens and physical viewport obligations at that same boundary.

Baselines:

- upstream egui/eframe `0.37.0`: `841a328ba2dd8f6996beb7189ebb7fc00e668a2a`
- public egui/eframe fork: `0716beee5b7284389f2e80ffb6b24b5af6ae2910`
- upstream winit `0.30.13`: `e9809ef5`
- public winit fork: `f378b50f29cafafec31cd8675c17646b4ef57d70`

The 0.37 branch merges upstream stable `0.37.0` into the minimal native-seam
branch. It preserves the same Winit event-time revision. No old hosted-cycle
or egui_tiles runtime is reintroduced. The migration adapts upstream root UI,
pass modes, screenshot callbacks, repaint observers, and widget/paint types.
Offscreen screenshots may finish without a surface texture, but only actual
surface presentation yields a native `Presented` result.

Egui 0.37 adds elastic trackpad/momentum consumption. Completed-pass scroll
queries freeze the relevant elastic state; where input source is missing and
Wheel and Trackpad could consume differently, the query returns `Unknown`.
It must not publish known-empty or select a product receiver behind that area.
Five real ScrollArea tests cover top/bottom edges, nested overscroll, momentum
reservation, frozen state and candidate order, and non-consuming areas.

### Touch position and geometry provenance

Touch events carry the existing PointerEventFacts rather than a second routing
schema. Wayland down/motion freezes a complete physical-position/content-geometry
pair under one window lock. Its original per-seat touch map retains a small Copy
sample and motion replaces it as one value. Up/cancel carries that last pair,
because those protocol events have no new coordinate. The shared event
constructor takes only the saved sample, contact ID and phase; terminal paths
cannot rescale an old point using current window geometry. A missing window in
cancel skips that contact while allowing the remaining contacts to finalize.

The adapter validates contact position against the facts and existing local
coordinate capability. A failed sample remains LocalUnknown, including terminal
events, so the real contact can end without a new position-based mutation.
Backends without original geometry retain DesktopUnknown and default facts.
This does not establish native capture or desktop hover, synthesize a contact
termination, enable a handoff, or qualify automatic Touch stationary behavior.
The deterministic producer/adapter tests and physical-evidence limits are
recorded separately in `docs/VALIDATION.md`.

Remove this fork change when upstream Touch carries an equivalent immutable
position/geometry pair and preserves the pair through coordinate-less terminal
events. The renderer-neutral core requires no Winit Touch type.

### Correlated native pointer observation prerequisite

This local, unpublished seam prepares future deadline-driven input without
introducing docking types, gesture tokens, an input queue or a capture registry
in the forks. `prepare_native_viewport_pointer_observation` reserves a typed
command identity before enqueueing into the existing property queue. The result
and existing property-retirement callback echo the viewport, expected physical
window and exact native device. Execution checks the actual window first;
an obsolete request cannot query its successor. Glow and WGPU pass the active
event loop where available; an immediate path without it finishes Unavailable.
No motion event is generated, and eframe does not retry a terminal result.

Winit's X11 query rejects native-event expansion and pending events before or
after its synchronous replies. A single root XIQueryPointer reply provides the
server-logical desktop point and direct root-child hover. Known direct Winit
children identify only that subtree, not embedded descendants. A WM or foreign
frame remains Unknown. Local position, geometry, modifiers, capture and delivery
are not inferred. A retained implicit capture does not prove current ownership:
ancestor unmap/remap and queued pre-command press events defeat that inference,
and XIQueryPointer does not identify a grab or protocol freeze. Traversing a
mutable external window tree with multiple queries also cannot prove one atomic
hover observation, so this seam does not do so.

The adapter does not enroll automatic native stationary scheduling from these
facts. It treats unowned observation retirements as Unreserved. A future host
must reserve a bounded request owner, preserve raw/output ordering and sample
time, keep core continuation identity outside the fork and require complete
native facts before recording a continuation. Unsupported backends remain
Unsupported; Unknown capture never inherits the expected source identity.
The public core's explicit fact-based stationary contract remains independent.

This seam can be removed when upstream offers equivalent correlated observation
and retirement boundaries, or when the native validation adapter no longer
requires active querying. Component tests and their real-X11 limits are recorded
in `docs/VALIDATION.md`; they do not certify automatic stationary behavior.

### Local close-hold prerequisite (not yet pinned)

The local egui fork adds neutral close ownership in
`b4ec9ff8bc719321442cf421fc7f5a496b80154d`. This commit is not pushed and is not
the native workspace's dependency revision. The published/pinned baseline above
remains unchanged. The working adapter consumes this API under explicit local
Cargo overrides, but this is not a remotely reproducible native dependency or a
completed budgeted native-driver migration.

Buffered input cannot use `CancelClose` as a scheduling hold: it would consume
the real request and publish a policy cancellation before core decided. Instead,
`NativeHostHandler::on_window_event` returns `NativeWindowEventDisposition`.
An exact `HoldClose` claims the callback's canonical attachment/window/event
request before backend input translation. Repeated pending OS close events keep
the same identity. The existing close ledger owns both the hold and its optional
final decision; there is no second close FIFO or docking policy in the fork.

`resolve_native_viewport_close` accepts only that held request on its attached
egui context. Root Allow requests application shutdown; child Allow releases the
gate while the host remains responsible for child omission/retirement. Only Veto
produces the original cancellation callback. Acceptance is not physical-close
proof. Ordinary Close/CancelClose cannot resolve a held request. Full UI and
logic-only updates share these semantics, including resolution from an input hook
before the first UI callback. An unclaimed close retains ordinary eframe behavior.

Both renderer input captures consume held Close from their original ViewportInfo,
not just a temporary RawInput clone. This prevents a root event from reappearing
after a child update resolves its Veto. Glow recreation now revokes the old
request and exact bidirectional mappings before dropping the old Window Arc or
requesting replacement admission. Window/viewport retirement and attachment drop
invalidate the old request without inventing cancellation. Resizing or invalidating
presentation alone does not end a held close.

The local qualification includes 69 Glow, 63 WGPU, 69 combined-renderer and 69
debug-assertions-disabled library tests. Real EpiIntegration Full/LogicOnly calls
exercise deferred decisions; the Glow recreation test uses renderer-owned mapping
state and a real deferred-admission callback without constructing an OS window.
These are framework/renderer-model results, not OS, physical input or GPU proof.
See `docs/VALIDATION.md` for review findings, fixes and exact remaining scope.

The adapter retains the complete opaque request in the callback's original
window-event record. Typed Allow/Veto uses the active UI context rather than a
cached context or a second command queue. Cancellation callbacks keep only the
complete token; the pending close owner supplies its original binding. An exact
callback is never redirected through the current viewport mapping. Missing
tokens, wrong contexts, or changed window routes fail closed without ordinary
Close/CancelClose fallback. Model fixtures deliberately carry no fork authority.
The public two-counter close-progress snapshot distinguishes retained input from
committed cancellation and lets the real WM-close smoke reject initially idle
false positives.

The working adapter now lends one dispatch budget from output begin through
Repeat/finalization, staging, close and shutdown, ending it at the physical output
terminal. Poll yields retain the original held request; they do not synthesize
CancelClose. Policy submissions and exact close decision plans resume on the
existing pending close owner. This remains locally qualified work against the
unpublished fork: tracked dependency pins, real-window journeys, input caps and
overload/latency qualification are not complete.

### Local property-command identity prerequisite (not yet pinned)

Local fork commit `7c7b3233541ec724bee561454ba8e9fca384f32e` adds correlated
host-owned focus and visibility commands and joins their transport with pointer
pass-through. Ordinary application commands and builder deltas never emit typed
results. The attachment owns this narrow property queue; native operation and
callback occur together in per-viewport enqueue order. Results retain dispatch
meaning only: Requested focus needs a subsequent focus observation, and a
visibility dispatch does not prove visibility or presentation.

Scheduling requests repaint without sending ordinary property companions.
Glow/WGPU logic-only dispatch explicitly includes pending typed targets, so a
hidden window needs no forced UI pass and no stale ordinary operation can
survive typed queue retirement. Window/viewport and attachment retirement clear
pending typed commands. Tests cover ordinary same-value/builder isolation,
cross-property order, missing targets/handlers, attachment changes and saved
FullOutput/LogicOutput command replay against a successor. Combined renderer
library tests pass 75 cases normally and 75 without debug assertions. The
adapter's exact receipt tests and three real macOS redock runs are recorded in
`docs/VALIDATION.md`. This commit is local and unpublished; it does not change
the dependency baseline or complete U7/U8 qualification.

Local follow-up `1f7c541fde99fd69192622a1a36cb642de58ed29` reports exact
pre-dispatch retirement for queued focus, visibility and pointer pass-through
commands. Window/viewport forgetting removes only the matching old queue and
notifies the handler in its original order after releasing the host locks.
Reentrant callbacks may enqueue successor work without having it removed by
the earlier cleanup. The receipt carries the original opaque token and requested
value; it does not claim an OS operation, focus observation or presentation.
The adapter settles this outcome through the same command owner as a dispatched
result, so retirement cannot leave an Awaiting reservation permanently live.

The follow-up passes 63 host-seam tests and a WGPU build. The adapter passes 271
all-target cases normally and 271 with native debug assertions disabled, plus
programmatic macOS redock. It remains local and unpublished; tracked pins are
unchanged. Remove the retirement callback together with the typed property queue
when upstream supplies equivalent exact execution-or-retirement results. No
docking identity, cancellation policy or second lifecycle owner belongs in this
seam.

### Local output-admission prerequisite (not yet pinned)

Local fork commit `af92097942f551914644f9d36f73dfba948e4997` makes
`NativeHostHandler::on_output_begin` return
`NativeOutputAdmission::Proceed | Defer`. This is a transport admission decision,
not docking or placement policy. Glow and WGPU ask before extracting buffered
egui-winit input and running UI. A deferred candidate spends no completion
ordinal, active output scope, finalizer or terminal callback. The framework
retains input and prior framebuffer state and schedules its existing root wake
after the current native event. Ordinary rendering without a host is unchanged.

The adapter reserves the exact accepted output owner before Proceed. Its
prepaid root and child control/staging storage lets accepted obligations finish
when ordinary capacity is frozen; a busy slot returns Defer without a second
rejected-token registry. Nested deferral preserves the outer token, and later
accepted child/root completion remains contiguous. Remove this result when
upstream eframe exposes equivalent pre-input output admission with truthful
defer/terminal ownership. Local model tests do not establish physical keyboard
input retry or the adapter's complete memory budget. Tracked pins remain the
published baseline above until an explicit publication step.

The complete eframe library passes 81 Glow tests, 75 WGPU tests and 81 combined
renderer tests without eframe debug assertions. Both renderer-only configurations
also compile with this seam disabled. Adapter qualification passes 339 native
tests in normal and assertion-disabled builds and the programmatic macOS redock
journey. These remain local, unpublished-fork results.

## Admitted seams

| Seam | Missing upstream fact | Affected platforms | Minimal patch boundary | Checked evidence | Removal or upstream condition |
| --- | --- | --- | --- | --- | --- |
| Event-time pointer facts | A pointer event must carry its own surface-local position, optional desktop position, modifiers, hover route, and capture route. Callback-time cursor or window queries cannot reconstruct these facts. | All native platforms. Exact fields differ: unavailable facts remain absent. | Add graph-neutral fields to winit pointer events and observe the immutable event in eframe before egui-winit translation. No dock identity or receiver decision enters either fork. | winit platform translations plus `event_ordinals_preserve_cross_window_facts`, `event_ordinals_preserve_cross_window_wheel_facts`, and the native adapter's exact/unknown wheel translation tests. | Remove when upstream winit carries equivalent event-time facts and upstream eframe exposes an ordered pre-translation observer. |
| Ordered device removal | Per-window callbacks cannot prove when a raw physical device disappears relative to button, scroll, touch, and capture records. Winit also states that raw-device IDs may not match the virtual IDs carried by window events, so targeting one window-input identity would invent correlation. | All native platforms. | Eframe forwards only a typed `NativeDeviceRemoval { ordinal }` before handling `DeviceEvent::Removed`. The adapter atomically terminates its complete input roster; the seam exposes neither the uncorrelated raw ID, raw motion, nor docking state. | `device_removal_callback_preserves_global_native_event_order` plus the native adapter's multi-contact, fixed-virtual-ID/master-device mismatch, complete-roster cancellation, tail-event, and ID-reuse tests. | Remove when upstream exposes an ordered removal boundary plus a proven raw-to-window device correlation, or an equivalent virtual-input reset callback. |
| Completed-pass hit snapshot | Multipass egui can replace an earlier widget tree. A native receiver resolver needs the final pass's known-empty or known-hit result, not raw rectangles rebuilt by an adapter. | All egui renderers. | Add a read-only, completed-pass widget hit query to egui. It contains widget identity and hit semantics only, never dock topology. | `last_pass_hit_test_distinguishes_unknown_from_known_empty` and `last_pass_hit_test_uses_the_final_multipass_widgets`. | Remove when upstream egui exposes an equivalent final-pass hit-test snapshot or receiver query. |
| Completed-pass scroll routing | Ordinary hover does not reveal which nested `ScrollArea` accepts one modifier-projected scroll edge, whether it is saturated, or whether a locked smooth sequence still owns the frontmost receiver. | All egui renderers. | Retain ordered, clipped scroll candidates in the final pass. Product candidates use opaque widget identities; framework `ScrollArea` records reuse its live direction, multiplier, offset, and saturation checks. The query is read-only and never exposes dock identities. | `last_pass_scroll_hit_distinguishes_unknown_from_known_empty`, `last_pass_scroll_hit_uses_the_final_multipass_candidates`, `nested_scroll_hit_respects_direction_and_saturation`, `smooth_scroll_begin_is_blocked_by_any_scroll_area_capacity`, `locked_scroll_hit_does_not_retarget_and_framework_scroll_blocks`, and the native adapter's exact delivery-query test. | Remove when upstream egui exposes an equivalent final-pass scroll receiver query with nested direction, saturation, and locked-owner semantics. |
| Host-owned post-output pass finalization | An ordinary output hook registered after native Dockspace can still request another pass or unwind. The native host needs the exact ended viewport, frozen repeat/terminal decision, and active output token only after every ordinary output hook succeeds. | All egui renderers used by the fork-backed native host. | Let the integration which owns the complete egui run install one finalizer outside the plugin list. Egui captures the ended viewport before restoring its parent and freezes `Repeat` or `Terminal` against the pass limit; eframe pairs that fact with the active opaque output token. No plugin runs afterward, and any unwind terminates that exact token as `NotPresented`. | `output_pass_settlement_reports_terminal_root_run`, `manual_end_pass_settles_once_for_the_ended_viewport`, `non_root_run_reports_the_ended_child_viewport`, `late_discard_is_frozen_before_output_pass_settlement`, `late_output_hook_panic_skips_output_pass_settlement`, `denied_late_discard_settles_the_max_pass_as_terminal`, `host_finalizer_runs_after_all_output_hooks`, `output_hook_panic_skips_host_finalizer`, `host_pass_finalizer_receives_the_active_output_token`, `unwinding_output_scope_reports_an_exact_not_presented_tombstone`, and the native adapter's late-discard/max-pass tests. | Remove when upstream egui and eframe expose an equivalent host-owned finalizer with the exact ended viewport, frozen repeat decision, active output identity, and unwind tombstone. |
| Ordered output token and terminal result | A generated `FullOutput` needs a context-local identity, generation order, producing viewport/window identity, admitted physical-create attempt, and a terminal `Presented` or `NotPresented` result. Callback order or `ViewportId` cannot substitute for exact physical incarnation. | WGPU and Glow native paths. | Mint an opaque token before the viewport UI callback, echo the admitted create attempt from the physical window state, keep texture ownership in eframe, and report one terminal renderer disposition after output handling. | `nested_outputs_use_completion_order_and_terminal_drop`, `deferred_output_echoes_the_exact_physical_create_attempt`, `output_settlement_is_idempotent`, `output_token_keeps_the_callback_window_identity`, and `output_tokens_expose_only_context_equality`. | Remove when upstream eframe provides an opaque per-output terminal callback with generation order, producing window identity, and physical-create incarnation. |
| Retained framebuffer status overlay | A fatal or quarantined native host must keep an exact valid framebuffer visible while showing a non-interactive status strip; a first frame, resized surface, replacement window, or invalidated presentation must fail closed to `NoFrame`. | WGPU and Glow native paths. | The host declares retain-with-overlay intent only. Eframe admits it only when semantic and renderer ledgers agree on viewport, window, physical size, and presentation generation; Glow skips clear and WGPU uses `LoadOp::Load`. The overlay still reports `NotPresented`, suppresses executable platform actions, preserves only its inert status-label accessibility, and cannot mint semantic presentation authority. | `retained_overlay_requires_an_exact_renderer_presentation`, `semantic_result_without_renderer_submission_cannot_enable_retention`, `ordinary_retain_skips_renderer_paint`, `deferred_clear_runs_when_early_clear_was_skipped_or_hidden`, `retained_overlay_loads_the_existing_color_attachment`, `fatal_overlay_uses_only_a_bottom_status_strip`, `retained_overlay_keeps_an_inert_accessible_status_label`, and `only_a_skipped_retain_omits_the_accessibility_update`. | Remove when upstream eframe exposes an equivalent renderer-proven retained overlay whose terminal result remains non-semantic. |
| Complete root viewport roster | One host boundary needs an exact live viewport roster rather than inferring liveness from callback presence. | All native platforms. | Capture the complete root roster at output start and attach it to the root output token. Child outputs never claim a complete roster. | `root_output_binds_one_complete_roster_to_the_active_token` and `deferred_output_never_claims_a_root_roster`. | Remove when upstream eframe exposes an atomic root viewport snapshot with exact membership. |
| Native window snapshot | Core must distinguish inner and outer physical rectangles, native scale, presentation scale, visibility, and minimized state. One rectangle or scale cannot stand in for another. | All native platforms; individual fields may be unavailable. | Capture independent optional facts at output start. Do not fill absent outer geometry or visibility from another field. | Root-roster tests exercise distinct inner/outer and native/presentation scale values. | Remove when upstream eframe provides the same independent typed viewport facts at a stable host boundary. |
| Deferred physical outer placement | Hidden staging needs a physical outer-rectangle request before native window construction. Logical or inner size is not an exact replacement. | Windows, macOS, and X11 when the backend can honor hidden placement; Wayland remains unsupported for global placement. | Allow the host to admit one exact child create attempt and supply an undecorated physical outer rectangle immediately before construction. Reject incompatible fullscreen, maximized, or monitor-targeted builders. | `deferred_window_preparation_is_exact_child_only_and_physical` and `deferred_admission_does_not_create_a_fake_failure_or_override`. | Remove when upstream eframe accepts exact physical outer placement for deferred viewports and reports inability as a typed result. |
| Hidden deferred rendering | Pre-show and post-show staging require UI and renderer output while the native child remains hidden. Upstream normally leaves hidden deferred viewports dormant. | Backends with a truthful hidden-window contract. Wayland is currently unsupported. | Add a host predicate for deferred children only; it does not show the window and does not bypass renderer settlement. Immediate viewports remain unavailable while the host seam is active. | `hidden_rendering_is_opt_in_and_never_applies_to_root` and `immediate_viewports_are_rejected_with_a_precise_error`. | Remove when upstream eframe supports hidden deferred rendering with a separate visibility contract. |
| Typed deferred-create failure | A logical viewport entry does not prove that the OS window and renderer were created. A viewport-only failure can arrive after replacement and target the wrong incarnation. | All native backends. | Eframe mints an opaque physical-create attempt before admission and reports `WindowUnavailable` or `VisibilityUnsupported` against that exact attempt. Do not expose OS handles or docking effects. | `viewport_create_failure_keeps_identity_and_wake`; the native adapter rejects a logical reservation without an admitted attempt. | Remove when upstream eframe exposes a terminal create result correlated to an exact physical attempt. |
| Focus dispatch result | Ordinary Focus commands can run after a newer host request is retained; matching only a window could consume that request with an older callback. Dispatch alone also cannot prove global focus. | All native platforms; OS focus restrictions remain effective. | `queue_native_viewport_focus` returns an opaque token, performs the native operation, and echoes `AlreadyFocused` or `Requested` for that exact command. Ordinary Focus has no typed callback. Requested still waits for a subsequent real focus observation. | `ordinary_same_value_and_builder_commands_never_claim_typed_tokens`, cross-property ordering, and the adapter's delayed foreign-token and requested/global-focus regressions. | Remove when upstream eframe exposes equivalent correlated focus dispatch and separately ordered native focus observations. |
| Visibility dispatch result | An ordinary same-value Visible command can be processed after a later host request acquired an owner. Route and value alone cannot identify its result; dispatch is also distinct from actual visibility. | Windows, macOS, and X11 can dispatch; Wayland is currently typed unsupported. | `queue_native_viewport_visibility` returns an opaque token and independently executes the typed operation after ordinary commands. It echoes that token with `Dispatched` or `Unsupported`. Ordinary commands and builder deltas have no typed result. A later window snapshot remains the visibility authority. | `viewport_visibility_result_keeps_exact_identity_and_wake`, property-queue isolation/order tests, and the adapter's `delayed_same_value_visibility_callback_cannot_claim_a_later_show_request`. | Remove when upstream eframe reports exact correlated visibility-command dispatch independently from visibility observation. |
| Pointer pass-through command result | Upstream applies `MousePassthrough` without an opaque command identity or typed terminal result. Matching only viewport, window, and value lets an unrelated same-value application command consume a core effect. | All native platforms; unsupported cursor hit-test control remains a typed terminal result. | Use the attachment-owned viewport property queue shared only by focus, visibility and pointer hit testing. Each typed operation executes in enqueue order after ordinary commands and echoes its opaque token with `Applied`, `Unsupported`, or `Failed`. Ordinary application commands remain callback-free. No generic command scheduler or dock identity enters the fork. | `pointer_passthrough_host_commands_keep_exact_tokens_and_viewport_order`, `viewport_pointer_passthrough_callback_reports_exact_command_result`, and the native adapter's same-value wrong-token regression. | Remove when upstream eframe exposes a correlated pointer hit-test command result, or when upstream winit provides an equivalent typed request/result API. |
| Work-area roster | Outside-all placement needs the selected display's usable work area and scale. Monitor bounds alone do not prove a work area. | Windows and macOS can be exact. X11 is exact only on the first X screen when one identity-transformed active CRTC covers the root and the EWMH/XSettings authorities are complete and self-consistent. Multi-CRTC X11 and Wayland remain `Unknown`. | Capture a complete root-level roster. Each record keeps opaque display identity, full display bounds, usable work-area bounds, and scale. X11 derives the snapshot from the event-loop connection and invalidates it on root, supporting-WM, RandR, or XSettings changes. Any incomplete platform capture produces `Unknown`, never a partial exact roster. | Host-roster coverage plus `accepts_exact_current_desktop_and_clips_each_monitor`, `rejects_unadvertised_or_unverified_wm_authority`, `rejects_inexact_desktop_arrays_and_nonzero_current_viewport`, `non_identity_crtc_transform_cannot_publish_scalar_scale_authority`, and the X11/XTest physical smoke. | Remove when upstream winit or eframe exposes a complete, typed work-area roster with honest platform support. |

macOS currently remains fail-closed for cross-surface physical promotion. Its
event-time same-window facts are covered by the pinned Winit tests, but the
repository does not advertise `AuthoritativeButtonState` until a real OS-input
lane proves the complete cross-window release contract. Windows keeps
`HoveredWindow` and `DesktopPointerPosition` unavailable until equivalent
platform-fact tests exist. X11 is the physical reference backend. The 0.37
revision must pass its real-window and physical CI lanes before the earlier
0.36 reference evidence can become a current-baseline certification claim.

## Explicit non-seams

The forks must not acquire any of the following:

- dock graph, node, root, item, surface, or drop-target identities;
- provider leases, reducer receipts, scene revisions, or lifecycle state machines;
- a hosted-cycle transaction protocol or a second effect/presentation ledger;
- test-only output-token constructors used to simulate the product lifecycle;
- source parsing, ABI provenance, digest gates, or scenario-runner logic.

Deterministic Rust tests cover lifecycle and failure matrices in the core and
native adapter. One ordinary executable proves the real event loop, two-window
creation, staging, first-live admission, programmatic redock, and retirement.
A separate bounded X11/XTest executable proves real pointer drag-out, exact
Center-guide drag-back, OS-window destruction, and quiescence through the
production Winit journal. Its inspection channel is read-only and private; it
does not construct targets or form a reusable E2E framework.
