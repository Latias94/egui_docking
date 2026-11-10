# Docking, multiview and core responsibility review

Date: 2026-11-10 (Asia/Shanghai). Baseline: `48f0bf9`.
Repair revision: `440ed2b`.
Status: architecture review, independent repair review and focused verification complete.

Three independent Codex review lanes inspect topology/product commands, native
hosting/lifecycle and cross-view input. The primary lane traces the public host
interface, compares the supported combinations and confirms executable findings.
Review and reference exclusions remain untouched. This review introduces no
change to the supported physical-platform profiles.

## Verdict

Dockspace already owns most renderer-neutral docking and native-window
semantics. Multiple logical surfaces and managed native windows are implemented
capabilities, not merely future extension points. Creating a window does not
prematurely transfer pane ownership: exact renderer readiness and first-live
presentation still gate the transfer. The egui native adapter implements OS and
renderer operations without constructing a second docking graph or recomputing
recovery transactions.

The largest architectural gap is composition. A host can use multiple logical
surfaces through HostLocal input, or managed native windows through NativeJoined
input. The public interface cannot fully combine several independent logical
surfaces inside one OS window with the managed native child lifecycle and one
continuous cross-window gesture. Programmatic commands also expose fewer payload
shapes than pointer-driven docking, and automatic native document restoration
implements one external Root hosting profile. These are explicit product gaps;
they are not newly discovered failures of documented supported operations.

One executable P2 was confirmed: the session enabled a same-surface contained
root Float command which the paint interface already disabled and execution
rejected. That inconsistency is repaired without broadening the architecture.

## Responsibility allocation

| Responsibility | Dockspace core | Host or framework adapter | Application |
| --- | --- | --- | --- |
| Dock topology and placement | Item ownership, groups, split layout, central leaf, reorder, merge, inner/outer edges, contained order and geometry | Measure actual UI chrome and pane minima; render the resulting ordered program | Supply pane identity/content and choose initial/default layouts |
| Cross-view interaction | Capture semantic source, resolve docking targets, policy, gesture continuation, local/desktop lane handoff, cancellation and delivery | Supply ordered event-time positions, delivery/capture/hover and framework blocking facts | Choose commands and interaction preferences |
| Native creation and removal | Decide source treatment, allocate semantic identity, reserve source content, first-live transfer, compensation, retirement and recovery | Create/close OS resources, prepare renderer, dispatch effects and report actual outcomes | Choose allowed native behavior and window-close policy |
| Coordinates and visibility | Validate coordinate generations, convert authoritative native coordinates, distinguish Unknown/Foreign/OutsideAll | Observe window/client/work-area bounds, scales, actual occlusion and presented output; map HostLocal regions | Choose intentional view placement and styling |
| Focus | Request semantic pane focus, correlate activation, preserve per-surface history and recovery intent | Execute OS activation and actual widget focus; report observations | Define pane focus target and unrelated application UI |
| Persistence | Atomic graph/key/frontier/preferences document, identity remap, native continuation and creation obligations | Supply current native facts and physical resource routes | Recognize external pane keys, load content and choose restore profile |
| Event loop and resources | Expose pending work, finite continuation requests and typed effects | Schedule callbacks, retain render data, obey capacity/backpressure and declare permanent quiescence | Own application data and process shutdown policy |

Renderer neutrality does not eliminate lifecycle semantics from core. Conversely,
the OS capture mechanism, graphics context creation and actual presentation cannot
be inferred from a docking graph and should not move into core.

## Capability coverage

| Journey | Public interface | Core implementation / independent evidence | Current limit |
| --- | --- | --- | --- |
| Multiple logical containers, including identical local bounds | `DockspaceLayout`, `enable_host_pointer`, `HostPointerInput` | `runtime/interaction.rs`; `host_coordinate_domains.rs`, `public_contract.rs` | Host supplies logical-region routing and a total event order |
| Incremental containers | `attach_empty_surface_current`, `retire_empty_surface_current` | `engine/logical_surface.rs`; `logical_surfaces.rs` | Nonempty content and native/recovery obligations must settle before retirement |
| Cross-surface item/root merge, reorder, edge dock and float | `DockPlacement`, `dock_item_current`, `dock_root_current`, prepared actions | `engine/product_action.rs`, `engine/product_action/contained.rs`; product actions and public migration controls | Programmatic group/native payload coverage is incomplete below |
| Multiple OS windows and native tear-off | `enable_managed_native_host`, native registration, `prepare_move_root_to_new_window` | `engine/native_tear_off.rs`, `native_projection.rs`, `native_admission.rs`; `direct_prepared_native.rs` | Complete-root programmatic entry; platform facts and capabilities still required |
| Existing logical forest gains native hosting | `create_native_surface_current` | `engine/surface_runtime.rs`; `native_surface_creation.rs` | Topology is preserved; exact native preparation and first-live admission remain mandatory |
| Same-window native input without desktop origin | `native_local_coordinates`, `NativePointerInput::new_local` | `runtime/native/local_pointer.rs`; `native_local_pointer.rs` | Native-local OS qualification is separate from recording-host evidence |
| Continuous local-to-desktop drag | `NativePointerInput::new_handoff` | `engine/pointer_handoff.rs`; `native_pointer_handoff.rs` | Both samples must describe the same event; handoff does not supply missing hover or capture facts |
| Native closure, resource loss and restoration | Native close request/decisions, exact effect reports and Destroyed facts | `close_workflow.rs`, `surface_runtime.rs`; `native_close_content_scope.rs`, recovery tests | Resource ownership and content approval retain independent obligations |
| Document restore and external Root continuation | Native document restore and Root-rebinding methods | `runtime/persistence.rs`, `runtime/layout_reset.rs`; `native_document_restore.rs` | Automatic hosting is one external Root plus its declared Children |
| DPI and render-density independence | HostLocal mapping and committed native coordinate capabilities | `coordinates.rs`, `host_coordinate_domains.rs`, native geometry/handoff tests | Mathematical/protocol coverage does not certify physical mixed-DPI dragging |

