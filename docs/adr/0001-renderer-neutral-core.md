# Keep the core renderer-neutral and synchronous

Dockspace is a reusable docking runtime rather than an egui widget. The core owns
layout, interaction policy and semantic transitions; applications own pane data,
renderers, clocks, event loops and OS resources. Adapters consume the public
runtime rather than adding framework-specific mutation paths to the engine.

The runtime stays synchronous. Typed effects and exact deferred continuations
bridge host work without imposing an executor, async trait or shared runtime
lock on every integration. Applications run background tasks and submit their
results at the ordinary frame boundary. This makes immediate, retained and
deferred rendering possible through the same core contract, at the cost of
explicit host orchestration.

See the [engine host guide](../../crates/dockspace/docs/guide.md) and
[adapter responsibilities](../ADAPTER_TESTING.md).
