# egui_dockspace

`dockspace` is the primary library: a renderer-neutral docking core intended
for custom game-engine editor UIs. Engines own their drawing, pane content,
input delivery, frame scheduling, and platform resources. `egui_dockspace`
and its native integration are interactive reference adapters used to validate
the core's behavior and host contracts. Open-GPUI adapter work is deferred.

The repository is undergoing a breaking refactor. There is no compatibility
layer for the former `egui_docking`, `egui_tiles`, or pre-refactor persistence
interfaces.

The current engine-host and direct-manipulation plan lives in
[`docs/plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md`](docs/plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md).
It prioritizes a documented public host API and an independent consumer with
no egui dependency before broader native certification. The recording consumer
now verifies owned output records, ordered presentation settlement, and commands
on two logical surfaces, complete ordered gestures, contained content, close
approval and exact feedback/focus. Native throughput, lifecycle consolidation
and platform certification remain separate unfinished gates.
The earlier product-boundary plan supplies the persistence and ownership context.

## Capability matrix

| Layer | Current product capability | Deliberate limit |
| --- | --- | --- |
| `dockspace` | Stable item/root/surface layout, validated atomic actions, core-derived paint and receiver plans, close workflow, optional session-owned persistence, and renderer-neutral native lifecycle semantics | Does not own a renderer, widget tree, animation clock, event loop, or OS window |
| `egui_dockspace` | Caller-owned official-egui runs support tab select/close/reorder, magnetic center and edge docking, splitter resize, keyboard and AccessKit actions, and contained move/resize | An unmodified `eframe::App::ui` callback cannot prove terminal presentation; preview-qualified drag and pane-focus settlement are typed unavailable there |
| `egui_dockspace_native` | Unpublished fork-backed coordinator with an eight-pane editor example, a bounded real-input workflow, a programmatic window smoke, and an X11/XTest physical drag smoke | Native consolidation and broader platform qualification remain open. The qualified reference lane is X11 on the first X screen with one exact active-CRTC work-area authority; local/headless gates do not certify other backends |
| Open-GPUI | Reference evidence only | No adapter or integration work is in the current plan; any future cutover requires a separate plan |

Caller-owned official-egui runs also retain explicit local touch identities:
short taps activate on release, pre-arm movement yields to pane content, and a
350ms stationary continuation arms ordinary docking/resize or a semantic context
menu. Explicit Touch resize targets request at least 44 logical points before
surface and protected-control clipping; mouse/pen/unknown-device geometry keeps
its existing profile. Native touch/stationary capture remains unqualified.

Local drags use the same finite continuation for tab hover selection and
overflow-edge scrolling. Each automatic scroll step waits for exact presentation
of its new geometry, and explicit scrolling or menu actions take ownership
through the existing tab-strip state.

The current implementation has a qualified renderer-neutral local/contained
contract, an official-egui caller-owned validation path, and an experimental
native multiview adapter. The fast-gesture and configuration-dependent failures
from the 2026-10-27 audit are repaired and covered by fresh regressions. See
[`docs/VALIDATION.md`](docs/VALIDATION.md). It is not yet a mature ImGui-level
docking and multiview crate.

Native drag-out creation keeps source ownership through staging and prospective child
painting. Only an accepted exact first-live content output publishes topology,
workspace revision and admission together. A dropped frame or output and a
cancelled pending create preserve source ownership. Native enrollment now selects
`DirectPrepared` for an engine-prepared window and renderer, or `Staged` for the
egui reference host. Both feed the same exact first-live publication and cleanup
owners. This strategy cutover passes the current core, adapter and reference
window gates; broader native input and platform readiness remain open. See
current evidence and remaining gates in
[`docs/VALIDATION.md`](docs/VALIDATION.md).

An existing complete logical surface can explicitly acquire native hosting
through the public host frame. It shares readiness, exact first-live and cleanup
ownership while retaining its topology and workspace version. Failed or cancelled
initial attempts can retry with a fresh window incarnation; old acknowledgements
and outputs remain tied to their original window. Managed native document
restore now atomically queues explicit Child declarations with their rebased
surface IDs and saved placement. Budgeted frames wait for fresh Root facts and
exact predecessor quiescence before creating each child once. The X11 reference
workflow verifies two live predecessors, two restored children and real content
editing. Explicit Root continuation can also remove the old logical host and
retain the same external OS window on another surface. Candidate and committed
exact binding pairs let hosts prepay resources and atomically publish the native
route and their root-surface field. The reference X11 workflow verifies layout
and JSON Root rebinding, then Default and named-preset restore through their saved
host identities, fresh content editing and unchanged physical window identity.
Positive stationary OS facts, remaining native completion gates and
reproducible fork delivery are still open.

