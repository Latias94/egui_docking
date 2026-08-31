---
title: "Dockspace Interaction Module Inventory"
status: "Verified"
reviewed: "2026-08-31"
---

# Dockspace Interaction Module Inventory

This inventory records the owner and authority boundary of the modules added or
deepened by the interaction and multiview conformance work. It is an audit aid,
not a source manifest or a stability promise.

## Official egui product renderer

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `product_render/actions.rs` | Translate current-pass egui responses into opaque prepared actions. | Receives `Response`, AccessKit requests, and core paint records. | Does not choose topology, targets, or native routes. |
| `product_render/tabs.rs` | Paint and register tab, close, pane, and group-region semantics. | Consumes exact tab paint records and returns prepared local actions. | Does not reconstruct tab geometry or mutate workspace state. |
| `product_render/tab_chrome.rs` | Overflow controls, menu rows, and pointer scrollbar. | Consumes core-owned scroll/menu records and typed capability state. | Does not own durable scroll offset or infer wheel delivery. |
| `product_render/contained.rs` | Contained title, close, resize, and egui window visuals. | Consumes contained paint records and core-prepared adjustments. | Does not persist contained geometry or stacking order. |
| `product_render/splitters.rs` | Splitter and junction pointer/keyboard/AccessKit controls. | Uses exact splitter records and opaque adjustments. | Does not calculate layout weights independently. |
| `product_render/guides.rs` | Paint exact guide/preview records and passive diagnostics. | Consumes core guide records only. | Does not resolve drop targets. |
| `product_render/drag_feedback.rs` | Paint source gaps, ghosts, and drag decoration. | Consumes core drag-decoration records. | Does not own gesture state or release authority. |
| `product_render/presentation_menu.rs` | Render root-scoped presentation commands. | Activates core-minted command records. | Does not derive native placement or docking direction. |
| `product_render/measurement.rs` | Report renderer intrinsics and exact surface bounds. | Produces measurement contributions for core tickets. | Does not publish scene authority. |
| `SurfacePaintPlan::visit_surface_program` | Execute the core-owned root, popup, preview, guide, and feedback order. | Supplies one validated ordered step stream to the renderer. | Does not mint presentation proof or own durable topology. |
| `product_render/mod.rs` | Orchestrate one surface render and final pass settlement. | Consumes the ordered `SurfacePaintPlan` program and joins private renderer modules. | Does not own a second graph, gesture machine, schedule, or output ledger. |

## Native coordinator

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `mailbox.rs` | Ordered callback/output journal and affine output reservations. | Accepts host callbacks and exposes one causal prefix. | Does not reduce core state or infer missing callbacks. |
| `pointer_event.rs` | Per-device mouse, touch, and scroll translation plus retirement tombstones. | Converts typed Winit/eframe facts into atomic native pointer batches. | Does not resolve egui receivers or choose drop targets. |
| `receiver.rs` | Query the final-presented egui hit/scroll authority. | Matches frozen widget identities to core receiver descriptors. | Does not use live hover, rectangles, or a second hit resolver. |
| `pass_actions.rs` | Durable local-action batches and native pass settlement. | Keeps local actions independent of output-token lifetime. | Does not retry consumed affine actions or publish presentation. |
| `coordinator.rs` | Join mailbox facts, native effects, roster, and host frames. | Owns adapter-side causal scheduling and exact binding sidecars. | Does not own workspace topology or platform facts. |
| `coordinator/effect_dispatch.rs` | Dispatch and classify one exact native effect result. | Preserves affine effect identity and typed recoverability. | Does not convert retryable rejection into session fatality. |
| `coordinator/close_driver.rs` | Drive configured close policy through exact callbacks. | Correlates close request, cancel, destruction, and transitions. | Does not invent application exit policy. |
| `coordinator/shutdown.rs` | Drain committed obligations after fatal/quarantine. | Preserves transitions, NoFrame invalidations, and close progress. | Does not erase unresolved core authority. |
| `surface_driver.rs` | Execute one semantic/control surface boundary and settle output. | Owns the adapter's host-frame commit sequence. | Does not keep a second presentation or action outcome ledger. |
| `application_action.rs` | Exactly-once public status for one programmatic action. | Correlates submitted action and presentation transition IDs. | Does not match by root/source heuristics. |
| `retirement.rs` | Retire deferred viewport sidecars after core effect acceptance. | Applies exact binding/incarnation retirement commands. | Does not decide source vacancy. |
| `work_area.rs` | Map the host's complete work-area roster into typed core facts. | Accepts only complete, validated platform snapshots. | Does not substitute monitor bounds for usable work area. |
| `capabilities.rs` | Conservative backend capability publication. | Maps only reviewed platform facts into core capabilities. | Does not promote a backend because an enum or code path exists. |

## Conformance and physical evidence

| Module | Sole responsibility | Interface boundary | Explicit non-ownership |
| --- | --- | --- | --- |
| `dockspace_host_conformance/scenario.rs` | Bounded semantic selectors, actions, observations, and capabilities. | Shared only by the two local semantic executors. | Contains no coordinates, graph IDs, receipts, or driver trait. |
| `dockspace_host_conformance/tests.rs` | Public-runtime core-host evidence and a private complete-roster fixture. | Commits or aborts one logical host boundary. | Does not claim real OS windows or mint native evidence. |
| `egui-native-physical-smoke/app.rs` | Minimal product application and passive semantic facts. | Publishes read-only diagnostics through the inspection seam. | Does not construct targets or invoke programmatic tear-off. |
| `egui-native-physical-smoke/driver/inspection.rs` | Bounded loopback inspection protocol client. | Reads viewport-specific AccessKit snapshots. | Does not form a reusable UI automation API. |
| `egui-native-physical-smoke/driver/x11.rs` | X11/XTest input and exact window-destruction observation. | Injects real OS pointer events and observes the selected XID. | Does not call core gesture APIs. |
| `egui-native-physical-smoke/driver/scenario.rs` | The single drag-out/redock scenario and causal barriers. | Derives points from current semantic/window/work-area facts. | Does not use fixed drop coordinates, sleeps, or screenshots. |

## Audit result

- Core remains the sole owner of topology, target resolution, durable geometry,
  focus intent, close policy evaluation, and presentation lifecycle.
- The official renderer owns no durable layout or scroll authority.
- The native adapter owns causal sidecars and resource lifetimes only; every
  mutation still crosses one core host-frame boundary.
- The conformance kit and physical smoke are bounded evidence executors, not a
  generic test engine or duplicate graph.
