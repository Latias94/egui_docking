---
title: "Dockspace Interaction Module Inventory"
status: "Source-reviewed; qualification scoped"
reviewed: "2026-10-31"
source_revision: "13dd12747b600d7c92921effa8cf521bfd120359"
---

# Dockspace Interaction Module Inventory

This inventory describes production ownership at `13dd127` and the W16
model-test evidence committed as `9140b18` on 2026-10-31.
`dockspace` is the renderer-neutral product; engine hosts own their drawing,
input facts, scheduling and platform resources. The egui adapters consume the
same public runtime and never own another docking graph.

The [canonical engine-host plan](../plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md)
governs the work. Dated [validation records](../VALIDATION.md) supply qualification;
this inventory is a source review, not a fresh runtime test or an overall U9
completion claim. [Platform support](../PLATFORM_SUPPORT.md) separates public
host contracts, official-egui output limits and finite native evidence.

## Core semantic and lifecycle owners

Paths in this section are under [`crates/dockspace/src/`](../../crates/dockspace/src).

| Module or owner | Responsibility and boundary | Explicit non-ownership |
| --- | --- | --- |
| `workspace.rs`, `graph.rs`, `operation.rs`, `transaction.rs` | Sole durable item/root/surface topology and checked atomic mutations. `model/` exposes stable views and prepared commands. | No renderer widgets, OS resources or adapter graph mirror. |
| `runtime/session.rs`, `runtime/host_frame.rs`, `engine/host_frame.rs` | `DockspaceSession` owns the engine. One affine host-frame candidate joins commands, measurements, input and observations; complete commit publishes it, drop/failed commit rolls it back. | No per-widget reducer or callback-order inference of missing surface facts. |
| `pointer_journal.rs`, `pointer_journal/ledger.rs`, `backend_ingress.rs`, `runtime/native/input_batch.rs` | Ordered pointer streams, scope, sequencing, causal prefixes and retryable recorded native input. Active edges and authority barriers retain their order. | No adapter-selected semantic target or timer-owned gesture. |
| `interaction.rs`, `engine/pointer_transaction.rs`, `engine/pointer_session.rs` | Existing interaction state owns arming, source, admitted lane, capture gates, release barrier, cancellation and terminal result. Contained and splitter handlers in `engine/pointer_contained.rs` and `engine/pointer_splitter.rs` reduce through that owner. | No independent local-response, contained or native terminal gesture machine. |
| `runtime/direct_manipulation.rs`, `target_field.rs`, `drop_resolver.rs` | Core acquisition, latch/hysteresis, deterministic target priority and preview selection. | A framework receiver answer cannot choose a replacement target after rejection. |
| `engine.rs` source-continuation checks, `engine/pointer_proof.rs` | A frozen `SceneGestureContinuation` preserves the exact active source through selection, raising or live geometry invalidating its originating scene. Workspace, policy, configuration, requirements, popup and owner changes are revalidated. | Source-coordinate continuity cannot authorize stale hover, a new receiver, or another gesture. |
| `interaction/timed.rs`, `engine/pointer_timed.rs`, `engine/pointer_touch.rs`, `engine/pointer_edge_scroll.rs`, `tab_strip.rs` | Finite stationary work belongs to the same gesture and existing overflow state. Public `runtime/pointer_continuation.rs` and `runtime/native/continuation.rs` project requests and exact-output waits. | A deadline/token is scheduling data, not fresh capture, time, position or receiver evidence. No second token issuer or native polling registry. |
| `scene_compiler.rs`, `runtime/paint.rs`, `runtime/paint/` | Compile one ordered hierarchical `SurfacePaintPlan` with geometry, content slots, clip/hit/semantic identities, previews and source gaps. `execute_surface_program` yields complete sink-acceptance evidence. | Traversal/receipt is not renderer submission or presentation proof. Engines may copy records into their own retained output. |
| `runtime/presentation.rs`, `presentation_observation.rs`, `engine/presentation_authority.rs` | One exact emission/output pipeline. `PaintedSurfaceOutput` carries an affine obligation; committed terminal observations select/retire exact artifacts and promote current interaction authority. | No public preview-only ACK, shape marker or cumulative frame counter can promote an output. |
| `frame/native_create.rs`, `frame/native_bringup.rs`, `engine/native_projection.rs`, `engine/native_admission.rs` | One pending native create/preparation owner and first-live barrier. `DirectPrepared` and `Staged` converge on the same prospective content, source proof, exact-output acceptance and atomic transfer/admission. | Created, renderer Prepared, Show dispatch and PreShow/PostShow output do not independently move durable source ownership. |
| `surface_recovery.rs`, `frame/recovery_replacement.rs`, `frame/recovery_cleanup.rs`, `engine/surface_vacancy.rs` | Recovery, incarnation-safe replacement, cleanup and tick-final vacancy semantics. Pending sources remain recoverable; late predecessor facts cannot mutate successors. | Host retirement receipts cannot decide graph vacancy or recover content twice. |

