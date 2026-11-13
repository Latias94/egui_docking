# Documentation

## Start here

- [Project overview](../README.md): what Dockspace does and which integration to use.
- [Examples](EXAMPLES.md): runnable core, egui and native starting points.
- [Core overview](../crates/dockspace/README.md): stable identities, layout and host responsibilities.
- [egui overview](../crates/egui_dockspace/README.md): host choices and pane rendering.
- [winit helpers](../crates/dockspace_winit/README.md): shared mouse input, coordinates and repaint scheduling.
- [Native overview](../crates/egui_dockspace_native/README.md): experimental window integration.
- [GPUI workbench](../integration/gpui-harness/README.md): retained views and single-window docking with a local Open GPUI checkout.

## Integration guides

- [Engine host guide](../crates/dockspace/docs/guide.md): frame orchestration,
  measurements, ordered input, output settlement, close decisions and persistence.
- [egui guide](../crates/egui_dockspace/docs/guide.md): complete-output and callback
  integration, focus, background work, style and configuration.
- [Adapter testing](ADAPTER_TESTING.md): host responsibilities and reusable
  framework-neutral scenarios.
- [Platform support](PLATFORM_SUPPORT.md): capabilities and native testing limits.

## Development

- [Contributing](../CONTRIBUTING.md): repository layout and local checks.
- [Maintaining](MAINTAINING.md): repository structure and package/release order.
- [Native qualification](../integration/egui-native-smoke/README.md): real-window
  and physical-input checks.
- [Host-update sampling](../integration/dockspace-host-harness/BENCHMARKING.md)
  and [product comparisons](../integration/product-comparison/README.md):
  reproducible workloads and interpretation limits.
- [Third-party sources](../THIRD_PARTY.md): prior art and fixture attribution.

## Architecture decisions

- [Renderer-neutral synchronous core](adr/0001-renderer-neutral-core.md)
- [Atomic frames and presentation authority](adr/0002-atomic-frames-and-presentation.md)
- [Native window composition](adr/0003-native-window-composition.md)
- [Separate official and fork workspaces](adr/0004-isolate-native-forks.md)
- [Motion and presentation geometry (proposed)](adr/0005-motion-and-presentation-geometry.md)
