# dockspace

`dockspace` is the renderer-neutral semantic core of `egui_dockspace`. It owns
docking topology, validated mutations, layout and paint plans, interaction
state, close decisions, persistence identity, and native-surface lifecycle
semantics. Renderers supply measurements, draw the returned plans, report
presentation results, and execute typed platform effects.

The default public interface is deliberately item- and surface-centric. Runtime
node identities, scene stamps, reducer inputs, provider leases, receipts, and
other backend state machines are private. Renderer implementations that are
migrating against the unstable low-level protocol must opt into the `backend`
feature and `dockspace::backend` namespace.

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

## Features

- `serde`: session-owned versioned JSON persistence with append-only external
  item identity and viewport placement.
- `backend`: unstable adapter protocol. Ordinary applications should not enable
  it.

This crate does not own an OS event loop, renderer, widget tree, animation
clock, or platform window. Those responsibilities belong to adapters.