`SurfacePaintDisposition::SourceGap` suppresses source pixels while preserving
the semantic identity and receivers retained by that program. An engine may
retain its own nodes/hit records; an immediate egui host preserves widget
registration. The core does not require egui widgets. This visual contract and
the active source continuation above do not relax target-side current-output
validation.

## Official egui product renderer

Paths are under [`crates/egui_dockspace/src/`](../../crates/egui_dockspace/src).
Ordered input is transported before measurement and painting. Pointer gestures
no longer originate from `Response` begin/move/release reconstruction.

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `product_dockspace.rs` | Application facade and local host-frame orchestration. | Joins the public session, measurements, commands and exact run settlement. | No second graph or output-promotion path. |
| `product_pointer.rs`, `product_pointer/touch.rs` | Freeze and normalize ordered host-local packets and explicit contact identities; execute finite continuation requests with fresh local facts. | Submit through the core journal and exact accepted-output receiver port. | No response-derived drag FSM, guessed event timestamp or native stationary capability. |
| `pass_settlement.rs` | Caller-owned `run_ui` final-pass and affine renderer receipt. | Returns exact complete output; drop/unwind reports retirement and terminal delivery wakes pending work. | Callback return does not imply presentation; ordinary `eframe::App::ui` cannot own this boundary. |
| `output_proof.rs` | Conservative complete shape/clip/transform-span integrity after output hooks. | Checks synchronous append-only painting on the supplied layer and removes private markers before return. | No GPU proof or target authority; cross-layer/mutated output cannot supply the proof. |
| `product_render/actions.rs` | Register receiver facts and translate semantic keyboard/menu/AccessKit activations into prepared commands. | Uses exact responses and core records. Pointer ownership is journal-driven. | No pointer phase reconstruction, threshold, topology or native-route decisions. |
| `product_render/tabs.rs` | Paint and register tab, close, pane, and group-region semantics. | Consumes exact tab paint records and returns prepared local actions. | Does not reconstruct tab geometry or mutate workspace state. |
| `product_render/tab_chrome.rs`, `product_render/touch_menu.rs`, `product_render/presentation_menu.rs` | Overflow, touch context and presentation-command UI. | Render core-owned scroll/menu/context records and activate prepared commands. | No durable scroll offset, guessed wheel delivery, native placement or independent context timer. |
| `product_render/contained.rs` | Contained chrome and transformed pane content. | Consumes the core's complete transient geometry, disposition and prepared adjustments. | No durable contained rectangle, stacking order or terminal gesture owner. |
| `product_render/splitters.rs` | Register pointer receivers and translate keyboard/AccessKit adjustments. | Uses exact splitter/junction and device-specific hit records. | No renderer-computed layout weights or resize FSM. |
| `product_render/guides.rs`, `product_render/drag_feedback.rs` | Paint exact guide/preview records, source gaps, ghosts and passive diagnostics. | Consume the core program's geometry and disposition. | No drop solver, gesture state or release/presentation authority. |
| `product_render/measurement.rs` | Report renderer intrinsics and exact surface bounds. | Produces measurement contributions for core tickets. | Does not publish scene authority. |
| `product_render/mod.rs` | Execute one ordered surface program and return exact receiver declarations/paint metadata. | Immediate egui consumer of `SurfacePaintPlan`; the outer host owns run settlement. | No second graph, program order, scheduler or output ledger. |

