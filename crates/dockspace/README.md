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

The current breaking-refactor plan prioritizes a documented engine-host
contract and an independent public consumer without egui. Immediate program
consumption and compilation into host-owned display lists or retained UI nodes
must use the same output identity and presentation rules. These expanded
integration gates are planned work; see the repository's engine-host plan and
`docs/VALIDATION.md` for the current evidence and remaining gaps.

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

This crate does not own an OS event loop, renderer, widget tree, animation
clock, or platform window. Those responsibilities belong to adapters.
