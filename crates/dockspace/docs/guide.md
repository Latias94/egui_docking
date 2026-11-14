# Engine host guide

[Crate overview](../README.md) · [Examples](../../../docs/EXAMPLES.md)


- [Host integration](#engine-host-integration)
- [Logical surfaces](#incremental-logical-surfaces) and [nested splits](#nested-split-resizing)
- [Pointer input](#pointer-device-and-event-time), [stationary interaction](#stationary-interaction) and [touch](#local-touch)
- [Native input budgets](#budgeted-native-input) and [window recovery](#native-creation-failure-and-recovery)
- [Close decisions](#application-close-decisions) and [arrangement locking](#arrangement-locking)
- [Layout defaults and persistence](#application-defaults-and-layout-reset)
- [Feedback and focus](#manipulation-feedback-and-focus)

`dockspace` is a renderer-neutral docking library for custom editor UIs,
including self-drawn game-engine interfaces. `egui_dockspace` is its interactive
reference adapter. The core owns
docking topology, validated mutations, layout and paint plans, interaction
state, close decisions, persistence identity, and native-surface lifecycle
semantics. Renderers supply measurements, draw the returned plans, report
presentation results, and execute typed platform effects.

The application interface is deliberately item- and surface-centric. Internal
node identities, raw reducer inputs, provider bookkeeping, and backend state
machines remain private. Renderer hosts use the typed `runtime` facade and its
opaque output handles; the raw engine and protocol modules are not a supported
integration surface.

Hosts can consume the ordered surface program immediately or compile it into
owned display lists or retained UI updates. Both use the same output identity
and presentation rules. The independent consumer in
`integration/dockspace-host-harness/` demonstrates this contract using only
public `dockspace` APIs; it does not depend on egui or an OS window.

```rust
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;

let item = ItemId::new(1);
let surface = SurfaceId::new(1);
let root = RootId::new(1);
let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
    surface,
    DockspaceRootLayout::new(root, DockspaceNode::central_tabs([item])),
)])?;
let session = DockspaceSession::from_layout(layout, DockPolicy::default())?;

assert!(session.view().item(item).is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Engine host integration

See the [adapter testing guide](../../../docs/ADAPTER_TESTING.md) for integration
responsibilities, retained UI publication and reusable semantic scenarios.

Application commands submitted through a host frame return a
`DockspaceSubmittedAction`. Keep each identity when batching commands, then use
`HostFrameReport::submitted_action_outcome` to obtain its exact committed result.
A successful submission means the candidate accepted the input; policy or
availability can still reject the product operation at commit. The identity
belongs to that frame attempt and does not resolve against a later report.

After commit, `report.notification()` exposes one application notification:
`layout_change()` summarizes all durable layout edits, while `events()` gives
ordered selection, membership, placement and close details. Split ratios and
contained bounds can change without a detailed event. For debounced document
persistence, use `notification.document_changed()`: it also covers committed
native-window placement without advancing the docking-layout version.
`native_window_placement_changed()` reports known outer bounds, client size,
native scale, first-live admission and geometry becoming authoritative again.
Renderer zoom and focus do not independently trigger a save. Rejected/no-op
actions, pending close decisions, pure rendering and unpublished frames do not
dirty the layout. Hosts own notification delivery and storage; core retains no
notification history.

To move a complete tabs leaf, capture it once with
`session.capture_tabs_group(DockAnchor::Item(item))`, then use
`prepare_dock_tabs` or `prepare_float_tabs`. The resulting action uses one atomic
payload and retains tab order and selection. Capture freezes session and
workspace authority; preparing or submitting later does not refresh a stale
group or look its old anchor up again. `DockspaceHostFrame` supplies the
corresponding `dock_tabs_current` and `float_tabs_current` conveniences.

1. Begin a host frame, submit ordered input and application commands, then
   begin encoding and supply surface measurements in logical coordinates.
   A surface is a logical layout and input domain; it need not be a platform
   window. Items and roots retain their stable application identities.
2. Obtain `SurfaceWork::Ready(SurfacePaintPlan)` and call `execute_surface_program` to accept its
   complete ordered program into your sink. Copy the required geometry,
   identities, and pane-content references into owned host records if rendering
   will happen after the frame borrow ends. `SourceGap` records preserve semantic
   identity while suppressing ordinary visible source paint. Measurement and
   encoding use the same private candidate. `Deferred(Unmeasured)` requests
   measurements; `Deferred(OutputCapacity)` is normal bounded backpressure
   before content callbacks, not a failed input update.
   An immediate UI can call `reserve_surface_output` before consuming ordinary
   pane input. This reserves the same frame's output room without sealing its
   pointer or semantic input. If it returns `false`, retain the input and wait
   for renderer settlement. A suspended update keeps the reservation; later
   measurement and `surface_work` still validate the actual output endpoint.
3. Pass the returned `SurfaceProgramExecutionReceipt` to
   `confirm_surface_painted`, then commit the frame. Frames containing only
   input or presentation reports can commit directly without painting.
   A receipt proves complete sink acceptance; the committed
   `PaintedSurfaceOutput` identifies that exact emission. Neither proves renderer
   submission or presentation.
4. After the host knows the final result, consume that output with
   `report_surface_presentation(Presented | Dropped)`. Report `Presented` only
   when the exact output reached the host's presentation boundary. An output
   abandoned by dropping its handle is retired as dropped.
5. Commit a subsequent host frame to accept the ordered presentation prefix.
   Only this successful core commit can grant receiver authority. Use
   `surface_presentation_commits` to promote or retire matching host artifacts,
   and `bind_presented_receiver` to bind current descriptors. Later results may
   arrive first, but cannot bypass an unsettled earlier output in the same
   stream or resurrect an obsolete artifact.

Application commands and prepared surface actions can operate without native
registration. For pointer input, call `enable_host_pointer` once on the session
and submit `HostPointerInput` records through
`DockspaceHostFrame::submit_host_pointer_batch`. The host assigns one total order
across its logical surfaces. Each record carries its own event, capture,
manipulation route, hover route, and independent receiver facts. Bind a
`HostPointerRoute` to an exact `PresentedDockspaceSurface` and event-time point;
do not reuse a final-frame position for earlier edges.

When the host proves that no framework widget blocks docking at an edge, use
`SurfacePointerReceiverFacts::with_clear_hover_lane` with that exact presented
surface. Core resolves the semantic hover winner after earlier edges have
reduced, so a single batch can suppress its complete moving source without
guessing a target in advance. `with_hover` and `with_no_hover` assert an exact
semantic winner or absence instead; they are not framework-clearance fallbacks.
Unknown widgets and overlays must remain blocked or unknown.

Input and drawing share logical surface geometry. The host interprets its
platform's input units and maps each event into logical coordinates relative to
the delivery and hover surface origins. Keep that mapping independent from
render-target resolution. The public `host_coordinate_domains` fixture varies
window DPI and framebuffer density separately, through immediate and deferred
record consumption, while capture keeps the source route outside its bounds.

During capture, manipulation coordinates remain in the source surface while
hover coordinates may name another surface, including one with identical local
bounds. Capture belongs to the host producer. Unknown logical routing never
proves that the desktop pointer is outside all windows. `disable_host_pointer`
drains and retires the producer; dropping or rejecting a host frame leaves its
committed input watermark unchanged.

A complete press/move/release batch commits an ordinary logical edit in its
successful update and emits delivery once. A later press or `Dropped` output
does not undo that edit. Input remains bound to its actually presented source
and hover snapshots, revalidated against their relevant current semantic scopes.
Native resource creation or retirement retains its separate exact first-live
and source-reservation rules. Pending-presentation queries describe those
obligations and never grant input authority.

Complete contained sources and resize previews include real content, chrome,
hit and semantic geometry in the surface program. Renderers should not add a
second ghost shell when a preview reports `is_layout_projected()`. Release
writes the durable rectangle in the accepted update. All admitted outputs own
their exact immutable scenes until terminal settlement; the default capacity is
64 outputs and can be configured explicitly.

An update can release its session borrow with `suspend`, retain one owned
`DockspaceUpdate` across renderer hooks and resume that same candidate. A
competing update is rejected while it is outstanding. Repeated actual encoding
passes use `restart_encoding_pass` without replaying input; discarded receipts
cannot confirm the replacement pass. `DeferredSurfaceAction` retains the
original semantic witness for actions produced during encoding and validates
its relevant scope in a subsequent update.

## Incremental logical surfaces

A custom renderer may add an empty docking container with
`frame.attach_empty_surface_current(surface, root)` and remove it with
`frame.retire_empty_surface_current(surface)`. Both return a submitted-action
identity; inspect the exact `HostInputOutcome::LogicalSurfaceAttached`,
`LogicalSurfaceRetired` or `LogicalSurfaceRejected` in the committed report.
An attached surface owns an empty central main root and can receive content
through the ordinary `DockAnchor::Central(root)` docking actions in the same frame.

These operations use the existing atomic workspace transaction, keep its epoch,
and preserve unaffected surfaces' current input proofs. Retire checks all main
and contained content. Close or migrate nonempty content before retrying; native
bindings, recovery owners and drag reservations must finish first. Reusing a
retired semantic surface/root ID requires fresh measurement and presentation;
predecessor receivers and late outputs cannot authorize the new container.

Host-frame candidates share the immutable canonical workspace until a checked
command needs its existing staging copy. Abandoning a frame still publishes no
partial topology, lifecycle, input watermark or presentation authority. Other
ledgers retain their existing owned representation; this is a measured, local
copy optimization rather than a second mutation authority.

## Nested split resizing

Dragging a separator moves the descendant boundaries which touch it. Descendant
siblings away from that boundary keep their projected pixel size. An orthogonal
split passes the moving edge through all its children; a split on the same axis
passes it only through its first or last child. This also applies to N-ary splits
and junctions with several incident separators.

The exact presented layout supplies the complete geometry and constraints.
All affected minima and maxima limit one shared displacement per axis before
weights are reconstructed. Pointer gestures, splitter adjustments and junction
adjustments use the same solver and publish their split updates in one atomic
workspace mutation. Zero effective movement preserves the original persistent
weights, including a gesture which moves away and returns to its starting point.
An overflowing, underallocated or compressed separator projection cannot begin
a resize; clipped hit geometry is never substituted for complete layout facts.

Pane minima describe useful content viewports, not full document sizes.
`DockPresentationConfig::builder().pane_overflow_policy(...)` chooses what
happens when their sum exceeds the available split extent:

- `PaneOverflowPolicy::Clip` (default) preserves minima and clips the layout to
  the surface; some panes may have no visible area.
- `PaneOverflowPolicy::Compress` reduces child extents in proportion to their
  minima so they fit. Compressed separators cannot be adjusted until their
  original minima fit again. The constraints and persistent split weights stay
  unchanged; enlarging the surface restores ordinary allocation.

Both policies use the same solver for drop previews and committed layouts.
Very small surfaces can still leave no room for content or tab chrome. Hosts
should offer a way to enlarge the surface or restore an accessible layout.
Scrolling, text wrapping and canvas navigation remain content concerns.

Hosts can prepare `SurfaceSplitterAdjustment::Minimum` or `Maximum` for a
separator or one junction axis, in addition to ordinary `Decrement`/`Increment`
steps. Endpoints use the same full-layout limits; they do not collapse panes.
`SurfacePaintPlan::splitter_resize_range` resolves current/minimum/maximum
fractions for a separator from its uncut geometry and all touching descendants.
This is an on-demand solver query for numeric accessibility or application
controls; ordinary drawing does not need to compute it for every separator.

## Pointer device and event time

Local and native pointer inputs accept `with_metadata(PointerEventMetadata::new(
PointerDeviceKind::Touch, observed_at))`. The timestamp is a `Duration` from one
nondecreasing host clock origin for the whole provider incarnation. Equal times
are valid within a batch; events from different pointer IDs still share that
ordering. The journal rejects a decreasing time or a change between known
device kinds within one live pointer stream without advancing its candidate
frontiers. Stream end permits pointer-ID reuse with a different device; overload
recovery preserves the provider clock, while a new provider may use a new origin.

Omitting metadata preserves Unknown device/time. Unknown samples do not inherit
another edge's observations or reset the retained clock. These fields retain
input facts; they do not independently authorize a timed mutation. Hosts must
not use a timer to fabricate pointer motion or claim current receiver authority.
`PointerEventMetadata::untimed(device)` preserves an observed device class when
the event timestamp is unavailable.
The official egui owned-run adapter forwards explicit `RawInput.time` as one
batch observation time, preserving Unknown when it is absent. It does not
claim per-OS-event timestamps or infer device class from generic pointer events.

Hosts can map a modifier or tool mode to
`PointerEventMetadata::with_docking_enabled(enabled)`. A missing directive
preserves the current gesture's intent; a new press starts enabled unless it
carries an explicit directive. Suppression removes every docking preview, guide
and target latch, including explicit guide and tab-gap targets, while preserving
capture and contained-content position. Re-enabling permits fresh acquisition on
a later real Move or Release; it cannot restore an earlier preview or change an
already frozen release.

For modifier-only input, submit
`SurfacePointerEvent::DockingEnabledChanged { enabled }` in the same ordered
host/surface batch and name the matching active pointer. This semantic control
requires no route, receiver or invented motion and does not advance pointer
sequence numbers. Managed native hosts instead record
`report_native_docking_enabled(binding, enabled)` in their existing joined
input order. It applies to that provider's active gesture only when the exact
current binding is its source or the known keyboard-focus window; it does not
fan out over devices or infer coordinates, buttons or capture.

## Stationary interaction

During a local drag or touch press, `DockspaceSession::pointer_continuation()` can return one
finite wake request. Schedule its deadline using the same clock origin as pointer
metadata. At wake time, query the request again and submit `request.event()` for
`request.pointer()` through the normal host or surface pointer input. Supply the
actual current observation time, capture and independently qualified manipulation
route; resolve fresh hover facts at `request.position()` on `request.surface()`.
The deadline itself does not prove that time advanced, and a retained receiver
from an earlier output is not a fresh answer. Do not synthesize a `Moved` event.

Drain accepted raw motion and terminal input before dispatching a due wake.
Motion updates the challenge without executing timed selection; re-query the
committed request after that batch and submit a surviving due continuation as
its next ordered edge. This keeps a same-batch release from losing its target
to a timer action, while permitting selection during continued motion.

The core consumes only its current gesture's opaque token. A dropped candidate
can retry it; successful execution, target loss, blockers or cancellation make
old tokens inert. A continuation cannot create a new pointer stream. Equivalent
repaints of the same semantic geometry and stream preserve the dwell start,
while execution still requires exact current presentation authority. A stream
takeover or changed scene cannot borrow the prior interval.

Tab hover selection has a default 150ms delay controlled
by `DockPresentationConfig::builder().hover_selection_delay(...)`. `None`
disables it. Selection keeps the active drag and requests no pane-content focus;
only a subsequent normal release can deliver the payload. Native stationary
input still needs independently captured routing and event-time facts.

A native desktop drag's continuation exposes core's frozen source-window
treatment through `source_input()`. This read-only `NativeDragSourceInput`
projection does not choose a mode, change the OS window or grant capture.
`DockspaceSession::native_pointer_capture_request()` exposes the current mouse
drag's exact capture requirement even before a finite stationary action exists.
Its identity remains stable across hover targets and changes for a new gesture.
Drain accepted callbacks and commit pending input before comparing it with a
retained request; a temporary `None` while input is pending does not complete
native release or cleanup. The host must retain the original native press and
independently settle its own capture operation.
When a host explicitly acquires fresh capture after the drag began, submit
`capture_acquired_event()` through the ordinary native input order with its
actual observation time and independently proven source delivery/capture.
Acquisition starts a fresh stationary interval; it never executes the previous
deadline, selects a tab, scrolls or delivers the payload. An older challenge
in the same drag may postpone the current target, while local-lane and other
gesture tokens cannot authorize acquisition. Repeating an accepted acquisition
does not reset the interval again. Missing facts clear timed work while known
capture loss still cancels the gesture, including observations on an obsolete
request. Hosts which already prove continuous capture need no acquisition
event. OS acquisition, loss and exact resource release remain host obligations.

Local active drags also scroll an overflowing tab strip while stationary near
its content edge. The default band is 24 logical points and the interval is
50ms; each accepted continuation advances at most one existing scroll-line
extent (40 points by default). Overlay controls are removed from the content
viewport, and each band is capped at half the remaining width with deterministic
midpoint direction. Close buttons, enabled or disabled strip controls, popups,
foreign blockers and ambiguous receivers prevent automatic scrolling.
`tab_strip_edge_scroll_band(0.0)` or `tab_strip_edge_scroll_interval(None)` disables
it. A zero-duration interval is rejected.

Edge scrolling and hover selection share one pending drag continuation, with
edge scrolling taking priority. Each step revokes its old token and waits for
the exact newly compiled output to be presented before returning the next wake.
The host still supplies fresh current-output receiver facts for every step.
`pointer_continuation_awaits_presentation(surface)` tells a deferred renderer host
when its terminal output result must wake the host to accept that result; it is
not a request to repaint continuously. A dropped candidate frame is retryable;
stale receipts, blockers and cancellation do not grant another step.

Manual scroll input, strip commands and menu changes revoke automatic work.
An active manual scroll sequence owns its strip until its terminal boundary;
fresh drag motion can reacquire an edge afterward. Scrolling changes neither
workspace selection nor pane focus. During dragging, the shared offset commit
reuses valid measurements to compile fresh geometry, preserving the source
gesture while keeping target authority behind the normal presentation barrier.

## Local touch

An explicitly classified local `Touch` contact waits on the existing pressed
gesture owner when it begins on a tab body, group grip, contained title, resize
handle or splitter. Pressing alone does not select, raise, focus or change
geometry. The default long-press delay is 350ms and the movement slop is 8 logical
points; configure them with `touch_long_press_delay` and
`touch_press_slop_distance` on the presentation builder.

A timely continuation arms the contact without changing selection. Moving beyond
the slop after arming enters the ordinary tab, splitter or contained manipulation
path, including its exact-output release barrier. A stationary armed
`ContactEnded(Primary)` requests a semantic menu through
`HostInputOutcome::Touch(DockspaceTouchEvent::ContextMenuRequested(...))`. The
request contains the published workspace version, pointer, surface, root,
optional item and logical point, with opaque equality identity for deduplication.
It does not select the item or execute a menu command.

An early contact end within slop activates the original source through the
shared selection/raise/focus path and consumes the gesture once. Pre-arm motion
beyond slop yields to application content without selecting or starting a drag.
Ordinary close and strip/menu controls retain short-tap semantics and apply the
same slop to both motion and release; they have no docking long-press deadline.
Unknown coordinates never prove motion or activate a frozen source. Untimed
touch can tap or yield, but cannot earn a timed long press.

The host retains responsibility for ordinary pane input and actual scrolling;
the core does not replay or synthesize content events after yielding. End the
actual contact with `ContactEnded`, or use explicit stream cancellation for
platform loss. Late terminals cannot create a contact or repeat an action.
For an explicitly observed Touch contact, query
`host_pointer_hit_for_device(surface, point, PointerDeviceKind::Touch)` and retain
that device class in each input edge. The same exact-output manifest supplies
expanded splitter, junction and contained resize targets to the query, receipt
validation and gesture preparation. Mouse, Pen, Unknown and `host_pointer_hit`
retain the base hit geometry.

`touch_resize_hit_extent` defaults to 44 logical points and accepts finite values
of at least 44. It specifies the requested extent before clipping, and never
reduces a larger configured base target. Surface/root bounds, title/tab/close/menu
chrome, visible strip controls and front contained windows may reduce a target
or leave it empty. A crowded highest-priority overlap is unavailable; structural
junctions still take precedence over ordinary handles. Contained targets may
extend outside the painted frame, while its drawing, durable bounds and drop
occlusion remain unchanged. Resize displacement starts at the real contact
point, including an outside-frame press.

Receiver descriptors continue to describe the base control, whose clipped
rectangle may be empty while a touch fragment remains visible. It is not a
bounding box for the touch fragments and cannot replace the device-aware
query or exact receipt validation. Framework widgets still need their ordinary
clearance proof. Native touch event-time/stationary capture remains separate
qualification work.

## Native Window Resources

Hosts with their own platform backend can use
`runtime::NativeWindows<K, T>` directly, without winit or another crate. `K` is
the framework's window ID (`Copy + Eq + Hash + Debug`); `T` holds the host's
window and renderer resources without `Clone`, `Send` or `Debug` requirements.
The optional `native-windows` feature on `dockspace_winit` specializes this
registry for winit's `WindowId`.

```rust,no_run
use dockspace::runtime::{NativeWindowBinding, NativeWindowInsertError, NativeWindows};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct AppWindowId(u64);

struct WindowResources {
    // Application-owned window and renderer state.
}

fn register(
    windows: &mut NativeWindows<AppWindowId, WindowResources>,
    binding: NativeWindowBinding,
    id: AppWindowId,
    resources: WindowResources,
) -> Result<(), NativeWindowInsertError<AppWindowId, WindowResources>> {
    windows.insert(binding, id, resources)
}
```

Resolve a framework ID with `binding(id)` at synchronous event delivery. Retain
that exact binding through deferred work and use `get(binding)` on completion,
so an old callback cannot address a replacement window that reused its ID.
The ID must include a framework generation, or the host must drain the old event
lane before reusing it. Looking up a bare reused ID after queuing cannot establish
which native lifetime originally produced that event. Keep retiring IDs reserved
until their original event lane is drained.
Rejected registrations return the original resource through `into_value()`.
Apply `continue_root(pair)` only for continuations in a committed host report.
The registry stores one physical window independently of its logical surfaces.
Several continuation pairs can share that physical window. `Ok(false)` means
that its resource entry did not move on this call; each distinct predecessor
surface still needs its own output and callback retirement. Keep those obligations
when the old window lookup has already moved to its successor.

Removing resources does not prove native destruction or callback quiescence.
Effects, window observations, renderer preparation, output settlement and
retirement remain part of the native host protocol below. This unbounded
container does not replace an adapter's prepaid callback storage guarantees.

## Budgeted Native Input

Declare backend support through `runtime::NativeHostCapabilities` and supply
independent event-time facts. Core derives a source mode from exact ownership
and complete payload: Preserve content drags do not require whole-window
`PointerHitTestControl`; PassThrough drags do. Inventory, hover, desktop pointer,
button state and hit observation remain required for both. Native tear-off adds
lifecycle and exact placement requirements. Core freezes the mode in the
existing route and retains it through pending release and placement-proof
validation. Hosts supply facts and execute effects without selecting an
internal source mode or constructing a placement proof.

Buffered native hosts can create a shared `NativeMonotonicClock` and start one
`NativeInputBudget` before claiming an immutable input prefix. Pass that same
budget to `DockspaceSession::poll_native_host_frame`; do not reset it between
translation, receiver queries and reduction. A budget permits 256 completed
records or two elapsed milliseconds, whichever comes first. The time limit is
cooperative: an already started record and its exact receiver queries finish
before yielding, even if that record alone exceeds the deadline.

`None` means core retains an unpublished candidate for the next scheduling turn.
Do not treat it as an error or an empty queue. A returned frame is committable,
but `native_input_pending()` may still be true. Render/settle that prefix, retain
the host's input/output barrier, and poll again with the next scheduling budget.
Only a committed report with `native_input_pending() == false` permits releasing
that barrier. Earlier accepted native input gates application semantic and
configuration commands without advancing their source sequence.

`record_native_pointer_sequence` admits all edges or none while preserving a
commit boundary after each edge. `record_native_pointer_batch` instead preserves
one indivisible commit group, suitable for complete terminal rosters. A large
group can pause computation across polls but cannot publish a partial roster.
Later accepted records never join the already frozen candidate. Dropping a
returned frame or failing an attempt leaves its input retryable.

Attach the original callback observation time and observed device class with
`NativePointerInput::with_metadata`. Use one monotonic clock origin for the
provider; queued translation and frame retries must not resample that time.
Receiver queries expose the retained facts through
`NativeReceiverQuery::metadata()`. These facts identify the edge being resolved;
they do not grant local coordinates, desktop routing or stationary authority.

Managed native input uses one joined recorder for desktop and native-local
locations. Obtain `NativeLocalCoordinates` with
`DockspaceSession::native_local_coordinates(binding)` after the binding's
positive client size and native/presentation scales have committed. This opaque
capability uses the existing coordinate generation and exact presentation host;
recorded but uncommitted facts cannot grant a newer capability.

Freeze `NativeLocalPointerFact` where the physical client point is generated,
including that event's client size and native scale. At callback capture, pass
the actual `NativeLocalGeometry` and exact binding to the capability's `capture`
method. Event geometry, callback geometry and committed geometry must agree.
The resulting opaque sample retains the original physical point and its one-time
conversion through the presentation scale. A queued old event cannot be rebound
to a newer scale. Submit it with `NativePointerInput::new_local`; if coordinates
cannot be proven, use `NativeLocalPointerLocation::Unknown` while preserving the
real native delivery and capture facts, particularly for release and cancel.

Each validated press freezes its coordinate lane on the existing gesture owner.
Native-local input can query only the same native endpoint and retains real
native capture and binding-retirement obligations. A finite point outside the
client area is preserved for the terminal edge, but does not grant desktop
outside-all authority. Coordinate withdrawal or an incompatible lane cancels
the old gesture without swallowing its physical terminal. After completion,
the next gesture on the same pointer can choose its lane again.

An explicit `NativeLocalToDesktopHandoff` retains the original local sample and
the desktop position/hover from that same event. Submit it through
`NativePointerInput::new_handoff`; recording does not inspect the active session
or grant an upgrade. Only Moved on an active native-local docking drag can
upgrade after exact source binding/generation, original Primary press, capture
continuity and same-point mapping through observed desktop content bounds all
hold. The existing stream, session and semantic source remain the owners. Core
rebinds its source continuation and discards old preview, target acquisition,
affordance, timer and position state. A destination on another window uses its
own desktop hover projection, with separately confirmed physical routing and
exact target-output presentation. A rejected packet follows its original local
event; the real release/cancel remains consumable. No ordinary Desktop packet
implicitly upgrades a native-local gesture. The public host API does not
establish platform callback qualification. The
reference adapter's committed capability, callback geometry and event-generation
geometry must all agree. Unknown local samples cannot authorize idle compaction;
exact samples can compare authority with `NativeLocalPointerSample::same_coordinates`.

Native hosts can query `native_pointer_continuation()` for finite hover dwell,
Touch long press or tab-strip edge scrolling. This read-only request projects
the existing gesture's pointer/event/token, deadline, target surface/point and
original source binding. A NativeLocal request also provides its still-current
coordinate capability for fresh capture. The request is not an observed
position, native capture or receiver proof. Surface-local getters do not expose
native requests, and pending recorded native input suppresses scheduling.

At the deadline, first commit accepted input, re-query the request, observe
fresh native position/delivery/capture facts, then record its event through the
ordinary native journal with the original clock and current-output receiver
answers. A different target window additionally requires confirmed Applied
source routing. Each edge-scroll step waits for exact target-output acceptance.
Unknown capture revokes the challenge, including capture facts carried by
secondary buttons or same-pointer scrolling; restoring capture does not revive
the old token. Obsolete same-stream tokens do not reinterpret an old lane.
The host must implement automatic wakes and fresh OS stationary/Touch
observations to exercise this contract on a platform.

`native_input_owned_capacity_bytes()` on the session or an open host frame
reports recorded input transport data: recorder envelopes and buffers, shared
payload data, replay record buffers, and a pending prefix-retirement receipt.
It uses vector capacity, including unused slots retained after retirement.
Replay batches share payload allocations and add only their own record buffers.
This query does not impose a cap. Host queues, pending effect/presentation
reports, reducer authority and candidate state, allocator overhead, and Arc
control blocks are outside its measurement boundary; it is not process memory.
Use the frame query while a frame is open so its cursor or commit guard is
included. Sessions without native input report zero.

`native_input_pending_report_capacity_bytes()` separately measures the native
presentation and effect report sidecars once through the session owner. A
presentation terminal fills its original pending slot; abandoned effect requests
use a shared prepaid queue. The query includes spare vector capacity, with the
same allocator/Arc metadata exclusions. Shared handles do not duplicate backing
charges.

A host can select `NativeInputStorageLimit` during
`enable_managed_native_host_with_input_storage_limit`. This optional recorded
transport allowance reserves recorder backing, prepared pointer journals, full
atomic replay, the commit guard and prefix retirement. The default remains
unlimited. `InputCapacity` preserves unaccepted data for retry after draining
accepted input. It does not include the separately measured pending report
sidecars or the host's callback/resource owners; combine those before advertising
a total input-memory limit.

Add `with_producer_storage_limit(NativeProducerStorageLimit::new(
binding_slots, work_area_slots, byte_limit))` for a separate producer allowance
in the same atomic enrollment. Both slot counts use `NonZeroUsize`.
`producer_storage_limit()`, `binding_slots()`, and `work_area_slots()` expose the
selected bounds so a host can reserve its own snapshot source and scratch data.
Without this option producer storage remains unlimited, including rosters larger
than a native adapter's selected profile.

Binding slots cover the complete current registry together with every retired
incarnation awaiting an accepted `report_native_binding_quiescence` append.
Missing inventory, disabled routing and registry removal do not release slots.
New root and owned-child registrations check this union and queued registrations
before appending, so a refused registration cannot trap an earlier accepted
prefix. A candidate-local allocation allowance also rejects pointer/product
tear-off before minting another binding. Accepted pointer releases can therefore
commit their ordinary typed rejection without replaying forever. Automatic
replacement falls back to host registration when no new lifetime fits.

The fixed byte promise covers producer inline state, actual reserved vector
capacity, old/new producer overlap, native roster/admission report construction,
snapshot entry/generation candidates and snapshot payload construction overlap.
Every final host-frame candidate, including document restore, prepares the full
producer replacement and native report before core publication. Rejection or a
dropped preparation leaves the original producer and publication prefix intact.
Snapshot ingestion checks each actual iterator entry before allocating bounded
scratch; iterator size hints never grant admission or advance generations.

`native_input_producer_storage_reserved_bytes()` returns the selected fixed peak
allowance. `native_input_producer_storage_capacity_bytes()` returns current
producer inline storage and vector backing, including spare capacity after
quiescence; it returns zero without native input. Combine this resident query
with the transport and pending-report queries for those distinct owners.
Caller-owned observation iterators, `NativeWorkAreaRoster` backing and complete
delivered report containers belong to the host. Final retained snapshot payloads
belong to transport; producer bytes cover their construction overlap only.
Reducer/workspace state, allocator metadata and Arc control blocks remain outside
these allowances. These primitives do not enable a total product memory limit.

Add `with_report_storage_limit(NativeReportStorageLimit::new(...))` to select a
separate terminal-report allowance during the same atomic enrollment. It bounds
unsettled presentation outputs, retained presentation stream metadata and
outstanding native-effect failure-report capabilities. Its byte quota reserves
complete preparation, returned capabilities and report/settlement overlap before
core publication. `native_input_report_storage_reserved_bytes()` returns that
fixed promise for combination with a host's other allowances. Insufficient
enrollment storage publishes no provider. Select the profile before issuing
presentation outputs; existing report capabilities and stream history must be
quiescent before selecting a new finite store.

Frame commit prepares and admits the complete presentation/effect emission set
before publishing the core candidate, including document restore. Report-capacity
rejection of a new application operation leaves the candidate unpublished and
returns no affine outputs. Already accepted native input can still commit when
effect reports are full: the original core effect ledger retains the ordered
undelivered suffix until report capacity is available. It does not create an
external request, synthetic result or duplicate dispatch. This lets an ingress
prefix commit before an older failure result retries into that freed ingress.
A successfully retiring presentation prefix can release slots for new outputs in
that same frame. Existing terminal reports and cross-thread output Drop fill
their original slots without growing backing. Effect requests retain their
failure-report reservation through negative-result conversion, rejected
submission and cleanup correlation; successful recording or accepted dispatch
ends that failure-report promise. Successful-dispatch acknowledgement facts
still belong to the host's separately reserved lifecycle/command owners.

Buffered hosts can call `prepare_native_pointer_capacity()` before beginning
their own translation iterator. It reclaims committed transport and returns an
optional finite edge count; a suspended replay keeps its original storage. Zero
capacity with `has_pending_native_input() == true` means the accepted prefix
must commit first. An empty recorder too small for even one pointer edge returns
`InputCapacity`. The returned count is a scheduling hint; the recording methods
still validate the actual payloads and complete batch admission.

The synchronous `begin_native_host_frame` convenience remains available for
hosts deliberately draining a complete interval; it does not enforce a time
budget. The working egui native coordinator uses the budgeted protocol and
exact neutral close holds. Its isolated workspace pins the required Git
dependencies. See [platform support](../../../docs/PLATFORM_SUPPORT.md) for
backend capabilities and the distinction between host-contract tests and
real-window qualification.

When a buffered managed host exhausts its own ingress capacity, it must freeze
ordinary admission and commit all previously accepted input before invoking
`DockspaceHostFrame::cancel_native_input_for_overload`. This terminal operation
needs no additional backend record or pointer edge, including when the ordinary
record budget is exhausted. It ends live gestures, routing and scrolling while
preserving the provider, monotonic identities, and accepted release, output and
lifecycle obligations. Measurement and paint may follow; further input in that
frame is rejected. Consume the exact submitted-action outcome only after the
frame commits. Dropping or failing the frame leaves cancellation unpublished.

Cancellation revokes button/capture authority to `Unknown`; it does not prove
physical release. Once earlier native records are committed, a host with fresh
complete facts can call `DockspaceSession::report_native_pointer_roster`.
That ordinary recorded checkpoint restores authority only after its frame
commits. Held buttons remain down, and an unknown roster keeps recovery open
for later complete facts, including after subsequent press/release edges.
Do not invent an empty roster when the platform cannot observe all pointers.
The runtime validates the exact current provider, bindings and pointer watermark.

If a raw device-removal notification cannot be correlated with the host's
delivered pointer and scroll identities, call
`report_native_input_device_removal()`. It records one ordered provider-wide
reset fact, regardless of live stream count; it does not synthesize per-stream
pointer edges or advance their watermark. Core cancels live interaction and
scroll sessions, preserves pending release/output obligations, and revokes
button/capture facts to Unknown. Later backend and application input may follow
in the same frame. A fresh complete pointer roster can recover authority after
the reset commits. The native reference adapter uses this path and retains late
device-tail tombstones and monotonic identity counters.

When one exact window stops delivering input, use
`report_native_input_binding_retired(binding)`. Its single fixed-size backend
fact ends streams with a held-button origin or retained capture at that binding
and closes its smooth-scroll provider tails. Unknown capture does not erase
the preceding dependency or grant observed capture authority; idle delivery
alone is not ownership. Other bindings and accepted release/output obligations
survive. No synthetic pointer edge advances the pointer watermark. Input
termination does not publish window destruction or declare callback quiescence;
the host still completes those exact lifecycle barriers separately.

The cancellation API does not itself impose ingress record/byte/age caps or
reserve a host overload latch. Those adapter responsibilities, release latency,
and physical OS behavior remain separate qualification work.

## Native creation failure and recovery

The host owns window and renderer preparation. Native enrollment no longer
selects a core preparation strategy. An engine may prepare its render target
directly; the egui reference adapter privately drives its hidden/show/render
sequence in the existing exact window-resource owner. Core receives the same
revocable readiness facts from both hosts, without placeholder paint requests
or a ShowWindow script.

Creation requests return `NativeEffectAcknowledgement::Create`. Dispatch
acceptance supplies only the correlation capability. Report actual creation through
`NativeWindowFacts::with_create_readiness(Created, Some(create_ack))`, or report
`Prepared` immediately when both creation and renderer preparation are complete.
Created is not a mandatory intermediate pass. Prepared also requires
current Visible state, exact client size, and independent native and
presentation scale factors. Report local geometry with `with_content_size`
when the desktop origin is unavailable. Existing `with_content_bounds` facts
also establish size, and a separately supplied size must agree with them.
Content bounds with a desktop origin and outer bounds are separate
placement/recovery facts; neither is invented from size or requested placement.
Geometry observation and readiness are separate;
an unchanged coordinate authority does not need a fabricated generation bump.
Creation acknowledgement settles independently of visibility. A host can report
it while visibility is Unknown and publish full readiness later without
repeating the acknowledgement.

Keep reporting current preparation facts in complete snapshots. Compatible
Prepared observations preserve their readiness basis;
Unknown, Created, failed preparation, coordinate/visibility authority loss and
provider retirement revoke it.
Re-preparation uses the original creation obligation and a fresh first-live
output floor. It does not reissue a non-idempotent CreateWindow. A host must
publish a readiness loss before it re-establishes preparation, even when the
window returns to identical geometry. Source reservation, the semantic child
program, first-live publication and exact cleanup owners remain in core,
including for replacement windows.
A replacement with still-valid measurements receives a fresh semantic output
ticket when readiness returns, so the next ordinary paint can satisfy its new
first-live floor. Changed coordinate or requirement authority follows the normal
measurement path. Old outputs remain ineligible for admission.

An admitted native root can measure, paint, settle exact output and process
stable-item commands using size and scales alone. Desktop hover conversion,
cross-window physical routing, default window placement and spatial recovery
still require the corresponding observed desktop facts. Missing desktop origin
returns an explicit coordinate-authority failure; it never becomes `(0, 0)`.
Native-local pointer input uses the separately captured capability described
above; local geometry alone never supplies a desktop route.

Renderer failure and lifecycle cancellation are distinct operations. Dropping
an affine output reports only that output's failure; an unresolved native create
still owns its window, staging resource and first-live obligation.
Drain accepted input before adding lifecycle cleanup. Do not append cleanup
after an overload cancellation has closed the frame's semantic input phase.

Source content retains its durable surface and root ownership until the exact
first-live content output is accepted. Host preparation placeholders do not move
it. A prospective child participates in the normal surface roster, measurement
and complete semantic paint program through a checked transient projection.
Measuring, painting, dropping or reporting failure for that projection does not
publish its workspace command. The current exact Presented output commits the
transfer, workspace revision and admission atomically in the host frame.

`frame.cancel_next_native_create()` cancels at most one current candidate,
including a child awaiting first-live presentation, through the existing
compensation protocol. Source ownership remains unchanged. Count each
successful cancellation against the host dispatch budget and commit before
publishing any resulting cleanup work. Frame drop leaves the original saga and
source sequence intact. A late output can settle only its original emission;
it cannot admit a cancelled child. Retained resources continue through ordinary
binding cleanup and exact terminal window facts.

After first-live admission, ordinary surface destruction uses the core's
existing bound recovery obligation. There is no separate pre-first-live
transferred-create recovery API.

An existing logical surface can acquire a native window with
`frame.create_native_surface_current(surface, recovery_host, placement)`.
The recovery host must be a current application root. The request captures the
surface's complete main root and contained forest in the existing recovery
owner, emits `CreateWindow`, and uses the same host-readiness and first-live barrier.
Admission preserves topology and workspace version. Logical content remains
suspended while native preparation is incomplete; an output's `Presented`
terminal must commit before the new binding gains input authority.

Initial creation and destruction-driven replacement retain distinct origins.
After an initial attempt fails or is cancelled, the same explicit request can
retry with a fresh binding and the original recovery obligation. Old cleanup,
create acknowledgements and outputs retain their predecessor identity. A healthy
attempt or an existing destruction-driven recovery returns typed `Conflict`.
If no source coordinates were ever observed, failure retains the full surface
pending an explicit retry; requested placement is never treated as observed
geometry for contained recovery. Reading saved hosting preferences remains inert.

For managed native document restoration, use
`frame.submit_native_document_restore(&restore, root_surface)` as the frame's
terminal input. It commits the document and explicit Child creation intents
together while preserving declared surface/recovery-host identities. The retained external Root may
omit its saved role declaration; every additional surface must declare Child
hosting to that Root and a positive saved outer placement. Another retained
external Root cannot silently become a Child. Ordinary `submit_document_restore`
keeps native declarations inert.

To replace the logical host itself while retaining an external Root window, use
`frame.submit_native_layout_reset(&reset, existing_root, incoming_root)` or the
persistence-enabled `submit_native_document_restore_rebinding_root`. The incoming
surface names the prepared layout/document; core preserves that identity
alongside Child recovery references and saved placement. The old
host surface may disappear entirely. These explicit operations reject an occupied
target rather than claiming another window.

Finish the frame with `frame.prepare_commit()` before preparing native resources.
The prepared value exposes the final `native_root_continuations()`,
`native_placement_detachments()` and complete predecessor/successor rosters after
vacancy reduction. Prepay the whole batch under the host's publication lock,
then call `prepared.commit()`. Dropping preparation rolls back the frame.
`frame.proposed_native_root_continuations()` remains an early replacement preview;
it does not describe final placement vacancies.

After the complete commit succeeds, install only the matching
`report.native_root_continuations()` exact pairs on the same OS windows, and
independently drain every `report.native_placement_detachments()` entry. An
application-owned root-surface field publishes at that same host commit boundary.
Late predecessor outputs/callbacks keep their original identity until quiescence;
they do not supply successor coordinates or interaction authority. Fresh OS
geometry plus measurement can establish coordinates before the successor output
is Presented; only its exact presentation authorizes input.

After committing, a managed host may call
`frame.advance_next_native_surface_create()` once per available lifecycle work
unit, before measurement or application terminal replacement. A temporary wait
for fresh Root coordinates, known capabilities, exact predecessor quiescence
or producer capacity appends no input. One successful call starts at most one
initial attempt or records its definite rejection. The returned `true` consumes
one unit of the host's shared dispatch budget; effect-report backpressure retains
undelivered Create in the original ledger. Each created child still requires
current host preparation and exact semantic first-live presentation.

Undispatched intents own neither a native binding nor an effect. Shutdown uses
`cancel_next_native_create` to stop them without Close. Cancelled/rejected intents
retain their complete suspended logical surface but no longer count as pending
creation; only explicit creation retries them. Saved preferences are never
scanned each frame to reopen windows. Hosts retain their ordinary exact output,
cleanup, Destroyed and producer-quiescence responsibilities.

Pure lifecycle cleanup can commit while effect-report storage is occupied;
compensating effects remain in the original ledger until delivery capacity
returns. Ordinary application input mixed into that frame keeps its capacity
gate. `session.has_pending_native_create()` is a derived lifecycle query, not a
complete host quiescence test: combine it with outstanding output, callback,
cleanup, input-restore and retirement work owned by the host.

## Application Close Decisions

A tab close requests that item; a contained-window close or `Close Group`
requests the complete root. A close request never docks the root back or deletes
content before application approval. The independent public consumer in
`integration/dockspace-host-harness/tests/close_commands.rs` shows the complete
Allow, Veto and deferred-continuation loop without a renderer or pointer host.

Inspect `HostFrameReport::inputs()` for `HostInputOutcome::CloseRequested`.
Each plan contains stable item identities and their exact decision tokens.
`DockspaceSession::pending_content_close_plans()` also discovers current item
and root requests after their original report has been consumed, including
requests from tab or contained chrome and deferred continuations. It projects
the existing core plans, excludes native surface-close plans and omits stale or
terminal requests. Keep each report's exact action result separately when
reporting completion; this query is current state rather than an event history.

`submit_prepared_close_request`, `resolve_close` and `continue_deferred_close`
return a `DockspaceSubmittedAction`. Use
`HostFrameReport::submitted_action_outcome(id)` to read that exact input when
other requests or decisions share its frame. An identifier from a dropped
attempt or another session cannot match a later report.

Hosts which retain only a request identity can visit
`DockspaceHostFrame::pending_close_decisions(request)` to read current unresolved
initial decisions without cloning the complete plan. Take a bounded prefix of
copied items, submit their tokens, and commit before querying the remainder in a
later frame. A dropped frame leaves the same decisions pending. Deferred
continuations remain separate; a foreign, stale, terminal or compacted request
yields no initial decisions. Earlier pending native input must settle first.
Submit initial decisions with `resolve_close`; a deferred item exposes a distinct
continuation for `continue_deferred_close`. Core keeps the entire group unchanged
until every required item allows the request. A veto retains the group; repeated
initial or continuation tokens return typed inert results. Keep application pane
data separately when closed items may be restored or reopened later.

Dock Back is an independent prepared root command,
`DockspaceSession::prepare_dock_root_back`. It preserves application content and
does not open a close plan.

An application's closed-pane inventory is its stable item catalog minus the
items currently returned by `session.view().item(id)`. Reopen with
`prepare_open_item` or `frame.open_item_current`, choosing an explicit current
`DockPlacement`. Core selects newly opened content and requests its pane focus;
an already open item returns `Existing` without moving it or stealing focus.
Missing anchors are rejected. The shell chooses a fallback from the current
surface/root/item view. Restore preserves declared presentation identities and
uses the new workspace epoch to reject old capabilities.

## Arrangement locking

Set `DockPolicy::set_arrangement_locked(true)` and publish it through
`DockspaceHostFrame::replace_policy`. This prevents opening/closing content,
docking/reordering, presentation changes and geometry edits. Selection, tab
navigation/scrolling, pane content and focus remain available. Bringing an
existing contained window to the front is part of focus; it does not move or
resize that window. Irrevocable native teardown/recovery can still finish.

The lock is an independent policy restriction. It preserves operation filters
and per-item rules, so unlocking restores their original behavior. Replacing
policy is the frame's terminal configuration step: commit an unlocked policy
before submitting layout edits or applying a saved layout in a later frame.
The replacement immediately updates the private candidate, so measurements and
encoding in that frame use the new policy. Earlier input retains its original
policy and presentation facts. Configuration after encoding begins is rejected.
`replace_policy` returns the same exact submission identifier; its
`HostInputOutcome::PolicyReplaced { changed, version }` records the version at
that input even when several configurations share one commit or a policy is
unchanged.
Dropping the unlock frame retains the published lock. Live document restore
rejects a locked arrangement; constructing an initially locked session is valid.

Live layout replacement creates a new focus request inside the same rollbackable
frame. It prefers the previous semantic target when that stable item remains
selected in the restored layout. Otherwise it uses the prior target surface when
retained, or the first current surface in stable identity order: main central
selection, the main tree's first visible selection, frontmost contained selection,
then the workspace. An empty workspace has no request. The saved selection and
topology are preserved. A newer pending intent takes precedence over older focus
history. Explicit Foreign/None focus stays outside dock content; it is distinct
from having no history and does not trigger the primary-pane fallback.
Old-epoch observations cannot settle the replacement request.

For a native destination the same intent waits internally for a fresh eligible
binding. It never reuses the old binding or publishes local authority while the
native window is unready. A pure logical surface stays local even when another
surface is bound to a native window. Hosts still confirm actual focus through
the request-bound observation API; requesting focus does not establish it.
The internal Waiting state is excluded from published focus requests and focus
deltas. A matching fresh global observation settles native facts without replacing
the restored semantic request's identity or causal order.

Per-item restrictions can express different kinds of pinning independently:

- `DockItemRule::set_allow_tab_reorder(false)` preserves the item's relative
  order inside its current tabs group. The item cannot be reordered, and other
  existing tabs cannot cross it. No-op placements remain valid.
- `set_allow_undocking(false)` prevents leaving the current docking owner.
- `set_source_enabled(false)` disables the item as a moving payload.
- `set_close_capability(Some(CloseCapability::Disabled))` prevents closing it.

Reorder restrictions do not prevent closing or undocking an item. Opening a new
tab before it may change its numeric index without crossing an existing tab.
Applications own pin icons and preview-document state; core enforces the chosen
layout restrictions for both programmatic commands and pointer drop previews.

## Application defaults and layout reset

Keep default layouts and preset names in the application. Applying a default
does not require serde, JSON, document lineage or a second docking session:

```rust
# use dockspace::model::DockspaceLayout;
# use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};
# fn apply(session: &mut DockspaceSession, default: DockspaceLayout) -> Result<(), Box<dyn std::error::Error>> {
let reset = session.prepare_layout_reset(default)?;
let mut frame = session.begin_host_frame()?;
frame.submit_layout_reset(&reset)?;
// A renderer may measure this candidate here. This example defers its surfaces.
frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
let report = frame.commit()?;
# let _ = report;
# Ok(())
# }
```

The prepared reset belongs to its session and published version. Drop or failed
commit leaves it available for retry; successful reset advances the workspace
epoch and invalidates it. A reset is the frame's last semantic mutation, followed
by measurement contributions or explicit unavailability. Measurements install
Ready scenes on commit, and a later frame paints them. Managed native hosts use
their existing `poll_native_host_frame` budget to finish accepted input, then
call `frame.submit_layout_reset(&reset)` when `native_input_pending()` is false.
Partial committed prefixes remain separate; an atomic group paused in a private
candidate resumes without repeating its already answered receiver questions.
The reset is appended to that final candidate rather than restarting replay.
The committed report retains the exact native prefix outcomes needed to settle
the original snapshot, focus and close records, followed by the unique terminal
replacement. Product outcomes superseded by that replacement remain omitted.
An integrating native host can inspect `frame.proposed_native_bindings()` to
prepay resources for the replacement before committing. These proposals confer
no current route or presentation authority; install successors only after the
complete committed report confirms their exact identities.
`begin_layout_reset_frame` and `begin_native_layout_reset_frame` remain
synchronous conveniences over the same submission path. Declared root, surface,
contained and pane identities remain stable, preserving identity-scoped policies.
The replacement advances the workspace epoch; old outputs, actions and native
bindings cannot authorize the new instance. Automatic allocation history remains
monotonic and merges restored document frontiers.

For a document-bound session, layout reset preserves key history and save
generation. Every item must already have a recognized key, including closed
history entries. Register new application keys before preparing the reset.
Arrangement lock requires an explicit committed unlock. Existing policy and
application pane data are not replaced by the supplied layout.

With serde enabled, `prepare_document_restore_json` similarly prepares a
complete lineage-bound restore, and `frame.submit_document_restore(&restore)`
appends it after the existing input prefix. The old document's item scope
governs every earlier input. A private final-replacement operation admits only
the reconciled target scope, retaining all known keys and admitting newly
recognized items solely for the restore. Scope expansion and append failure
poison the same candidate; a dropped or failed frame publishes no workspace,
key map, placement, generation or input watermark. The synchronous document
restore conveniences also resume the same candidate.

Reset and document restore supersede earlier product outcomes in their frame:
the report exposes one `LayoutReset` or `DocumentRestored` result with its
replacement-before and replacement-after versions. Measurement may follow,
but later semantic/configuration input is rejected. The input replay budget
does not impose a hard time bound on layout construction or document decoding.
For native replacement, consume the complete `native_bindings()` successor
roster; old binding and output authority cannot revive interaction. Logical
surfaces without a binding do not automatically become new OS windows.

The document preserves explicit native hosting intent independently of window
placement. The same document-owned per-surface record can describe a native
application root or a Dockspace child with its recovery-host surface, even when
no exact physical outer rectangle has been observed. Purely logical surfaces do
not acquire a native role from their main-root shape or an old placement hint.
Saving captures current exact roles and child recovery ownership, retains
declared preferences for unbound surfaces, and removes rows whose logical surface
no longer exists. Placement hints capture confirmed physical outer bounds,
client size and native scale; they grant no current coordinate authority.

Use `surface_presentation_preferences()` on `DockspaceSession`,
`PreparedDockspaceDocumentRestore` or `DockspaceHostFrame` to inspect the same
read-only product values. Prepared, candidate and committed values preserve
the declared source-surface and child recovery-host identities. Dropping the frame publishes none of
those changes; successful commit publishes preferences, graph, keys and
generation atomically. This projection does not save new observations or
advance a generation. The workspace `view()` remains the content authority,
including each surface's complete main root and contained forest.

The pre-release document uses one schema version, `[1, payload]`, for the complete
atomic document. Workspace, external keys and surface preferences are ordinary
strict objects without independent version envelopes. Nullable fields must be
present, using `null` for intentional absence; missing, duplicate and unknown
fields reject.
An unsupported outer version returns `DockspacePersistenceErrorKind::UnsupportedVersion`
before payload decoding or application-key recognition.
Native binding, provider, window token, recovery
anchor and work-area token are never persisted. The declarations are inputs to
an explicit host restoration policy, and do not create windows or bypass exact
preparation and first-live presentation.

## Manipulation feedback and focus

`HostFrameReport::manipulation_feedback()` exposes committed pending and terminal
results without exposing graph, receiver-proof or reducer internals. A consumed
release may publish `Pending(AwaitingPresentation)` before the exact preview is
presented. Its later delivered or cancelled result carries the same opaque
`manipulation_id()` and a distinct `id()`. Use the latter to deduplicate repeated
reads or announcements. Motion alone produces no feedback; reading a result
never moves focus or replaces pane content. `disable_host_pointer` returns any
committed cancellation on its retirement outcome, including a release already
waiting for presentation.

Core focus requests identify `DockspaceFocusTarget::Pane(item)` or `Workspace`.
A focused tab close chooses the next visual tab, or the previous tab at the end;
a focused group close uses a surviving observed destination when available,
otherwise the remaining surface workspace. Closing an unfocused target does not
override a newer focus intent. Pending, vetoed and rejected close decisions do
not change focus. Hosts execute a request and submit its exact observation only
at their valid output boundary; feedback itself is never focus authority.

## Runnable independent host

The repository's `integration/dockspace-host-harness/examples/engine_host.rs`
uses only public core and application types. Run it with:

```text
cargo run --manifest-path integration/dockspace-host-harness/Cargo.toml --example engine_host --locked
```

It demonstrates two logical surfaces without native windows, owned output after
the frame borrow ends, exact Presented/Dropped settlement, application focus,
complete root-close approval and feedback deduplication. The recording renderer
is one possible host-owned format, not a core renderer or a GPU-present claim.
The companion public-contract tests use the same API from an independent Cargo
workspace, with minimal features and optional persistence/motion.

## Motion

The default `motion` feature separates the target layout from its
rendered geometry. It adds no platform, renderer or async dependency. Without
the feature, target and presentation rectangles
are identical.

Before encoding, set one monotonic timestamp and policy for the entire update:

```rust
# #[cfg(feature = "motion")]
# fn update(frame: &mut dockspace::runtime::DockspaceHostFrame<'_>) -> Result<(), dockspace::runtime::DockspaceRuntimeError> {
use std::time::Duration;
use dockspace::motion::{MotionFrame, MotionPreference, MotionSettings};

frame.set_motion(MotionFrame {
    now: Duration::from_millis(1000), // Use your host's monotonic elapsed time.
    settings: MotionSettings::default(),
    preference: MotionPreference::Full, // Supply the preference actually observed.
})?;
# Ok(())
# }
```

Keep the timestamp fixed across rendering passes and surfaces in that update.
Set `MotionSettings::mode` to `MotionMode::Off` to disable motion at runtime.
Omitting `set_motion` keeps a custom host static. To remove the implementation,
disable the default feature with `default-features = false` on the dependency.
Default layout and preview tweens finish in 200 ms and 125 ms respectively.

Changing motion settings does not invalidate measurements. `System` resolves
the supplied preference; `Unknown` permits motion by default. Set
`unknown_prefers_reduced_motion = true` for a conservative fallback. Off, reduced
motion and zero timing use immediate geometry and request no motion frames.

Render content, chrome and clips at the rectangles in `SurfacePaintPlan`.
Retained frameworks should key content views with `content_identity(item)` and
apply the supplied geometry directly. Running a second layout animation in the
framework would separate visible content from core hit and accessibility bounds.
Text is reflowed at the sampled size rather than scaled as a texture.

Motion changes presentation only: semantic commands commit immediately. New
trajectories start at confirmed Presented geometry; active trajectories retarget
from their current analytic position and velocity. Springs preserve unconstrained
channel velocities; tweens promise position continuity. Clipping, nonnegative-size
projection, direct gestures and explicit settlement can reset velocity.

The first delivery animates surviving visible items within the same surface and
root. Newly visible items appear at target, and removed items have no exit
snapshot. Root changes and native prospective-child first-live output are
immediate. It does not move OS windows or promise atomic cross-window animation.
During topology changes, panes may overlap. One core pane order drives drawing
and hit precedence. A moving root without a valid partition exposes no splitter,
junction or structural drop operation; unaffected roots and content actions remain
available. The target layout remains available internally for constraints.

After committing an output, check `session.surface_motion_is_active(surface)` to
decide whether a visible surface needs another frame. Stop when it settles, pause
hidden surfaces, and wait for the existing terminal-output wakeup under output
backpressure. Resume with current time; do not replay intermediate ticks. As with
static output, report Presented or Dropped and commit that report before granting
input to its receivers. Evaluating another sample does not authorize it. A reported
newer Presented output awaiting an earlier terminal result conservatively blocks
old input until the ordered terminal prefix can commit.

Focus and drop-decoration tracks may consume `plan.motion_frame()` but have no
input authority. Keep final drop-target geometry available separately to custom
painters. `partition_available(root)` describes this artifact, so model settlement
alone cannot restore structural receivers on an older visible frame.

Run the independent public-host motion contract and cost probe:

```sh
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features motion --test motion --locked
cargo run --release --manifest-path integration/dockspace-host-harness/Cargo.toml --features motion --example motion --locked
```

The probe separates target preparation, active/static tick preparation and
display-list traversal at 16 and 64 panes. It excludes application content and GPU
rendering; use the egui workbench exercise to measure those host costs separately.

## Features

- `serde`: session-owned versioned JSON persistence with append-only external
  item identity and viewport placement.

This crate does not own an OS event loop, renderer, widget tree, animation
clock, or platform window. Those responsibilities belong to adapters.