Managed engine hosts can also capture native client coordinates from committed
size and scale facts without desktop position or work areas. The joined recorder
retains one input order, and each gesture freezes its coordinate lane through
exact output settlement. The reference adapter now freezes event-time local
samples at callback capture and accounts for their immutable payloads within the
same bounded input budget. Its callback contract is qualified; native-local OS
input remains separate acceptance work. The public engine-host contract also
qualifies explicit local-to-desktop handoff
within one active drag, preserving its stream, session and semantic source while
revoking old target/output state. The reference adapter's same-event packet
capture/emission and full raw-to-output handoff traces are also qualified;
real native-local OS handoff evidence remains separate from those contracts.

The public native host API also qualifies explicit stationary hover selection,
Touch long press/tap/yield and tab-strip edge scrolling through the same gesture
and exact-output state. Native requests expose scheduling and sampling inputs;
the host still supplies fresh native facts. The reference adapter now schedules
Desktop Mouse observations at core deadlines, retaining at most one query until
its ordered terminal fully commits. Unknown or unavailable facts cannot trigger
an automatic retry of the same core token. Successful OS stationary execution,
physical Touch and the complete native readiness matrix remain unfinished.

Native binding retention now uses `terminal_binding_guards()` in the diagnostic
manifest. It covers observed destruction and conclusively never-created
bindings, including a published Pending binding cancelled before its Create
effect could be delivered. The previous `destroyed_binding_guards()` name and
destroyed-only missing/provider-mismatch error variants are replaced. Exact
producer quiescence and committed-prefix retirement still own guard release;
there is no synthetic Destroyed observation for a failed create.

The experimental native adapter reserves 4,096 raw callback slots and two
reusable complete-roster payloads. It admits at most 256 simultaneously retained
native binding lifetimes, including pending retirement; a route/create pair
shares one commitment. More than 64 work-area records revoke work-area authority
to `Unknown` while preserving the complete window inventory. Snapshot storage
pressure freezes ordinary admission through the existing overload boundary.
Selected 4 MiB profiles have finite qualification: W16 roster/property/Destroyed
models, real I64/W16/R258 with O256/control2/protected-staging1, and the separate
1 kHz XTest center-load journey. Their configured owner counts and excluded data
are recorded in `docs/PLATFORM_SUPPORT.md`. Combined close/create maxima and
sustained active-service acceptance remain open; 2 ms is a cooperative yield
threshold, with overruns retained in measurement, rather than a hard real-time
guarantee.
`NativeDockspaceApp::input_storage_capacity_bytes()` measures the current raw,
shared-roster, core transport/report, window/output/close/retirement, callback and
pointer/work-area data at a locked host boundary. It counts actual vector
capacity and existing entry data, without allocator metadata, render/receiver
state or future terminal reservations. The real-window smoke samples this query
through its lifecycle; those samples do not prove the complete admission limit
or transient construction peaks.

## Architecture

- `dockspace` is the only semantic authority for topology, selection, drop,
  close, persistence identity, and native lifecycle state.
- `egui_dockspace` supplies egui measurements, resources, painting, local
  responses, and accessibility without reconstructing a second dock graph.
- `egui_dockspace_native` supplies event-time platform facts, OS effects,
  renderer settlement, and window lifecycle without owning another docking
  engine.

Default product interfaces are item- and surface-centric. Raw graph nodes,
scene stamps, reducer inputs, provider leases, persistence candidates,
and backend FSMs are private. Renderers and native hosts integrate through the
narrow `DockspaceSession` runtime facade rather than an unstable raw backend
namespace.

The public host contract distinguishes complete surface-program acceptance,
renderer submission, final host presentation, and successful core acceptance of
the presentation report. Opaque execution receipts and output handles bind each
step to its exact emission. Engines can record their own drawing or retained
nodes after the frame borrow ends; a logical surface does not require an OS
window. The independent
[`dockspace-host-harness`](integration/dockspace-host-harness/README.md) demonstrates
the public path without any egui or platform dependency.

Run its complete engine-owned output example:

```text
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host --locked
```

The example retains its recording after the frame borrow ends, settles two
logical surfaces independently, executes application focus, approves a complete
root close, and deduplicates typed feedback. `Presented`/`Dropped` are recording
host observations; this example does not submit work to a GPU or create windows.

Facade errors follow the same rule. Both the renderer-neutral and egui product
interfaces expose six action-oriented categories: invalid configuration,
persistence, unsupported operation, operation conflict, host protocol, and
internal failure. Exact implementation diagnostics remain in the standard
error source chain.

