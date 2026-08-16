# egui 0.36 native fork seam admission

This document records why each remaining egui, eframe, or winit fork seam is
needed by the native docking vertical slice. It is an engineering admission
record, not a compatibility promise or an automated source-analysis gate.

The product contract remains simple:

- `dockspace` owns docking topology, lifecycle meaning, and authority.
- Forks expose graph-neutral facts or renderer terminal results only.
- Missing platform facts remain `Unknown` or `Unsupported`.
- A seam is removed when upstream exposes an equivalent typed fact, or when the
  native product no longer depends on that fact.

Pinned baselines:

- upstream egui/eframe `0.36.1`: `4c1f2fae95475a40e524884ebb298bcb1714b08e`
- public egui/eframe fork: `a3c5ee57f4158ef717b5a956bf0d5c4aeda186ed`
- upstream winit `0.30.13`: `e9809ef5`
- public winit fork: `180bfc09743586137fec014ef5543cdde56ce5d0`

## Admitted seams

| Seam | Missing upstream fact | Affected platforms | Minimal patch boundary | Checked evidence | Removal or upstream condition |
| --- | --- | --- | --- | --- | --- |
| Event-time pointer facts | A pointer event must carry its own surface-local position, optional desktop position, modifiers, hover route, and capture route. Callback-time cursor or window queries cannot reconstruct these facts. | All native platforms. Exact fields differ: unavailable facts remain absent. | Add graph-neutral fields to winit pointer events and observe the immutable event in eframe before egui-winit translation. No dock identity or receiver decision enters either fork. | winit platform translations plus `event_ordinals_preserve_cross_window_facts`, `event_ordinals_preserve_cross_window_wheel_facts`, and the native adapter's exact/unknown wheel translation tests. | Remove when upstream winit carries equivalent event-time facts and upstream eframe exposes an ordered pre-translation observer. |
| Completed-pass hit snapshot | Multipass egui can replace an earlier widget tree. A native receiver resolver needs the final pass's known-empty or known-hit result, not raw rectangles rebuilt by an adapter. | All egui renderers. | Add a read-only, completed-pass widget hit query to egui. It contains widget identity and hit semantics only, never dock topology. | `last_pass_hit_test_distinguishes_unknown_from_known_empty` and `last_pass_hit_test_uses_the_final_multipass_widgets`. | Remove when upstream egui exposes an equivalent final-pass hit-test snapshot or receiver query. |
| Ordered output token and terminal result | A generated `FullOutput` needs a context-local identity, generation order, producing viewport/window identity, admitted physical-create attempt, and a terminal `Presented` or `NotPresented` result. Callback order or `ViewportId` cannot substitute for exact physical incarnation. | WGPU and Glow native paths. | Mint an opaque token before the viewport UI callback, echo the admitted create attempt from the physical window state, keep texture ownership in eframe, and report one terminal renderer disposition after output handling. | `nested_outputs_use_completion_order_and_terminal_drop`, `deferred_output_echoes_the_exact_physical_create_attempt`, `output_settlement_is_idempotent`, `output_token_keeps_the_callback_window_identity`, and `output_tokens_expose_only_context_equality`. | Remove when upstream eframe provides an opaque per-output terminal callback with generation order, producing window identity, and physical-create incarnation. |
| Complete root viewport roster | One host boundary needs an exact live viewport roster rather than inferring liveness from callback presence. | All native platforms. | Capture the complete root roster at output start and attach it to the root output token. Child outputs never claim a complete roster. | `root_output_binds_one_complete_roster_to_the_active_token` and `deferred_output_never_claims_a_root_roster`. | Remove when upstream eframe exposes an atomic root viewport snapshot with exact membership. |
| Native window snapshot | Core must distinguish inner and outer physical rectangles, native scale, presentation scale, visibility, and minimized state. One rectangle or scale cannot stand in for another. | All native platforms; individual fields may be unavailable. | Capture independent optional facts at output start. Do not fill absent outer geometry or visibility from another field. | Root-roster tests exercise distinct inner/outer and native/presentation scale values. | Remove when upstream eframe provides the same independent typed viewport facts at a stable host boundary. |
| Deferred physical outer placement | Hidden staging needs a physical outer-rectangle request before native window construction. Logical or inner size is not an exact replacement. | Windows, macOS, and X11 when the backend can honor hidden placement; Wayland remains unsupported for global placement. | Allow the host to admit one exact child create attempt and supply an undecorated physical outer rectangle immediately before construction. Reject incompatible fullscreen, maximized, or monitor-targeted builders. | `deferred_window_preparation_is_exact_child_only_and_physical` and `deferred_admission_does_not_create_a_fake_failure_or_override`. | Remove when upstream eframe accepts exact physical outer placement for deferred viewports and reports inability as a typed result. |
| Hidden deferred rendering | Pre-show and post-show staging require UI and renderer output while the native child remains hidden. Upstream normally leaves hidden deferred viewports dormant. | Backends with a truthful hidden-window contract. Wayland is currently unsupported. | Add a host predicate for deferred children only; it does not show the window and does not bypass renderer settlement. Immediate viewports remain unavailable while the host seam is active. | `hidden_rendering_is_opt_in_and_never_applies_to_root` and `immediate_viewports_are_rejected_with_a_precise_error`. | Remove when upstream eframe supports hidden deferred rendering with a separate visibility contract. |
| Typed deferred-create failure | A logical viewport entry does not prove that the OS window and renderer were created. A viewport-only failure can arrive after replacement and target the wrong incarnation. | All native backends. | Eframe mints an opaque physical-create attempt before admission and reports `WindowUnavailable` or `VisibilityUnsupported` against that exact attempt. Do not expose OS handles or docking effects. | `viewport_create_failure_keeps_identity_and_wake`; the native adapter rejects a logical reservation without an admitted attempt. | Remove when upstream eframe exposes a terminal create result correlated to an exact physical attempt. |
| Visibility dispatch result | Dispatching `Visible(true/false)` is distinct from later observing actual visibility. Core needs to know whether eframe invoked a supported operation without treating that as visibility proof. | Windows, macOS, and X11 can dispatch; Wayland is currently typed unsupported. | Report `Dispatched` or `Unsupported` immediately after viewport command processing, then rely on a later window snapshot for the actual visibility fact. | `viewport_visibility_result_keeps_exact_identity_and_wake`. | Remove when upstream eframe reports typed visibility-command dispatch independently from visibility observation. |
| Pointer pass-through command result | Upstream applies `MousePassthrough` without an opaque command identity or typed terminal result. Matching only viewport, window, and value lets an unrelated same-value application command consume a core effect. | All native platforms; unsupported cursor hit-test control remains a typed terminal result. | Add one property-specific host command queue in eframe. It returns an opaque token, applies after ordinary viewport commands, and echoes the token with `Applied`, `Unsupported`, or `Failed`. Ordinary application commands remain callback-free. No generic command scheduler or dock identity enters the fork. | `pointer_passthrough_host_commands_keep_exact_tokens_and_viewport_order`, `viewport_pointer_passthrough_callback_reports_exact_command_result`, and the native adapter's same-value wrong-token regression. | Remove when upstream eframe exposes a correlated pointer hit-test command result, or when upstream winit provides an equivalent typed request/result API. |
| Work-area roster | Outside-all placement needs the selected display's usable work area and scale. Monitor bounds alone do not prove a work area. | Windows and macOS can be exact. X11 lacks the required complete EWMH mapping in this fork; Wayland has no global placement authority, so both remain `Unknown`. | Capture a complete root-level roster. Each record keeps opaque display identity, full display bounds, usable work-area bounds, and scale. Any incomplete platform capture produces `Unknown`, never a partial exact roster. | Host-roster coverage verifies exact and unknown roster transport. Platform-specific collection tests and the native outside-all route test remain required before cross-platform native support is admitted. | Remove when upstream winit or eframe exposes a complete, typed work-area roster with honest platform support. |

## Explicit non-seams

The forks must not acquire any of the following:

- dock graph, node, root, item, surface, or drop-target identities;
- provider leases, reducer receipts, scene revisions, or lifecycle state machines;
- a hosted-cycle transaction protocol or a second effect/presentation ledger;
- test-only output-token constructors used to simulate the product lifecycle;
- source parsing, ABI provenance, digest gates, or scenario-runner logic.

Deterministic Rust tests cover lifecycle and failure matrices in the core and
native adapter. One ordinary executable now proves the real event loop,
two-window creation, staging, first-live admission, programmatic redock, and
retirement. It deliberately does not simulate physical cross-window pointer
input and is not a reusable E2E framework.
