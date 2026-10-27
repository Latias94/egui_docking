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

## Application Close Decisions

A tab close requests that item; a contained-window close or `Close Group`
requests the complete root. A close request never docks the root back or deletes
content before application approval. The independent public consumer in
`integration/dockspace-host-harness/tests/close_commands.rs` shows the complete
Allow, Veto and deferred-continuation loop without a renderer or pointer host.

Inspect `HostFrameReport::inputs()` for `HostInputOutcome::CloseRequested`.
Each plan contains stable item identities and their exact decision tokens.
Submit initial decisions with `resolve_close`; a deferred item exposes a distinct
continuation for `continue_deferred_close`. Core keeps the entire group unchanged
until every required item allows the request. A veto retains the group; repeated
initial or continuation tokens return typed inert results. Keep application pane
data separately when closed items may be restored or reopened later.

Dock Back is an independent prepared root command,
`DockspaceSession::prepare_dock_root_back`. It preserves application content and
does not open a close plan.

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