The [renderer contract](../../crates/egui_dockspace/src/product_render/README.md)
details exact-pass declarations, explicit ordinary-content pass-through and
conservative popup/modal/foreign-overlay blockers. Current local product
qualification includes the [complete 60-case L03 matrix](../VALIDATION.md#2026-10-31-l03-single-batch-burst-qualification);
an unmodified eframe callback still reports presentation-dependent features
unavailable.

## Native host records, resources and lifecycle

Paths are under [`crates/egui_dockspace_native/src/`](../../crates/egui_dockspace_native/src).
The earlier validation name **`HostRecords` is historical**. Current
`mailbox.rs` defines `NativeHostBridge` with one `Mutex<NativeHostState>`.
That state joins the viewport map, admission/journal, output/window resources,
retirement and close owners. Separately shared route/retirement/close mirrors
are not current owners.

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `mailbox.rs`, `mailbox/journal.rs`, `mailbox/admission.rs` | Single locked host-state ingress and original ordinal/age retention through raw-to-core transfer and complete commit. | Exposes immutable causal prefixes, output/lifecycle barriers and fixed-boundary overload admission. | No inferred callbacks, refreshed retry age or semantic reduction inside capture. |
| `event.rs`, `pointer_event.rs` | Freeze original routes/metadata/local geometry and translate device/contact/scroll input. | NativeLocal/Desktop and explicit positive handoff use the same recorder and frozen gesture lane. | No receiver solver, synthetic movement or new gesture on handoff. Missing event geometry stays unknown. |
| `mailbox/window_resource.rs` | `NativeWindowResource` joins exact binding creation data/acknowledgements, hidden/staging work, command callbacks and pending NoFrame invalidation. | Prepaid original owners retain callback envelopes through submission/full commit and route retirement. | No second lifecycle controller; renderer-ready/staging state cannot publish first-live transfer. |
| `mailbox/output_resource.rs` | One `NativeOutputResource` per exact token owns dispatch budget, affine paint and renderer terminal/envelope. | Ordinary output backing and prepaid root/child controls share one settlement protocol; promised staging retains its original owner. | No terminal queue minted on callback, release of partial prefixes or diagnostic-control semantic authority. |
| `viewport_map.rs`, `viewport_map/create_receipt.rs`, `viewport_map/window_receipt.rs` | Exact route, local-coordinate capability, semantic wake and non-Clone create/physical Destroyed receipts. | Receipts move to the existing retirement entry across displacement/unbind; predecessor identity survives successor reuse. | No early window-index publication, ownership transfer or reuse of a captured predecessor terminal slot. |
| `retirement.rs` | Sole `NativeRetirementState`, held inside `NativeHostState`. | Correlates cleanup and create/Destroyed receipts until exact core commit, tombstone and route cleanup permit retirement/quiescence. | Does not decide source vacancy or treat raw handoff as terminal commit. |
| `close_control.rs`, `coordinator/close_driver.rs` | Sole held-close state in `NativeHostState`, with configured policy execution. | Capture exact request before Hold; a fixed successor retains the original reservation until committed cancellation and semantic Clear. | No duplicate resource close slot, lifecycle close counter or inferred application-exit policy. |
| `receiver.rs` | Query exact final-presented hit/scroll facts against retained output/widget identity. | Bind framework answers to current core receiver descriptors at the input ordinal. | No live-hover approximation or independent target resolver. |
| `coordinator.rs`, `coordinator/effect_dispatch.rs` | Join the session, original prefixes, snapshots and correlated native effect results. | Dispatch retains affine effect identity; multiple prefixes within one UI invocation share the same original token budget. | No second graph or promise that the whole UI invocation is one atomic transaction. |
| `coordinator/storage.rs`, `mailbox/capacity.rs`, `mailbox/roster.rs`, `mailbox/terminal_roster.rs` | Prepay/account bounded owner storage and complete-roster backing before admission. | Preserve original owners during ordinary freeze and protected terminal progress. | Owner-level limits/samples alone do not certify combined W16/O256 saturation or reducer service time. |
| `host_frame.rs`, `surface_driver.rs`, `pass_actions.rs` | Join the public frame, root/child semantic or control paint, exact output binding and semantic-action settlement. | Complete frame commit precedes callback-prefix release; pending exact outputs/semantic work retain an after-current wake. | No second presentation ledger, replay of consumed affine actions or UI-frame-number proof. |
| `application_action.rs` | Exactly-once public status for one programmatic action. | Correlates submitted action and presentation transition IDs. | Does not match by root/source heuristics. |
| `deferred_viewport.rs`, `coordinator/shutdown.rs`, `lifecycle_progress.rs` | Execute egui's selected Staged strategy, drain fatal/quarantined obligations and project fixed-size progress. | Reuse existing binding/output/retirement owners and preserve NoFrame/close work. | Progress counters do not own lifecycle semantics; egui staging is not a universal engine protocol. |
| `work_area.rs`, `window_snapshot.rs` | Translate complete work-area/window observations into core facts. | Require current placement, scale and readiness provenance. | No monitor-bound substitute, partial exact roster or guessed renderer readiness. |
| `capabilities.rs` | Conservative backend capability publication. | Maps only reviewed platform facts into core capabilities. | Does not promote a backend because an enum or code path exists. |

First-live is a core semantic boundary; window/output resources above are
adapter lifetimes. The [2026-10-30 R15 replacement](../VALIDATION.md#2026-10-30-r15-first-live-ownership-replacement)
and [preparation-strategy qualification](../VALIDATION.md#2026-10-30-native-preparation-strategies)
supersede earlier early-transfer/staging descriptions. The
[single-host-state record](../VALIDATION.md#2026-10-29-u7-single-native-host-state-prerequisite)
explains the historical `HostRecords` consolidation.

The existing [`coordinator/tests/combined_soak.rs`](../../crates/egui_dockspace_native/src/coordinator/tests/combined_soak.rs)
now contains `configured_maximum_roster_and_terminal_fanout_survive_ordinary_freeze_and_retry`.
Its [2026-10-31 focused model qualification](../VALIDATION.md#2026-10-31-configured-maximum-roster-and-terminal-fanout)
passes 4/4 normally and 4/4 without native debug assertions. W16 holds every
configured binding and a complete roster; 32 real fork-issued property command
identities and 16 Destroyed callbacks retain their original protected owners
after ordinary admission freezes at 1,360 raw cursor records. Dropped candidate
and whole-batch retirement preparation retry, then all bindings/references retire
to quiescence. Owned/reserved input data peaks at 2,325,314 / 4,194,296 bytes
under 4 MiB, with the recorded input-data exclusions. This is a constant-clock
model, with no OS windows/output tokens, O256 combination or active-release
qualification for that cursor-only fixture.

The public native stationary projection is implemented, but automatic native
adapter scheduling is not qualified. Fresh query capture/delivery remain
`Unknown`; the [pointer-observation prerequisite](../VALIDATION.md#2026-10-30-native-pointer-observation-prerequisite)
does not authorize a timer to reuse retained capture or generate motion.

## Conformance and physical evidence

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `crates/dockspace_host_conformance/src/scenario.rs` | Finite semantic journeys, L03 basin/normalized inset/cadence cases and normalized outcomes. | Local core-host and product executors share product concepts, not pixel coordinates. | No screen coordinates, graph IDs, receipts, driver trait or second target oracle. |
| `crates/dockspace_host_conformance/src/tests.rs`, `integration/dockspace-host-harness/` | Core-host semantic parity and ordinary public-only immediate/deferred/runtime consumers. | Recording renderers retain exact identities and settle their own outputs after frame borrows. | No real OS windows or GPU/physical certification. |
| `integration/egui-product-harness/`, `integration/egui-official-harness/` | Local product behavior and official crates.io compatibility/blocker/output limits. | Use the public adapter, including the complete single-packet L03 Burst matrix. | Native physical qualification cannot be inferred from local egui input. |
| `integration/egui-native-smoke/src/main.rs`, `integration/egui-native-smoke/src/bin/output_pressure/host.rs` | Programmatic real-window lifecycle and finite real output-owner pressure cases. | Observe first-live, exact retirement, bounded input-data samples and quiescence. | Programmatic redock is not a physical drag or complete configured-cap qualification. |
| `integration/egui-native-smoke/src/bin/physical_x11/app.rs`, `assertions.rs` | Minimal application, passive current semantic facts and model/preview checks. | Read-only inspection reflects actual emitted program and current workspace. | No constructed drop target or programmatic tear-off bypass. |
| `physical_x11/driver/inspection.rs`, `physical_x11/driver/x11.rs` | Bounded loopback AccessKit inspection, checked XTest input and exact XID destruction. | Paths continue under the same smoke `src/bin/` directory. | No generic GUI automation API or core gesture injection. |
| `physical_x11/driver/scenario.rs`, `physical_x11/pointer_load.rs` | Eight finite target journeys and optional center-only 1 kHz injection/callback measurement. | Current semantic bounds, normalized samples, physical rounding, held input, emitted preview, model, DestroyNotify, retirement and quiescence. | No fallback target, screenshot oracle or step-as-release/presentation proof; load rejects edge/guide targets. |

The [latest edge record](../VALIDATION.md#2026-10-31-x11-non-guide-pane-edge-docking)
qualifies four edge, three center and one guide-control physical journeys at
`13dd127`. The [load record](../VALIDATION.md#2026-10-31-continuous-x11-input-and-pending-release-wake)
qualifies its separate center-only finite profile. X11 authority is limited to
the first X screen with one exact identity-transformed active CRTC/work-area
authority. macOS programmatic/same-window facts, Windows/Wayland limits and
stationary `Unknown` facts are detailed in [platform support](../PLATFORM_SUPPORT.md).

## Provenance and remaining boundaries

The preceding owner map replaces the 2026-08-31 inventory's current-state
claims. Earlier authority/conformance plans, pointer-journal design documents
and Open-GPUI catalogs remain historical design/reference material where their
API names or ownership differ from this source. Open-GPUI cutover is deferred.

Native qualification uses unpublished local egui `70f7b517` and Winit
`6c10c87d`; tracked remote pins remain `0716beee` and `f378b50f`. Required seams
are missing from that tracked pair, so a remote-only clean checkout cannot
reproduce these native results. Full revisions and evidence are in
[platform support](../PLATFORM_SUPPORT.md#dependency-reproducibility-and-open-scope).

The [subsequent W16/O256 real-output record](../VALIDATION.md#2026-10-31-simultaneous-w16-roster-and-o256-real-outputs)
adds a finite simultaneous window/output profile in the existing smoke runner.
Its fixed satellite identities are fixture expectations, outside native runtime
storage, and introduce no production registry or graph.

The remaining close/create capacity, service-time, broader
physical/platform, stationary capture, native showcase, dependency/package and
evidence/deletion gates retain their existing owners in the canonical plan.
This update adds no runtime tool, replacement plan or overall U9 completion claim.
