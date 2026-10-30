# dockspace

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

1. Begin a host frame and supply surface measurements in logical coordinates.
   A surface is a logical layout and input domain; it need not be a platform
   window. Items and roots retain their stable application identities.
2. Obtain a `SurfacePaintPlan` and call `execute_surface_program` to accept its
   complete ordered program into your sink. Copy the required geometry,
   identities, and pane-content references into owned host records if rendering
   will happen after the frame borrow ends. `SourceGap` records preserve semantic
   identity while suppressing ordinary visible source paint.
3. Pass the returned `SurfaceProgramExecutionReceipt` to
   `confirm_surface_painted`, account for unpainted surfaces, then commit the
   frame. A receipt proves complete sink acceptance; the committed
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

A complete press/move/release batch can finish input processing before its
preview is presented. The core freezes the release, then waits for the exact
preview output's presentation result. Hosts continue rendering pending feedback
without inventing more pointer events. Complete contained sources and resize
previews include real content, chrome, hit and semantic geometry in the surface
program. Renderers should not add a second ghost shell when a preview reports
`is_layout_projected()`. Only terminal delivery writes the durable rectangle.
The independent public consumer verifies
this contract without egui or native hosting. The common public-host and egui gesture matrices now pass, including complete
single-batch input. Optional native consolidation and platform qualification
remain separate; see `docs/VALIDATION.md` for evidence and limits.

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

## Budgeted Native Input

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
implicitly upgrades a native-local gesture. This public host contract is locally
qualified in `docs/VALIDATION.md`; adapter packet emission and native stationary
scheduling remain separate work. The egui native reference adapter has a
qualified event-time local geometry callback contract; the public host API does
not establish platform callback qualification. Its
committed capability, callback geometry and event-generation geometry must all
agree. Unknown local samples cannot authorize idle compaction; exact samples
can compare authority with `NativeLocalPointerSample::same_coordinates`.

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
exact neutral close holds under a local, unpublished fork override. Its tracked
dependency pins do not yet reproduce that adapter. See `docs/VALIDATION.md` for
the separately qualified core, adapter-model, and real-window evidence.

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

Native enrollment takes an explicit `NativeCreateStrategy`, frozen for the
provider lifetime and carried by CreateWindow and RequestReplacement. Select
`DirectPrepared` when the engine prepares its own platform window, render target
and renderer resources. Select `Staged` for the egui reference host's exact
hidden/pre-show/show/post-show sequence. DirectPrepared requires no staging
placeholder output or core ShowWindow request.

Creation requests return `NativeEffectAcknowledgement::Create`, independently
of Show's `Presentation` acknowledgement. Dispatch acceptance is only the
correlation capability. Report actual creation through
`NativeWindowFacts::with_create_readiness(Created, Some(create_ack))`, or report
`Prepared` immediately when both creation and renderer preparation are complete.
Created is not a mandatory intermediate pass. DirectPrepared also requires
current Visible state, exact client size, and independent native and
presentation scale factors. Report local geometry with `with_content_size`
when the desktop origin is unavailable. Existing `with_content_bounds` facts
also establish size, and a separately supplied size must agree with them.
Content bounds with a desktop origin and outer bounds are separate
placement/recovery facts; neither is invented from size or requested placement.
Geometry observation and readiness are separate;
an unchanged coordinate authority does not need a fabricated generation bump.
Creation acknowledgement settles independently of visibility. A Staged host
can report it while visibility is Unknown and later report Hidden without
repeating the acknowledgement; its exact staging outputs are still required.

Keep reporting current preparation facts in complete snapshots. In
DirectPrepared, compatible Prepared observations preserve their readiness basis;
Unknown, Created, failed preparation, coordinate/visibility authority loss and
provider retirement revoke it.
Re-preparation uses the original creation obligation and a fresh first-live
output floor. It does not reissue a non-idempotent CreateWindow. Staged hosts
retain their exact output evidence instead of using Prepared to bypass staging.
Both strategies use the same source reservation, semantic child program,
first-live publication and exact cleanup owners, including replacement windows.
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
first-live content output is accepted. Staged placeholders do not move
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
surface/root/item view, including after restore remaps retired presentation IDs.

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

## Application defaults and layout reset

Keep default layouts and preset names in the application. Applying a default
does not require serde, JSON, document lineage or a second docking session:

```rust
# use dockspace::model::DockspaceLayout;
# use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};
# fn apply(session: &mut DockspaceSession, default: DockspaceLayout) -> Result<(), Box<dyn std::error::Error>> {
let reset = session.prepare_layout_reset(default)?;
let mut frame = session.begin_layout_reset_frame(&reset)?;
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
`begin_native_layout_reset_frame` to reduce their accepted native prefix before
the same replacement. Retired root, surface and contained identities are rebased
against that candidate; stable pane items and declared selections remain intact.

For a document-bound session, layout reset preserves key history and save
generation. Every item must already have a recognized key, including closed
history entries. Register new application keys before preparing the reset.
Arrangement lock requires an explicit committed unlock. Existing policy and
application pane data are not replaced by the supplied layout.

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
workspace, with default features and optional persistence.

## Features

- `serde`: session-owned versioned JSON persistence with append-only external
  item identity and viewport placement.

This crate does not own an OS event loop, renderer, widget tree, animation
clock, or platform window. Those responsibilities belong to adapters.