Evidence paths in this table refer to `crates/dockspace/src/` and
`integration/dockspace-host-harness/tests/` unless identified as core tests.

## Confirmed P2: root Float availability disagrees with execution

On a measured surface A, a contained root R already floats on A. The paint-plan
command reports `AlreadyContained`, but
`session.presentation_commands(R).float_to(A)` returned Ready. Immediate direct
execution rejected the same unchanged state with Conflict because RehomeRoot
correctly requires its dedicated metadata-update command for an existing
same-surface contained presentation.

The public test fails in run `bc20a2f5-3d7b-4c9d-85cd-780d83040790`
([before](2026-11-10-docking-multiview-responsibility-review/root-float-before.log)).
Direct rejection and cross-surface successful rehome already pass in that run.
The repair rejects the same-owner case inside the existing root-float compiler,
so availability and submission share the decision. It retains the existing
Conflict type and dedicated operation checks. Cross-surface migration still
preserves root/floating identity, group order and selection. The public method
documentation now states the same-owner limit.

## G01: combine several logical containers with managed native windows

This is the highest architectural priority for a general editor host if several
independent docking containers must share one native window.

Current evidence:

- `viewport_registry.rs::register_existing_with_admission` rejects both a
  duplicate surface and a duplicate OS resource token. The native registry
  represents one surface per window lifetime.
- `runtime/native/session.rs::enroll_native_host` rejects an existing HostLocal
  producer. Managed native and HostLocal producers cannot independently mutate
  one gesture.
- NativeLocal captures the complete client geometry and divides its point by
  presentation scale. It has no separately bound child-surface placement.
- `runtime/native/receiver.rs` chooses a surface before asking the host for
  receiver clearance; the resolver cannot choose another logical container.
- HostLocal route validation requires a Headless endpoint rather than a managed
  native endpoint.

A single surface's splits and contained roots already provide many panes in one
window. That is not equivalent to independent SurfaceIds with distinct docking
containers, bounds, rendering schedules and surface-scoped policy. Likewise,
using HostLocal for all OS windows forfeits core-managed native establishment;
it is not a complete adapter solution to this combination.

The core extension should distinguish the physical window lifetime from the
logical surface placement. One window may host multiple surfaces; each surface
has an explicit current placement within one endpoint. The host supplies the
client mapping, scale, clip and actual blocking facts. Core owns their binding
and generation relationships, target policy, gesture continuity, shared-window
close scope and native lifetime accounting. Keep one joined input order and
reuse the existing lifecycle owners.

Do not implement this by removing token uniqueness, issuing fake tokens for the
same resource or allowing unrelated pointer producers to race. Those changes
would duplicate close/retirement obligations or disconnect input from the actual
window. An arbitrary render graph, generic window manager or arbitrary affine
transforms are unnecessary for the initial contract.

First acceptance fixture: two independent logical containers in one real Root
window, a runtime-owned Child, drag between the containers and through the Child
without restarting the gesture, then dock back. Include nonzero client offsets,
independent logical/render scales, blocked foreign hover, failed creation,
late outputs, shared-window close and Root continuation. This fixture should
drive the smallest model change.

## G02: programmatic native tear-off should match pointer payloads

Applications need a command equivalent to dragging tab B out of `[A,B]`, and a
command to detach a whole tabs group from a split while keeping its sibling.
Core already captures Item/Tabs/Subtree payloads in pointer interaction. Public
programmatic native entry points currently accept only RootId. FloatItem creates
contained content, not a native window.

Two apparent workarounds change the required behavior. Floating first requires
publishing and measuring that intermediate root before native preparation;
creating an empty surface and moving content before requesting hosting commits
the move before native creation can fail. Neither preserves the original source
until the target's first-live admission as the native tear-off contract does.

