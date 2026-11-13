# Native multiview

Read `docs/PLATFORM_SUPPORT.md` and the native-hosting sections of
`crates/dockspace/docs/guide.md`. For the egui adapter, also read
`crates/egui_dockspace_native/README.md` and its `examples/basic.rs`.

## Select a supported native host

The egui native adapter is experimental and uses the pinned forks in
`integration/egui-fork-workspace/Cargo.toml` and its lockfile. Use that workspace
to run its examples and verify a native consumer. Keep its patches scoped to
the native consumer; official egui's single-window adapter has a separate host
path. Read the current platform matrix before promising physical cross-window
dragging, global desktop coordinates or mixed-monitor DPI behavior.

`solar_system` exercises programmatic creation, movement, Dock Back and
recreation. It supplies useful lifecycle coverage; physical OS dragging and
native close-button behavior require their own real-input checks. The GPUI
harness is a retained single-window consumer; its presence does not establish
GPUI native multiview coverage.

## Compose the current winit helpers

For a framework that owns its platform implementation, use
`dockspace::runtime::NativeWindows<K, T>` with the framework's window IDs. It has no UI or OS backend
dependency. It manages resources and committed binding continuations; the
framework still supplies its real observations, frame fences and callback
retirement. It is an unbounded container and does not replace an adapter's
prepaid storage guarantees.

For a custom winit host, keep `MouseInput` and `RepaintScheduler` per window,
`SurfaceCoordinates` per logical placement, and one shared docking session.
Dispatch by `WindowId`, retain exact native bindings across delayed callbacks,
and take the earliest deadline across windows for the event loop's wait. A window
with several logical surfaces needs a placement roster rather than one fixed
window-to-surface assignment.

With the optional `native-windows` feature (included by `native`), `NativeWindows<T>` indexes application
resources by exact core-issued window bindings and applies committed Root
continuations through the same shared implementation. The separate
`native-geometry` feature supplies `observe_window` for geometry and known presentation facts;
the physical placement helpers compensate actual decoration extents. Keep the
logical placement roster separate from that physical resource registry.

Follow `crates/dockspace_winit/examples/multiview.rs` for a real-window software
host, including failed renderer preparation and compensation, first-live
presentation, Dock Back, close recovery and shutdown. The example uses
programmatic commands and X11 on Linux. Native effect settlement, readiness,
actual destruction and cross-window capture/hover facts remain host obligations.
Stock winit mouse events do not
supply original OS timestamps or authoritative global pointer/capture facts;
integrate the platform observations required by the core's managed native path.
Use one input path for each physical event, including when the UI framework has
its own winit bridge.

## Connect a custom native host

Retire the local pointer producer before enrolling a managed native host through
`enable_managed_native_host`, using real initial pointer facts and supported
capabilities. Register existing external Root windows with `register_native_root`
and retain their committed bindings. Publish complete window observations through
`report_managed_native_snapshot` and service recorded input with
`poll_native_host_frame`, answering receiver queries from actual host facts.
Respect `native_input_pending()` when a processing budget leaves an accepted
prefix unfinished. Follow the [custom host](custom-host.md) output settlement
path alongside native effects and lifecycle reports.

Use these independent public-consumer tests as API recipes:

- `integration/dockspace-host-harness/tests/direct_prepared_native.rs`: native enrollment, creation and acknowledgements.
- `integration/dockspace-host-harness/tests/native_local_pointer.rs`: event-time local coordinates and native input.
- `integration/dockspace-host-harness/tests/native_pointer_handoff.rs`: source capture and cross-window hover handoff.

## Preserve native identities and lifetimes

A logical `SurfaceId`, a docking `RootId` and an OS-window lifetime are separate
identities. Use core-issued native bindings for the exact window incarnation;
reused OS handles cannot refresh an old binding. An external Root window can
host multiple logical placements. Publish its complete placement roster
atomically before measuring, and observe each physical window once.

Execute typed native effects through the host and return actual acknowledgements
and observations. When core retains a pending native establishment or presentation
transfer, keep source ownership until its required exact first-live presentation
commits. Follow the emitted transition: ordinary logical moves can commit in their
input update. Created/Prepared state, a show request or a frame count cannot finish
a pending transfer. Keep callback/output lanes alive through retirement
and report quiescence before reclaiming the old lifetime. On close or destruction,
account for the whole window roster, deferred vetoes and recovery anchors.

OS-window focus and placement remain observations. Raising a contained group,
activating a native window and changing its window level are separate operations.
Read the adapter's window-options API for the needed stacking behavior; a managed
Child's lifecycle ownership does not by itself imply a transient OS parent or a
relative z-order guarantee.

## Use the egui native command slot

Construct `NativeDockspaceApp` with a session, host surface, pane provider and a
style that maps to the session's presentation configuration. Keep registry/view
callbacks short: they hold the runtime lock. Call `request_command` after those
callbacks return. Committed notification callbacks run outside that lock and
expose scoped access through `NativeNotificationContext`.

Admission queues one command. Consume `take_action_status()` before sending the
next command or waiting for quiescence. On rejected admission,
`NativeCommandRequestError::reason()` identifies the failure and `into_command()`
returns the unchanged command. Retain a Busy command until progress frees the
slot; Stopped requires runtime recovery, and HostSurfaceRequired requires a
request/layout that retains the fixed host. Stored tabs groups retain their
captured session/version through retries. Semantic action completion and
`is_surface_presented()` answer different questions.

Use `pending_work()` to diagnose blocked progress and `output_backlog()` to
inspect outstanding output owners. Drain obligations according to the native
lifecycle rather than closing every window immediately during shutdown.

## Verify the implemented capabilities

Test creation failure, exact first-live transfer, Dock Back, deferred close/veto,
unexpected destruction, restoration/recreation and shutdown for the supported
paths. Exercise at least two windows with real input and rendering when claiming
native interaction support. Report programmatic lifecycle, physical drag and
platform coverage separately, following the current qualification matrix.
