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
this contract without egui or native hosting. The complete adapter gesture
matrix remains in progress; see `docs/VALIDATION.md` for evidence and limits.

## Features

- `serde`: session-owned versioned JSON persistence with append-only external
  item identity and viewport placement.

This crate does not own an OS event loop, renderer, widget tree, animation
clock, or platform window. Those responsibilities belong to adapters.