Expose stable item/group/root command selection and lower it inside core to the
existing payload and creation workflow. Group preparation must retain its exact
source membership rather than later discovering a different group through a
reused anchor. Keep raw NodeId private. Pointer, menu, keyboard and automation
should use the same policy, reservation, first-live and compensation rules.

Acceptance must prove original-source preservation after failure/cancellation,
no early topology transfer, exact target-payload transfer at first-live, retained
group order/selection and unrelated sibling invariance.

## G03: default placement belongs in the application command interface

Default native placement already belongs to core:
`engine/native_tear_off.rs::derive_default_native_window_placement` converts
source geometry, preserves window insets, selects a work area, cascades and
clamps. The paint command consumes it. The independent session command requires
an explicit `NativeWindowPlacement` instead.

Retain explicit placement, and expose the same default derivation through the
non-encoding command interface. Application menus, shortcuts and automation
should not enter paint encoding or duplicate work-area selection to request
normal core behavior. The command should use current authoritative geometry
and return a typed unavailable result when those facts are absent; no historical
position cache or new placement algorithm is needed.

## G04: arbitrary multi-Root automatic restore is not implemented

Core can register several external Root windows. The persistence schema already
represents per-surface Root/Child declarations and different recovery hosts;
document capture and validation impose no single-Root count restriction.
However,
`runtime/persistence.rs::native_surface_restore_plan` selects one Root and
requires every other surface to be an explicit Child recovering to that Root.
The existing automatic native restore is a fixed-Root hosting profile. Its
internal `NativeSurfaceRestorePlan` also carries one Root, so generalization
requires changing the restore orchestration rather than just adding a convenience
overload. Current method documentation accurately describes that restriction.

Generalizing this should use an explicit application mapping of incoming external
roots to current OS window lifetimes and core-owned Child creation intents.
Identity rebasing, recovery-host validation, continuation and cleanup must remain
atomic in core. The existing document schema need not change merely to represent
multiple Roots. Persisted preferences must never become current OS authority.
This does not require a host to recreate the graph or infer native resource
identity from saved numeric values.

## Physical-platform qualification remains separate

The current reference is strongest on finite X11 profiles: physical target
journeys, foreign InputOnly blocking, stationary hover/overflow and complete
editor flows. Its exact work-area authority is limited to the first X screen
with one identity-transformed active CRTC. Multiple-monitor and mixed-DPI
physical docking are not established by those profiles.

macOS has programmatic real-window lifecycle and output-pressure evidence, but
no authoritative-button capability for the physical cross-window release
contract. Windows lacks several required desktop pointer/hover/button facts in
the reference mapping. Wayland's reference mapping lacks managed native
lifecycle/global placement/work-area authority; supported local operations can
still work without those global facts. Native Touch OS interaction and broader
native-local handoff qualification remain separate work.

These are backend capabilities and evidence gaps. Core should accept exact facts
from a capable adapter and return honest typed limitations otherwise. It should
not invent desktop positions, infer occlusion from window rectangles or weaken
first-live readiness to obtain nominal platform parity.

## Recommended implementation order

1. Keep the confirmed availability repair and its public regression.
2. Specify the combined same-window/multi-container/native-Child fixture, then
   separate native endpoint identity from logical placement as required by it.
3. Add programmatic item/group native tear-off and default placement using the
   existing creation workflow, with source-preservation acceptance tests.
4. Generalize automatic restore to explicit external-Root mappings if the host
   requires multiple independent Root windows.
5. Qualify additional physical backends and monitor/DPI profiles through actual
   input and rendering. Do not substitute capability flags for those results.

G01 through G04 are design and feature recommendations. This review does not
claim their implementation, does not classify them as regressions of the current
documented contract and does not start a speculative broad refactor.

## Repair verification

All 42 public command, native creation, logical surface, layout-management and
retained-receiver controls pass in run
`942b7d5e-7280-4f72-96dc-f91e46b5c18b`
([command](2026-11-10-docking-multiview-responsibility-review/host-commands-after.json),
[result](2026-11-10-docking-multiview-responsibility-review/host-commands-after.log)).
The three new root-float cases include the formerly failing availability check,
an unchanged direct-rejection control and a successful cross-surface control.
Independent review confirms the shared compiler check and unchanged native
rehome barriers. All 48 adjacent core command/native rehome cases pass in run
`7ac5e8b0-2368-46a9-8973-3eb1c9f0aa6b`, and core correctness/suspicious Clippy,
strict Rustdoc and workspace/host formatting checks pass. Exact commands and
results are in
[serial-gates.json](2026-11-10-docking-multiview-responsibility-review/serial-gates.json).
Source is unchanged from `440ed2b` after verification. G01 through G04 remain
explicitly unimplemented recommendations.

The preceding full qualification at `444d618`, recorded in the lifecycle and
persistence review, remains historical evidence for its tested source. This
review does not claim a new full-platform or mixed-DPI qualification from a
command-availability change.