## Persistence

The optional `serde` feature adds product document persistence. Applications
allocate stable pane identities through `DockspaceDocumentBootstrap`, build a
persistent session, and save or restore one versioned JSON document. The
document atomically binds topology, append-only external pane keys, allocator
frontiers, lineage and generation, and viewport placement.

Document V2 also binds explicit native root/child hosting preferences and each
child's recovery-host surface. `surface_presentation_preferences()` on a session,
prepared restore or candidate frame returns these declarations and optional
physical placement hints. The content graph remains in `view()`. Submission
rebases surface and recovery-host references together; reading a declaration
does not create an OS window or grant native authority. V1 documents are rejected
as unsupported because their placement records cannot prove native ownership.
Native multi-surface window recreation is still under implementation.

Raw workspace snapshots, independently replaceable key maps, and mutable
restore candidates are not product interfaces.

## egui and native references

The publishable crates use official egui and eframe `0.37.0`. The excluded
native workspace still pins the last published egui/eframe fork baseline,
`0716beee5b7284389f2e80ffb6b24b5af6ae2910`, with Winit pinned at
`f378b50f29cafafec31cd8675c17646b4ef57d70`. Current native qualification requires
newer local, unpublished egui and Winit revisions through explicit Cargo
overrides. Their exact source revisions and qualification limits are recorded
in [`docs/VALIDATION.md`](docs/VALIDATION.md). The tracked native manifests and
CI cannot yet reproduce the newer work from their remote pins alone. The
manifest, lockfile, CI, and this document must move together when those forks
are published.
Remaining fork seams and their removal conditions are documented in
[`docs/knowledge/egui-native-fork-seam-admission.md`](docs/knowledge/egui-native-fork-seam-admission.md).

The official adapter and admitted native fork use egui/eframe `0.37.0`.
The fork branch `dockspace-native-seam-0.37` retains graph-neutral seams for
exact native facts and output finalization. Winit remains on stable `0.30.13`,
and egui_tiles remains reference-only.

The optional native pointer observation seam feeds the adapter's finite
stationary scheduler. Its current X11 query reports server-logical desktop
position and limited direct-root-child hover; capture and delivery remain
Unknown, so querying does not authorize a stationary gesture. Hosts must not
fill those gaps from the expected window or cached implicit capture. The
renderer-neutral core supports stationary continuations when a host can supply
complete native facts.

The native application facade uses one bounded command/result slot for docking,
selection, reopening, content-close requests, exact initial/deferred close
decisions and arrangement locking. Consume `take_action_status()` before queuing
another command; `NativeActionStatus::Dock` carries the original docking status.
`pending_content_close_plans()` discovers current chrome and application requests
directly from the core's existing inventory. The `basic` example shows the eight
application-owned pane entries and explicit Allow/Veto/Defer controls. Pane data
remains application-owned while an item is closed.

This facade keeps its configured root logical surface. A close that would remove
its only presentation returns `NativeActionRequestError::HostSurfaceRequired`,
including an exact Allow for a chrome-created plan. Closing individual panes in
a central root retains its empty dock area and permits reopening. Closing a
complete root when another presentation remains uses the ordinary atomic close.
`request_layout_reset(default)` queues an application-owned default through the
same slot. The optional native `serde` feature adds `save_document_json()` and
`request_document_restore_json(bytes, recognize)`. Preparation validates the
configured host before occupying the slot; an obsolete queued replacement
returns `NativeActionStatus::Stale` without stopping the runtime. A successful
`LayoutReset` or `DocumentRestored` result confirms logical publication. Fresh
native facts and exact presentation still establish the successor's input and
focus authority. Old renderer outputs and command receipts retain their original
binding until their own terminal settlement.

The current facade accepts replacement layouts containing only its configured
host surface, including contained windows. Resetting from a live child retires
that child through the existing destruction protocol. Recreating additional
native windows from a multi-surface document remains subsequent work. The
example owns its immutable default and at most eight named JSON presets; it
retains recognized unavailable panes as reloadable placeholders. Full U10
acceptance remains open; see the dated qualification in
[`docs/VALIDATION.md`](docs/VALIDATION.md).

Hosts that own official crates.io egui's complete run use the Dockspace wrapper
to bind local settlement to that run without application-managed frame counts:

```rust,ignore
let output = egui_dockspace::run_ui(&context, raw_input, |ui| {
    dockspace
        .show_single_surface(surface, ui, panes)
        .expect("the Dockspace frame advances");
});
let (full_output, receipt) = output.into_parts();
let result = renderer.present(full_output); // Presented or Dropped, from the output owner.
receipt.report(result);
```

`egui_dockspace::run_ui` returns `DockspaceOutput`. Split it with
`into_parts()` to obtain `FullOutput` and a one-use `DockspaceOutputReceipt`.
The renderer owner reports `Presented` only after that complete output reaches
presentation, or `Dropped` when it is discarded; dropping the receipt also
retires it. Core accepts exact emissions at the next successful frame boundary,
including delayed or out-of-order reports. Returning from egui never grants
presentation authority by itself.

The adapter checks the complete ordered surface shape span after every output
hook, including ordinary pane/tab drawing, previews, clip and transform data.
It removes only its private markers. This conservative path requires synchronous,
append-only pane drawing on the supplied layer; cross-layer writes and nonempty unenumerated layers are unavailable
for settlement. After `into_parts()`, the host is responsible for preserving the
complete output it reports. Shape checks do not inspect application paint callback
internals or observe GPU submission. Discard, unwind, duplicate enrollment,
foreign contexts/viewports and output mismatch cannot promote partial evidence.

An unmodified `eframe::App::ui` callback may still call
`show_single_surface` directly for rendering, selection, close, menus, and
discrete keyboard or AccessKit actions. Because eframe owns the surrounding
`RawInput` and `FullOutput`, that response reports
`PresentationSettlementRequired`; tab/group/contained pointer drags are not
started instead of showing a preview that can never settle. The fork-backed
native adapter owns the stronger post-plugin finalizer and exact unwind
abandonment required for editor-grade native multiview.

Dear ImGui and `dear-imgui-rs` are behavioral references for frame phases,
viewport lifecycle, wakeups, geometry/DPI facts, and teardown coverage. Their
global context registries, raw-pointer ownership, timeout focus heuristics, and
renderer-owned docking authority are intentionally not copied.

## Development

The workspace uses Rust 1.95 and edition 2024. Reuse the normal target directory
and run Cargo serially when the machine is busy.

```text
cargo fmt --all -- --check
python3 scripts/check_core_dependencies.py
cargo check --workspace --all-features --all-targets --locked -j1
cargo nextest run --workspace --all-features --all-targets --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked --test-threads=1
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --test-threads=1
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --features serde --test persistence --test-threads=1
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --all-features --test-threads=1
cargo package --package dockspace --package egui_dockspace --all-features --locked -j1
```

The fork-backed native workspace uses one thin Cargo launcher:

```text
python3 scripts/run_egui_fork_harness.py
```

To run the supported official-crates.io tier in a real window, use the small
caller-owned `egui-winit`/`egui-wgpu` host. It calls
`egui_dockspace::run_ui` around the complete frame, so tab drag, magnetic dock,
and contained-window movement have terminal-output settlement without any fork:

```text
cargo run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --bin official-caller-owned-demo --locked -j1
```

The ordinary `crates/egui_dockspace/examples/basic.rs` example demonstrates the
honest unmodified-eframe boundary: rendering, selection, close, menus, and
keyboard adjustments remain available while the status strip explains why
preview-qualified drag is disabled:

```text
cargo run --manifest-path crates/egui_dockspace/Cargo.toml --features example-app --example basic --locked -j1
```

To exercise draggable tabs, magnetic docking, contained movement, and the
fork-backed exact-output path, run the separate native example. It starts with
eight stable panes for documents, scene hierarchy, assets, console, history,
profiling, a drawn preview and editable Inspector notes. The Inspector group
starts as an egui-styled contained window in the root surface; it does not open
a child OS window automatically:

```text
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --example basic --locked -j1
```

Add `--features serde` to enable application-owned named JSON presets. The
example's preset profile requires all eight panes in the root host and waits
for current presentation and native cleanup before saving. Default layout works
without serde. Both operations keep application pane data outside the layout.

The example exposes all three presentation paths without conflating their
evidence. Use the contained title bar to move or resize the same-surface window,
or open its **⋮** menu for **Dock Back** and **Move to New Window**. The toolbar
duplicates the native move and dock-back commands as explicitly labelled
programmatic fallbacks. On a capability-qualified backend, drag a tab or the
contained title bar outside an OS window to request a physical native child,
then drag a child tab onto a root-window docking guide to return it. The default
child close policy recovers the previous presentation. Physical dragging
remains fail-closed on backends without complete cross-window pointer facts, so
the programmatic lifecycle demo is not described as physical-input evidence.
The pinned macOS Winit facts are tested for same-window routing, but macOS does
not advertise cross-surface physical promotion until an OS-input lane proves
the complete button/capture contract. Windows likewise keeps hover and desktop
pointer authority unavailable until equivalent platform-fact tests land. X11
is the current certified physical reference backend.

The finite `--smoke` mode locates real editable content and checks application
text through native first-live, Dock Back, OS-close recovery and a fresh Default
presentation. With serde it also restores an unloaded pane's placeholder,
reloads it and verifies another real focused edit. In an isolated
Linux X11 display with a window manager and `xdotool`, run
`python3 integration/egui-native-smoke/editor_x11.py /path/to/examples/basic`
after building the example. The external driver supplies actual pointer,
keyboard and window-close input; diagnostics locate the controls without
granting docking or presentation authority. This editor workflow does not
certify physical drag or other operating systems.

The [platform support matrix](docs/PLATFORM_SUPPORT.md) separates the public
engine-host contract, official-egui output limits, backend capability bits and
the exact scope of real-window/physical qualification.

CI keeps those claims separate. The programmatic Glow smoke proves hidden
creation, staging, first-live admission, redock, and retirement through product
commands. The X11/XTest smoke locates only read-only semantic receivers, injects
real OS pointer events through Winit, and proves drag-out, first-live, the exact
Center-guide control, three non-guide pane-center points and four non-guide
pane-edge points, both tab-half insertion sides, two nested-leaf targets, real
merge/split topology, OS-window destruction, and
retirement quiescence. The center cases use
`EGUI_DOCKSPACE_PHYSICAL_TARGET=pane-center-a`, `pane-center-b` or
`pane-center-c`; the edge cases use `pane-edge-north`, `pane-edge-south`,
`pane-edge-east` or `pane-edge-west`; tab insertion uses `tab-half-before` or
`tab-half-after`; the fixed two-axis nested layout uses `nested-pane-center` or
`nested-pane-edge` (west). Leaving it unset runs the guide control.
Each pane case excludes all actual guide hit bounds and the outer-root band
after physical rounding and again before release. Edge cases require the
matching exact half-root preview and ordered, equal-weight split in the original root.
Tab halves exclude Close Main and actual guides, require the narrow insertion
marker and verify the precise resulting tab order. Nested cases preserve the
empty central source, ancestor weights and unaffected sibling; west insertion
must split the target leaf, retaining the three alternating split axes.
These finite native smokes do not certify overflowed/clipped tab insertion,
arbitrary nested targets, mixed DPI or the complete local journey matrix. They do not expose core
geometry or provide a reusable GUI test engine.

The optional high-rate extension sets `EGUI_DOCKSPACE_PHYSICAL_LOAD=1khz`
together with one explicit pane-center target. It establishes the real Dock
preview first, sends 1,000 distinct XTest motions with no catch-up burst, then
releases immediately without an inspection or drain-to-idle barrier. The
measurement requires the exact source and desktop-point sequence, reports
actual injection/native callback rates, and bounds release-to-terminal commit
using the earlier callback and later model observation. The qualified local
profile is an optimized build with 2 CPU and `LP_NUM_THREADS=1` for software GL;
unoptimized and renderer-oversubscribed attempts remain recorded failures.
This is finite XTest/event-loop evidence, not a hardware polling-rate claim or
the separate 2 ms reducer-service qualification. See `docs/VALIDATION.md`.

The real output-pressure fixture also measures the synchronous host interval
from output admission through the last pass finalizer's return, including
commit, effect scheduling and fixture overhead. It reports repeated passes,
child/root samples, deferred admission and observed overruns separately.
Standalone native host-service events now report their own elapsed samples,
so moving terminal settlement outside UI does not hide that work. GPU
and terminal waits are excluded; these finite samples establish neither a hard
2 ms limit nor return to the OS event loop.

## Packaging and release order

`dockspace` must still be published before `egui_dockspace`, because the
adapter's packaged manifest resolves its declared core dependency from the
registry. Cargo 1.95 can verify both packages before publication when they are
selected in the same `cargo package` command. Its temporary local registry
supplies the matching core package while it verifies the adapter's normalized
manifest.

The 2026-11-01 local gate packages and verifies both crates, then builds independent
consumers against extracted archives. The core consumer passes its default and
serde suites and runs the self-drawn host example; the adapter consumer passes
the official egui 0.37 suite. The only patched registry dependency is `dockspace`
from the extracted core archive; the egui family remains on crates.io.
This qualifies package contents and downstream use, not registry publication or
the optional native workspace's unpublished fork dependencies. Exact evidence
and the retained lockfile warning are recorded in
[`docs/VALIDATION.md`](docs/VALIDATION.md).

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT
OR Apache-2.0. See [`THIRD_PARTY.md`](THIRD_PARTY.md) for prior-art and fixture
provenance.
